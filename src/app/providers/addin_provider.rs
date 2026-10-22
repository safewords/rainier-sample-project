//! `AddInServiceProvider` — the add-ins, assembled.
//!
//! Every add-in this application turned on, built in one place from
//! [`Services`] and bound into the container. The cargo features decide what
//! is here at all; the environment decides what is *configured* — a build
//! with `sso` and no `GITHUB_CLIENT_ID` has the sign-in routes and no GitHub.
//!
//! # Built before boot, not in `register`
//!
//! Everything else in `app/providers` binds factories and lets the container
//! build on first resolve. The add-ins cannot wait for that, because their
//! **routes** hold them: `accounts.routes(router)` mounts closures over the
//! `Accounts` it is called on, and routes are declared before any provider
//! runs. So [`AddIns::build`] runs in `bootstrap.rs`, beside the database and
//! the cache it needs, and this provider only hands the finished pieces to
//! the container for controllers and middleware to resolve.
//!
//! # Their own event bus
//!
//! The account flows and the Stripe webhooks raise events — `Registered`,
//! `CustomerSubscriptionUpdated` — onto a dispatcher they are given when they
//! are built. The application's dispatcher does not exist yet at that point,
//! so the add-ins get [`AddIns::events`], and the listeners that care
//! register on it here: the subscription copy is kept current by one.

use std::sync::Arc;

use rainier_framework::accounts::{
    Accounts, Features, ResetPasswordMail, SendsPasswordResetLinks, SendsVerificationLinks,
    VerifyEmailMail,
};
use rainier_framework::auth::{AuthManager, Hasher, Impersonation, SessionGuard, UserProvider};
use rainier_framework::cache::Cache;
use rainier_framework::crypt::hash::{HashDriver, HashManager};
use rainier_framework::crypt::{Encryption, Key, KeyRing, UrlSigner};
use rainier_framework::events::Dispatcher;
use rainier_framework::prelude::*;

use crate::app::models::Member;
use crate::app::repositories::AccountStore;
use crate::bootstrap::Mode;
use crate::config::services::Services;

/// Every configured add-in, ready to mount.
pub struct AddIns {
    /// What they were built from.
    pub services: Arc<Services>,
    /// The tables behind every account port.
    pub store: Arc<AccountStore>,
    /// The browser's guard, named `web`.
    pub guard: Arc<SessionGuard<Member>>,
    /// The manager the `web` guard is resolved through.
    pub auth: Arc<AuthManager<Member>>,
    /// Registration, login, reset, verification — and the second factor.
    pub accounts: Arc<Accounts<Member>>,
    /// The add-ins' event bus — see the module docs.
    pub events: Arc<Dispatcher>,
    /// Who may act as whom: administrators, as anybody who is not one.
    pub impersonation: Arc<Impersonation<Member>>,

    /// Third-party calls. Under test, the fake a feature test queues answers on.
    #[cfg(any(feature = "billing-stripe", feature = "sso", feature = "captcha"))]
    pub outbound: super::outbound::Outbound,

    /// The human check on registration — `None` when no secret is configured.
    #[cfg(feature = "captcha")]
    pub captcha: Option<Arc<rainier_framework::Captcha>>,
    /// The captcha's own fake, under test.
    #[cfg(feature = "captcha")]
    pub captcha_fake: Option<super::outbound::CaptchaFake>,

    /// The sign-in providers that are configured.
    #[cfg(feature = "sso")]
    pub sso: rainier_framework::sso::Sso,

    /// Stripe, when a key is configured.
    #[cfg(feature = "billing-stripe")]
    pub billing: Option<Billing>,

    /// The OAuth server and its tables.
    #[cfg(feature = "oauth-server")]
    pub oauth: OAuth,

    /// What the assembly would have warned about. Logged by `bootstrap.rs`
    /// once the application has booted: the add-ins are built before the
    /// log subscriber is installed, so a warning raised here would go
    /// nowhere — and these are exactly the ones worth seeing.
    pub notices: Vec<String>,
}

/// What the billing routes need.
#[cfg(feature = "billing-stripe")]
#[derive(Clone)]
pub struct Billing {
    /// The API client.
    pub stripe: Arc<rainier_framework::billing::Stripe>,
    /// The webhook receiver, when a signing secret is configured.
    pub webhooks: Option<Arc<rainier_framework::billing::Webhooks>>,
    /// The local copy of every subscription.
    pub subscriptions: Arc<crate::app::repositories::SubscriptionRepository>,
    /// The price a checkout subscribes to.
    pub price: String,
}

/// What the OAuth routes and the `oauth` guard need.
#[cfg(feature = "oauth-server")]
#[derive(Clone)]
pub struct OAuth {
    /// The server.
    pub server: Arc<rainier_framework::oauth_server::AuthorizationServer>,
    /// Its tables — also what `oauth:client` registers clients in.
    pub store: Arc<crate::app::repositories::DatabaseOAuthStore>,
}

/// The webhook secret under test, so a feature test can sign a delivery.
#[cfg(feature = "billing-stripe")]
pub const TEST_WEBHOOK_SECRET: &str = "whsec_sample_test";

impl AddIns {
    /// Build every add-in the features turned on.
    pub fn build(
        mode: Mode,
        env: &rainier_framework::config::Env,
        settings: &rainier_framework::config::Config,
        services: Arc<Services>,
        database: &Database,
        cache: Arc<dyn Cache>,
        #[cfg(any(feature = "billing-stripe", feature = "sso", feature = "captcha"))]
        outbound: super::outbound::Outbound,
    ) -> Result<Self> {
        // The same key the framework encrypts with. Without one, a random key
        // per boot — which signs nobody out only because there is nobody to
        // sign out on a fresh clone. `APP_KEY` is the fix, as it is for every
        // encrypted thing.
        let keys = KeyRing::from_base64(&env.string("APP_KEY", ""), &[])
            .unwrap_or_else(|_| KeyRing::new(Key::generate()));
        let crypt = Arc::new(Encryption::from_keys(keys.clone()));

        let hasher: Arc<dyn Hasher> = Arc::new(match mode {
            Mode::Testing => HashManager::insecure_for_tests(HashDriver::Argon2id)?,
            Mode::Running => HashManager::new(settings.setting(crate::config::keys::HASH_DRIVER)?)?,
        });

        let store = Arc::new(AccountStore::new(database.clone(), Arc::clone(&hasher), crypt));
        let guard =
            Arc::new(SessionGuard::new("web", Arc::clone(&store) as Arc<dyn UserProvider<Member>>));
        let auth = Arc::new(AuthManager::<Member>::new("web").register(guard.clone()));
        let events = Arc::new(Dispatcher::new());

        let links = Arc::new(MailedLinks { app_name: services.app_name.clone() });
        #[allow(unused_mut, reason = "two-factor and passkeys add to it")]
        let mut accounts = Accounts::new(
            guard.clone(),
            Arc::clone(&store) as _,
            Arc::clone(&hasher),
            Arc::clone(&cache),
        )
        .features(Features {
            // Mounted by `routes/addins.rs` instead, where the captcha can sit
            // in front of it. Every other flow is the scaffolding's own route.
            registration: false,
            email_verification: true,
            two_factor: cfg!(feature = "two-factor"),
            passkeys: cfg!(feature = "passkeys"),
            ..Features::default()
        })
        .creates_users_with(Arc::clone(&store) as _)
        .sends_reset_links_with(Arc::clone(&links) as _)
        .sends_verification_links_with(links as _)
        .signs_urls_with(Arc::new(UrlSigner::new(keys.clone())))
        .events(Arc::clone(&events))
        .app_url(services.app_url.clone())
        .prefix("account")
        .names("accounts.")
        .home("/")
        // Under test the outbox is the memory mail transport, and a test wants
        // the link in it before the request returns.
        .deliver_links_inline(mode == Mode::Testing)
        .middleware(crate::app::http::kernel::browser(&services));

        #[cfg(feature = "two-factor")]
        {
            accounts = accounts.two_factor_with(
                Arc::clone(&store) as _,
                rainier_framework::mfa::Totp::new().issuer(services.app_name.clone()),
            );
        }
        #[cfg(feature = "passkeys")]
        {
            accounts = accounts.passkeys_with(
                Arc::clone(&store) as _,
                rainier_framework::mfa::RelyingParty::new(
                    services.host(),
                    services.app_name.clone(),
                )
                .origins([services.app_url.clone()]),
            );
        }

        let impersonation = {
            let services = Arc::clone(&services);
            Arc::new(
                Impersonation::new(move |staff: &Member, target: &Member| {
                    services.is_admin(&staff.email) && !services.is_admin(&target.email)
                })
                .on_taken(|staff, target| tracing::warn!(staff, target, "impersonation began"))
                .on_left(|staff, target| tracing::warn!(staff, target, "impersonation ended")),
            )
        };

        #[allow(unused_mut, reason = "only some add-ins have anything to say")]
        let mut notices = Vec::new();

        Ok(Self {
            #[cfg(feature = "captcha")]
            captcha_fake: (mode == Mode::Testing).then(|| {
                super::outbound::CaptchaFake(Arc::new(
                    rainier_framework::http_client::FakeTransport::new(),
                ))
            }),
            #[cfg(feature = "captcha")]
            captcha: None,
            #[cfg(feature = "sso")]
            sso: sso(mode, &services, &outbound)?,
            #[cfg(feature = "billing-stripe")]
            billing: billing(mode, &services, &outbound, database, &events, &cache, &mut notices),
            #[cfg(feature = "oauth-server")]
            oauth: oauth(mode, &services, database, &hasher, &keys, &mut notices)?,
            #[cfg(any(feature = "billing-stripe", feature = "sso", feature = "captcha"))]
            outbound,
            services,
            store,
            guard,
            auth,
            accounts: Arc::new(accounts),
            events,
            impersonation,
            notices,
        }
        .with_captcha())
    }

    /// The captcha, over its own fake under test or the real transport when
    /// a secret is configured.
    fn with_captcha(self) -> Self {
        #[cfg(feature = "captcha")]
        {
            let mut this = self;
            let transport: Option<(Arc<dyn rainier_framework::http_client::Transport>, String)> =
                match (&this.captcha_fake, &this.services.captcha_secret) {
                    (Some(fake), _) => Some((Arc::clone(&fake.0) as _, "test-secret".into())),
                    (None, Some(secret)) => Some((Arc::clone(&this.outbound.http), secret.clone())),
                    (None, None) => None,
                };
            this.captcha = transport.map(|(http, secret)| {
                let mut captcha = rainier_framework::Captcha::turnstile(http, secret);
                if let Some(hostname) = &this.services.captcha_hostname {
                    captcha = captcha.expect_hostname(hostname.clone());
                }
                Arc::new(captcha.expect_action("register"))
            });
            if this.captcha.is_none() {
                this.notices.push(
                    "the captcha feature is on but CAPTCHA_SECRET is empty: registration is unchecked"
                        .into(),
                );
            }
            this
        }
        #[cfg(not(feature = "captcha"))]
        self
    }
}

/// Sign-in providers, from whichever credentials are configured.
///
/// Under test GitHub is always declared, with throwaway credentials, so the
/// round trip can be driven against the fake.
#[cfg(feature = "sso")]
fn sso(
    mode: Mode,
    services: &Services,
    outbound: &super::outbound::Outbound,
) -> Result<rainier_framework::sso::Sso> {
    use serde_json::{json, Map, Value};

    let callback = |name: &str| format!("{}/auth/{name}/callback", services.app_url);
    let mut providers = Map::new();

    let github = match mode {
        Mode::Testing => Some(("test-client".to_string(), "test-secret".to_string())),
        Mode::Running => services.github.clone(),
    };
    if let Some((id, secret)) = github {
        providers.insert(
            "github".into(),
            json!({ "client_id": id, "client_secret": secret, "redirect": callback("github") }),
        );
    }
    if let Some((name, issuer, id, secret)) = &services.oidc {
        providers.insert(
            name.clone(),
            json!({
                "driver": "oidc",
                "issuer": issuer,
                "client_id": id,
                "client_secret": secret,
                "redirect": callback(name),
            }),
        );
    }

    let config: rainier_framework::sso::SsoConfig =
        serde_json::from_value(Value::Object(providers))?;
    rainier_framework::sso::Sso::from_config(&config, Arc::clone(&outbound.http))
}

/// Stripe, when there is a key — and the webhook listener that keeps the
/// local copy of each subscription current.
#[cfg(feature = "billing-stripe")]
fn billing(
    mode: Mode,
    services: &Services,
    outbound: &super::outbound::Outbound,
    database: &Database,
    events: &Arc<Dispatcher>,
    cache: &Arc<dyn Cache>,
    notices: &mut Vec<String>,
) -> Option<Billing> {
    use rainier_framework::billing::{
        CustomerSubscriptionCreated, CustomerSubscriptionDeleted, CustomerSubscriptionUpdated,
        Stripe, WebhookVerifier, Webhooks,
    };

    let (secret, webhook_secret) = match mode {
        Mode::Testing => ("sk_test_sample".to_string(), Some(TEST_WEBHOOK_SECRET.to_string())),
        Mode::Running => match &services.stripe_secret {
            Some(secret) => (secret.clone(), services.stripe_webhook_secret.clone()),
            None => {
                notices.push("STRIPE_SECRET is empty: the billing routes answer 404".into());
                return None;
            }
        },
    };

    let subscriptions =
        Arc::new(crate::app::repositories::SubscriptionRepository::new(database.clone()));

    // Created, updated, deleted: one listener each, all writing the same row.
    // A deletion is recorded rather than removed — its snapshot says
    // `canceled`, which is the answer the access check should give.
    macro_rules! keep_current {
        ($event:ty) => {{
            let subscriptions = Arc::clone(&subscriptions);
            events.listen(move |event: Arc<$event>| {
                let subscriptions = Arc::clone(&subscriptions);
                async move { subscriptions.record(&event.snapshot()).await }
            });
        }};
    }
    keep_current!(CustomerSubscriptionCreated);
    keep_current!(CustomerSubscriptionUpdated);
    keep_current!(CustomerSubscriptionDeleted);

    let webhooks = webhook_secret.map(|secret| {
        Arc::new(
            Webhooks::new(WebhookVerifier::new(secret), Arc::clone(events))
                // Stripe delivers at least once. The shared cache is what makes
                // a redelivery to a second replica a duplicate rather than a
                // second write.
                .deduplicate_with(Arc::clone(cache)),
        )
    });
    if webhooks.is_none() {
        notices
            .push("STRIPE_WEBHOOK_SECRET is empty: subscriptions will not be kept current".into());
    }

    Some(Billing {
        stripe: Arc::new(Stripe::new(Arc::clone(&outbound.http), secret)),
        webhooks,
        subscriptions,
        price: services.stripe_price.clone(),
    })
}

/// The OAuth server, over this application's `oauth_*` tables.
///
/// Refresh tokens and codes are sealed with the application key, so a
/// restart does not void every refresh token in circulation.
#[cfg(feature = "oauth-server")]
fn oauth(
    mode: Mode,
    services: &Services,
    database: &Database,
    hasher: &Arc<dyn Hasher>,
    keys: &KeyRing,
    notices: &mut Vec<String>,
) -> Result<OAuth> {
    use rainier_framework::crypt::AeadEncrypter;
    use rainier_framework::oauth_server::{
        AccessTokenSigner, AuthorizationServer, EncrypterSealer, OAuthStore, Scopes,
    };

    let signer =
        match &services.oauth_private_key {
            Some(key) if key.contains("-----BEGIN") => AccessTokenSigner::from_pem(key)?,
            Some(path) => {
                AccessTokenSigner::from_pem(&std::fs::read_to_string(path).map_err(|e| {
                    Error::internal(format!(
                        "OAUTH_PRIVATE_KEY names `{path}`, which could not be read: {e}"
                    ))
                })?)?
            }
            // A key per boot outside production: every token dies with the
            // process, which is fine for a laptop and nothing else.
            None if !services.production || mode == Mode::Testing => {
                if mode == Mode::Running {
                    notices.push(
                        "OAUTH_PRIVATE_KEY is empty: signing tokens with a key generated for \
                         this process — every token is void after a restart"
                            .into(),
                    );
                }
                AccessTokenSigner::generate(2048)?
            }
            None => return Err(Error::internal(
                "the oauth-server feature is on and OAUTH_PRIVATE_KEY is empty: production needs \
                 a stable signing key — `openssl genrsa 2048`",
            )),
        }
        .issued_by(services.app_url.clone());

    let store = Arc::new(crate::app::repositories::DatabaseOAuthStore::new(database.clone()));
    let sealer = Arc::new(EncrypterSealer::new(Arc::new(AeadEncrypter::new(keys.clone()))));

    let server = AuthorizationServer::new(
        Arc::clone(&store) as Arc<dyn OAuthStore>,
        sealer,
        signer,
        Arc::clone(hasher),
    )
    .scopes(
        Scopes::new()
            .with("account", "Read your account")
            .with("posts", "Write and publish posts as you"),
    );

    Ok(OAuth { server: Arc::new(server), store })
}

/// Reset and verification links, sent as mail through the application's
/// mailer.
///
/// The scaffolding ships a sender over a `Mailer` it is handed, and this one
/// differs in exactly one way: it **resolves** the mailer when it sends. The
/// account flows are assembled before boot, and the mailer is built during it
/// from the views and `config/mail.rs` — so there is no mailer to hand over
/// yet. Resolving per send also means a test's captured transport is the one
/// that receives.
pub struct MailedLinks {
    app_name: String,
}

fn mailer() -> Result<Arc<rainier_framework::mail::Mailer>> {
    rainier_framework::container::facade_application().resolve()
}

#[async_trait]
impl SendsPasswordResetLinks<Member> for MailedLinks {
    async fn send_reset_link(&self, member: &Member, url: &str) -> Result<()> {
        mailer()?
            .send(&ResetPasswordMail {
                to: member.email.clone(),
                url: url.to_string(),
                app_name: self.app_name.clone(),
                expires_in_minutes: 60,
            })
            .await
            .map(drop)
    }
}

#[async_trait]
impl SendsVerificationLinks<Member> for MailedLinks {
    async fn send_verification_link(&self, member: &Member, url: &str) -> Result<()> {
        mailer()?
            .send(&VerifyEmailMail {
                to: member.email.clone(),
                url: url.to_string(),
                app_name: self.app_name.clone(),
            })
            .await
            .map(drop)
    }
}

/// Hands the assembled add-ins to the container.
pub struct AddInServiceProvider {
    /// What `bootstrap.rs` built.
    pub addins: Arc<AddIns>,
}

impl ServiceProvider for AddInServiceProvider {
    fn name(&self) -> &'static str {
        "AddInServiceProvider"
    }

    fn register(&self, app: &Application) -> Result<()> {
        let addins = &self.addins;

        // What `kernel::web_auth()` and `AuthenticatedUser<Member>` resolve.
        app.instance_arc(Arc::clone(&addins.auth));
        app.instance_arc(Arc::clone(&addins.store));
        app.instance_arc(Arc::clone(addins));

        // The captcha's own fake, so a feature test can queue its answers.
        #[cfg(feature = "captcha")]
        if let Some(fake) = &addins.captcha_fake {
            app.instance(fake.clone());
        }

        Ok(())
    }
}
