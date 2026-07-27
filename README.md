# Rainier Sample Project

A starter application for [Rainier], laid out the way a Laravel project is.
Clone it, rename it, and build.

```sh
git clone https://github.com/safewords/rainier-sample-project.git my-app
cd my-app
cp .env.example .env

cargo run -- app:seed      # a demo user and a few posts
cargo run -- serve         # http://127.0.0.1:8000
```

It runs against SQLite in memory out of the box, so there is nothing to install
and nothing to configure. Point `DATABASE_URL` at a file, MySQL or Postgres when
you want the data to survive — nothing else in the app changes.

## What's here

Everything is wired and working: models, repositories, a router with groups and
named routes, request contracts, a token guard, a policy, an event, a queued
job, mailables, notifications with an in-app bell menu, relationships with
eager loading, broadcasting with channel authorisation, Blade-style views, and a
console command. Delete what you do not need.

```text
src/
  main.rs             artisan            — the console entry point
  bootstrap.rs        bootstrap/app.php  — assembles and boots the app
  config/             config/*.php       — one module per concern
  app/
    models/           app/Models         — User, Post, Tag, and their relationships
    http/
      kernel.rs       app/Http/Kernel    — global middleware, and the groups
      controllers/    app/Http/Controllers
      middleware/     app/Http/Middleware — an X-Request-Id example
      requests/       app/Http/Requests  — request contracts
    repositories/     —                  — where the domain's queries get names
    providers/        app/Providers      — service registration
    jobs/             app/Jobs
    mail/             app/Mail
    notifications/    app/Notifications  — a message to a recipient, over their channels
    policies/         app/Policies
    console/commands/ app/Console/Commands
  database/
    migrations/       database/migrations — one module per migration
    seeders.rs        database/seeders
  routes/
    channels.rs       routes/channels.php — who may subscribe to what
    web.rs            routes/web.php
    api.rs            routes/api.php
    console.rs        routes/console.php
resources/views/      resources/views    — templates
tests/feature.rs      tests/Feature      — end-to-end tests
Dockerfile            —                  — a two-stage production image
.github/workflows/    —                  — fmt, clippy, tests, docker
```

Two entries have no Laravel counterpart, because PHP has nowhere to put them.
`repositories/` is where a query gets a name — `published_page` rather than a
`Criteria` each controller assembles. And `migrations/` is a directory of
modules listed in `mod.rs` rather than files discovered by timestamp, because
Rust does not autoload and a list you can read beats a scan you cannot.

## Commands

```sh
cargo run -- list                   # everything available
cargo run -- serve --port=3000
cargo run -- route:list             # the route table, with middleware
cargo run -- route:list --json
cargo run -- migrate --pretend      # what would run
cargo run -- migrate:rollback       # undo the last batch
cargo run -- queue:work --once      # drain the queue
cargo run -- app:seed --fresh       # your own command
cargo run -- key:generate           # an APP_KEY to paste into .env
```

## The API

Log in as the seeded user (`ada@example.com` / `correct-horse`) to get a token:

```sh
TOKEN=$(curl -s localhost:8000/login \
  -H 'content-type: application/json' \
  -d '{"email":"ada@example.com","password":"correct-horse"}' | jq -r .token)

curl localhost:8000/api/posts
curl localhost:8000/api/me -H "authorization: Bearer $TOKEN"

curl -X POST localhost:8000/api/posts \
  -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"title":"Hello","body":"A body long enough to clear the minimum."}'
```

Publishing a post raises an **event**; a listener queues a **job**; the job
sends the author a **notification**, which goes out over mail *and* the
database channel:

```sh
curl -X POST localhost:8000/api/posts/hello/publish -H "authorization: Bearer $TOKEN"
curl localhost:8000/api/notifications -H "authorization: Bearer $TOKEN"
```

The bell menu fills once a worker has run the job. Out of the box the queue is
`MemoryQueue`, which lives inside one process — so `cargo run -- queue:work`
in a second terminal will not see it. Switch the driver to `DatabaseQueue` in
`app/providers/app_provider.rs` (and merge its migrations) and it will; the
feature tests drive a worker directly and assert on both channels.

An event is a fact with no recipient; a notification is a message to a named
one; a broadcast is a push to whoever is connected. `src/app/notifications/mod.rs`
has the table that tells them apart.

The index returns each post with its **author** and **tags** in three queries,
whatever the page size — `Post::author()` is a `belongs_to`, `Post::tags()` a
`belongs_to_many` through `post_tag`. Publishing also **broadcasts**
`post.published` on the public `posts` channel; `src/routes/channels.rs` says
who may subscribe to the private ones.

## Two things Rust does differently

Neither is a Rainier decision — both fall out of the language, and both are the
things a Laravel developer trips over first.

1. **Nothing is autoloaded.** Every directory has a `mod.rs` listing its files.
   Adding a controller means adding one line to
   `src/app/http/controllers/mod.rs`. The compiler tells you when you forget.

2. **Nothing is discovered by name.** A provider is registered in
   `bootstrap.rs`, a route in `routes/`, a job in the provider's `JobRegistry`,
   a listener in `EventServiceProvider`. The wiring is explicit — more typing,
   but no "why isn't my listener firing?".

## Adding things

| To add a… | Write it in | And register it in |
|---|---|---|
| model | `app/models/` | `app/models/mod.rs` |
| controller | `app/http/controllers/` | `app/http/controllers/mod.rs` + a route |
| route | `routes/web.rs` or `routes/api.rs` | — |
| request contract | `app/http/requests/` | `app/http/requests/mod.rs` |
| middleware | `app/http/middleware/` | `app/http/kernel.rs` |
| job | `app/jobs/` | the `JobRegistry` in `app/providers/app_provider.rs` |
| mailable | `app/mail/` | — (constructed where it is sent) |
| notification | `app/notifications/` | — (constructed where it is sent) |
| relationship | the model, as a `pub fn` | — (nothing to register) |
| broadcast channel rule | `routes/channels.rs` | — (the list *is* the registration) |
| policy | `app/policies/` | — (called from a controller) |
| command | `app/console/commands/` | `routes/console.rs` |
| migration | `database/migrations/` | `database/migrations/mod.rs` — append to `all()` |
| service | anywhere | `app/providers/app_provider.rs` |
| config section | `config/` | one line in `config/mod.rs` |
| cache or session driver | — | `.env`, plus the cargo feature |

## Testing

```sh
cargo test
```

`tests/feature.rs` boots the real application and drives the real kernel —
real routes, real middleware, real database, real migrations. Only the mail
transport is a double, so a test can assert on what was sent.

Tests boot an application each and share the process-global facades, so they
take a lock and run one at a time. Keep that in mind if you add one.

## Sessions and encryption

`GET /visits` is the smallest useful example: a counter in the session, a
flashed message that survives exactly one further request, and a CSRF token.

```sh
curl -c jar -b jar localhost:8000/visits
curl -c jar -b jar localhost:8000/visits    # visits: 1, and the flash arrives
```

The `web` group includes `session`; the `api` group deliberately does not, so
an API call authenticating with a token does not allocate a session row and a
cookie it will never use. A route outside `web` has `request.session() == None`.

Encryption is wired from `APP_KEY`:

```rust
let sealed = Crypt::instance().encrypt("a card number")?;
let signed = Crypt::instance().sign("unsubscribe-42")?;   // readable, not editable
```

Generate a key with `cargo run -- key:generate`. Without one a random key is
minted per boot, which works and silently invalidates everything the last boot
encrypted — so it warns.

## Going to production

- **Database.** Set `DATABASE_URL`. SQLite in memory is wiped on exit.
- **Queue.** `QUEUE_DRIVER=sync` runs jobs inline, so a failed job fails the
  request that dispatched it. Switch to the database driver (see the note in
  `app/providers/app_provider.rs`) and run `cargo run -- queue:work`.
- **Mail.** `MAIL_DRIVER=log` writes to the log. `file` writes `.eml` files you
  can open in a browser.
- **Sessions.** `SESSION_DRIVER=memory` is per-process. Pick one of:
  `database` (merge `DatabaseSessionStore::migrations()` into
  `database/migrations/mod.rs`; never evicts), `cache` (Redis or Memcached, expires
  itself, can evict), or `cookie` (no server state, and no way to revoke a
  session). Set `SESSION_SECURE=true` whichever you choose.
- **Cache.** `CACHE_DRIVER=redis` with `cargo build --features redis`, or
  `redis-cluster` for a sharded cluster, or `memcached`.
- **Keys.** Set `APP_KEY`. Rotate by moving the old one into
  `APP_PREVIOUS_KEYS` and putting a new one in `APP_KEY`.
- **Debug.** `APP_DEBUG=false` — with it on, internal error messages reach the
  client, and those routinely contain a connection string or a query.

## Docker

```sh
docker build -t rainier-sample .

docker run --rm -p 8000:8000 \
  -e APP_KEY="base64:$(openssl rand -base64 32)" \
  -e DATABASE_URL="sqlite:///data/app.sqlite?mode=rwc" \
  -v rainier-data:/data \
  rainier-sample
```

Two stages: a builder with the toolchain, and a Debian slim runtime holding the
binary, the templates and nothing else. It runs as a non-root user and
`HEALTHCHECK` curls `/health`.

Note the volume. `DATABASE_URL` defaults to `sqlite::memory:`, which is wiped
when the container stops — fine for a smoke test and wrong for anything else.
Point it at a mounted path or a real server.

`.dockerignore` excludes `.cargo/config.toml`, which is the local development
override pointing at a sibling checkout of the framework. Git ignores it;
Docker has to be told separately, and a build with it present cannot resolve
the dependency.

## Continuous integration

`.github/workflows/ci.yml` runs on every push and pull request, and weekly —
the framework is a git dependency with no version to pin against, so `main`
moving changes this build with no commit here to trigger it.

| Job | Checks |
|---|---|
| `check` | `cargo fmt --check`, `cargo clippy -D warnings` |
| `test` | the suite under each of the four feature combinations |
| `migrations` | migrate → rollback → migrate, against a real SQLite file |
| `routes` | `route:list`, which compiles the router and builds every middleware |
| `docker` | builds the image, starts it, and waits for `/health` |

The last three assert behaviour rather than compilation. `migrations` requires
the rollback to **fail**, because `0005_normalise_emails` is deliberately
irreversible and a deploy script chaining one has to stop. `docker` runs the
image because building one and running one are different claims, and the
second is the one that breaks.

[Rainier]: https://github.com/safewords/rainier-framework
