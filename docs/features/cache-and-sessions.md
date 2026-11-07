# Cache and session drivers

> **Status:** STABLE
> **Owners (code):** `src/config/cache.rs`, `src/config/session.rs`, `src/bootstrap.rs` (`cache`, `sessions`)
> **Related docs:** [boot-and-configuration.md](boot-and-configuration.md), [compat-php.md](compat-php.md) (the session store wrapper); framework: [cache], [sessions]
> **Last verified against code:** 2026-11-06 @ `741ad28`

[cache]: https://github.com/safewords/rainier-framework/blob/main/docs/cache.md
[sessions]: https://github.com/safewords/rainier-framework/blob/main/docs/sessions.md

## Overview

`config/cache.rs` declares named cache stores. The cache manager built from
them backs the cache facade, locks, rate limits (the `ThrottleRequests` in
`kernel.rs`), Stripe webhook de-duplication, the accounts add-in's
throttling, and `SESSION_DRIVER=cache`. Sessions are built in `bootstrap.rs`
because the driver is a branch, and a branch inside a builder chain is an
easy place for a wiring mistake to go unnoticed (`bootstrap.rs:294-298`).
Only routes in the `web` group have a session (`kernel.rs:112-114`).

## How it works today

### Stores, by cargo feature

| Store name | Declared when | Driver | Cargo feature | Source |
|---|---|---|---|---|
| `memory` | always (default `CACHE_STORE`) | `MemoryStore`, prefix `rainier_sample` | none | `config/cache.rs:132-136` |
| `shared` | `REDIS_URL` set | `RedisStore`: 250 ms response timeout, 1 s connect timeout, reconnect | `redis` (or `redis-cluster`) | `config/cache.rs:142-145`, `:180-196` |
| `sessions` | `SESSION_REDIS_URL` set | the same Redis tuning, separate instance | `redis` | `config/cache.rs:151-154` |
| `memcached` | `MEMCACHED_URL` set | `MemcachedStore`, pool of 8 | `memcached` | `config/cache.rs:164-168` |

`bootstrap::cache` builds every declared store once
(`bootstrap.rs:414-422`). The result goes to `with_cache`, so sessions,
locks and rate limits share one manager (`bootstrap.rs:207-213`). A store
that cannot be reached, or whose driver feature is missing, fails the boot.

### Session drivers

| `SESSION_DRIVER` | Store | Notes | Source |
|---|---|---|---|
| `memory` (`.env.example`) | `MemorySessionStore` | one process only | `bootstrap.rs:313` |
| `database` | `DatabaseSessionStore` | needs `DatabaseSessionStore::migrations()` merged in, which is commented out today (`migrations/mod.rs:104-106`); never evicts | `bootstrap.rs:315-317` |
| `cache` | `CacheSessionStore` over `SESSION_STORE`, or the default store | an undeclared name fails the boot and lists the declared ones | `bootstrap.rs:328-343` |
| `cookie` | `CookieSessionStore`, encrypted with `APP_KEY` | no server state, no way to revoke | `bootstrap.rs:348-351` |

The cookie is `SESSION_COOKIE` (default `rainier_session`), `SameSite=Lax`,
`Secure` set from `SESSION_SECURE`, and the lifetime is `SESSION_LIFETIME`
seconds (default 7200) (`bootstrap.rs:384-391`, `config/session.rs:11-37`).
Under `compat-php` with `PHP_APP_KEY` the store is wrapped in a
`ForeignSessionStore` (`bootstrap.rs:357-380`).

### Flow

`GET /visits` is the worked example. It increments a session counter, reads
last request's flash message, flashes a new one, and returns the CSRF token
(`home_controller.rs:46-65`). A forged session cookie is replaced, not
trusted, and an API route sets no session cookie (`tests/feature.rs:927-1030`).

### Configuration

`CACHE_STORE`, `REDIS_URL`, `SESSION_REDIS_URL`, `MEMCACHED_URL`,
`SESSION_DRIVER`, `SESSION_STORE`, `SESSION_COOKIE`, `SESSION_LIFETIME`,
`SESSION_SECURE`.

### Edge cases & gotchas

- **There is no fallback to memory.** It used to exist and was removed on
  purpose. A per-process cache standing in for Redis gives locks that hold on
  one replica only, rate limits counted per replica, and sessions that sign
  users out whenever the load balancer moves them (`bootstrap.rs:396-413`,
  `Cargo.toml:31-40`).
- **Separate stores for sessions and cache.** They want opposite eviction
  policies, which is why `SESSION_REDIS_URL` declares a second store.
- **`CACHE_DRIVER` in `.env` stops the boot**, with a message telling you
  to use `CACHE_STORE` (`config/cache.rs:118-126`).
- **Locks are per-process by default.** At boot the app warns that
  `on_one_server` guarantees nothing until `CACHE_STORE=shared`
  (`app_provider.rs:72-77`).
- **`SESSION_SECURE` defaults to `false`.** Set it in production.

## History & references

- `bootstrap.rs:396-413`: the reversal of the "a cache can be lost" fallback.
- Tests: `bootstrap.rs:486-551`; the CI matrix builds the `redis`,
  `redis-cluster` and `memcached` variants (`ci.yml:83-85`).

## Proposed / Ideas (backlog)

| Idea | Motivation | Sketch | Status |
|---|---|---|---|
| Default `SESSION_SECURE` to `true` when `APP_ENV=production` | An easy setting to forget | Derive the default from `APP_ENV` in `config/session.rs` | Open |
