//! `config/session.php`.

use rainier_framework::config::{Config, Env};
use rainier_framework::prelude::*;

use crate::config::keys::{
    SESSION_COOKIE, SESSION_DRIVER, SESSION_LIFETIME, SESSION_SAME_SITE, SESSION_SECURE,
};

/// Session settings, read by `bootstrap.rs` when it builds the store.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // `memory` is per-process: right for development, wrong the moment there
    // are two instances, because a user's session appears to vanish and
    // reappear as the load balancer moves them around.
    //
    // Note what `SessionDriver` does *not* have: `redis`. Sessions in Redis are
    // the `cache` driver pointed at Redis — one store to configure, one pool to
    // open. A closed set makes that choice visible instead of leaving someone
    // to discover `SESSION_DRIVER=redis` does nothing.
    config.set(SESSION_DRIVER, env.setting::<SessionDriver>("SESSION_DRIVER")?)?;

    config.set(SESSION_COOKIE, env.string("SESSION_COOKIE", "rainier_session"))?;
    config.set(SESSION_LIFETIME, env.int("SESSION_LIFETIME", 7200))?;

    // Off by default so `http://localhost` works. Turn it on in production —
    // a session cookie sent over plain HTTP is a session anyone on the path
    // can take.
    config.set(SESSION_SECURE, env.bool("SESSION_SECURE", false))?;

    // A literal, not an environment variable: this is a property of how the
    // application is built, not of where it is deployed. `Lax` lets a
    // top-level navigation from another site carry the cookie, which is what
    // makes a link from an email land logged in; `Strict` does not.
    config.set(SESSION_SAME_SITE, "lax".to_string())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_development_ones() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.setting(SESSION_DRIVER).unwrap(), SessionDriver::Memory);
        assert_eq!(config.get(SESSION_LIFETIME), Some(7200));
        assert_eq!(config.get(SESSION_SECURE), Some(false));
    }

    #[test]
    fn production_settings_come_from_the_environment() {
        let config = Config::new();
        configure(
            &config,
            &Env::parse("SESSION_DRIVER=database\nSESSION_SECURE=true\nSESSION_LIFETIME=86400"),
        )
        .unwrap();

        let driver = config.setting(SESSION_DRIVER).unwrap();
        assert_eq!(driver, SessionDriver::Database);
        assert!(driver.is_shared() && driver.is_durable() && driver.is_revocable());

        assert_eq!(config.get(SESSION_SECURE), Some(true));
        assert_eq!(config.get(SESSION_LIFETIME), Some(86400));
    }

    #[test]
    fn redis_is_not_a_session_driver_and_says_so() {
        // `SESSION_DRIVER=redis` is a reasonable thing to try and the wrong
        // answer here. The error lists what is actually available.
        let err = configure(&Config::new(), &Env::parse("SESSION_DRIVER=redis")).unwrap_err();

        assert!(err.message().contains("SESSION_DRIVER"), "{}", err.message());
        assert!(err.message().contains("`cache`"), "{}", err.message());
    }

    #[test]
    fn same_site_is_not_environment_driven() {
        // It is a literal on purpose; this pins that.
        let config = Config::new();
        configure(&config, &Env::parse("SESSION_SAME_SITE=strict")).unwrap();

        assert_eq!(config.get(SESSION_SAME_SITE).as_deref(), Some("lax"));
    }
}
