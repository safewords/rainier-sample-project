//! Third-party services and the add-ins' own settings.
//!
//! Read once, at boot, into one struct — the shape the framework's own
//! `MetricsSettings` and `TelemetrySettings` take — because the add-ins are
//! assembled in `bootstrap.rs` **before** the application exists: their routes
//! are declared then, and a route needs its handler's dependencies in hand.
//! So these are not keys in the config tree for a controller to look up later;
//! they are what the wiring is built from.
//!
//! Every credential is optional. A service with none configured is simply not
//! wired — "sign in with GitHub" is absent rather than broken — except where a
//! missing one would be a security decision made silently, which
//! [`Services::check`] refuses in production.

use rainier_framework::config::Env;
use rainier_framework::prelude::*;

/// Everything the add-ins are configured from.
#[derive(Clone, PartialEq)]
pub struct Services {
    /// What the application calls itself, in mail subjects and on the
    /// authenticator app.
    pub app_name: String,
    /// Where it is served from — the origin passkeys are bound to, and the base
    /// of every link mailed out.
    pub app_url: String,
    /// Whether this is production, where some missing settings are refused.
    pub production: bool,
    /// Who may impersonate other users. Addresses, compared case-insensitively.
    pub admin_emails: Vec<String>,

    /// A GitHub OAuth app: `(client id, client secret)`.
    pub github: Option<(String, String)>,
    /// An OpenID Connect issuer: `(name, issuer URL, client id, client secret)`.
    pub oidc: Option<(String, String, String, String)>,

    /// Stripe's secret key, `sk_…`.
    pub stripe_secret: Option<String>,
    /// The webhook endpoint's signing secret, `whsec_…`.
    pub stripe_webhook_secret: Option<String>,
    /// The price a checkout subscribes to, `price_…`.
    pub stripe_price: String,

    /// The captcha provider's secret.
    pub captcha_secret: Option<String>,
    /// The hostname a solved captcha must report — a token solved on somebody
    /// else's site is not one for ours.
    pub captcha_hostname: Option<String>,

    /// A Discord webhook published posts are announced to.
    pub discord_webhook: Option<String>,

    /// The OAuth server's signing key, as PEM — the contents, or a path to it.
    pub oauth_private_key: Option<String>,

    /// The PHP application's `APP_KEY`, when running beside one.
    pub php_app_key: Option<String>,
    /// The PHP application's session cookie name.
    pub php_session_cookie: String,
    /// This application's own session cookie — `SESSION_COOKIE`, as
    /// `bootstrap.rs` reads it.
    pub session_cookie: String,
}

/// Read `name`, treating an empty value as unset — `FOO=` in an `.env` is
/// how a template says "fill this in", not a credential.
fn optional(env: &Env, name: &str) -> Option<String> {
    env.get(name).map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

/// The Discord webhook under test — a Discord-shaped URL the fake transport
/// answers, so a test can assert on what would have been posted.
pub const TEST_DISCORD_WEBHOOK: &str = "https://discord.com/api/webhooks/1/sample-test";

/// The administrator under test.
pub const TEST_ADMIN: &str = "admin@example.com";

/// The PHP application's key under test — 32 bytes of `0x5a`, base64.
#[cfg(feature = "compat-php")]
pub const TEST_PHP_APP_KEY: &str = "base64:WlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlo=";

impl Services {
    /// The services for `mode`: the environment's when running; under test,
    /// a fixed origin and a test webhook, and never "production" — a test
    /// suite should not refuse to boot over a key it was never meant to have.
    pub fn for_mode(mode: crate::bootstrap::Mode, env: &Env) -> Self {
        let mut services = Self::from_env(env);
        if mode == crate::bootstrap::Mode::Testing {
            services.production = false;
            services.app_url = "https://app.test".into();
            services.discord_webhook = Some(TEST_DISCORD_WEBHOOK.into());
            services.admin_emails = vec![TEST_ADMIN.into()];
            #[cfg(feature = "compat-php")]
            {
                services.php_app_key = Some(TEST_PHP_APP_KEY.into());
                services.php_session_cookie = "php_session".into();
            }
        }
        services
    }

    /// Read them from the environment.
    pub fn from_env(env: &Env) -> Self {
        let app_url = env.string("APP_URL", "http://localhost:8000");

        Self {
            app_name: env.string("APP_NAME", "Rainier Sample"),
            app_url: app_url.trim_end_matches('/').to_string(),
            production: env.string("APP_ENV", "production") == "production",
            admin_emails: env
                .string("ADMIN_EMAILS", "")
                .split(',')
                .map(|email| email.trim().to_lowercase())
                .filter(|email| !email.is_empty())
                .collect(),

            github: optional(env, "GITHUB_CLIENT_ID").zip(optional(env, "GITHUB_CLIENT_SECRET")),
            oidc: match (
                optional(env, "OIDC_ISSUER"),
                optional(env, "OIDC_CLIENT_ID"),
                optional(env, "OIDC_CLIENT_SECRET"),
            ) {
                (Some(issuer), Some(id), Some(secret)) => {
                    Some((env.string("OIDC_NAME", "work"), issuer, id, secret))
                }
                _ => None,
            },

            stripe_secret: optional(env, "STRIPE_SECRET"),
            stripe_webhook_secret: optional(env, "STRIPE_WEBHOOK_SECRET"),
            stripe_price: env.string("STRIPE_PRICE", "price_pro_monthly"),

            captcha_secret: optional(env, "CAPTCHA_SECRET"),
            captcha_hostname: optional(env, "CAPTCHA_HOSTNAME"),

            discord_webhook: optional(env, "DISCORD_WEBHOOK_URL"),

            oauth_private_key: optional(env, "OAUTH_PRIVATE_KEY"),

            php_app_key: optional(env, "PHP_APP_KEY"),
            php_session_cookie: env.string("PHP_SESSION_COOKIE", "php_session"),
            session_cookie: env.string("SESSION_COOKIE", "rainier_session"),
        }
    }

    /// Whether `email` belongs to an administrator.
    pub fn is_admin(&self, email: &str) -> bool {
        let email = email.trim().to_lowercase();
        self.admin_emails.contains(&email)
    }

    /// The host passkeys are bound to — `APP_URL` without its scheme or port.
    pub fn host(&self) -> String {
        let without_scheme =
            self.app_url.split_once("://").map_or(&*self.app_url, |(_, rest)| rest);
        without_scheme.split(['/', ':']).next().unwrap_or("localhost").to_string()
    }

    /// Refuse the combinations that would quietly weaken something.
    ///
    /// Only in production. Locally a missing Stripe secret means "no billing
    /// today"; in production a missing *webhook* secret beside a real API key
    /// means accepting forged subscription updates, which is not a thing to
    /// find out about from an invoice.
    pub fn check(&self) -> Result<()> {
        if !self.production {
            return Ok(());
        }
        if self.stripe_secret.is_some() && self.stripe_webhook_secret.is_none() {
            return Err(Error::internal(
                "STRIPE_SECRET is set but STRIPE_WEBHOOK_SECRET is not: webhooks cannot be \
                 verified, so none would be accepted",
            ));
        }
        Ok(())
    }
}

impl std::fmt::Debug for Services {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Names and URLs only. Every secret here would otherwise reach a log
        // the first time somebody writes `{services:?}`.
        f.debug_struct("Services")
            .field("app_name", &self.app_name)
            .field("app_url", &self.app_url)
            .field("github", &self.github.is_some())
            .field("oidc", &self.oidc.as_ref().map(|(name, issuer, ..)| (name, issuer)))
            .field("stripe", &self.stripe_secret.is_some())
            .field("captcha", &self.captcha_secret.is_some())
            .field("discord", &self.discord_webhook.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_configured_by_default() {
        let services = Services::from_env(&Env::parse("").isolated());

        assert!(services.github.is_none());
        assert!(services.oidc.is_none());
        assert!(services.stripe_secret.is_none());
        assert!(services.admin_emails.is_empty());
    }

    #[test]
    fn an_empty_value_is_unset_rather_than_an_empty_secret() {
        let services =
            Services::from_env(&Env::parse("GITHUB_CLIENT_ID=\nGITHUB_CLIENT_SECRET=").isolated());
        assert!(services.github.is_none());
    }

    #[test]
    fn half_an_oidc_declaration_declares_nothing() {
        let services = Services::from_env(
            &Env::parse("OIDC_ISSUER=https://login.example.com\nOIDC_CLIENT_ID=app").isolated(),
        );
        assert!(services.oidc.is_none());
    }

    #[test]
    fn admins_are_matched_whatever_the_case() {
        let services = Services::from_env(
            &Env::parse("ADMIN_EMAILS=Ada@Example.com, grace@example.com").isolated(),
        );

        assert!(services.is_admin("ada@example.com"));
        assert!(services.is_admin(" GRACE@example.com "));
        assert!(!services.is_admin("linus@example.com"));
    }

    #[test]
    fn the_passkey_host_is_the_bare_hostname() {
        let services =
            Services::from_env(&Env::parse("APP_URL=https://app.example.com:8443/").isolated());
        assert_eq!(services.host(), "app.example.com");
        assert_eq!(services.app_url, "https://app.example.com:8443");
    }

    #[test]
    fn production_refuses_a_stripe_key_with_no_way_to_verify_webhooks() {
        let env = Env::parse("APP_ENV=production\nSTRIPE_SECRET=sk_live_x").isolated();
        assert!(Services::from_env(&env).check().is_err());

        let env = Env::parse("APP_ENV=local\nSTRIPE_SECRET=sk_test_x").isolated();
        assert!(Services::from_env(&env).check().is_ok());
    }

    #[test]
    fn secrets_stay_out_of_debug() {
        let env = Env::parse(
            "STRIPE_SECRET=sk_live_secret\nGITHUB_CLIENT_ID=id\nGITHUB_CLIENT_SECRET=gh_secret",
        )
        .isolated();
        let printed = format!("{:?}", Services::from_env(&env));

        assert!(!printed.contains("sk_live_secret"), "{printed}");
        assert!(!printed.contains("gh_secret"), "{printed}");
    }
}
