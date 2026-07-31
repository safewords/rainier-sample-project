//! `config/hashing.php`.
//!
//! Which algorithm password hashing **writes**. Verification is deliberately
//! not governed by it — the `HashManager` dispatches on the stored hash's own
//! prefix, so every registered driver's rows keep verifying whatever this
//! says — which is what makes changing algorithm a deploy: flip the value,
//! and rows convert on their next successful login.

use rainier_framework::config::{Config, Env};
use rainier_framework::crypt::hash::HashDriver;
use rainier_framework::prelude::*;

use crate::config::keys;

/// Hashing settings, read back by `AppServiceProvider::hashing`.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // A `HashDriver`, not a string: `HASH_DRIVER=argon2` fails here naming
    // the variable and the valid values, rather than silently hashing with
    // something the configuration did not say. `bcrypt` needs its cargo
    // feature, and selecting it without one fails at boot naming it.
    config.set(keys::HASH_DRIVER, env.setting_or("HASH_DRIVER", HashDriver::Argon2id)?)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argon2id_writes_until_a_deployment_says_otherwise() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.setting(keys::HASH_DRIVER).unwrap(), HashDriver::Argon2id);
    }

    #[test]
    fn a_driver_outside_the_set_stops_the_boot() {
        let err = configure(&Config::new(), &Env::parse("HASH_DRIVER=argon2")).unwrap_err();

        assert!(err.message().contains("HASH_DRIVER"), "{}", err.message());
    }
}
