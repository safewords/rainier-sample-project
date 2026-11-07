# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
There are no tagged releases: the layout is copied, not depended on, and
its trunk is `main`. History before this file is in `git log`.

## [Unreleased]

### Added

- Every framework add-in, each behind its own Cargo feature of the same name
  (`accounts`, `two-factor`, `passkeys`, `sso`, `billing-stripe`,
  `oauth-server`, `telemetry`, `captcha`, `notify-discord`, `compat-php`);
  `add-ins` turns on all but `compat-php`. The default build is unchanged.
  The tables they use are created in every build, so turning one on is never
  a schema change.

### Changed

- Builds on Rust 1.99.1 and links with mold.

### Fixed

- The image build reads the framework revision from a `?rev=` lock source
  too. Every image build since the framework was pinned by revision had
  failed at `cargo install`.
