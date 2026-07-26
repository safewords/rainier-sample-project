//! `config/mail.php`.

use rainier_framework::config::{Config, Env};
use rainier_framework::prelude::*;

use crate::config::keys::{MAIL_ALWAYS_TO, MAIL_FILE_PATH};

/// Mail settings.
///
/// The framework already read `MAIL_DRIVER`, `MAIL_FROM` and `MAIL_FROM_NAME`;
/// this adds what is specific to this application.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // Where the `file` transport writes its `.eml` files.
    config.set(MAIL_FILE_PATH, env.string("MAIL_FILE_PATH", "storage/mail"))?;

    // Set this in staging and leave it set: every message goes here instead of
    // to its real recipients. The difference between testing a flow against a
    // copy of production data and emailing all of those customers.
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
        // The framework sets `mail.driver`; this pins the property that makes
        // a forgotten `MAIL_DRIVER` harmless.
        assert!(!MailDriver::default().delivers());
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
