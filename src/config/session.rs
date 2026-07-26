//! `config/session.php`.

use rainier_framework::config::{Config, Env};
use rainier_framework::prelude::*;

/// Session settings, read by `bootstrap.rs` when it builds the store.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // `memory` is per-process: right for development, wrong the moment there
    // are two instances, because a user's session appears to vanish and
    // reappear as the load balancer moves them around.
    config.set("session.driver", env.string("SESSION_DRIVER", "memory"))?;

    config.set("session.cookie", env.string("SESSION_COOKIE", "rainier_session"))?;
    config.set("session.lifetime", env.int("SESSION_LIFETIME", 7200))?;

    // Off by default so `http://localhost` works. Turn it on in production —
    // a session cookie sent over plain HTTP is a session anyone on the path
    // can take.
    config.set("session.secure", env.bool("SESSION_SECURE", false))?;

    // A literal, not an environment variable: this is a property of how the
    // application is built, not of where it is deployed. `Lax` lets a
    // top-level navigation from another site carry the cookie, which is what
    // makes a link from an email land logged in; `Strict` does not.
    config.set("session.same_site", "lax")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_development_ones() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.string("session.driver").as_deref(), Some("memory"));
        assert_eq!(config.int("session.lifetime"), Some(7200));
        assert_eq!(config.bool("session.secure"), Some(false));
    }

    #[test]
    fn production_settings_come_from_the_environment() {
        let config = Config::new();
        configure(
            &config,
            &Env::parse("SESSION_DRIVER=database\nSESSION_SECURE=true\nSESSION_LIFETIME=86400"),
        )
        .unwrap();

        assert_eq!(config.string("session.driver").as_deref(), Some("database"));
        assert_eq!(config.bool("session.secure"), Some(true));
        assert_eq!(config.int("session.lifetime"), Some(86400));
    }

    #[test]
    fn same_site_is_not_environment_driven() {
        // It is a literal on purpose; this pins that.
        let config = Config::new();
        configure(&config, &Env::parse("SESSION_SAME_SITE=strict")).unwrap();

        assert_eq!(config.string("session.same_site").as_deref(), Some("lax"));
    }
}
