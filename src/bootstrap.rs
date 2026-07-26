//! Bootstrapping — `bootstrap/app.php`.
//!
//! One function that assembles the application: configuration, views, the
//! database, providers, middleware, listeners, routes. Everything the rest of
//! the app relies on is wired here, in one readable place.

use std::sync::Arc;

use rainier_framework::config::Env;
use rainier_framework::database::Database;
use rainier_framework::http::SameSite;
use rainier_framework::prelude::*;
use rainier_framework::session::{
    DatabaseSessionStore, MemorySessionStore, SessionConfig, SessionManager, SessionStore,
};
use rainier_framework::view::BladeEngine;

use crate::app::http::kernel;
use crate::app::providers::{AppServiceProvider, EventServiceProvider};
use crate::config;
use crate::routes;

/// How the application is wired.
///
/// A parameter rather than a constant, because a test wants captured mail and
/// an in-memory queue while a running app wants neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Normal operation: real transports, real logging.
    Running,
    /// Under test: mail is captured so it can be asserted on, and nothing
    /// process-global is installed.
    Testing,
}

/// Build and boot the application.
pub async fn boot(mode: Mode) -> Result<Arc<Application>> {
    let database = connect(mode).await?;
    let env = Env::load_or_default(".env");

    let mut builder = Rainier::new(".");
    if mode == Mode::Testing {
        // A test suite boots an application per test; installing a global
        // subscriber each time would have them fighting over process state.
        builder = builder.without_tracing();
    }

    builder
        .configure(|c| {
            // The builder has already read `.env`; this layers the
            // application's own sections over the framework's defaults.
            if let Err(e) = config::configure(c, &env) {
                tracing::error!(error = %e, "configuration failed");
            }
        })
        .with_sessions(sessions(&env, &database))
        .with_views(Arc::new(
            // Templates are re-read on every render outside production, so an
            // edit shows up without a restart.
            match mode {
                Mode::Running => BladeEngine::new("resources/views"),
                Mode::Testing => BladeEngine::new("resources/views").without_cache(),
            },
        ))
        .with_database(database.clone())
        .with_provider(AppServiceProvider { mode, database })
        .with_middleware(kernel::register)
        .with_events(EventServiceProvider::register_listeners)
        .with_routes(|router| {
            // Web first, so `/` is matched before any catch-all the API adds.
            // Routes are tried in declaration order and the first match wins.
            routes::web::routes(router);
            routes::api::routes(router);
        })
        .boot()
        .await
}

/// Build the session store from `config/session.rs`.
///
/// A function rather than a line in the builder because the driver is a
/// branch, and a branch in the middle of a builder chain is where a wiring
/// mistake hides.
fn sessions(env: &Env, database: &Database) -> SessionManager {
    let lifetime = chrono::Duration::seconds(env.int("SESSION_LIFETIME", 7200));

    let store: Arc<dyn SessionStore> = match env.string("SESSION_DRIVER", "memory").as_str() {
        "database" => Arc::new(DatabaseSessionStore::new(database.clone()).with_lifetime(lifetime)),
        // Anything unrecognised falls back to memory rather than failing the
        // boot: a typo in a driver name should not take the application down,
        // and the log line says what happened.
        other => {
            if other != "memory" {
                tracing::warn!(driver = %other, "unknown SESSION_DRIVER; using memory");
            }
            Arc::new(MemorySessionStore::new(lifetime))
        }
    };

    SessionManager::with_config(
        store,
        SessionConfig::default()
            .cookie(env.string("SESSION_COOKIE", "rainier_session"))
            .secure(env.bool("SESSION_SECURE", false))
            .same_site(SameSite::Lax)
            .lifetime(lifetime),
    )
}

/// Open the database.
///
/// SQLite in memory by default, so a fresh clone runs with no setup. Point
/// `DATABASE_URL` at MySQL or Postgres and nothing else changes — that is the
/// ORM's whole premise.
async fn connect(mode: Mode) -> Result<Database> {
    use polyormous::{PoolConfig, SeaOrmExecutor};

    let url = rainier_framework::config::Env::load_or_default(".env")
        .string("DATABASE_URL", "sqlite::memory:");

    // An in-memory SQLite database exists only as long as its connection, so
    // a pool of five would produce five empty databases. The serverless preset
    // is `max = 1`, which keeps exactly one.
    let pool = if url.starts_with("sqlite::memory:") || mode == Mode::Testing {
        PoolConfig::serverless()
    } else {
        PoolConfig::default()
    };

    let executor = SeaOrmExecutor::connect(&url, &pool)
        .await
        .map_err(|e| Error::internal(format!("could not connect to `{url}`: {e}")))?;

    // No `bind_executor!` here: `SeaOrmExecutor` belongs to polyORMous and
    // `Connection` to the framework, so the orphan rule puts that impl out of
    // an application's reach. Rainier ships it behind the `sea-orm-executor`
    // feature, which this crate enables. Use `bind_executor!` for an executor
    // *you* wrote.
    Ok(Database::new(executor))
}
