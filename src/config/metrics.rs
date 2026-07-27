//! `config/metrics.php` — Prometheus.

use rainier_framework::config::{Config, Env};
use rainier_framework::prelude::*;

use crate::config::keys::{METRICS_ENABLED, METRICS_PATH};

/// Metrics settings, read by `bootstrap.rs`.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // Off unless asked for. A timer and a lock on every request is not much,
    // but it is not nothing, and an application nobody scrapes should not pay
    // it.
    config.set(METRICS_ENABLED, env.bool("METRICS_ENABLED", false))?;

    // Configurable because this is the one endpoint you may want somewhere
    // unguessable — it tells a reader your traffic shape, your error rate and
    // every route you serve.
    config.set(METRICS_PATH, env.string("METRICS_PATH", "/metrics"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_off_unless_asked_for() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.get(METRICS_ENABLED), Some(false));
    }

    #[test]
    fn the_path_can_be_moved_somewhere_less_obvious() {
        let config = Config::new();
        configure(&config, &Env::parse("METRICS_PATH=/internal/x9f2/metrics")).unwrap();

        assert_eq!(config.get(METRICS_PATH).as_deref(), Some("/internal/x9f2/metrics"));
    }
}
