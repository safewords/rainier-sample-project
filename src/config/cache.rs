//! `config/cache.php`.
//!
//! A **default** store plus every store the application declares by name, each
//! naming its own driver and its own settings — the fourth section of the same
//! shape as `storage.rs`, `queue.rs` and `database.rs`.
//!
//! `memory` is the default. It is per-process, which is right for development
//! and for a test suite and wrong the moment a second instance exists: two
//! replicas each hold their own copy, so a rate limiter counts to its limit
//! *twice* and a lock is a lock within one process and nothing at all between
//! two.
//!
//! # Why a cache is worth a section when the others already have one
//!
//! Because a cache's failures are the quietest of the four. Everything
//! downstream of a cache is built to treat absence as normal — a miss is not an
//! error, a value that is not there is recomputed — so a store pointed at the
//! wrong server never reports anything. It is not an outage. It is a permanent
//! miss, which reads as a slow application. And when what was cached was a
//! lock or a rate-limit counter, it is not slow, it is wrong.
//!
//! One driver plus one set of connection settings cannot express two stores on
//! two servers, and building the second from the first's connector gives it the
//! right *name* pointed at the wrong host.
//!
//! # Why `sessions` is its own store and not a second name for `shared`
//!
//! They want opposite things from the server. A cache should evict under memory
//! pressure — `allkeys-lru` is the correct policy for one, because dropping a
//! cached value costs a recomputation. A session store must not: evicting a
//! session logs somebody out, and under load it logs out whoever has been idle
//! longest, which is exactly the person about to come back.
//!
//! One Redis cannot hold both policies, so they are two declarations. Sharing
//! one and hoping is how a deploy under load signs everybody out at once with
//! nothing in any log to say why.
//!
//! # `CACHE_DRIVER` is not a variable this application has
//!
//! The same retirement `STORAGE_DRIVER` and `QUEUE_DRIVER` got, for the same
//! reason: it names one store, this file declares several, and the framework
//! refuses the two together rather than letting one silently lose.
//!
//! The refusal is re-stated in [`configure`] rather than left to the framework,
//! and that is not belt-and-braces. The framework's check lives on the path that
//! *builds* the cache, and `bootstrap.rs` hands over a built manager with
//! `Rainier::with_cache` so that sessions and locks share one store — which
//! skips that path, and with it the check. Setting `CACHE_DRIVER` went from a
//! boot failure to silently ignored. Re-stating it here puts it back.
//!
//! It survives in one place and it is worth knowing which. `cargo rainier
//! features` sizes a Docker image by reading driver names out of an environment
//! file, and it cannot read a section — so `.env.build` still says
//! `CACHE_DRIVER=redis` to compile the client in. That file never reaches the
//! running container; see the `Dockerfile`, where the distinction is written
//! down beside the `COPY` that keeps it out.
//!
//! # What a Redis store waits for, and why it has no pool
//!
//! The connection multiplexes: one socket carries every concurrent command and
//! replies are matched to requests by id. A pool on top would open more sockets
//! without moving more commands, so there is nothing to size — `max_connections`
//! on a Redis store is refused by name rather than accepted and ignored.
//!
//! What it can honour is three settings, and `reconnect` is the one to reach
//! for first. **A multiplexed connection does not re-open itself.** One socket
//! dropped by an idle proxy ends the driver task, and every command on every
//! clone of that handle fails for the life of the process — in an application
//! where the same Redis carries sessions, locks and rate limits at once, and
//! where nothing in the symptom names Redis.
//!
//! All three are off unless declared, so a store that says nothing behaves as it
//! did before they existed. Which is why they are declared here.

use std::time::Duration;

use rainier_framework::cache::{MemcachedStore, MemoryStore, RedisStore, Stores};
use rainier_framework::config::{Config, Env};
use rainier_framework::keys::CACHE_STORES;
use rainier_framework::prelude::*;

/// What namespaces this application's keys on a server it shares.
///
/// A literal rather than a setting: it is a property of the application, and a
/// deployment that changed it would only be renaming its own keys out from
/// under itself.
///
/// It is **not** written to the `cache.prefix` configuration key, and that is a
/// deliberate omission rather than a gap. The framework reads that key on one
/// path only — the `CACHE_DRIVER` shorthand this application does not use — so
/// setting it here would write a value nothing reads, and leave the next person
/// to change it expecting an effect. A prefix belongs to a store, so each store
/// below carries it.
const PREFIX: &str = "rainier_sample";

/// Cache settings, read back by the framework's own bootstrap.
///
/// Records declarations; it opens no connection. That is what keeps "which
/// stores exist" answerable from configuration alone, including by a test that
/// never reaches a server.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // The refusal the framework would have made, made here instead — because
    // this application took it away from it.
    //
    // Rainier refuses `CACHE_DRIVER` beside a declared `cache.stores` section.
    // But that check lives in `build_cache`, and `build_cache` does not run when
    // an application hands over a manager with `Rainier::with_cache` — which
    // `bootstrap.rs` does, so that sessions, locks and rate limits share one
    // store. The guard was suppressed as a side effect, and the variable went
    // from a boot failure to silently ignored.
    //
    // That is the worst of the three outcomes. An operator migrating an older
    // `.env` sets `CACHE_DRIVER=redis`, gets an in-process cache, and nothing
    // says anything: locks that hold within one replica, a rate limiter that
    // counts to its limit once per replica, sessions that vanish as the load
    // balancer moves somebody. Exactly the failure this section exists to
    // prevent, reintroduced by the wiring that reads it.
    if env.get("CACHE_DRIVER").is_some_and(|driver| !driver.trim().is_empty()) {
        return Err(Error::internal(
            "`CACHE_DRIVER` is set and `config/cache.rs` declares a `cache.stores` section. They \
             are two answers to one question and this application will not choose between them: a \
             read from the wrong store is a miss, and a miss is not a failure, so the only symptom \
             would be an application that is merely slow and locks that are not locks. Drop the \
             variable and name a declared store with `CACHE_STORE` instead — see `.env.example`",
        ));
    }

    // The store everything uses unless it names another. It must be one of the
    // stores declared below; the framework checks that before building
    // anything, so a typo fails immediately rather than after opening
    // connections that were never going to be used.
    let default = env.string("CACHE_STORE", "memory");

    // Always declared, because it needs nothing configured: a fresh clone and a
    // test suite both need somewhere for a cached value to go.
    let mut stores = Stores::new(default).with("memory", MemoryStore::new().prefix(PREFIX));

    // The shared store, declared only when a server is named. A Redis store
    // built from an empty URL is not a store that fails later — it is one that
    // dials nothing — so leaving it undeclared is what lets the framework say
    // "no such store" instead.
    let url = env.string("REDIS_URL", "");
    if !url.is_empty() {
        stores = stores.with("shared", tuned(RedisStore::new(url)));
    }

    // Sessions on their own server, for the eviction-policy reason in the
    // module docs. Declared only when one is named: without it, `CACHE_STORE`
    // and the session store both fall back to whatever the default is, which is
    // the single-Redis deployment and a perfectly reasonable place to start.
    let session_url = env.string("SESSION_REDIS_URL", "");
    if !session_url.is_empty() {
        stores = stores.with("sessions", tuned(RedisStore::new(session_url)));
    }

    // An alternative to `shared` rather than a companion to it — a deployment
    // has one or the other, and points `CACHE_STORE` at whichever it declared.
    //
    // It is here for the contrast, which is what makes the Redis stores' lack
    // of a pool a design rather than a gap. A Memcached connection has no
    // request ids: replies are matched to requests by order, so one connection
    // serves one command at a time and concurrency genuinely needs more of
    // them. Hence `pool_size` here and no pool setting at all above.
    let memcached_url = env.string("MEMCACHED_URL", "");
    if !memcached_url.is_empty() {
        stores = stores
            .with("memcached", MemcachedStore::new(memcached_url).prefix(PREFIX).pool_size(8));
    }

    config.set(CACHE_STORES, stores)?;

    Ok(())
}

/// The connection settings every Redis store here shares.
///
/// A function rather than a repetition, because the three values are a decision
/// about *this application's* tolerance rather than about a particular server,
/// and two copies of a decision drift.
fn tuned(store: RedisStore) -> RedisStore {
    store
        .prefix(PREFIX)
        // Milliseconds, and it has to be: a cache read is on the hot path of
        // nearly every request, so its budget cannot be written in whole
        // seconds — where the only values available are `0`, which fails
        // everything, and `1`, which is already longer than a request can
        // afford to wait.
        .response_timeout(Duration::from_millis(250))
        // Booting against a route that goes nowhere should say so in a second,
        // not in the several minutes a TCP connect takes to give up.
        .connect_timeout(Duration::from_secs(1))
        // The important one. Without it a single dropped socket — an idle proxy
        // is the usual culprit — breaks the cache until the process is
        // restarted, and nothing about the symptom points at Redis.
        .reconnect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::cache::{CacheDriver, StoreConfig};

    fn stores_from(env: &Env) -> Stores {
        let config = Config::new();
        configure(&config, env).unwrap();
        config.get(CACHE_STORES).expect("the cache section is set")
    }

    #[test]
    fn a_deployment_that_configures_nothing_still_has_somewhere_to_cache() {
        let stores = stores_from(&Env::parse(""));

        assert_eq!(stores.default_name(), "memory");
        assert_eq!(stores.get("memory").expect("declared").driver(), CacheDriver::Memory);
        assert!(stores.get("shared").is_none());
    }

    #[test]
    fn a_shared_store_is_declared_only_when_a_server_names_one() {
        // Not a store that fails on first use: a Redis store with an empty URL
        // dials nothing. Leaving it undeclared is what lets the framework
        // answer "no such store", which is a different sentence from a miss —
        // and a miss is what every other cache failure looks like.
        assert!(stores_from(&Env::parse("")).get("shared").is_none());

        let declared = stores_from(&Env::parse("REDIS_URL=redis://cache.internal:6379/"));
        assert_eq!(declared.get("shared").expect("declared").driver(), CacheDriver::Redis);
    }

    #[test]
    fn sessions_are_their_own_store_and_not_the_shared_ones_connector() {
        let stores = stores_from(&Env::parse(
            "REDIS_URL=redis://cache.internal:6379/\n\
             SESSION_REDIS_URL=redis://sessions.internal:6379/",
        ));

        let shared = format!("{:?}", stores.get("shared").expect("declared"));
        let sessions = format!("{:?}", stores.get("sessions").expect("declared"));

        // The assertion that matters. A configuration that built the second
        // from the first's connector would pass every other test here, and put
        // sessions on the server whose eviction policy throws them away.
        assert_ne!(shared, sessions);
        assert!(sessions.contains("sessions.internal"), "{sessions}");
    }

    #[test]
    fn a_redis_store_reconnects_and_bounds_what_it_waits_for() {
        // All three are off unless declared, so asserting they are on is
        // asserting a decision rather than a default. The reconnection one is
        // the one that turns a dropped socket from a broken process into a
        // pause.
        let stores = stores_from(&Env::parse("REDIS_URL=redis://cache.internal:6379/"));
        let StoreConfig::Redis(store) = stores.get("shared").expect("declared") else {
            panic!("the shared store should be Redis");
        };

        let settings = store.connection_settings();
        assert!(settings.reconnects(), "a multiplexed connection that cannot re-open is a restart");
        assert_eq!(settings.response_timeout(), Some(Duration::from_millis(250)));
        assert_eq!(settings.connect_timeout(), Some(Duration::from_secs(1)));
    }

    #[test]
    fn the_retired_driver_variable_stops_the_boot_rather_than_being_ignored() {
        // The guard `bootstrap.rs` took away by handing the framework a built
        // manager. Without this the variable is *silently* ignored, which is
        // the one outcome worse than either answer winning: an operator
        // migrating an older `.env` gets an in-process cache and no error.
        let err = configure(&Config::new(), &Env::parse("CACHE_DRIVER=redis")).unwrap_err();

        assert!(err.message().contains("CACHE_DRIVER"), "{}", err.message());
        // And it names the way out, because the fix is a different variable
        // rather than a different value.
        assert!(err.message().contains("CACHE_STORE"), "{}", err.message());

        // An empty value is not a selection, so *this* check lets it through.
        // That is this check's own semantics and not a reading of the
        // framework's: an empty `CACHE_DRIVER` never reaches here, because the
        // framework parses the variable strictly in its own configuration pass
        // and fails first.
        //
        // So a blanked line does not boot, and should not. An empty value also
        // arrives from `CACHE_DRIVER=$SOMETHING_UNSET` in a compose file, where
        // nobody wrote an empty string down — and treating that as "unset"
        // would hand the deployment an in-process cache, which is the failure
        // the check above exists to stop. Delete the line; do not blank it.
        // Rainier's `docs/configuration.md` has it under "An empty value is not
        // an unset one".
        assert!(configure(&Config::new(), &Env::parse("CACHE_DRIVER=")).is_ok());
    }

    #[test]
    fn the_default_store_follows_the_deployment() {
        assert_eq!(stores_from(&Env::parse("")).default_name(), "memory");
        assert_eq!(
            stores_from(&Env::parse("CACHE_STORE=shared\nREDIS_URL=redis://cache:6379/"))
                .default_name(),
            "shared"
        );
    }

    #[test]
    fn every_store_namespaces_its_keys_and_no_variable_can_change_that() {
        // The prefix travels with the store rather than with the process, which
        // is what makes it true of the store the framework actually builds. An
        // earlier version wrote it to `cache.prefix` instead, where nothing on
        // this path reads it — so two applications on one Redis would have
        // collided on every key while the configuration said otherwise.
        let stores = stores_from(&Env::parse(
            "CACHE_PREFIX=other\n\
             REDIS_URL=redis://cache.internal:6379/\n\
             SESSION_REDIS_URL=redis://sessions.internal:6379/",
        ));

        for name in ["memory", "shared", "sessions"] {
            assert_eq!(
                stores.get(name).expect("declared").prefix(),
                Some(PREFIX),
                "`{name}` does not namespace its keys"
            );
        }
    }

    #[test]
    fn only_the_store_with_a_pool_declares_a_pool_size() {
        // The contrast, asserted rather than only described. Redis multiplexes,
        // so a pool would add sockets without moving commands and the setting
        // is refused by name; Memcached matches replies to requests by order,
        // so concurrency needs more connections.
        let stores = stores_from(&Env::parse(
            "REDIS_URL=redis://cache.internal:6379/\nMEMCACHED_URL=cache.internal:11211",
        ));

        let StoreConfig::Memcached(memcached) = stores.get("memcached").expect("declared") else {
            panic!("the memcached store should be Memcached");
        };
        assert_eq!(memcached.pool_limit(), Some(8));

        // Not vacuous: the Redis store has no such method to call, which is the
        // strongest form the point can take — the setting is unwritable rather
        // than ignored. What is assertable here is that it is still a Redis
        // store and not something a pooling deployment quietly turned into one.
        assert_eq!(stores.get("shared").expect("declared").driver(), CacheDriver::Redis);
    }

    #[test]
    fn a_credential_never_reaches_a_rendering_of_the_section() {
        // A configuration dump at boot must not put the Redis password into the
        // log of every process that started.
        let stores = stores_from(&Env::parse("REDIS_URL=redis://user:super-secret@cache:6379/"));

        let rendered = format!("{stores:?}");
        assert!(!rendered.contains("super-secret"), "{rendered}");
        // Not vacuous — the store itself does render.
        assert!(rendered.contains("cache"), "{rendered}");
    }
}
