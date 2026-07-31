//! `config/queue.php`.
//!
//! Which store dispatched jobs wait in. `sync` — the framework's default —
//! runs them inline, which is right for development (no worker to forget)
//! and wrong under load (a slow job makes a slow request). The provider
//! builds whatever this names, and a driver whose cargo feature is missing
//! fails the boot naming it.

use rainier_framework::config::{Config, Env};
use rainier_framework::prelude::*;

use crate::config::keys;

/// Queue settings, read back by `AppServiceProvider::queue`.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // A `QueueDriver`, not a string: `QUEUE_DRIVER=databse` fails here
    // listing the valid values, rather than silently running jobs inline
    // while everybody believes a worker has them.
    config.set(
        keys::QUEUE_DRIVER,
        env.setting_or("QUEUE_DRIVER", QueueDriver::Sync)?,
    )?;

    // The queue a job goes on when it does not name one.
    config.set(keys::QUEUE_DEFAULT, env.string("QUEUE_DEFAULT", "default"))?;

    // The SQS half, read only when the driver is `sqs`.
    config.set(keys::QUEUE_SQS_URL, env.string("SQS_QUEUE_URL", ""))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs_run_inline_until_a_deployment_says_otherwise() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(
            config.setting(keys::QUEUE_DRIVER).unwrap(),
            QueueDriver::Sync
        );
        assert_eq!(config.get(keys::QUEUE_DEFAULT).as_deref(), Some("default"));
    }

    #[test]
    fn a_misspelled_driver_stops_the_boot() {
        let err = configure(&Config::new(), &Env::parse("QUEUE_DRIVER=databse")).unwrap_err();

        assert!(err.message().contains("QUEUE_DRIVER"), "{}", err.message());
        assert!(
            err.message().contains("`database`"),
            "the message should list the valid values, got {}",
            err.message()
        );
    }
}
