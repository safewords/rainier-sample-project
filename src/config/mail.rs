//! `config/mail.rs` — every mail setting this application can adjust.
//!
//! The framework owns the keys (they live in `rainier_framework::keys`, all
//! under `mail.*`); this section is where the application reads them from its
//! environment, in one screen, with the defaults it wants. The transport
//! itself is built from these values by `rainier_framework::mail::transport`
//! — see `app/providers/app_provider.rs`.
//!
//! ```env
//! MAIL_DRIVER=smtp      # log | file | memory | smtp | ses | postmark | mailgun | sendgrid | resend
//! ```
//!
//! The senders are behind cargo features (`mail-smtp`, `mail-ses`, …— see
//! `Cargo.toml`); selecting one the build did not enable fails the boot
//! naming the feature. For development against a real SMTP conversation:
//!
//! ```sh
//! docker run --rm -p 1025:1025 -p 8025:8025 axllent/mailpit
//! ```
//!
//! with `MAIL_DRIVER=smtp`, `MAIL_HOST=localhost`, `MAIL_PORT=1025`,
//! `MAIL_ENCRYPTION=none`.

use rainier_framework::config::{Config, Env};
use rainier_framework::keys::{
    MAIL_ALWAYS_TO, MAIL_DRIVER, MAIL_ENCRYPTION, MAIL_FILE_PATH, MAIL_FROM_ADDRESS,
    MAIL_FROM_NAME, MAIL_HOST, MAIL_MAILGUN_DOMAIN, MAIL_MAILGUN_ENDPOINT, MAIL_MAILGUN_SECRET,
    MAIL_PASSWORD, MAIL_PORT, MAIL_POSTMARK_TOKEN, MAIL_RESEND_KEY, MAIL_SENDGRID_KEY,
    MAIL_TIMEOUT, MAIL_USERNAME,
};
use rainier_framework::prelude::*;

/// Mail settings, read by the provider when it builds the mailer.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // A `MailDriver`, not a string: `MAIL_DRIVER=smpt` fails here, naming the
    // variable and listing the valid values — rather than booting on the log
    // driver and delivering nothing until somebody asks where the mail went.
    config.set(MAIL_DRIVER, env.setting::<MailDriver>("MAIL_DRIVER")?)?;

    // Who messages come from when a mailable does not say.
    config.set(MAIL_FROM_ADDRESS, env.string("MAIL_FROM", "hello@example.com"))?;
    config.set(MAIL_FROM_NAME, env.string("MAIL_FROM_NAME", "Rainier Sample"))?;

    // Where the `file` transport writes its `.eml` files.
    config.set(MAIL_FILE_PATH, env.string("MAIL_FILE_PATH", "storage/mail"))?;

    // The SMTP conversation. The port default of 0 means "whatever the
    // encryption arrangement conventionally uses" — 587 for `starttls`, 465
    // for `tls`, 25 for `none`. `starttls` here is a *required* upgrade, and
    // `none` is for a capture container on localhost, nothing else.
    config.set(MAIL_HOST, env.string("MAIL_HOST", ""))?;
    config.set(MAIL_PORT, env.int("MAIL_PORT", 0))?;
    config.set(MAIL_USERNAME, env.string("MAIL_USERNAME", ""))?;
    config.set(MAIL_PASSWORD, env.string("MAIL_PASSWORD", ""))?;
    config.set(MAIL_ENCRYPTION, env.setting::<MailEncryption>("MAIL_ENCRYPTION")?)?;
    config.set(MAIL_TIMEOUT, env.int("MAIL_TIMEOUT", 30))?;

    // The API providers, one credential each. The `ses` driver has no entry
    // here on purpose: region and credentials come from the AWS default
    // chain, exactly as the other AWS drivers resolve theirs.
    config.set(MAIL_POSTMARK_TOKEN, env.string("MAIL_POSTMARK_TOKEN", ""))?;
    config.set(MAIL_MAILGUN_DOMAIN, env.string("MAIL_MAILGUN_DOMAIN", ""))?;
    config.set(MAIL_MAILGUN_SECRET, env.string("MAIL_MAILGUN_SECRET", ""))?;
    config.set(MAIL_MAILGUN_ENDPOINT, env.string("MAIL_MAILGUN_ENDPOINT", ""))?;
    config.set(MAIL_SENDGRID_KEY, env.string("MAIL_SENDGRID_KEY", ""))?;
    config.set(MAIL_RESEND_KEY, env.string("MAIL_RESEND_KEY", ""))?;

    // Set this in staging and leave it set: every message goes here instead
    // of to its real recipients. The difference between testing a flow
    // against a copy of production data and emailing all of those customers.
    // Absent unless non-empty, so `MAIL_ALWAYS_TO=` in a `.env` cannot
    // silently redirect all mail to the empty address.
    if let Some(address) = env.get("MAIL_ALWAYS_TO").filter(|a| !a.trim().is_empty()) {
        config.set(MAIL_ALWAYS_TO, address)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_driver_cannot_mail_real_people() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        // The property that makes a forgotten `MAIL_DRIVER` harmless.
        assert!(!config.setting(MAIL_DRIVER).unwrap().delivers());
    }

    #[test]
    fn a_misspelled_driver_stops_the_boot_listing_the_choices() {
        let err = configure(&Config::new(), &Env::parse("MAIL_DRIVER=smpt")).unwrap_err();

        assert!(err.message().contains("MAIL_DRIVER"), "{}", err.message());
        assert!(err.message().contains("`smtp`"), "{}", err.message());
    }

    #[test]
    fn the_smtp_settings_read_from_the_environment() {
        let config = Config::new();
        configure(
            &config,
            &Env::parse(
                "MAIL_DRIVER=smtp\nMAIL_HOST=smtp.example.com\nMAIL_PORT=2525\n\
                 MAIL_USERNAME=postmaster\nMAIL_ENCRYPTION=tls",
            ),
        )
        .unwrap();

        assert_eq!(config.setting(MAIL_DRIVER).unwrap(), MailDriver::Smtp);
        assert_eq!(config.get(MAIL_HOST).as_deref(), Some("smtp.example.com"));
        assert_eq!(config.get(MAIL_PORT), Some(2525));
        assert_eq!(config.setting(MAIL_ENCRYPTION).unwrap(), MailEncryption::Tls);
    }

    #[test]
    fn encryption_defaults_to_required_starttls() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.setting(MAIL_ENCRYPTION).unwrap(), MailEncryption::StartTls);
    }

    #[test]
    fn the_file_path_has_a_default() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.get(MAIL_FILE_PATH).as_deref(), Some("storage/mail"));
    }

    #[test]
    fn always_to_is_absent_unless_asked_for() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert!(!config.has(MAIL_ALWAYS_TO), "redirecting all mail must be deliberate");
    }

    #[test]
    fn always_to_is_read_when_set() {
        let config = Config::new();
        configure(&config, &Env::parse("MAIL_ALWAYS_TO=dev@example.com")).unwrap();

        assert_eq!(config.get(MAIL_ALWAYS_TO).as_deref(), Some("dev@example.com"));
    }

    #[test]
    fn a_blank_always_to_is_not_a_redirect() {
        // `MAIL_ALWAYS_TO=` in a `.env` must not redirect every message to the
        // empty address, which would silently stop all mail.
        let config = Config::new();
        configure(&config, &Env::parse("MAIL_ALWAYS_TO=")).unwrap();

        assert!(!config.has(MAIL_ALWAYS_TO));
    }
}
