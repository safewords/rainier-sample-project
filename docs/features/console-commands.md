# Console commands

> **Status:** STABLE
> **Owners (code):** `src/routes/console.rs`, `src/app/console/commands/`, `src/main.rs`, `src/database/seeders.rs`
> **Related docs:** [queues-and-jobs.md](queues-and-jobs.md) (the schedule), [models-and-migrations.md](models-and-migrations.md), [add-ins.md](add-ins.md) (`oauth:client`); framework: [console], [scheduling]
> **Last verified against code:** 2026-11-06 @ `fd84ed4`

[console]: https://github.com/safewords/rainier-framework/blob/main/docs/console.md
[scheduling]: https://github.com/safewords/rainier-framework/blob/main/docs/scheduling.md

## Overview

Every process is a console command on the single `app` binary. `main.rs`
boots the application and then runs `routes::console::commands()` against
`argv` (`main.rs:29`). The framework ships the built-in commands. The
application registers two of its own (`routes/console.rs:18-26`).

## How it works today

### Interfaces

| Command | Source | Notes |
|---|---|---|
| `serve [--port=]` | framework | HTTP and WebSockets on one port; `SERVER_HOST`/`SERVER_PORT` |
| `route:list [--json]` | framework | the compiled route table and its middleware; the Docker `HEALTHCHECK` runs it ([docker-and-ci.md](docker-and-ci.md)) |
| `migrate [--pretend]`, `migrate:rollback` | framework | boot-time migration is skipped for these (`app_provider.rs:98-100`) |
| `queue:work [--once] [--queue=a,b] [--connection=] [--max-jobs=] [--sleep=]` | framework | with no `--queue`, the queues that registered jobs use |
| `schedule:run`, `schedule:work`, `schedule:list` | framework | run the tasks in `routes/console.rs:37-99` |
| `key:generate` | framework | prints an `APP_KEY` |
| `list` | framework | everything above |
| `app:seed [--fresh]` | `commands/seed.rs:11-47` | idempotent demo data; `--fresh` deletes every post first |
| `oauth:client --name= [--redirect=] [--public] [--personal]` | `commands/oauth_client.rs:12-95` | feature `oauth-server`; the secret is printed once and only its hash is stored |

### Flow

To add a command, implement `Command` (`name`, `description`, `help`,
`handle(args, app) -> exit code`) in `app/console/commands/`, export it from
`commands/mod.rs`, and `.register` it in `routes/console.rs`. Commands get the
fully booted `Application`, so they resolve repositories the way controllers
do (`seed.rs:32-46`).

`oauth:client` picks grant types from its flags. `--personal` gets personal
access. `--public` gets authorization code and refresh, and is told to use
PKCE. A confidential client also gets client credentials
(`oauth_client.rs:55-65`). Without `--personal`, `--redirect` is required
(`:47-50`).

### Edge cases & gotchas

- **`app:seed` and `serve` are separate processes.** With the default
  `sqlite::memory:` database they do not share data
  ([boot-and-configuration.md](boot-and-configuration.md)).
- **Every command boots the whole application and runs migrations**, apart
  from the `migrate*` commands. `route:list` is included.
- `main` calls `application.terminate()` after the command finishes, so
  terminating hooks run once the output is written (`main.rs:31-33`).

## History & references

- Tests: `routes/console.rs:101-182` (registration, schedule expressions, guards), `tests/feature.rs:903` (seeding is idempotent); CI `migrations` job (`ci.yml:139-178`).

## Proposed / Ideas (backlog)

| Idea | Motivation | Sketch | Status |
|---|---|---|---|
| A `health` command for the container probe | `route:list` boots and migrates | Exit 0 if `GET /health` on localhost answers | Open |
