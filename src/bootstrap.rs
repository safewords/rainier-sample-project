//! Bootstrapping — `bootstrap/app.php`.
//!
//! One function that assembles the application: configuration, views, the
//! database, providers, middleware, listeners, routes. Everything the rest of
//! the app relies on is wired here, in one readable place.

use std::sync::Arc;

use rainier_framework::cache::{Cache, MemoryCache};
use rainier_framework::config::Config;
use rainier_framework::config::Env;
use rainier_framework::crypt::{Encryption, Key, KeyRing};
use rainier_framework::database::Database;
use rainier_framework::http::SameSite;
use rainier_framework::observability::{MetricsSettings, OpenApiSettings, TelemetrySettings};
use rainier_framework::prelude::*;
use rainier_framework::session::{
    CacheSessionStore, CookieSessionStore, DatabaseSessionStore, MemorySessionStore, SessionConfig,
    SessionManager, SessionStore,
};
use rainier_framework::view::TemplateEngine;

use crate::app::http::kernel;
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
        .with_sessions(sessions(&env, &database)?)
        // Uploaded files. Local by default; `STORAGE_DRIVER=s3` (behind the
        // `s3` cargo feature) reaches AWS, R2, MinIO — anything that speaks
        // the API.
        .with_instance(storage(&settings).await?)
        .with_views(Arc::new(
            // Templates are re-read on every render outside production, so an
            // edit shows up without a restart.
            match mode {
                Mode::Running => TemplateEngine::new("resources/views"),
                Mode::Testing => TemplateEngine::new("resources/views").without_cache(),
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
fn sessions(env: &Env, database: &Database) -> Result<SessionManager> {
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

        // Sessions in whatever the cache is pointed at — Redis, a Redis
        // Cluster, or Memcached, all behind one port. The cache expires them
        // itself, so nothing has to sweep.
        SessionDriver::Cache => {
            Arc::new(CacheSessionStore::new(cache(env)?).with_lifetime(lifetime))
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

/// Build the cache from `config/cache.rs`.
///
/// Three failure modes, and they get three different answers:
///
/// | | |
/// |---|---|
/// | `CACHE_DRIVER=redys` | **error** — a value outside the set, caught by `setting` |
/// | `CACHE_DRIVER=redis`, no `redis-driver` feature | **error** — naming the feature to enable |
/// | `CACHE_DRIVER=redis`, Redis unreachable | warn, fall back to memory |
///
/// Only the last is a runtime condition the application can be expected to
/// survive. The first two are mistakes in the deployment, and a cache that
/// silently is not the one you asked for is worse than a boot that stops.
fn cache(env: &Env) -> Result<Arc<dyn Cache>> {
    let driver = env.setting::<CacheDriver>("CACHE_DRIVER")?;

    match driver {
        CacheDriver::Memory => Ok(Arc::new(MemoryCache::new())),

        CacheDriver::Redis | CacheDriver::RedisCluster => {
            #[cfg(feature = "redis")]
            {
                use rainier_framework::drivers::RedisConnector;

                let url = env.string("REDIS_URL", "redis://127.0.0.1:6379/");
                let connector = if driver == CacheDriver::RedisCluster {
                    #[cfg(feature = "redis-cluster")]
                    {
                        let seeds: Vec<String> =
                            url.split(',').map(str::trim).map(String::from).collect();
                        RedisConnector::open_cluster(seeds)
                    }
                    #[cfg(not(feature = "redis-cluster"))]
                    {
                        return Err(missing_feature(driver));
                    }
                } else {
                    RedisConnector::open(&url)
                };

                // Connecting is async and this is not, so the connection is
                // opened on a blocking handle. A cache that is briefly
                // unreachable must not stop the application booting, so *this*
                // failure — unlike the two above — falls back with a loud line.
                match connector.and_then(|connector| {
                    tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current()
                            .block_on(rainier_framework::cache::RedisCache::connect(&connector))
                    })
                }) {
                    Ok(redis) => Ok(Arc::new(redis)),
                    Err(e) => {
                        tracing::error!(
                            error = %e.message(),
                            "could not reach Redis; using memory"
                        );
                        Ok(Arc::new(MemoryCache::new()))
                    }
                }
            }
            #[cfg(not(feature = "redis"))]
            {
                Err(missing_feature(driver))
            }
        }

        CacheDriver::Memcached => {
            #[cfg(feature = "memcached")]
            {
                use rainier_framework::cache::MemcachedCache;
                use rainier_framework::drivers::MemcachedConnector;

                let url = env.string("MEMCACHED_URL", "127.0.0.1:11211");
                Ok(Arc::new(MemcachedCache::new(MemcachedConnector::open(url))))
            }
            #[cfg(not(feature = "memcached"))]
            {
                Err(missing_feature(driver))
            }
        }

        // This application does not build the DynamoDB store. Saying so beats
        // a `_ =>` arm, which would swallow a driver added to the framework
        // later.
        CacheDriver::DynamoDb => Err(Error::internal(
            "CACHE_DRIVER=dynamodb is not wired up in this application; see `bootstrap::cache`",
        )),

        // Workers KV needs either a Worker binding or REST credentials, and
        // neither is something this application has. Worth knowing before
        // reaching for it anyway: KV is eventually consistent and has no
        // compare-and-set, so it cannot back the session store or the
        // scheduler's locks — both of which this application uses.
        //
        // This arm is why the `DynamoDb` one above is spelled out rather than
        // written as `_`: the framework added a driver in 1.1.0, and the
        // compiler said so here instead of the new variant silently falling
        // into a catch-all.
        CacheDriver::Kv => Err(Error::internal(
            "CACHE_DRIVER=kv is not wired up in this application; see `bootstrap::cache`",
        )),
    }
}

/// The driver exists but this build cannot construct it.
///
/// A different message from "unknown driver" on purpose: the fix is a cargo
/// feature, not a spelling correction, and the message says which one.
#[allow(dead_code, reason = "used only by the arms whose feature is off")]
fn missing_feature(driver: CacheDriver) -> Error {
    match driver.feature() {
        Some(feature) => Error::internal(format!(
            "CACHE_DRIVER={driver} needs the `{feature}` cargo feature, which this build does \
             not have"
        )),
        None => Error::internal(format!("CACHE_DRIVER={driver} is not available in this build")),
    }
}

/// Build the file storage from `config/storage.rs`.
///
/// In `bootstrap` rather than a provider because the S3 arm resolves a
/// credential chain, which is async — the same reason a connecting
/// broadcaster lives here.
async fn storage(settings: &Config) -> Result<rainier_framework::filesystem::Storage> {
    use rainier_framework::filesystem::{FilesystemDriver, Storage};

    match settings.setting(config::keys::STORAGE_DRIVER)? {
        FilesystemDriver::Local => Ok(Storage::local(
            settings.get_or(config::keys::STORAGE_ROOT, "storage/app".into()),
        )),

        FilesystemDriver::Memory => Ok(Storage::memory()),

        FilesystemDriver::S3 => {
            #[cfg(feature = "s3")]
            {
                use rainier_framework::drivers::AwsConnector;
                use rainier_framework::filesystem::S3Filesystem;

                let bucket = settings.get_or(config::keys::STORAGE_BUCKET, String::new());
                if bucket.is_empty() {
                    return Err(Error::internal(
                        "STORAGE_DRIVER=s3 needs STORAGE_BUCKET to name the bucket",
                    ));
                }

                // The default credential chain, pinned to a region when one
                // is named. An endpoint is what points the same driver at R2
                // or MinIO instead of AWS.
                let region = settings.get_or(config::keys::STORAGE_REGION, String::new());
                let mut connector = if region.is_empty() {
                    AwsConnector::from_env().await
                } else {
                    AwsConnector::in_region(region).await
                };

                let endpoint = settings.get_or(config::keys::STORAGE_ENDPOINT, String::new());
                if !endpoint.is_empty() {
                    connector = connector.endpoint(endpoint);
                }

                let mut disk = S3Filesystem::new(&connector, bucket);

                let prefix = settings.get_or(config::keys::STORAGE_URL_PREFIX, String::new());
                if !prefix.is_empty() {
                    disk = disk.with_url_prefix(prefix);
                }

                Ok(Storage::new(std::sync::Arc::new(disk)))
            }
            #[cfg(not(feature = "s3"))]
            {
                Err(Error::internal(
                    "STORAGE_DRIVER=s3 needs the `s3` cargo feature, which this build does not \
                     have",
                ))
            }
        }
    }
}

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
