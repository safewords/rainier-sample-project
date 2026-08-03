//! `config/queue.php`.
//!
//! A **default** connection plus every connection the application declares by
//! name, each naming its own driver and its own settings.
//!
//! `sync` is the default and runs jobs inline, which is right for development —
//! no worker to forget — and wrong under load, because a slow job makes a slow
//! request. A driver whose cargo feature is missing fails the boot naming it,
//! rather than quietly running everything inline while everybody believes a
//! worker has it.
//!
//! # Why more than one connection is worth declaring
//!
//! Because a queue is not only a backend, it is a *destination*. Bulk work on
//! its own connection can be drained by its own workers, scaled separately, and
//! paused without stopping the interactive queue. One connection cannot express
//! that: everything lands in the same place and starves everything else.
//!
//! # The rule that makes this safe
//!
//! Naming an undeclared connection resolves to **nothing**, never to the
//! default. That matters more here than anywhere else in configuration,
//! because dispatching to the wrong backend is the quietest failure in the
//! system: the job is accepted, stored somewhere nobody drains, and never runs.
//! Nothing errors, nothing retries, and the only symptom is work that did not
//! happen — noticed, if at all, days later by its absence.
//!
//! The same is true of the queue *name* within a connection. A worker draining
//! `bulk` never sees a job dispatched to `blk`.

use rainier_framework::config::{Config, Env};
use rainier_framework::keys::QUEUES;
use rainier_framework::prelude::*;
use rainier_framework::queue::{ConnectionConfig, Connections};

use crate::config::keys;

/// Queue settings, read back by the framework's own bootstrap.
///
/// Records declarations; it opens no connection and starts no worker. That is
/// what keeps "which queues exist" answerable from configuration alone,
/// including by a test that never reaches a broker.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // Retained because a deployment already sets them, and they still mean what
    // they meant: the driver and queue name of the *default* connection. An
    // application with one queue says nothing more than this.
    config.set(keys::QUEUE_DRIVER, env.setting_or("QUEUE_DRIVER", QueueDriver::Sync)?)?;
    config.set(keys::QUEUE_DEFAULT, env.string("QUEUE_DEFAULT", "default"))?;
    config.set(keys::QUEUE_SQS_URL, env.string("SQS_QUEUE_URL", ""))?;

    // The connection everything dispatches to unless it names another. It must
    // be one of the connections declared below; the framework refuses at boot
    // if it names one that was never declared, rather than accepting jobs into
    // nowhere.
    let default = env.string("QUEUE_CONNECTION", "sync");

    let connections = Connections::new(default)
        // Inline. Always declared, because it needs nothing configured and a
        // developer with no broker still has somewhere for a job to go.
        .with("sync", ConnectionConfig::sync())
        // Rows in a table — the connection to reach for when there is a
        // database and no broker. Durable across a restart, unlike `sync`,
        // which simply runs the work and forgets it happened.
        .with("database", ConnectionConfig::database())
        // A second connection for bulk work, on the same driver as `database`
        // but drained separately. Declaring it is what allows a worker to be
        // pointed at it alone: a long import cannot then delay a
        // password-reset email, which is the whole reason to separate them.
        .with("bulk", ConnectionConfig::database());

    config.set(QUEUES, connections)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connections_from(env: &Env) -> Connections {
        let config = Config::new();
        configure(&config, env).unwrap();
        config.get(QUEUES).expect("the queues section is set")
    }

    #[test]
    fn jobs_run_inline_until_a_deployment_says_otherwise() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.setting(keys::QUEUE_DRIVER).unwrap(), QueueDriver::Sync);
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

    #[test]
    fn every_declared_connection_is_reachable_by_name() {
        let connections = connections_from(&Env::parse(""));

        for name in ["sync", "database", "bulk"] {
            assert!(connections.get(name).is_some(), "`{name}` is not declared");
        }
    }

    #[test]
    fn an_undeclared_name_resolves_to_nothing_rather_than_the_default() {
        // The assertion this section exists for. A misspelling that fell back
        // to the default would be accepted, stored, and drained by the wrong
        // worker — or by none — with nothing reporting it.
        let connections = connections_from(&Env::parse(""));

        assert!(connections.get("blk").is_none());
        assert_ne!(connections.default_name(), "blk");
    }

    #[test]
    fn the_default_connection_follows_the_deployment() {
        assert_eq!(connections_from(&Env::parse("")).default_name(), "sync");
        assert_eq!(
            connections_from(&Env::parse("QUEUE_CONNECTION=database")).default_name(),
            "database"
        );
    }

    #[test]
    fn bulk_is_a_separate_destination_and_not_an_alias() {
        // Same driver, different connection. If these were one entry, pointing
        // a worker at `bulk` would drain the interactive queue too, and the
        // separation the docs promise would not exist.
        let connections = connections_from(&Env::parse(""));

        assert!(connections.get("database").is_some());
        assert!(connections.get("bulk").is_some());
        assert_eq!(connections.names().count(), 3);
    }
}
