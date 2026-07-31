//! Configuration — the `config/` directory.
//!
//! One **module** per concern, each writing into the same dotted tree
//! (`app.name`, `session.lifetime`, `posts.per_page`). The reason to split
//! them is a file you can read in one screen.
//!
//! ```text
//! src/config/
//!   mod.rs        the entry point, calling each section
//!   keys.rs       every key this application names
//!   app.rs        the application's name, environment, URL, cipher
//!   database.rs   the one URL persistence hangs on
//!   session.rs    the session driver and cookie
//!   cache.rs      the cache driver and prefix
//!   queue.rs      where dispatched jobs wait
//!   hashing.rs    which algorithm passwords are written with
//!   mail.rs       the mail driver, and everything each transport needs
//!   storage.rs    where uploaded files live
//!   kafka.rs      one cluster, three ports — queue, broadcast, relay
//!   posts.rs      an application's own section
//!   server.rs     the timeout and compression nginx would do
//!   metrics.rs    Prometheus, off by default
//!   openapi.rs    the generated API document
//!   telemetry.rs  OpenTelemetry
//! ```
//!
//! Read any of it back with the `Config` facade:
//!
//! ```ignore
//! use crate::config::keys::POSTS_PER_PAGE;
//!
//! let per_page = Config::instance().get_or(POSTS_PER_PAGE, 15);
//! ```
//!
//! ## Nothing here is a magic string
//!
//! Every path is a [`Key<T>`] declared once, and every value with a fixed set
//! of options is an enum:
//!
//! ```ignore
//! config.set(CACHE_DRIVER, env.setting::<CacheDriver>("CACHE_DRIVER")?)?;
//! //         ^ a Key<CacheDriver>       ^ fails on anything outside the set
//! ```
//!
//! Which turns three classes of mistake into something that happens at the
//! right time:
//!
//! | Mistake | Before | Now |
//! |---|---|---|
//! | `config.set("cache.drivers", …)` | writes where nothing reads | does not compile |
//! | `config.get::<String>("posts.per_page")` | `None`, then a fallback everywhere | does not compile |
//! | `CACHE_DRIVER=redys` | boots on an in-process cache | fails the boot, listing the options |
//!
//! [`Key<T>`]: rainier_framework::config::Key
//!
//! ## Where a value belongs
//!
//! | The value… | Goes… |
//! |---|---|
//! | differs per environment (URLs, credentials, drivers) | in `.env`, read with `env.string(..)` |
//! | differs per environment but has a safe default | in `.env` with a fallback |
//! | is the same everywhere in this application | a **literal** here |
//! | is the same for every application | not configuration — a constant |
//!
//! Not every setting needs an environment variable, and pretending otherwise
//! turns `.env` into a dumping ground. A literal here is still discoverable in
//! one place, readable through the facade, and overridable in a test.

use rainier_framework::config::{Config, Env};
use rainier_framework::prelude::*;

pub mod app;
pub mod cache;
pub mod database;
pub mod hashing;
pub mod kafka;
pub mod keys;
pub mod mail;
pub mod metrics;
pub mod openapi;
pub mod posts;
pub mod queue;
pub mod server;
pub mod session;
pub mod storage;
pub mod telemetry;

/// Apply every configuration section.
///
/// The framework has already filled in its own defaults (`app.*`, `server.*`,
/// `database.*`, `queue.*`, `mail.*`) from the environment by the time this
/// runs, so each section only sets what is specific to this application — and
/// overrides anything it wants to differ.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    app::configure(config, env)?;
    database::configure(config, env)?;
    session::configure(config, env)?;
    cache::configure(config, env)?;
    queue::configure(config, env)?;
    hashing::configure(config, env)?;
    mail::configure(config, env)?;
    storage::configure(config, env)?;
    kafka::configure(config, env)?;
    posts::configure(config, env)?;
    server::configure(config, env)?;

    // Observability: all three off unless a deployment asks.
    metrics::configure(config, env)?;
    openapi::configure(config, env)?;
    telemetry::configure(config, env)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_apply_with_no_environment() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.string("app.name").as_deref(), Some("Rainier Sample"));
        assert_eq!(config.int("posts.per_page"), Some(15));
        assert_eq!(config.int("session.lifetime"), Some(7200));
    }

    #[test]
    fn the_environment_overrides_a_default() {
        let config = Config::new();
        configure(&config, &Env::parse("APP_NAME=Custom\nPOSTS_PER_PAGE=50")).unwrap();

        assert_eq!(config.string("app.name").as_deref(), Some("Custom"));
        assert_eq!(config.int("posts.per_page"), Some(50));
    }

    #[test]
    fn every_section_is_reached() {
        // A section added to the directory but not to `configure` would
        // silently never apply, and the symptom is a fallback used everywhere.
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        for key in [
            keys::APP_NAME.path(),
            keys::DATABASE_URL.path(),
            keys::SESSION_DRIVER.path(),
            keys::CACHE_DRIVER.path(),
            keys::QUEUE_DRIVER.path(),
            keys::HASH_DRIVER.path(),
            rainier_framework::keys::MAIL_DRIVER.path(),
            keys::STORAGE_DRIVER.path(),
            keys::KAFKA_BROKERS.path(),
            keys::POSTS_PER_PAGE.path(),
            keys::SERVER_REQUEST_TIMEOUT_SECS.path(),
            keys::METRICS_ENABLED.path(),
            keys::OPENAPI_ENABLED.path(),
            keys::TELEMETRY_ENABLED.path(),
        ] {
            assert!(config.has(key), "`{key}` was not set — is its section wired into configure?");
        }
    }

    #[test]
    fn a_driver_outside_its_set_stops_the_boot_and_says_what_was_expected() {
        // The property the whole typed-config layer exists for. Before it,
        // every one of these booted happily on the default driver.
        let cases = [
            ("CACHE_DRIVER=redys", "CACHE_DRIVER", "`memcached`"),
            ("SESSION_DRIVER=redis", "SESSION_DRIVER", "`cookie`"),
            ("QUEUE_DRIVER=databse", "QUEUE_DRIVER", "`database`"),
            ("HASH_DRIVER=argon2", "HASH_DRIVER", "`argon2id`"),
            ("STORAGE_DRIVER=r2", "STORAGE_DRIVER", "`s3`"),
            ("APP_CIPHER=laravel", "APP_CIPHER", "`php`"),
        ];

        for (env, variable, expected_in_message) in cases {
            let err = configure(&Config::new(), &Env::parse(env)).unwrap_err();

            assert!(err.message().contains(variable), "{env}: {}", err.message());
            assert!(
                err.message().contains(expected_in_message),
                "{env}: the message should list the valid values, got {}",
                err.message()
            );
        }
    }

    #[test]
    fn a_driver_reads_back_as_the_enum_it_was_written_as() {
        let config = Config::new();
        configure(&config, &Env::parse("CACHE_DRIVER=memcached\nSESSION_DRIVER=cache")).unwrap();

        assert_eq!(config.setting(keys::CACHE_DRIVER).unwrap(), CacheDriver::Memcached);
        assert_eq!(config.setting(keys::SESSION_DRIVER).unwrap(), SessionDriver::Cache);
    }
}
