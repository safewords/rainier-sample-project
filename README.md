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
eager loading, broadcasting with channel authorisation, a WebSocket chat room,
Blade-style views, and a console command. Delete what you do not need.

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
    ws.rs             —                  — WebSocket endpoints, on the same port
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

There is a **WebSocket** room on the same port, needing no second process:

```sh
# any WebSocket client; the token is the one from /login
websocat -H "authorization: Bearer $TOKEN" ws://localhost:8000/ws/rooms/lobby
```

Open two and they hear each other. `authorize` runs before the handshake, so a
connection with no token is refused with a `403` rather than opened and then
closed.

An event is a fact with no recipient; a notification is a message to a named
one; a broadcast is a push to whoever is connected; a socket is a conversation. `src/app/notifications/mod.rs`
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
| WebSocket endpoint | `routes/ws.rs` | — (the list *is* the registration) |
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

`TestApp` does the driving, so a test is three lines:

```rust
let app = App::boot().await;

app.send(app.get("/health")).await.assert_ok().assert_json_path("status", "ok");
```

Each test gets its own application, and `TestApp` scopes the facades to the
thread it runs on, so they no longer resolve out of each other's containers.
The boot itself is still serialised: the bootstrap installs its application
globally before the providers run, because a provider legitimately reaches for
a facade while it is being registered.

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

## Frontend assets

Vite, the way a PHP framework wires it — and entirely optional: `cargo run`
works before npm ever has. `resources/js` and `resources/css` are source; the layout names
them with a directive:

```html
@vite(['resources/css/app.css', 'resources/js/app.js'])
```

```sh
npm install
npm run dev     # hot reload: writes public/hot, @vite points at the dev server
npm run build   # compiles public/build + manifest.json, @vite emits hashed URLs
```

With neither running, `@vite` renders an HTML comment saying which command to
run and the page arrives unstyled rather than down — the layout keeps a small
inline fallback so it stays presentable.

The pieces, each small enough to read: `vite.config.js` (the entries, the
manifest, and a ~20-line inline plugin that maintains `public/hot`),
`asset_controller` (serves `public/build` under `/build/{path*}`,
traversal-safe, `immutable`-cached — Rainier is the web server, so the built
files need a route), and the Dockerfile's `assets` stage (the image compiles
its own bundle; nothing built locally leaks in). The framework side is
documented in the framework's `docs/vite.md`.

## Sizing the binary

Every subsystem a deployment does not use is a cargo feature it does not
enable — the Redis and Memcached clients, each mail sender, SQS, Kafka, S3,
the bcrypt driver, the JWT stack, the outbound HTTP transport. The default
build carries none of them, and a fresh clone still runs: SQLite in memory,
inline jobs, logged mail, local files.

Dead-code elimination will not do this for you. The driver `match`es in
`bootstrap.rs` and the providers are deliberately exhaustive — that is what
makes a misconfigured deployment fail loudly — so every compiled driver is
*referenced*, and the linker keeps it. Features are the sizing mechanism.

And cargo cannot flip them on by itself: features are resolved **before**
anything compiles, they are additive-only, and a build script cannot add one.
"The compiler sees `MAIL_DRIVER=smtp` and enables `mail-smtp`" is not a thing
cargo can do. What it *can* do is be told — so the feature list is computed
rather than hand-maintained, by the framework's own tool:

```sh
cargo install cargo-rainier   --git https://github.com/safewords/rainier-framework   # once

cargo rainier features                       # what .env implies, with reasons
cargo rainier features --env .env.build
cargo rainier features --env .env.build --list   # bare list, for scripts
cargo rainier features --check               # CI: fail on a selection nothing forwards
cargo rainier build --env .env.build --release
```

An environment file is **required** — an explicit `--env`, or `.env`. There
is deliberately no fallback to `.env.example`: sizing a build from the
example's defaults would shape the binary like the documentation rather than
the deployment, silently. Preview against the defaults with
`--env .env.example` when that is what you mean.

The logic is the framework's `rainier-features` crate — the driver→feature
mapping is knowledge about Rainier, and its tests there walk every driver
enum so a new driver learns its feature in the same commit that adds it.
This application deliberately carries **no tool of its own**: the Dockerfile
installs `cargo-rainier` pinned to the same framework revision the lockfile
pins, so the mapping an image is sized with is the one the code compiles
against. It reads the two honest
sources — the deployment's environment file for every runtime driver
selection, and the source tree for the compile-time choices (`Jwt`, the
`Http` facade) — and emits the minimal
`--no-default-features --features "…"` invocation:

```text
# from .env.production
#   redis          CACHE_DRIVER=redis
#   mail-smtp      MAIL_DRIVER=smtp
cargo build --release --no-default-features --features "mail-smtp,redis"
```

A selection nothing forwards — `CACHE_DRIVER=dynamodb`, which this
application never wired — is an error rather than a silently smaller list,
and `--check` turns it into a failing CI step. When the framework grows a
driver, the compiler already points at every `match` arm that must learn it —
and `rainier-features`' own tests point at the mapping table, in the same
repository as the driver.

## Going to production

- **Database.** Set `DATABASE_URL`. SQLite in memory is wiped on exit.
- **Queue.** `QUEUE_CONNECTION=sync` runs jobs inline, so a failed job fails the
  request that dispatched it. Switch to `database` and run
  `cargo run -- queue:work`; `bulk` is a second connection on the same driver,
  drained by its own workers. The connections are declared in
  `config/queue.rs`.
- **Mail.** `MAIL_DRIVER=log` writes to the log. `file` writes `.eml` files you
  can open in a browser.
- **Sessions.** `SESSION_DRIVER=memory` is per-process. Pick one of:
  `database` (merge `DatabaseSessionStore::migrations()` into
  `database/migrations/mod.rs`; never evicts), `cache` (one of the declared
  cache stores, expires itself, can evict), or `cookie` (no server state, and no
  way to revoke a session). Set `SESSION_SECURE=true` whichever you choose.
- **Cache.** Set `REDIS_URL` and build with `--features redis`; point
  `CACHE_STORE` at the `shared` store that declares. `SESSION_REDIS_URL`
  declares a second store for sessions, which want the opposite eviction policy
  from a cache. The stores are declared in `config/cache.rs`.

> `CACHE_DRIVER`, `QUEUE_DRIVER` and `STORAGE_DRIVER` are **not** variables this
> application reads. Each named a single backend, and each configuration section
> declares several. Keep all three out of `.env`; they survive in exactly one
> place, `.env.build`, which sizes the Docker image and never reaches the running
> container.
>
> They do not fail the same way, which is worth knowing when you are cleaning up
> an old `.env`:
>
> | Variable | In `.env` |
> |---|---|
> | `QUEUE_DRIVER` | boot failure — Rainier refuses it beside the `queues` section |
> | `CACHE_DRIVER` | boot failure, refused by `config/cache.rs` itself |
> | `STORAGE_DRIVER` | read by nothing at runtime; harmless, and does nothing |
>
> `CACHE_DRIVER` is the interesting one. Rainier's own refusal lives on the path
> that builds the cache, and `bootstrap.rs` skips that path — it hands over a
> built manager with `Rainier::with_cache` so sessions, locks and rate limits
> share one store. That suppressed the guard, and a suppressed guard here means
> `CACHE_DRIVER=redis` would be *silently* ignored and leave you on an in-process
> cache. `config/cache.rs` re-states the refusal for that reason.
- **Keys.** Set `APP_KEY`. Rotate by moving the old one into
  `APP_PREVIOUS_KEYS` and putting a new one in `APP_KEY`.
- **Debug.** `APP_DEBUG=false` — with it on, internal error messages reach the
  client, and those routinely contain a connection string or a query.

## Docker

The image is **sized to its deployment**: the builder computes the feature
set from a selections-only env file and builds with it. The file is required
— a build without one fails at its `COPY` rather than shipping a
documentation-shaped binary — and it must hold **driver selections only,
never secrets**, because it travels into builder layers and the build cache.
Secrets keep arriving at `docker run` as `-e`.

```sh
printf 'CACHE_DRIVER=redis
MAIL_DRIVER=smtp
' > .env.build

docker build -t rainier-sample .
docker build --build-arg ENV_FILE=.env.staging.build -t rainier-sample:staging .

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

The fmt and clippy gates also run as a committed pre-commit hook — enable it
once per clone with `git config core.hooksPath .githooks`. Bypass once with
`--no-verify`, or skip only the lint with `SKIP_CLIPPY=1 git commit …`.

| Job | Checks |
|---|---|
| `check` | `cargo fmt` (self-healing on `main`), `cargo clippy -D warnings` |
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
