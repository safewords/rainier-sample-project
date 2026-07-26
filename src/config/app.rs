//! `config/app.php`.

use rainier_framework::config::{Config, Env};
use rainier_framework::prelude::*;

/// Application-wide settings.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    config.set("app.name", env.string("APP_NAME", "Rainier Sample"))?;
    config.set("app.locale", env.string("APP_LOCALE", "en"))?;

    // Encryption keys are read from `APP_KEY` by the framework rather than
    // from here, so a key never lands in a config value that something might
    // log. `APP_PREVIOUS_KEYS` is a comma-separated list of retired keys,
    // still needed to read what they wrote.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_applied() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.string("app.name").as_deref(), Some("Rainier Sample"));
        assert_eq!(config.string("app.locale").as_deref(), Some("en"));
    }

    #[test]
    fn the_environment_wins() {
        let config = Config::new();
        configure(&config, &Env::parse("APP_NAME=Acme\nAPP_LOCALE=fr")).unwrap();

        assert_eq!(config.string("app.name").as_deref(), Some("Acme"));
        assert_eq!(config.string("app.locale").as_deref(), Some("fr"));
    }
}
