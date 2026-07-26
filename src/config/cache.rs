//! `config/cache.php`.

use rainier_framework::config::{Config, Env};
use rainier_framework::prelude::*;

/// Cache settings, read by `bootstrap.rs` when it builds the store.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // memory | redis | redis-cluster | memcached
    //
    // The non-memory drivers are behind cargo features, so selecting one in a
    // build that did not enable it falls back to memory with a warning rather
    // than failing — a cache is the one dependency an application should be
    // able to lose.
    config.set("cache.driver", env.string("CACHE_DRIVER", "memory"))?;

    config.set("cache.redis_url", env.string("REDIS_URL", "redis://127.0.0.1:6379/"))?;
    config.set("cache.memcached_url", env.string("MEMCACHED_URL", "127.0.0.1:11211"))?;

    // A literal: this is what namespaces our keys on a shared server, and it is
    // a property of the application rather than of a deployment.
    config.set("cache.prefix", "rainier_sample")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_in_process() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.string("cache.driver").as_deref(), Some("memory"));
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

        assert_eq!(config.string("cache.driver").as_deref(), Some("redis-cluster"));
        assert!(config.string("cache.redis_url").unwrap().contains(','));
    }

    #[test]
    fn the_prefix_is_not_environment_driven() {
        let config = Config::new();
        configure(&config, &Env::parse("CACHE_PREFIX=other")).unwrap();

        assert_eq!(config.string("cache.prefix").as_deref(), Some("rainier_sample"));
    }
}
