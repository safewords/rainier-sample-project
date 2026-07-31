//! `config/kafka.php` — one cluster, three ports.
//!
//! Kafka can back the queue (`QUEUE_DRIVER=kafka`), broadcasting, and the
//! socket relay, and "we have a Kafka" is one fact about a deployment — so
//! its settings are one section, not three. Empty brokers mean no cluster is
//! configured, which is what an application that does not use Kafka leaves
//! it as; anything here beyond that needs the `kafka` cargo feature (and
//! `kafka-tls` for a managed cluster) before a driver can be built from it.

use rainier_framework::config::{Config, Env};
use rainier_framework::prelude::*;

use crate::config::keys;

/// Kafka settings, read by whichever drivers a deployment selects.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    config.set(keys::KAFKA_BROKERS, env.string("KAFKA_BROKERS", ""))?;
    config.set(keys::KAFKA_GROUP, env.string("KAFKA_GROUP", "app"))?;
    config.set(keys::KAFKA_TOPIC_PREFIX, env.string("KAFKA_TOPIC_PREFIX", "app"))?;
    config
        .set(keys::KAFKA_BROADCAST_TOPIC, env.string("KAFKA_BROADCAST_TOPIC", "app.broadcast"))?;

    config.set(keys::KAFKA_TLS, env.bool("KAFKA_TLS", false))?;
    config.set(keys::KAFKA_USERNAME, env.string("KAFKA_USERNAME", ""))?;
    config.set(keys::KAFKA_PASSWORD, env.string("KAFKA_PASSWORD", ""))?;
    // The framework refuses a misspelled mechanism at boot rather than
    // falling back to PLAIN — which would send the password in the clear.
    config.set(keys::KAFKA_SASL_MECHANISM, env.string("KAFKA_SASL_MECHANISM", ""))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_cluster_is_configured_until_a_deployment_names_one() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.get(keys::KAFKA_BROKERS).as_deref(), Some(""));
        assert_eq!(config.get(keys::KAFKA_GROUP).as_deref(), Some("app"));
    }

    #[test]
    fn a_deployment_names_its_brokers() {
        let config = Config::new();
        configure(&config, &Env::parse("KAFKA_BROKERS=kafka-1:9092,kafka-2:9092\nKAFKA_TLS=true"))
            .unwrap();

        assert_eq!(config.get(keys::KAFKA_BROKERS).as_deref(), Some("kafka-1:9092,kafka-2:9092"));
        assert_eq!(config.get(keys::KAFKA_TLS), Some(true));
    }
}
