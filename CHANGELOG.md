# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
There are no tagged releases: the layout is copied, not depended on, and
its trunk is `main`. History before this file is in `git log`.

## [Unreleased]

### Added

- An unused dependency fails the build: `[lints.cargo] unused_dependencies = "deny"`
  in `Cargo.toml`. Cargo 1.100 only warns by default, and clippy's
  `-D warnings` does not see it.
- Every framework add-in, each behind its own Cargo feature of the same name
  (`accounts`, `two-factor`, `passkeys`, `sso`, `billing-stripe`,
  `oauth-server`, `telemetry`, `captcha`, `notify-discord`, `compat-php`);
  `add-ins` turns on all but `compat-php`. The default build is unchanged.
  The tables they use are created in every build, so turning one on is never
  a schema change.

### Changed

- The front end builds with Vite 8 (Rolldown); `vite.config.js` uses
  `build.rolldownOptions`, Vite 8's name for `build.rollupOptions`. CI builds
  the assets on Node 22, the same as the image. The manifest and the `@vite`
  entries are unchanged.
- The image is Alpine: `rust:1.100.0-alpine3.24` builds a static musl binary,
  `node:22-alpine3.24` builds the assets, and `alpine:3.24` runs it. `mimalloc`
  is the global allocator on musl, since musl's own serialises threads.
- Builds on Rust 1.99.1 and links with mold.
- Builds on Rust 1.100.0. `rust-version`, the Dockerfile's builder image
  (`rust:1.100.0-bookworm`) and every CI job's toolchain move together, and
  1.100 is now the minimum: the code uses `str::trim_prefix`, so an older
  toolchain no longer compiles it.
- On the latest framework, whose JWTs (jsonwebtoken 12) and RSA keys (rsa 0.10)
  hash with the same sha2 as the rest of it, and on the newest release of every
  dependency in the lockfile. The debug profile now optimises `crypto-bigint`
  and `crypto-primes`, rsa 0.10's arithmetic, so booting with `oauth-server`
  and no `OAUTH_PRIVATE_KEY` stays fast.

### Fixed

- The image build reads the framework revision from a `?rev=` lock source
  too. Every image build since the framework was pinned by revision had
  failed at `cargo install`.
