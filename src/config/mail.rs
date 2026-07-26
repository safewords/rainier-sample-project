//! `config/mail.php`.

use rainier_framework::config::{Config, Env};
use rainier_framework::prelude::*;

/// Mail settings.
///
/// The framework already read `MAIL_DRIVER`, `MAIL_FROM` and `MAIL_FROM_NAME`;
/// this adds what is specific to this application.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // Where the `file` transport writes its `.eml` files.
    config.set("mail.file_path", env.string("MAIL_FILE_PATH", "storage/mail"))?;

    // Set this in staging and leave it set: every message goes here instead of
    // to its real recipients. The difference between testing a flow against a
    // copy of production data and emailing all of those customers.
    if let Some(address) = env.get("MAIL_ALWAYS_TO").filter(|a| !a.trim().is_empty()) {
        config.set("mail.always_to", address)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_path_has_a_default() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.string("mail.file_path").as_deref(), Some("storage/mail"));
    }

    #[test]
    fn always_to_is_absent_unless_asked_for() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert!(!config.has("mail.always_to"), "redirecting all mail must be deliberate");
    }

    #[test]
    fn always_to_is_read_when_set() {
        let config = Config::new();
        configure(&config, &Env::parse("MAIL_ALWAYS_TO=dev@example.com")).unwrap();

        assert_eq!(config.string("mail.always_to").as_deref(), Some("dev@example.com"));
    }

    #[test]
    fn a_blank_always_to_is_not_a_redirect() {
        // `MAIL_ALWAYS_TO=` in a `.env` must not redirect every message to the
        // empty address, which would silently stop all mail.
        let config = Config::new();
        configure(&config, &Env::parse("MAIL_ALWAYS_TO=")).unwrap();

        assert!(!config.has("mail.always_to"));
    }
}
