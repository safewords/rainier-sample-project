//! `AccountStore` — where the account add-ins read and write users.
//!
//! The add-ins define *ports*: small traits saying what they need from the
//! application's tables and nothing about the tables themselves. This one type
//! implements all of them over this application's schema, so the answer to
//! "where does a passkey live?" is one file.
//!
//! | Port | For | Tables |
//! |---|---|---|
//! | `AccountUsers`, `CreatesNewUsers` | registration, login, reset, verification | `users`, `email_verifications` |
//! | `UserProvider<Member>` | the `web` session guard | the same |
//! | `TwoFactorUsers` | `two-factor` | `two_factor_enrolments` |
//! | `PasskeyRepository` | `passkeys` | `passkeys` |
//! | `SocialAccounts` | `sso` | `social_accounts` |
//!
//! Every user it hands back is a [`Member`] — the user plus what the add-ins
//! know about them, read together so the guard's one lookup answers every
//! synchronous question a flow asks later.

use std::sync::Arc;

use chrono::Utc;
use rainier_framework::accounts::{AccountUsers, CreatesNewUsers, NewUser};
use rainier_framework::auth::{Credentials, Hasher, UserProvider};
use rainier_framework::crypt::Encryption;
use rainier_framework::database::Repository;
use rainier_framework::prelude::*;

use crate::app::models::{BillingCustomer, EmailVerification, Member, User};
use crate::app::repositories::UserRepository;

/// The account add-ins' view of this application's tables.
pub struct AccountStore {
    users: UserRepository,
    verifications: EntityRepository<EmailVerification>,
    customers: EntityRepository<BillingCustomer>,
    #[cfg_attr(not(feature = "two-factor"), allow(dead_code, reason = "two-factor only"))]
    enrolments: EntityRepository<crate::app::models::TwoFactorEnrolment>,
    #[cfg_attr(not(feature = "passkeys"), allow(dead_code, reason = "passkeys only"))]
    passkeys: EntityRepository<crate::app::models::Passkey>,
    #[cfg_attr(not(feature = "sso"), allow(dead_code, reason = "sso only"))]
    social: EntityRepository<crate::app::models::SocialAccount>,
    hasher: Arc<dyn Hasher>,
    #[cfg_attr(not(feature = "two-factor"), allow(dead_code, reason = "two-factor only"))]
    crypt: Arc<Encryption>,
}

impl AccountStore {
    /// A store over `database`, checking passwords with `hasher` and
    /// encrypting second-factor secrets with `crypt`.
    pub fn new(database: Database, hasher: Arc<dyn Hasher>, crypt: Arc<Encryption>) -> Self {
        Self {
            users: UserRepository::new(database.clone()),
            verifications: EntityRepository::new(database.clone()),
            customers: EntityRepository::new(database.clone()),
            enrolments: EntityRepository::new(database.clone()),
            passkeys: EntityRepository::new(database.clone()),
            social: EntityRepository::new(database),
            hasher,
            crypt,
        }
    }

    /// The user, with everything the add-ins know about them.
    ///
    /// Two keyed reads beside the user's own. They are on the session guard's
    /// path, so every browser request pays them — which is the price of
    /// keeping the add-ins' state out of `users`, and the reason both are
    /// primary-key lookups.
    pub async fn member(&self, user: User) -> Result<Member> {
        let verified = self.verifications.find_key(user.id).await?;
        let customer = self.customers.find_key(user.id).await?;

        Ok(Member {
            email_verified_at: verified.map(|row| row.verified_at),
            stripe_customer_id: customer.map(|row| row.stripe_customer_id),
            user,
        })
    }

    /// The member with this id.
    pub async fn member_by_id(&self, id: u64) -> Result<Option<Member>> {
        match self.users.find_key(id).await? {
            Some(user) => Ok(Some(self.member(user).await?)),
            None => Ok(None),
        }
    }

    /// Record the Stripe customer a checkout created for `member`.
    #[cfg(feature = "billing-stripe")]
    pub async fn remember_customer(&self, member: &Member, customer_id: &str) -> Result<()> {
        let row = BillingCustomer { user_id: member.id, stripe_customer_id: customer_id.into() };
        self.customers.upsert(&row, &["user_id"], &["stripe_customer_id"]).await?;
        Ok(())
    }

    /// The member billed as `customer_id`, for a webhook to find.
    #[cfg(feature = "billing-stripe")]
    pub async fn member_by_customer(&self, customer_id: &str) -> Result<Option<Member>> {
        match self.customers.first_by("stripe_customer_id", customer_id.into()).await? {
            Some(row) => self.member_by_id(row.user_id).await,
            None => Ok(None),
        }
    }

    fn parse_id(id: &str) -> Option<u64> {
        id.parse().ok()
    }
}

#[async_trait]
impl AccountUsers<Member> for AccountStore {
    async fn find_by_id(&self, id: &str) -> Result<Option<Member>> {
        match Self::parse_id(id) {
            Some(id) => self.member_by_id(id).await,
            None => Ok(None),
        }
    }

    async fn find_by_email(&self, email: &str) -> Result<Option<Member>> {
        match self.users.by_email(email).await? {
            Some(user) => Ok(Some(self.member(user).await?)),
            None => Ok(None),
        }
    }

    async fn set_password_hash(&self, member: &Member, hash: &str) -> Result<()> {
        self.users
            .update_column(Criteria::new().where_eq("id", member.id), "password", hash)
            .await?;
        Ok(())
    }

    async fn mark_email_verified(&self, member: &Member) -> Result<()> {
        let row = EmailVerification { user_id: member.id, verified_at: Utc::now() };
        // Insert-or-ignore: following the link twice keeps the first date.
        self.verifications.upsert(&row, &["user_id"], &[]).await?;
        Ok(())
    }
}

#[async_trait]
impl CreatesNewUsers<Member> for AccountStore {
    async fn create(&self, new_user: NewUser) -> Result<Member> {
        // The flow has validated the address and the password; the rest of
        // the form is ours.
        let name = new_user.input["name"].as_str().unwrap_or_default().trim().to_string();
        if name.is_empty() {
            return Err(Error::validation(serde_json::json!({ "name": ["Tell us your name."] })));
        }

        let user =
            self.users.create(User::new(name, new_user.email, new_user.password_hash)).await?;
        welcome(&user).await;
        Ok(Member::new(user))
    }
}

/// The welcome email — the same one `app:seed` and `register_user` send.
///
/// Logged rather than raised if it fails: the account exists either way, and a
/// registration that answered 500 after creating the row would invite a
/// second attempt into a "that address is taken".
async fn welcome(user: &User) {
    let sent = async {
        rainier_framework::container::facade_application()
            .resolve::<rainier_framework::mail::Mailer>()?
            .send(&crate::app::mail::WelcomeMail {
                name: user.name.clone(),
                email: user.email.clone(),
            })
            .await
    };
    if let Err(e) = sent.await {
        tracing::warn!(error = %e.message(), "the welcome email was not sent");
    }
}

/// What the `web` session guard reads the logged-in member back with.
#[async_trait]
impl UserProvider<Member> for AccountStore {
    async fn retrieve_by_id(&self, id: &str) -> Result<Option<Member>> {
        self.find_by_id(id).await
    }

    async fn retrieve_by_credentials(&self, credentials: &Credentials) -> Result<Option<Member>> {
        match credentials.get("email") {
            Some(email) => self.find_by_email(email).await,
            None => Ok(None),
        }
    }

    async fn validate_credentials(
        &self,
        member: &Member,
        credentials: &Credentials,
    ) -> Result<bool> {
        use rainier_framework::auth::Authenticatable;

        Ok(match (member.auth_password_hash(), credentials.password_value()) {
            (Some(hash), Some(password)) => self.hasher.verify(password, hash),
            _ => false,
        })
    }
}

#[cfg(feature = "two-factor")]
mod two_factor {
    use super::*;
    use crate::app::models::TwoFactorEnrolment;
    use rainier_framework::accounts::{TwoFactorRecord, TwoFactorUsers};

    #[async_trait]
    impl TwoFactorUsers<Member> for AccountStore {
        async fn find_two_factor(&self, member: &Member) -> Result<Option<TwoFactorRecord>> {
            let Some(row) = self.enrolments.find_key(member.id).await? else {
                return Ok(None);
            };

            Ok(Some(TwoFactorRecord {
                secret: self.crypt.decrypt(&row.secret)?,
                confirmed: row.confirmed,
                recovery_codes: serde_json::from_str(&row.recovery_codes)?,
                last_used_step: row.last_used_step.map(|step| step as u64),
            }))
        }

        async fn save_two_factor(
            &self,
            member: &Member,
            record: Option<&TwoFactorRecord>,
        ) -> Result<()> {
            let Some(record) = record else {
                self.enrolments.delete(member.id.into()).await?;
                return Ok(());
            };

            let row = TwoFactorEnrolment {
                user_id: member.id,
                secret: self.crypt.encrypt(&record.secret)?,
                confirmed: record.confirmed,
                recovery_codes: serde_json::to_string(&record.recovery_codes)?,
                last_used_step: record.last_used_step.map(|step| step as i64),
            };
            self.enrolments
                .upsert(
                    &row,
                    &["user_id"],
                    &["secret", "confirmed", "recovery_codes", "last_used_step"],
                )
                .await?;
            Ok(())
        }
    }
}

#[cfg(feature = "passkeys")]
mod passkeys {
    use super::*;
    use crate::app::models::Passkey;
    use rainier_framework::accounts::{PasskeyRepository, StoredPasskey};
    use rainier_framework::mfa::PasskeyCredential;

    fn credential(row: &Passkey) -> Result<PasskeyCredential> {
        Ok(serde_json::from_str(&row.credential)?)
    }

    /// A credential id as `PasskeyCredential::id_base64url` spells it —
    /// unpadded base64url — so a lookup by raw id finds the stored row.
    fn base64url(id: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::with_capacity(id.len().div_ceil(3) * 4);
        for chunk in id.chunks(3) {
            let n = (u32::from(chunk[0]) << 16)
                | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
                | u32::from(*chunk.get(2).unwrap_or(&0));
            for i in 0..=chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i)) as usize & 63] as char);
            }
        }
        out
    }

    #[async_trait]
    impl PasskeyRepository<Member> for AccountStore {
        async fn passkeys_for(&self, member: &Member) -> Result<Vec<PasskeyCredential>> {
            self.passkeys
                .find_by("user_id", member.id.into())
                .await?
                .iter()
                .map(credential)
                .collect()
        }

        async fn find_passkey(&self, credential_id: &[u8]) -> Result<Option<StoredPasskey>> {
            let Some(row) =
                self.passkeys.first_by("credential_id", base64url(credential_id).into()).await?
            else {
                return Ok(None);
            };
            Ok(Some(StoredPasskey {
                user_id: row.user_id.to_string(),
                credential: credential(&row)?,
            }))
        }

        async fn store_passkey(
            &self,
            member: &Member,
            name: &str,
            credential: &PasskeyCredential,
        ) -> Result<()> {
            self.passkeys
                .create(Passkey {
                    id: 0,
                    user_id: member.id,
                    credential_id: credential.id_base64url(),
                    name: name.to_string(),
                    credential: serde_json::to_string(credential)?,
                    created_at: Utc::now(),
                })
                .await?;
            Ok(())
        }

        async fn update_passkey(&self, credential: &PasskeyCredential) -> Result<()> {
            self.passkeys
                .update_column(
                    Criteria::new().where_eq("credential_id", credential.id_base64url()),
                    "credential",
                    serde_json::to_string(credential)?,
                )
                .await?;
            Ok(())
        }

        async fn delete_passkey(&self, member: &Member, credential_id: &[u8]) -> Result<bool> {
            let removed = self
                .passkeys
                .delete_matching(
                    Criteria::new()
                        .where_eq("user_id", member.id)
                        .where_eq("credential_id", base64url(credential_id)),
                )
                .await?;
            Ok(removed > 0)
        }
    }
}

#[cfg(feature = "sso")]
mod social {
    use super::*;
    use crate::app::models::SocialAccount;
    use rainier_framework::sso::{SocialAccounts, SocialUser};

    #[async_trait]
    impl SocialAccounts<Member> for AccountStore {
        async fn find_by_provider(
            &self,
            provider: &str,
            provider_id: &str,
        ) -> Result<Option<Member>> {
            let link = self
                .social
                .first_matching(
                    Criteria::new()
                        .where_eq("provider", provider)
                        .where_eq("provider_id", provider_id),
                )
                .await?;
            match link {
                Some(link) => self.member_by_id(link.user_id).await,
                None => Ok(None),
            }
        }

        async fn find_by_email(&self, email: &str) -> Result<Option<Member>> {
            AccountUsers::find_by_email(self, &email.trim().to_lowercase()).await
        }

        async fn link(&self, member: &Member, social: &SocialUser) -> Result<()> {
            self.social
                .create(SocialAccount {
                    id: 0,
                    user_id: member.id,
                    provider: social.provider.clone(),
                    provider_id: social.id.clone(),
                    created_at: Utc::now(),
                })
                .await?;
            Ok(())
        }

        async fn create_user(&self, social: &SocialUser) -> Result<Member> {
            let email = social.email.clone().unwrap_or_default().trim().to_lowercase();
            let name = social
                .name
                .clone()
                .or_else(|| social.nickname.clone())
                .unwrap_or_else(|| email.clone());

            // No password: this account signs in through the provider. An
            // empty hash is what `Member::auth_password_hash` reads as none,
            // and a password reset is how its owner adds one later.
            let user = self.users.create(User::new(name, email, String::new())).await?;
            let mut member = Member::new(user);

            // The provider already proved the address — not asked twice.
            if social.email_verified == Some(true) {
                self.mark_email_verified(&member).await?;
                member.email_verified_at = Some(Utc::now());
            }
            welcome(&member.user).await;
            Ok(member)
        }
    }
}
