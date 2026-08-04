//! Bootstrapping — `bootstrap/app.php`.
//!
//! One function that assembles the application: configuration, views, the
//! database, providers, middleware, listeners, routes. Everything the rest of
//! the app relies on is wired here, in one readable place.

use std::sync::Arc;

use rainier_framework::cache::{CacheManager, CacheResources};
use rainier_framework::config::Config;
use rainier_framework::config::Env;
use rainier_framework::crypt::{Encryption, Key, KeyRing};
use rainier_framework::database::Database;
use rainier_framework::http::SameSite;
use rainier_framework::observability::{MetricsSettings, OpenApiSettings, TelemetrySettings};
use rainier_framework::prelude::*;
use rainier_framework::queue::{JobRegistry, MemoryQueue, QueueManager};
use rainier_framework::session::{
    CacheSessionStore, CookieSessionStore, DatabaseSessionStore, MemorySessionStore, SessionConfig,
    SessionManager, SessionStore,
};
use rainier_framework::view::{TemplateEngine, Vite};

use crate::app::http::kernel;
use crate::app::jobs::NotifyAuthor;
use crate::app::providers::{AppServiceProvider, EventServiceProvider, RepositoryServiceProvider};
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
    let env = Env::load_or_default(".env");

    // One registry, shared between the socket handler and anything else that
    // wants to push into a room.
    let rooms = Arc::new(rainier_framework::websocket::Rooms::new());

    // Observability is read from `config/` before the builder runs, because
    // whether metrics exist decides whether the middleware is installed — and
    // middleware is declared, not added later.
    let settings = Config::new();
    config::configure(&settings, &env)?;

    // After configuration, which is what it reads — `DATABASE_URL` lands in
    // the tree like every other setting rather than being pulled from the
    // raw environment here.
    let database = connect(mode, &settings).await?;

    // And the cache, from the same tree. Built here rather than left to the
    // framework because the session store needs one *before* the application
    // boots, and building a second would give sessions a different backend from
    // the locks and rate limits — see `cache`.
    let cache = cache(&settings).await?;

    let metrics = MetricsSettings::from_config(&settings);
    let telemetry = TelemetrySettings::from_config(&settings);
    let api_docs = OpenApiSettings::from_config(&settings);

    if telemetry.exports() {
        // Configured to export, and this build cannot. Saying so beats a
        // deployment that believes it is sending spans to a collector that
        // never hears from it.
        tracing::warn!(
            endpoint = telemetry.endpoint.as_deref().unwrap_or_default(),
            "an OTLP endpoint is configured but this binary was built without the `otlp` \
             feature; traces are propagated but not exported"
        );
    }

    let registry = metrics.registry();

    let mut builder = Rainier::new(".");
    if mode == Mode::Testing {
        // A test suite boots an application per test; installing a global
        // subscriber each time would have them fighting over process state.
        builder = builder.without_tracing();
    }

    // Bound only when metrics are on, so `/metrics` answers 404 rather than an
    // empty scrape that looks like an idle application.
    if let Some(metrics) = registry.clone() {
        builder = builder.with_instance_arc(metrics);
    }

    // Every job a worker must be able to run. A job missing from here fails at
    // run time with "no job is registered as …", so add to this list whenever
    // you add a job.
    //
    // It belongs on the builder and not in a provider, and that is not a style
    // preference: providers register *after* the queue is built, because a
    // provider may legitimately resolve one. A registry handed over in a
    // provider therefore reaches nothing, and the job it was carrying is
    // dispatched, accepted, and fails when a worker picks it up.
    let jobs = JobRegistry::new().with::<NotifyAuthor>();

    builder = match mode {
        // In memory under test: a test wants a job *queued* so it can assert on
        // it, not run under its feet. Handed over whole rather than declared,
        // because `config/queue.rs` describes a deployment and a test is not
        // one — and because handing one over is how you *deliberately* override
        // the section.
        Mode::Testing => {
            builder.with_queue(QueueManager::new(Arc::new(MemoryQueue::new()), Arc::new(jobs)))
        }
        // Otherwise whatever `config/queue.rs` declared, built by the framework
        // from each connection's own settings.
        Mode::Running => builder.with_jobs(jobs),
    };

    let app = builder
        .configure(|c| {
            // The builder has already read `.env`; this layers the
            // application's own sections over the framework's defaults.
            if let Err(e) = config::configure(c, &env) {
                tracing::error!(error = %e, "configuration failed");
            }

            // Said here rather than left to `APP_ENV`, because unset means
            // **production** — the right default for a deployment and the
            // wrong one for a test. Several of the framework's boot checks
            // refuse in production where they would otherwise warn, so a
            // suite that did not say this would fail to boot on the honest
            // answer to a question it never meant to ask.
            if mode == Mode::Testing {
                let _ = c.set(config::keys::APP_ENV, AppEnv::Testing);
            }
        })
        .with_sessions(sessions(&env, &database, &cache)?)
        // The cache the framework would otherwise have built itself, built from
        // the same declared section it would have read. Supplying one *wins*
        // over `config/cache.rs` — which is exactly the silent divergence the
        // missing `storage()` below warns about — so what makes this safe is
        // that it is not a second answer: it is the section's own answer,
        // resolved once so that sessions, locks and rate limits share it.
        .with_cache(cache.clone())
        // Uploaded files are *not* wired here. `config/storage.rs` declares
        // the disks and the framework builds them — see the note where this
        // application's own `storage()` builder used to be. Calling
        // `with_instance` with a `Storage` would silently win over every
        // declared disk.
        .with_views(Arc::new(
            // Templates are re-read on every render outside production, so an
            // edit shows up without a restart. The Vite resolver rides along
            // for the layout's `@vite` — over `public`, where `npm run dev`
            // writes `hot` and `npm run build` writes `build/manifest.json`.
            // (The framework's *default* engine attaches one itself; an
            // application supplying its own engine attaches its own.)
            match mode {
                Mode::Running => {
                    TemplateEngine::new("resources/views").with_vite(Vite::new("public"))
                }
                Mode::Testing => TemplateEngine::new("resources/views")
                    .without_cache()
                    .with_vite(Vite::new("public").without_cache()),
            },
        ))
        .with_database(database.clone())
        // Registration order is declaration order. It does not matter here —
        // `register` binds factories and resolves nothing — but keeping the
        // repositories first reads as the dependency direction.
        .with_provider(RepositoryServiceProvider { database: database.clone() })
        .with_provider(AppServiceProvider { mode, database })
        // Sockets, on the same port as everything above. The `Rooms` registry
        // is bound too, so a controller can push into a room from an ordinary
        // HTTP request — which is most of what a socket is for.
        .with_websockets(routes::ws::routes(Arc::clone(&rooms)))
        .with_instance_arc(rooms)
        .with_schedule(routes::console::schedule)
        .with_middleware({
            let trace = telemetry.middleware();
            move |registry| kernel::register(registry, trace)
        })
        .with_events(EventServiceProvider::register_listeners)
        .with_routes(|router| {
            // Web first, so `/` is matched before any catch-all the API adds.
            // Routes are tried in declaration order and the first match wins.
            routes::web::routes(router);
            routes::api::routes(router, registry.clone());
        })
        .boot()
        .await?;

    // The document needs the *compiled* router, which only exists once the
    // application has booted — so it is rendered here and bound, rather than
    // handed to the builder.
    if api_docs.enabled {
        let router = app.resolve::<rainier_framework::routing::CompiledRouter>()?;
        if let Some(rendered) = api_docs.render(routes::openapi::document(), &router) {
            app.instance_arc(rendered);
        }
    }

    Ok(app)
}

/// Build the session store from `config/session.rs`.
///
/// A function rather than a line in the builder because the driver is a
/// branch, and a branch in the middle of a builder chain is where a wiring
/// mistake hides.
fn sessions(env: &Env, database: &Database, cache: &CacheManager) -> Result<SessionManager> {
    let lifetime = chrono::Duration::seconds(env.int("SESSION_LIFETIME", 7200));

    // An exhaustive `match` on a closed set, not a string compare with a
    // fallback arm. Two things follow. A misspelled `SESSION_DRIVER` fails here
    // with the list of valid values, instead of quietly logging everyone out on
    // every deploy. And adding a store to the framework makes *this* a compile
    // error — which is the correct list of places that need to learn about it.
    let store: Arc<dyn SessionStore> = match env.setting::<SessionDriver>("SESSION_DRIVER")? {
        SessionDriver::Memory => Arc::new(MemorySessionStore::new(lifetime)),

        SessionDriver::Database => {
            Arc::new(DatabaseSessionStore::new(database.clone()).with_lifetime(lifetime))
        }

        // Sessions in one of the stores `config/cache.rs` declared. The store
        // expires them itself, so nothing has to sweep.
        //
        // `SESSION_STORE` names *which* declared store, because sessions and
        // cached values want opposite eviction policies — see the note in
        // `config/cache.rs`. Naming one that was never declared is an error
        // rather than a fallback to the default: a session store that quietly
        // became the wrong one would log people out on the deploy that changed
        // it, and the fallback is what would have hidden the typo.
        SessionDriver::Cache => {
            let name = env.string("SESSION_STORE", "");
            let store = if name.is_empty() {
                Arc::clone(cache.store())
            } else {
                Arc::clone(cache.store_named(&name).ok_or_else(|| {
                    Error::internal(format!(
                        "SESSION_STORE={name} is not a declared cache store; declared stores are \
                         {}. See `config/cache.rs`",
                        cache.store_names().collect::<Vec<_>>().join(", ")
                    ))
                })?)
            };

            Arc::new(CacheSessionStore::new(store).with_lifetime(lifetime))
        }

        // The whole session, encrypted, in the cookie. No server state at all —
        // and therefore no way to revoke a session. See the docs before
        // choosing it.
        SessionDriver::Cookie => Arc::new(CookieSessionStore::new(Encryption::from_keys(
            KeyRing::from_base64(&env.string("APP_KEY", ""), &[])
                .unwrap_or_else(|_| KeyRing::new(Key::generate())),
        ))),
    };

    Ok(SessionManager::with_config(
        store,
        SessionConfig::default()
            .cookie(env.string("SESSION_COOKIE", "rainier_session"))
            .secure(env.bool("SESSION_SECURE", false))
            .same_site(SameSite::Lax)
            .lifetime(lifetime),
    ))
}

/// Build every cache store `config/cache.rs` declared.
///
/// This used to be a hundred lines of `match driver { … }` with a `#[cfg]` arm
/// per backend, reading `CACHE_DRIVER` out of the environment. All of it is
/// gone, and what replaced it is the section: a store names its own driver, so
/// there is no driver name here to match on.
///
/// One behaviour changed with it and it is worth saying out loud, because it is
/// a deliberate reversal. The old version treated an unreachable Redis as
/// survivable — it logged and fell back to an in-process cache — on the
/// argument that a cache is the one dependency an application should be able to
/// lose. That argument is right about *cached values* and wrong about
/// everything else this store carries. A per-process cache in place of a shared
/// one is not a degraded cache: it is a `LockManager` whose locks hold within
/// one replica and nowhere else, a rate limiter that counts to its limit once
/// per replica, and a session store that logs a user out whenever the
/// load balancer sends them somewhere new. None of those report anything.
///
/// So a store that cannot be reached now fails the boot, naming it. A driver
/// whose cargo feature is missing does the same, for the reason it always did.
async fn cache(settings: &Config) -> Result<CacheManager> {
    // `CacheResources` is for the one driver no configuration file can
    // describe: Workers KV needs a binding that exists inside a Worker and an
    // API client outside one. This application declares no `kv` store, so it
    // has nothing to carry — and a store that needed one and was not given it
    // would be a boot failure naming the missing piece, not a store that
    // quietly became something else.
    settings.require(rainier_framework::keys::CACHE_STORES)?.build(&CacheResources::new()).await
}

// There is no `storage()` builder here any more, and its absence is the point.
//
// It used to read one driver and one set of connection settings and hand the
// result to `with_instance`. Two things were wrong with that. It could describe
// only a single disk, so an application wanting a second one on another service
// had nowhere to say so. And supplying storage explicitly *overrides* the
// declared `filesystems` section, so an application that declared disks and
// also called this got the single disk and no error about the rest.
//
// `config/storage.rs` now declares what exists and the framework builds each
// disk from its own settings. Everything this function did is still done —
// including resolving a credential chain asynchronously — just not here.

/// Open the database.
///
/// SQLite in memory by default, so a fresh clone runs with no setup. Point
/// `DATABASE_URL` at MySQL or Postgres and nothing else changes — that is the
/// ORM's whole premise.
async fn connect(mode: Mode, settings: &Config) -> Result<Database> {
    // The pool config is the ORM's; the executor is a *driver*. Rainier keeps
    // service interfacing in `rainier-drivers` so the ORM core has no optional
    // dependencies and stays compilable for wasm.
    use rainier_framework::drivers::sql::SeaOrmExecutor;
    use rainier_orm::PoolConfig;

    let url = settings.get_or(config::keys::DATABASE_URL, "sqlite::memory:".into());

    // An in-memory SQLite database exists only as long as the connection
    // holding it, so the pool must keep exactly one **and never reap it**.
    // `serverless()` is a pool of one and looks right, but it closes an idle
    // connection after two seconds — which migrates cleanly at boot and then
    // answers `no such table` to the first request a human makes.
    let pool = if url.starts_with("sqlite::memory:") || mode == Mode::Testing {
        PoolConfig::in_memory()
    } else {
        PoolConfig::default()
    };

    let executor = SeaOrmExecutor::connect(&url, &pool)
        .await
        .map_err(|e| Error::internal(format!("could not connect to `{url}`: {e}")))?;

    // No `bind_executor!` here: `SeaOrmExecutor` belongs to `rainier-drivers`
    // and `Connection` to the framework, so the orphan rule puts that impl out
    // of an application's reach. Rainier ships it behind the `sea-orm-executor`
    // feature, which this crate enables. Use `bind_executor!` for an executor
    // *you* wrote.
    Ok(Database::new(executor))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Configuration as `boot` reads it: the application's own sections over a
    /// fresh tree, from a given environment.
    fn settings_from(env: &Env) -> Config {
        let settings = Config::new();
        config::configure(&settings, env).unwrap();
        settings
    }

    #[tokio::test]
    async fn whatever_declares_the_cache_also_reaches_the_code_that_opens_it() {
        // The same regression `config/database.rs` has a test named for, in the
        // shape the cache takes — and it is worth its own assertion because the
        // cache's version of it is the quietest of the four. A section declared
        // in `config/cache.rs` and a manager built from anything else is not an
        // error anybody sees: a read from the wrong store is a miss, a miss is
        // not a failure, so the application is merely slow and its locks are
        // not locks.
        //
        // What has to be true is that `cache` builds *the declared section* and
        // not a second answer assembled beside it.
        let settings = settings_from(&Env::parse(""));
        let manager = cache(&settings).await.expect("the declared stores build");

        let declared = settings.require(rainier_framework::keys::CACHE_STORES).unwrap();
        assert_eq!(manager.driver(), "memory");
        assert!(manager.has_store(declared.default_name()));

        // Every declared store is reachable by the name it was declared under,
        // so `SESSION_STORE` naming one resolves to that one.
        for name in declared.names() {
            assert!(manager.has_store(name), "`{name}` was declared and was not built");
        }
    }

    #[tokio::test]
    async fn a_session_store_naming_an_undeclared_cache_store_stops_the_boot() {
        // Not a fallback to the default. A session store that quietly became
        // the wrong one would sign everybody out on the deploy that changed it,
        // and the fallback is what would have hidden the typo.
        let env = Env::parse("SESSION_DRIVER=cache\nSESSION_STORE=shard");
        let settings = settings_from(&env);
        let cache = cache(&settings).await.expect("the declared stores build");
        let (database, _) = rainier_framework::database::testing::fake_database(
            rainier_framework::database::testing::MemoryConnection::new(
                rainier_framework::database::Dialect::Sqlite,
            ),
        );

        let err = sessions(&env, &database, &cache).expect_err("`shard` is not declared");

        assert!(err.message().contains("SESSION_STORE"), "{}", err.message());
        // And it lists what *is* declared, so the fix is a read rather than a
        // search.
        assert!(err.message().contains("memory"), "{}", err.message());
    }

    #[tokio::test]
    async fn the_default_session_store_is_the_one_the_cache_section_defaults_to() {
        // `SESSION_STORE` unset means "wherever the cache lives", which is the
        // single-Redis deployment and the right place to start. The assertion
        // is that it resolves at all — a version of this that looked the name
        // up unconditionally would fail on the empty string.
        let env = Env::parse("SESSION_DRIVER=cache");
        let settings = settings_from(&env);
        let cache = cache(&settings).await.expect("the declared stores build");
        let (database, _) = rainier_framework::database::testing::fake_database(
            rainier_framework::database::testing::MemoryConnection::new(
                rainier_framework::database::Dialect::Sqlite,
            ),
        );

        assert!(sessions(&env, &database, &cache).is_ok());
    }
}
