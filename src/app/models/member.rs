//! `Member` — a [`User`] as the account add-ins see one.
//!
//! The account flows ask questions of a user that the `users` row cannot
//! answer on its own — *is this address verified?*, *who is their Stripe
//! customer?* — and they ask them synchronously, on a value already in hand.
//! So the answers are loaded **with** the user, once, by
//! [`AccountStore`](crate::app::repositories::AccountStore), and carried here.
//!
//! # A second user type, deliberately
//!
//! The JSON API authenticates a [`User`] with a bearer token, through the
//! `api` guard, and nothing about that changes. The browser session
//! authenticates a `Member`, through the `web` guard. They are one row read two
//! ways, and the framework's auth manager is generic over the type it hands
//! back — so each gets its own manager rather than one manager returning
//! whichever shape a caller hoped for.

use std::ops::Deref;

use chrono::{DateTime, Utc};
use rainier_framework::accounts::AccountUser;
use rainier_framework::auth::Authenticatable;
use serde::Serialize;

use crate::app::models::User;

/// A user, with what the account add-ins know about them.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Member {
    /// The row itself.
    #[serde(flatten)]
    pub user: User,

    /// When they verified their address, if they have.
    pub email_verified_at: Option<DateTime<Utc>>,

    /// Their Stripe customer, once they have been to checkout.
    #[serde(skip)]
    pub stripe_customer_id: Option<String>,
}

impl Member {
    /// A user nobody has verified or billed yet.
    pub fn new(user: User) -> Self {
        Self { user, email_verified_at: None, stripe_customer_id: None }
    }
}

impl Deref for Member {
    type Target = User;

    fn deref(&self) -> &User {
        &self.user
    }
}

impl Authenticatable for Member {
    fn auth_identifier(&self) -> String {
        self.user.auth_identifier()
    }

    fn auth_password_hash(&self) -> Option<&str> {
        // An account created by "sign in with GitHub" has no password, and an
        // empty hash is how it is stored — see `AccountStore::create_user`.
        Some(self.user.password.as_str()).filter(|hash| !hash.is_empty())
    }
}

impl AccountUser for Member {
    fn email(&self) -> &str {
        &self.user.email
    }

    fn has_verified_email(&self) -> bool {
        self.email_verified_at.is_some()
    }
}

#[cfg(feature = "billing-stripe")]
impl rainier_framework::billing::Billable for Member {
    fn stripe_customer_id(&self) -> Option<String> {
        self.stripe_customer_id.clone()
    }

    fn billing_email(&self) -> Option<String> {
        Some(self.user.email.clone())
    }

    fn billing_name(&self) -> Option<String> {
        Some(self.user.name.clone())
    }

    fn billing_reference(&self) -> String {
        self.user.id.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_is_verified_only_once_a_date_is_recorded() {
        let mut member = Member::new(User::new("Ada", "ada@example.com", "hash".into()));
        assert!(!member.has_verified_email());

        member.email_verified_at = Some(Utc::now());
        assert!(member.has_verified_email());
    }

    #[test]
    fn an_account_with_no_password_cannot_log_in_with_one() {
        let member = Member::new(User::new("Ada", "ada@example.com", String::new()));
        assert_eq!(member.auth_password_hash(), None);
    }

    #[test]
    fn the_password_hash_never_reaches_the_json() {
        let member = Member::new(User::new("Ada", "ada@example.com", "$argon2id$x".into()));
        let json = serde_json::to_string(&member).unwrap();

        assert!(json.contains("ada@example.com"), "{json}");
        assert!(!json.contains("argon2"), "{json}");
    }
}
