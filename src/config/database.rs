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
//!
//! # A pool belongs to a connection
//!
//! `reporting` below takes a **smaller** pool than the primary, which is a
//! setting one shared connector could not express. A report holds its
//! connection for seconds; the ceiling is what stops a refreshed dashboard from
//! reaching the server's own `max_connections`, where the failure stops being a
//! slow report and becomes `Too many connections` for everything on that
//! server.

use rainier_framework::config::{Config, Env};
use rainier_framework::database::{DatabaseConfig, Databases, PoolSettings, ServerDatabase};
use rainier_framework::keys::DATABASES;
use rainier_framework::prelude::*;

use crate::config::keys;

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

    // `DATABASE_URL` and a declared section are the **two ways to say the same
    // thing**, and the framework refuses both at once — see `keys::DATABASES`.
    // So when a deployment injects a DSN, this section declares nothing at all
    // and lets that DSN be the single connection.
    //
    // Not a special case: it is the ordinary one. A Kubernetes secret or a
    // platform add-on supplying `DATABASE_URL` is saying "this is the
    // database", and an application that also declared a section would be
    // naming a second answer to a question that already had one.
    //
    // Declaring both used to look harmless here and was not: with a section
    // declared *and* `DATABASE_URL` set, the process ran against an in-memory
    // database instead of either. Migrations applied to memory and vanished on
    // exit, and nothing said so — the run reported eleven migrations applied.
    if let Some(url) = env.get("DATABASE_URL").filter(|url| !url.trim().is_empty()) {
        // Validated here so a malformed DSN fails in this file, naming the
        // variable, rather than further in with less context.
        Databases::from_url(&url)?;

        // The DSN still has to reach `bootstrap::connect`, which opens the
        // single connection this application uses. Setting the *config* key is
        // not what the framework's conflict check looks at — that keys on the
        // environment variable — so this is the one place both can be true.
        config.set(keys::DATABASE_URL, url)?;
        return Ok(());
    }

    // No DSN. The section and `connect` must name the **same** database, or the
    // process migrates one and queries the other — which is exactly what
    // happened when only the section was set: `connect` fell back to its own
    // `sqlite::memory:` and every command got a private database, so `migrate`
    // reported eleven migrations applied and the next process found none.
    //
    // In memory by default, unchanged: a fresh clone runs with no setup, and a
    // test suite gets a database per process rather than a file every test in
    // every suite shares. Point `DB_DATABASE` at a path to keep it.
    let (url, declared) = match env.get("DB_DATABASE").filter(|p| !p.trim().is_empty()) {
        Some(path) => (format!("sqlite://{path}?mode=rwc"), DatabaseConfig::sqlite(path)),
        None => ("sqlite::memory:".to_string(), DatabaseConfig::sqlite_in_memory()),
    };

    config.set(keys::DATABASE_URL, url)?;
    let mut databases = Databases::new(&default).with("primary", declared);

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

        // A small pool, and deliberately smaller than the primary's — the other
        // half of what a per-connection setting buys, beside the credentials
        // and the charset above.
        //
        // A report holds its connection for as long as it runs, which is
        // seconds rather than milliseconds. Without a ceiling, a dashboard
        // refreshed by a dozen people opens a dozen connections and keeps them,
        // and the wall it hits is the *server's* `max_connections` — at which
        // point the failure is not a slow report. It is `Too many connections`
        // for everything on that server, including the query that would have
        // told somebody.
        //
        // `acquire_timeout` chooses which failure saturation produces once the
        // ceiling is reached: a report that waits ten seconds for a free
        // connection and then fails, rather than a request that waits forever
        // and a client that gives up without anything here noticing.
        reporting = reporting.pool(PoolSettings::new().max_connections(5).acquire_timeout(10));

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
    fn whatever_names_the_database_also_reaches_the_code_that_opens_it() {
        // The regression this exists for, and it was silent. `bootstrap::connect`
        // opens `keys::DATABASE_URL` and falls back to `sqlite::memory:` when it
        // is unset. A version of this file declared only the section, so every
        // process quietly ran against its own in-memory database: `migrate`
        // reported eleven migrations applied and the next process found none,
        // because they had never left that process.
        //
        // Both paths must therefore set it — the DSN one and the declared one.
        let with_dsn = Config::new();
        configure(&with_dsn, &Env::parse("DATABASE_URL=sqlite://given.sqlite?mode=rwc")).unwrap();
        assert_eq!(
            with_dsn.get(keys::DATABASE_URL).as_deref(),
            Some("sqlite://given.sqlite?mode=rwc")
        );

        let declared = Config::new();
        configure(&declared, &Env::parse("")).unwrap();
        assert_eq!(declared.get(keys::DATABASE_URL).as_deref(), Some("sqlite::memory:"));

        // And when a path is named, both must name *that* — the section and the
        // code that opens it agreeing is the whole property. Disagreeing is how
        // a migration lands in one database and a query reads another.
        let on_disk = Config::new();
        configure(&on_disk, &Env::parse("DB_DATABASE=var/app.sqlite")).unwrap();
        let url = on_disk.get(keys::DATABASE_URL).expect("a named path reaches connect");
        assert!(url.contains("var/app.sqlite"), "{url}");
        assert!(!url.contains(":memory:"), "{url}");
    }

    #[test]
    fn a_dsn_means_the_section_declares_nothing() {
        // The framework refuses a declared section and `DATABASE_URL` together,
        // because they are two answers to one question. When the deployment
        // supplies the DSN, this file must stay quiet.
        let config = Config::new();
        configure(&config, &Env::parse("DATABASE_URL=sqlite://given.sqlite?mode=rwc")).unwrap();

        assert!(!config.has(DATABASES.path()));
    }

    #[test]
    fn one_dsn_is_still_the_whole_configuration() {
        // The case to protect: an application with one database says one thing,
        // and that one thing is enough to open it.
        //
        // This used to assert the section was declared as well. It was wrong —
        // declaring both is what the framework refuses, and asserting it kept a
        // configuration that ran against neither.
        let config = Config::new();
        configure(&config, &Env::parse("DATABASE_URL=mysql://app@db/app")).unwrap();

        assert_eq!(config.get(keys::DATABASE_URL).as_deref(), Some("mysql://app@db/app"));
        assert!(!config.has(DATABASES.path()));
    }

    #[test]
    fn a_malformed_dsn_fails_here_rather_than_further_in() {
        let err = configure(&Config::new(), &Env::parse("DATABASE_URL=not-a-dsn")).unwrap_err();

        assert!(!err.message().is_empty(), "the failure should say something");
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
    fn the_reporting_connection_is_capped_and_the_primary_is_not() {
        // The setting a shared connector could not express: two connections,
        // two ceilings. The primary is SQLite here and takes the driver's own
        // pool; capping the replica is what stops a refreshed dashboard from
        // exhausting the *server's* connection budget, which fails everything
        // on that server rather than only the report.
        let databases = databases_from(&Env::parse("REPORTING_DB_HOST=reports.internal"));

        let DatabaseConfig::Server(reporting) = databases.get("reporting").expect("declared")
        else {
            panic!("the reporting connection should be a server database");
        };

        let pool = reporting.pool_settings().expect("declared");
        assert_eq!(pool.max(), Some(5));
        assert_eq!(pool.acquire_timeout_period(), Some(std::time::Duration::from_secs(10)));
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
