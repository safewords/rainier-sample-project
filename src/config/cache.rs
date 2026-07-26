//! `config/cache.php`.

use rainier_framework::config::{Config, Env};
use rainier_framework::prelude::*;

use crate::config::keys::{CACHE_DRIVER, CACHE_MEMCACHED_URL, CACHE_PREFIX, CACHE_REDIS_URL};

/// Cache settings, read by `bootstrap.rs` when it builds the store.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // A `CacheDriver`, not a string. `CACHE_DRIVER=redys` fails here, naming
    // the variable and listing the five valid values — rather than booting on
    // an in-process cache that looks fine until a rate limiter lets through
    // `N ×` its limit across `N` instances.
    //
    // The non-memory drivers are behind cargo features; selecting one the build
    // did not enable is caught in `bootstrap.rs`, where the store is built and
    // the message can name the feature.
    config.set(CACHE_DRIVER, env.setting::<CacheDriver>("CACHE_DRIVER")?)?;

    config.set(CACHE_REDIS_URL, env.string("REDIS_URL", "redis://127.0.0.1:6379/"))?;
    config.set(CACHE_MEMCACHED_URL, env.string("MEMCACHED_URL", "127.0.0.1:11211"))?;

    // A literal: this is what namespaces our keys on a shared server, and it is
    // a property of the application rather than of a deployment.
    config.set(CACHE_PREFIX, "rainier_sample".to_string())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_in_process() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.setting(CACHE_DRIVER).unwrap(), CacheDriver::Memory);
        assert!(!config.setting(CACHE_DRIVER).unwrap().is_shared());
    }

    #[test]
    fn a_cluster_takes_a_comma_separated_seed_list() {
        let config = Config::new();
        configure(
            &config,
            &Env::parse("CACHE_DRIVER=redis-cluster
REDIS_URL=redis://a:6379,redis://b:6379"),
        )
        .unwrap();

        assert_eq!(config.setting(CACHE_DRIVER).unwrap(), CacheDriver::RedisCluster);
        assert!(config.get(CACHE_REDIS_URL).unwrap().contains(','));
    }

    #[test]
    fn a_misspelled_driver_stops_the_boot() {
        // The behaviour worth pinning: not a warning, not a fallback.
        let err = configure(&Config::new(), &Env::parse("CACHE_DRIVER=redys")).unwrap_err();

        assert!(err.message().contains("CACHE_DRIVER"), "{}", err.message());
        assert!(err.message().contains("`memcached`"), "{}", err.message());
    }

    #[test]
    fn the_driver_is_stored_as_the_text_a_dotenv_would_hold() {
        // So `config:show` and a JSON dump both read the way the operator
        // wrote it.
        let config = Config::new();
        configure(&config, &Env::parse("CACHE_DRIVER=redis-cluster")).unwrap();

        assert_eq!(config.string("cache.driver").as_deref(), Some("redis-cluster"));
    }

    #[test]
    fn the_prefix_is_not_environment_driven() {
        let config = Config::new();
        configure(&config, &Env::parse("CACHE_PREFIX=other")).unwrap();

        assert_eq!(config.get(CACHE_PREFIX).as_deref(), Some("rainier_sample"));
    }
}
