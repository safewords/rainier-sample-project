# File storage

> **Status:** PARTIAL — the disks are declared and built; no application code reads or writes them
> **Owners (code):** `src/config/storage.rs`
> **Related docs:** [boot-and-configuration.md](boot-and-configuration.md), [docker-and-ci.md](docker-and-ci.md) (`STORAGE_DRIVER` in `.env.build`); framework: [filesystem]
> **Last verified against code:** 2026-11-06 @ `fd84ed4`

[filesystem]: https://github.com/safewords/rainier-framework/blob/main/docs/filesystem.md

## Overview

`config/storage.rs` declares the `filesystems` section: one local disk that
always exists, and up to two S3-API disks that appear when their bucket is
configured. The framework builds every disk from this section and binds it.
The app has no `storage()` builder of its own any more, because supplying a
`Storage` instance would quietly override every declared disk
(`bootstrap.rs:214-218`, `:424-435`). No route or job in the sample uses a
disk. The section is there to show how one is declared.

## How it works today

### Interfaces

| Disk | Declared when | Driver | Settings | Source |
|---|---|---|---|---|
| `uploads` (default `FILESYSTEM_DISK`) | always | local | `STORAGE_ROOT` (default `storage/app`) | `config/storage.rs:51-56` |
| `documents` | `STORAGE_BUCKET` set | S3 (feature `s3`) | `STORAGE_ENDPOINT`, `STORAGE_REGION`, `STORAGE_URL_PREFIX`; credentials from the SDK chain | `config/storage.rs:59-84` |
| `archive` | `ARCHIVE_BUCKET` **and** `ARCHIVE_ENDPOINT` set | S3 (R2-style) | `ARCHIVE_REGION` (default `auto`), `ARCHIVE_ACCESS_KEY_ID`/`ARCHIVE_SECRET_ACCESS_KEY` when both are set | `config/storage.rs:98-118` |

`public/` is a separate thing. The framework serves it as static files
([frontend-vite.md](frontend-vite.md)).

### Configuration

The Docker image creates `storage/app` and `/data`, both owned by the
`rainier` user (`Dockerfile:169-170`). Name a secret by its Kubernetes Secret
and key, and keep it out of `.env.build`.

### Edge cases & gotchas

- **`STORAGE_DRIVER` is not read at runtime.** It matters only in
  `.env.build`. Leave it out there and an image whose section declares an S3
  disk gets built with no S3 client, which shows up on the first upload
  (`Dockerfile:65-67`).
- **`archive` needs both a bucket and an endpoint.** A bucket on its own
  declares nothing, and no warning is logged (`config/storage.rs:100`).

## History & references

- `config/keys.rs:62-71`: storage used to be six scalars that described a single disk.

## Proposed / Ideas (backlog)

| Idea | Motivation | Sketch | Status |
|---|---|---|---|
| A worked upload example | The disks are declared and never used | `POST /api/posts/{post}/image` writing to `uploads` | Open |
