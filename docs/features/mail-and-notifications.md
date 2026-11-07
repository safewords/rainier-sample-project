# Mail and notifications

> **Status:** STABLE
> **Owners (code):** `src/config/mail.rs`, `src/app/mail/`, `src/app/notifications/`, `src/app/http/controllers/notification_controller.rs`, `src/app/providers/app_provider.rs` (`mail`, `notifications`), `resources/views/mail/`
> **Related docs:** [queues-and-jobs.md](queues-and-jobs.md), [realtime.md](realtime.md), [add-ins.md](add-ins.md) (reset and verification mail, Discord); framework: [mail], [notifications]
> **Last verified against code:** 2026-11-06 @ `741ad28`

[mail]: https://github.com/safewords/rainier-framework/blob/main/docs/mail.md
[notifications]: https://github.com/safewords/rainier-framework/blob/main/docs/notifications.md

## Overview

There are two mailables (`WelcomeMail`, `PostLiveMail`) and one user
notification (`PostLive`), sent over database, broadcast and mail. The
database channel stores rows for an in-app bell menu, which the API serves.
The mailer is built by the framework from the `mail.*` section, and the
transport is picked with `MAIL_DRIVER`. In `Mode::Testing` the transport is a
`MemoryTransport` bound in the container, so a test can check what was sent
(`app_provider.rs:162-195`).

## How it works today

### Data model

The `notifications` table comes from `DatabaseChannel::migrations()`
(`migrations/mod.rs:98`). Rows are keyed by `(notifiable_type,
notifiable_id)`, which is `("User", user.id)` (`models/user.rs:58-76`).

### Interfaces

| Kind | Name | Details | Source |
|---|---|---|---|
| Mailable | `WelcomeMail { name, email }` | addresses itself; view `mail.welcome` | `mail/welcome.rs:11-27` |
| Mailable | `PostLiveMail { name, title, slug }` | no `to`, so the notification's recipient supplies it; links to the absolute `api.posts.show` URL; header `X-Campaign: post-live` | `mail/post_live.rs:14-45` |
| Notification | `PostLive` (`post.live`) | `via` database + broadcast + mail; `to_data` `{post_id, slug, title}` | `notifications/post_live.rs:16-68` |
| Notification | `PostAnnounced` (`post.announced`) | Discord only (feature `notify-discord`), to the `Community` notifiable | `notifications/post_announced.rs:14-72` |
| HTTP | `GET /api/notifications` | `unread`, `limit` (1-100); returns `{unread, data[]}` | `notification_controller.rs:21-42` |
| HTTP | `POST /api/notifications/{id}/read` | 404 unless it is the caller's | `notification_controller.rs:45-59` |
| HTTP | `POST /api/notifications/read` | `{marked}` | `notification_controller.rs:62-68` |
| Env | `MAIL_DRIVER`, `MAIL_FROM`, `MAIL_FROM_NAME`, `MAIL_FILE_PATH`, `MAIL_HOST`/`PORT`/`USERNAME`/`PASSWORD`/`ENCRYPTION`/`TIMEOUT`, per-provider keys, `MAIL_ALWAYS_TO` | | `config/mail.rs:34-78` |

### Drivers by cargo feature

| `MAIL_DRIVER` | Cargo feature |
|---|---|
| `log` (`.env.example`), `file` (`.eml` under `MAIL_FILE_PATH`), `memory` | none |
| `smtp` (Mailpit: `MAIL_HOST=localhost MAIL_PORT=1025 MAIL_ENCRYPTION=none`) | `mail-smtp` |
| `ses`, `postmark`, `mailgun`, `sendgrid`, `resend` | `mail-ses` … `mail-resend` |

Choosing a sender without its feature fails the boot and names the missing
feature (`Cargo.toml:73-77`).

### Flow

`NotifyAuthor` calls `Notifier::send(&author, &PostLive)`
([queues-and-jobs.md](queues-and-jobs.md)). The notifier is built with the
database, broadcast and mail channels, plus Discord under `notify-discord`
(`app_provider.rs:228-250`). Each channel the notification picks in `via()`
gets its own rendering: `to_data` for the database, `to_mail` for mail, and
the recipient's private channel for broadcast. `register_user` sends
`WelcomeMail` directly (`providers/mod.rs:44-46`).

### Edge cases & gotchas

- **`MAIL_ALWAYS_TO`** sends every message to a single address, which is
  meant for staging. It is set only when non-empty (`config/mail.rs:73-75`).
- **The `PostLive` mail links to the JSON API.** No web page shows a post.
- **The mailer emits events** (`with_events`, `app_provider.rs:192`), so
  telemetry can trace mail sends.

## History & references

- `app/notifications/mod.rs:3-36`: events compared with notifications, and the chain.
- Tests: `tests/feature.rs:471-610`, `:746`.

## Proposed / Ideas (backlog)

| Idea | Motivation | Sketch | Status |
|---|---|---|---|
| Implement `mail:weekly-digest` | The task is scheduled and only logs | A `Digest` mailable over `published_page` | Open |
