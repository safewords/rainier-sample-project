//! `config/database.php`.
//!
//! A **default** connection plus every connection the application declares by
//! name, each naming its own driver and its own settings.
//!
//! # One database stays one line
//!
//! `DATABASE_URL` declares the default connection and nothing else is needed —
//! point it at SQLite, MySQL or Postgres and no other code changes. In memory
//! by default, so a fresh clone runs with no setup.
//!
//! That is deliberate and it is the case to protect. Most applications have one
//! database, and a configuration layer that made the common case more verbose in
//! order to express the rare one would be a worse layer. Everything below is
//! opt-in.
//!
//! # When a second connection is worth declaring
//!
//! A reporting replica, a legacy database being migrated away from, an
//! analytics store. Each carries its own host, credentials and settings — they
//! are different servers, and a connection built from another's settings is
//! pointed at the wrong one.
//!
//! # Why `reporting` below is not just `DATABASE_URL` with a different host
//!
//! Because it is not only the host that differs. It has its own credentials,
//! and it is declared read-only by convention here. Sharing a connection and
//! swapping the host at the call site is how a report ends up writing to
//! production: nothing about the call site says which server it is talking to,
//! so nothing about it looks wrong.
//!
//! # `charset` and `collation` are correctness, not tuning
//!
//! `utf8mb4` is what makes four-byte characters work. A connection that
//! negotiates plain `utf8` does not reject emoji or much CJK text loudly —
//! MySQL's `utf8` is three-byte, so the value is **truncated at the first
//! four-byte character** and the row still saves. The loss is found later, by
//! whoever reads it back.
//!
//! `strict` is the same shape: without it MySQL truncates an over-long or
//! out-of-range value instead of erroring, so the write succeeds and the stored
//! value is not what was sent.

use rainier_framework::config::{Config, Env};
use rainier_framework::database::{DatabaseConfig, Databases, ServerDatabase};
use rainier_framework::keys::DATABASES;
use rainier_framework::prelude::*;

/// Database settings, read back by the framework's own bootstrap.
///
/// Records declarations; it opens no connection. That is what keeps "which
/// databases exist" answerable from configuration alone, including by a test
/// that never reaches a network.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // The connection everything uses unless it names another. Whatever this
    // holds must be one of the connections declared below — the framework
    // refuses at boot if it names one that was never declared, rather than
    // starting and failing on the first query.
    let default = env.string("DB_CONNECTION", "primary");

    // `DATABASE_URL` wins outright when it is set, because a deployment that
    // injects one DSN — a Kubernetes secret, a platform add-on — is saying
    // "this is the database", and quietly preferring a declared connection
    // over it would connect somewhere the deployment never named.
    let mut databases = match env.get("DATABASE_URL").filter(|url| !url.is_empty()) {
        Some(url) => Databases::from_url(&url)?,
        None => Databases::new(&default).with(
            "primary",
            DatabaseConfig::sqlite(env.string("DB_DATABASE", "database/app.sqlite")),
        ),
    };

    // A second connection, on its own server with its own credentials —
    // declared only when one is named, because a `ServerDatabase` with an empty
    // host is not a connection that fails later, it is one that resolves
    // nothing.
    let reporting_host = env.string("REPORTING_DB_HOST", "");
    if !reporting_host.is_empty() {
        let mut reporting = ServerDatabase::mysql(env.string("REPORTING_DB_DATABASE", "reporting"))
            .host(reporting_host)
            // Four-byte characters, and an error rather than a truncation. See
            // the module docs — both of these are correctness settings.
            .charset("utf8mb4")
            .collation("utf8mb4_unicode_ci")
            .strict(true);

        if let Some(port) = env.get("REPORTING_DB_PORT").and_then(|p| p.parse::<u16>().ok()) {
            reporting = reporting.port(port);
        }

        // Both halves together or neither. A username with no password is a
        // real configuration — trust and peer authentication use it — but a
        // password with no username has nobody to own it.
        let user = env.string("REPORTING_DB_USERNAME", "");
        let password = env.string("REPORTING_DB_PASSWORD", "");
        if !user.is_empty() {
            reporting = reporting.credentials(user, password);
        }

        // A managed database usually requires TLS, and the CA has to come from
        // somewhere. Without it the driver falls back to whatever it considers
        // default, which is not necessarily verification.
        let ca = env.string("REPORTING_DB_SSL_CA", "");
        if !ca.is_empty() {
            reporting = reporting.tls_ca(ca);
        }

        databases = databases.with("reporting", reporting);
    }

    config.set(DATABASES, databases)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn databases_from(env: &Env) -> Databases {
        let config = Config::new();
        configure(&config, env).unwrap();
        config.get(DATABASES).expect("the databases section is set")
    }

    #[test]
    fn a_fresh_clone_gets_a_working_default_and_declares_nothing_else() {
        let databases = databases_from(&Env::parse(""));

        assert_eq!(databases.default_name(), "primary");
        assert!(databases.get("primary").is_some());
        assert!(databases.get("reporting").is_none());
    }

    #[test]
    fn one_dsn_is_still_the_whole_configuration() {
        // The case to protect: an application with one database says one thing.
        let databases = databases_from(&Env::parse("DATABASE_URL=mysql://app@db/app"));

        assert!(databases.get(databases.default_name()).is_some());
    }

    #[test]
    fn a_second_connection_is_declared_only_when_a_host_names_one() {
        // Not a connection that fails on first use: a server database with an
        // empty host resolves nothing. Leaving it undeclared is what lets a
        // caller be told "no such connection" rather than watch a query hang.
        assert!(databases_from(&Env::parse("")).get("reporting").is_none());

        let declared = databases_from(&Env::parse("REPORTING_DB_HOST=reports.internal"));
        assert!(declared.get("reporting").is_some());
    }

    #[test]
    fn the_two_connections_are_not_the_same_server() {
        let databases = databases_from(&Env::parse(
            "REPORTING_DB_HOST=reports.internal\nREPORTING_DB_DATABASE=metrics",
        ));

        let primary = format!("{:?}", databases.get("primary").expect("declared"));
        let reporting = format!("{:?}", databases.get("reporting").expect("declared"));

        // The assertion that matters. A configuration that built the second
        // from the first's settings would pass every other test here and send
        // reports to production.
        assert_ne!(primary, reporting);
        assert!(reporting.contains("reports.internal"), "{reporting}");
    }

    #[test]
    fn a_password_never_reaches_a_rendering_of_the_section() {
        let databases = databases_from(&Env::parse(
            "REPORTING_DB_HOST=reports.internal\n\
             REPORTING_DB_USERNAME=reader\n\
             REPORTING_DB_PASSWORD=super-secret",
        ));

        // A configuration dump at boot must not put the password into the log
        // of every process that started.
        let rendered = format!("{databases:?}");
        assert!(!rendered.contains("super-secret"), "{rendered}");
        // Not vacuous — the connection itself does render.
        assert!(rendered.contains("reports.internal"), "{rendered}");
    }
}
