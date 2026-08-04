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
//!
//! # `QUEUE_DRIVER` is not a variable this application has
//!
//! It was, and it had to go the way `STORAGE_DRIVER` went. `QUEUE_DRIVER` names
//! *one* connection and this file declares three, so the two are two answers to
//! the same question — and the framework refuses them together rather than
//! picking one, for the reason above: whichever loses still accepts every
//! dispatch aimed at it.
//!
//! That refusal is a boot failure, and it was reachable from the documented
//! first step. `.env.example` set `QUEUE_DRIVER=sync`, the README says to copy
//! it, and this file declared the section unconditionally — so a fresh clone
//! that followed the instructions did not start. The section is now the only
//! answer here, and [`QUEUE_CONNECTION`](configure) is what a deployment sets
//! to choose between the connections it declares.
//!
//! A misspelling is still caught, earlier and by the compiler: a connection
//! names its driver with [`ConnectionConfig::database`] rather than with a
//! string, so there is no `databse` to write.
//!
//! # `retry_after` belongs to a connection, not to a process
//!
//! [`DatabaseConnection::reservation`] is how long a worker's claim on a job
//! lasts, and it is the setting that makes per-connection settings worth the
//! ceremony: `default` and `bulk` sit on the same driver and cannot share a
//! number.
//!
//! It has to exceed the longest a job on that connection can run. Below it, a
//! job that is *still running* is reclaimed and handed to a second worker while
//! the first still has it — so it runs twice, at the same time, and neither
//! worker knows. Nothing fails and nothing is retried, because from where both
//! workers stand nothing went wrong. For a job that sends mail or charges a
//! card, that is the expensive failure.
//!
//! One number cannot serve both connections here. A notification takes a
//! second, so a short claim is right: a worker killed mid-job should have its
//! work picked up promptly rather than left for a quarter of an hour. A bulk
//! import legitimately runs for minutes, and the same short claim would hand it
//! to a second worker while the first was halfway through.
//!
//! `Connections::check_reservations` compares these against a worker's own
//! timeout, so the mistake can be made a boot failure wherever both are in
//! scope.

use std::time::Duration;

use rainier_framework::config::{Config, Env};
use rainier_framework::keys::QUEUES;
use rainier_framework::prelude::*;
use rainier_framework::queue::{ConnectionConfig, Connections, DatabaseConnection, SqsConnection};

use crate::config::keys;

/// Queue settings, read back by the framework's own bootstrap.
///
/// Records declarations; it opens no connection and starts no worker. That is
/// what keeps "which queues exist" answerable from configuration alone,
/// including by a test that never reaches a broker.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // The queue *name* — the lane a job waits in inside whichever backend it
    // was dispatched to. Not the connection, which is the next question down and
    // the one the section answers.
    //
    // `queue.sqs_url` used to be set beside it and is not any more. The URL is a
    // property of the SQS connection, and it is declared on that connection
    // below; keeping the scalar as well would have left a key nothing reads, for
    // the next person to change and expect an effect.
    config.set(keys::QUEUE_DEFAULT, env.string("QUEUE_DEFAULT", "default"))?;

    // The connection everything dispatches to unless it names another. It must
    // be one of the connections declared below; the framework refuses at boot
    // if it names one that was never declared, rather than accepting jobs into
    // nowhere.
    let default = env.string("QUEUE_CONNECTION", "sync");

    let mut connections = Connections::new(default)
        // Inline. Always declared, because it needs nothing configured and a
        // developer with no broker still has somewhere for a job to go.
        .with("sync", ConnectionConfig::sync())
        // Rows in a table — the connection to reach for when there is a
        // database and no broker. Durable across a restart, unlike `sync`,
        // which simply runs the work and forgets it happened.
        //
        // Ninety seconds is the driver's own default, restated here because
        // the number beside `bulk` only reads as a decision next to the one it
        // differs from.
        .with("database", DatabaseConnection::new().reservation(Duration::from_secs(90)))
        // A second connection for bulk work, on the same driver as `database`
        // but drained separately. Declaring it is what allows a worker to be
        // pointed at it alone: a long import cannot then delay a
        // password-reset email, which is the whole reason to separate them.
        //
        // And the claim is twenty minutes rather than ninety seconds, which is
        // the half a single connection could not express. A job here runs for
        // minutes by design; under the interactive connection's claim it would
        // be reclaimed while still running and executed twice, concurrently,
        // with nothing reporting it.
        .with("bulk", DatabaseConnection::new().reservation(Duration::from_secs(20 * 60)));

    // A managed queue, declared only when one is named. An `SqsConnection` with
    // an empty URL is not a connection that fails later — it is one that posts
    // to nothing — so leaving it undeclared is what lets a dispatch to `sqs` be
    // refused by name.
    let queue_url = env.string("SQS_QUEUE_URL", "");
    if !queue_url.is_empty() {
        let mut sqs = SqsConnection::new(queue_url)
            // SQS's spelling of the same reservation the two above declare, and
            // the same trap: below the worker's job timeout the message becomes
            // visible again while the first worker is still running it.
            .visibility_timeout(Duration::from_secs(5 * 60))
            // Long polling. At zero a worker on an empty queue makes a *billed*
            // request every time round its loop, which is a bill rather than a
            // bug, but it is a bill nobody chose.
            .wait_time(Duration::from_secs(20));

        let region = env.string("AWS_REGION", "");
        if !region.is_empty() {
            sqs = sqs.region(region);
        }

        // ElasticMQ or LocalStack in development; absent means AWS's own.
        let endpoint = env.string("SQS_ENDPOINT", "");
        if !endpoint.is_empty() {
            sqs = sqs.endpoint(endpoint);
        }

        connections = connections.with("sqs", sqs);
    }

    // There is no `kafka` connection here, and its absence is deliberate rather
    // than an omission — `config/kafka.rs` still configures the cluster, and
    // broadcasting and the socket relay still use it.
    //
    // A `KafkaConnection` carries brokers, a group, a topic prefix and a lease,
    // and **nothing else**: it has no field for TLS, none for a SASL mechanism,
    // and none for the credentials `config/kafka.rs` reads. So declaring one
    // here would not fail — it would connect, in the clear, to a cluster this
    // deployment configured for TLS, and offer no credentials to a cluster that
    // requires them. That is the wrong kind of quiet, and it is the reason to
    // wait rather than to declare it and hope.

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

        assert_eq!(config.get(QUEUES).expect("declared").default_name(), "sync");
        assert_eq!(config.get(keys::QUEUE_DEFAULT).as_deref(), Some("default"));
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

    #[test]
    fn bulk_lets_a_job_run_far_longer_before_anyone_else_may_take_it() {
        // The behaviour the per-connection setting buys, and the reason one
        // number could not serve both. Under `database`'s claim a twenty-minute
        // import is reclaimed while still running and executed twice at once,
        // with nothing failing and nothing reporting it.
        let connections = connections_from(&Env::parse(""));

        let interactive =
            connections.get("database").expect("declared").reservation_period().expect("declared");
        let bulk =
            connections.get("bulk").expect("declared").reservation_period().expect("declared");

        assert!(bulk > interactive, "bulk {bulk:?} should outlast interactive {interactive:?}");
        assert!(bulk >= Duration::from_secs(15 * 60), "{bulk:?}");
    }

    #[test]
    fn a_managed_queue_is_declared_only_when_one_is_named() {
        // Not a connection that fails on first use: an `SqsConnection` with an
        // empty URL posts to nothing. Leaving it undeclared is what lets a
        // dispatch to `sqs` be refused by name rather than accepted into
        // nowhere — which is this section's whole subject.
        assert!(connections_from(&Env::parse("")).get("sqs").is_none());

        let declared = connections_from(&Env::parse(
            "SQS_QUEUE_URL=https://sqs.us-east-1.amazonaws.com/1/jobs",
        ));
        assert_eq!(declared.get("sqs").expect("declared").driver(), QueueDriver::Sqs);
    }

    #[test]
    fn the_managed_queue_waits_longer_than_a_job_can_run() {
        // The same reservation trap the two database connections carry, under
        // SQS's name for it. Below the worker's timeout the message becomes
        // visible again while the first worker still has it.
        let connections = connections_from(&Env::parse(
            "SQS_QUEUE_URL=https://sqs.us-east-1.amazonaws.com/1/jobs",
        ));

        let visibility =
            connections.get("sqs").expect("declared").reservation_period().expect("declared");
        assert!(visibility >= Duration::from_secs(60), "{visibility:?}");
    }

    #[test]
    fn nothing_here_reads_the_variable_that_would_refuse_the_section() {
        // The regression this file exists at all to avoid, and it was reachable
        // from the README's first step: `.env.example` set `QUEUE_DRIVER=sync`,
        // this section is declared unconditionally, and the framework refuses
        // the two together — so a fresh clone that copied the example did not
        // boot.
        //
        // The framework keys on the *environment* variable, not on anything set
        // here, so the only fix that holds is for the variable to be absent
        // from this application. Asserting the section still stands with it set
        // would assert the wrong thing; what has to be true is that this file
        // does not make the value mean anything.
        let with_it = connections_from(&Env::parse("QUEUE_DRIVER=redis"));
        let without = connections_from(&Env::parse(""));

        assert_eq!(with_it.default_name(), without.default_name());
        assert_eq!(with_it.names().count(), without.names().count());
    }
}
