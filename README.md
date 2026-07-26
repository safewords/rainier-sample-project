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
job, mailables, Blade-style views, and a console command. Delete what you do not
need.

```text
src/
  main.rs             artisan            — the console entry point
  bootstrap.rs        bootstrap/app.php  — assembles and boots the app
  config.rs           config/*.php       — configuration, read from .env
  app/
    models/           app/Models         — User, Post
    http/
      kernel.rs       app/Http/Kernel    — global middleware, aliases, groups
      controllers/    app/Http/Controllers
      middleware/     app/Http/Middleware — an X-Request-Id example
      requests/       app/Http/Requests  — request contracts
    providers/        app/Providers      — service registration, repositories
    jobs/             app/Jobs
    mail/             app/Mail
    policies/         app/Policies
    console/commands/ app/Console/Commands
  database/
    migrations.rs     database/migrations
    seeders.rs        database/seeders
  routes/
    web.rs            routes/web.php
    api.rs            routes/api.php
    console.rs        routes/console.php
resources/views/      resources/views    — templates
tests/feature.rs      tests/Feature      — end-to-end tests
```

## Commands

```sh
cargo run -- list                   # everything available
cargo run -- serve --port=3000
cargo run -- route:list             # the route table, with middleware
cargo run -- route:list --json
cargo run -- migrate --pretend      # what would run
cargo run -- queue:work --once      # drain the queue
cargo run -- app:seed --fresh       # your own command
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
| policy | `app/policies/` | — (called from a controller) |
| command | `app/console/commands/` | `routes/console.rs` |
| migration | `database/migrations.rs` | — (append to `all()`) |
| service | anywhere | `app/providers/app_provider.rs` |

## Testing

```sh
cargo test
```

`tests/feature.rs` boots the real application and drives the real kernel —
real routes, real middleware, real database, real migrations. Only the mail
transport is a double, so a test can assert on what was sent.

Tests boot an application each and share the process-global facades, so they
take a lock and run one at a time. Keep that in mind if you add one.

## Going to production

- **Database.** Set `DATABASE_URL`. SQLite in memory is wiped on exit.
- **Queue.** `QUEUE_DRIVER=sync` runs jobs inline, so a failed job fails the
  request that dispatched it. Switch to the database driver (see the note in
  `app/providers/app_provider.rs`) and run `cargo run -- queue:work`.
- **Mail.** `MAIL_DRIVER=log` writes to the log. `file` writes `.eml` files you
  can open in a browser.
- **Debug.** `APP_DEBUG=false` — with it on, internal error messages reach the
  client, and those routinely contain a connection string or a query.

[Rainier]: https://github.com/safewords/rainier-framework
