# Frontend: views and Vite

> **Status:** STABLE
> **Owners (code):** `resources/views/`, `resources/js/app.js`, `resources/css/app.css`, `vite.config.js`, `package.json`, `public/`, `src/bootstrap.rs:219-234`
> **Related docs:** [docker-and-ci.md](docker-and-ci.md) (the `assets` stage); framework: [views], [vite]
> **Last verified against code:** 2026-11-06 @ `741ad28`

[views]: https://github.com/safewords/rainier-framework/blob/main/docs/views.md
[vite]: https://github.com/safewords/rainier-framework/blob/main/docs/vite.md

## Overview

The sample has **no SPA**. It has one server-rendered page (`GET /`), a
layout, two mail templates, and a Vite pipeline that exists to show the
plumbing. `resources/js/app.js` is a single `console.debug` line
(`app.js:1-7`). The framework serves `public/` as static files, so the built
bundle needs no route (`routes/web.rs:12-24`). The whole frontend is
optional: `cargo run` works before `npm` has ever been run.

## How it works today

### Interfaces

| Kind | Name | Notes | Source |
|---|---|---|---|
| View | `home` | extends `layouts.app`; lists `posts`, or says to run `app:seed` | `resources/views/home.view.html` |
| View | `layouts.app` | `@yield('title')`, `@yield('content')`, `@vite([...css, ...js])`, inline fallback styles | `resources/views/layouts/app.view.html` |
| View | `mail.welcome`, `mail.post_live` | rendered by the mailables | `resources/views/mail/` |
| View | `posts.show` | **no route renders it** | `resources/views/posts/show.view.html` |
| npm | `npm run dev` | Vite dev server; the inline `rainier` plugin writes the dev URL to `public/hot` and removes it on exit or signal | `vite.config.js:22-52` |
| npm | `npm run build` | `public/build` plus `manifest.json`, entries `resources/css/app.css` and `resources/js/app.js` | `vite.config.js:54-70` |
| Static | `public/**` | served without a route; no traversal, no dotfiles, and a declared route takes precedence | `tests/feature.rs:1333-1390` |

### Flow

`bootstrap.rs` creates a `TemplateEngine` over `resources/views` with
`Vite::new("public")`. In testing, both the template cache and the manifest
cache are turned off (`bootstrap.rs:226-233`). `@vite` checks for
`public/hot` and emits dev-server tags if it exists. Otherwise it looks for
`public/build/manifest.json` and emits hashed URLs. If neither exists it
emits an HTML comment naming the command to run, and the page renders
without its stylesheet instead of failing.

### Configuration

None. Vite 6 (`package.json`). CI uses Node 20 (`ci.yml:127-131`) and the
image uses Node 22 (`Dockerfile:132`).

### Edge cases & gotchas

- **CI has to build the assets before `cargo test`**, because
  `the_built_frontend_is_served_from_public` requests
  `/build/manifest.json` (`ci.yml:120-131`).
- **The README is stale.** It describes an `asset_controller` serving
  `/build/{path*}` (`README.md:303`). That controller was deleted when the
  framework started serving `public/` (`routes/web.rs:16-24`).
- **Different Node versions** in CI and the image (20 against 22).

## History & references

- `routes/web.rs:12-24`: why the asset controller was deleted.

## Proposed / Ideas (backlog)

| Idea | Motivation | Sketch | Status |
|---|---|---|---|
| Route `GET /posts/{post}` to `posts.show` | The view is orphaned, and Discord and the mail link need a page | A web route rendering the view for published posts | Open |
