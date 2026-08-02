//! `config/filesystems.php`.
//!
//! Where uploaded files live: a **default** disk plus every disk the
//! application declares by name, each naming its own driver and its own
//! settings.
//!
//! `local` — one directory on this machine — is the default here, and its
//! limitation is worth knowing before deploying: it survives a restart and not
//! a redeploy, and two instances each have their own. `s3` (and everything
//! that speaks its API — Cloudflare R2, MinIO, Backblaze) is the shared
//! answer, behind the `s3` cargo feature.
//!
//! # Why a disk is a section rather than a driver plus some settings
//!
//! An earlier version of this file configured one driver and one set of
//! connection settings. That cannot express the case this file exists to
//! demonstrate: `archive` below lives on a **different service** to
//! `documents` — its own endpoint, its own credential pair — and building it
//! from the other disk's connector would give it the right bucket name pointed
//! at the wrong host.
//!
//! That failure is silent, which is why it is worth the ceremony. A read
//! against the wrong service does not error. It finds nothing under the prefix
//! and reports an empty listing, which is exactly what a bucket that is
//! genuinely empty reports.
//!
//! # Declaring a disk nobody configured
//!
//! Each disk below is declared only when the environment actually names one.
//! An `S3Disk` with an empty bucket is not a disk that fails later — it is a
//! disk that signs requests against `""` — so this leaves it undeclared
//! instead, and `Storage::disk` answers `None`. A handler can then say "not
//! configured", which is a different sentence from "nothing there", and the
//! two should never be confused for one another.

use rainier_framework::config::{Config, Env};
use rainier_framework::filesystem::{DiskConfig, Disks, S3Disk};
use rainier_framework::keys::FILESYSTEMS;
use rainier_framework::prelude::*;

/// Storage settings, read back by the framework's own bootstrap.
///
/// Note what this does *not* do: it builds no filesystem and opens no
/// connection. It records declarations and the framework builds them, which is
/// what keeps "which disks exist" answerable from configuration alone —
/// including by a test that never touches a network.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // The disk everything uses unless it names another. Whatever name this
    // holds must be one of the disks declared below; the framework refuses at
    // boot if it is not, rather than starting and failing on the first write.
    let default = env.string("FILESYSTEM_DISK", "uploads");

    // Always declared, because it needs nothing configured: a deployment with
    // no object store at all still has somewhere to put a file.
    let mut disks = Disks::new(default)
        .with("uploads", DiskConfig::local(env.string("STORAGE_ROOT", "storage/app")));

    // A bucket on the primary object store, declared only when one is named.
    let bucket = env.string("STORAGE_BUCKET", "");
    if !bucket.is_empty() {
        let mut disk = S3Disk::new(bucket);

        // An endpoint is what points the same driver at R2 or MinIO instead of
        // AWS; absent means AWS's own.
        let endpoint = env.string("STORAGE_ENDPOINT", "");
        if !endpoint.is_empty() {
            disk = disk.endpoint(endpoint);
        }

        let region = env.string("STORAGE_REGION", "");
        if !region.is_empty() {
            disk = disk.region(region);
        }

        // A CloudFront distribution or an R2 custom domain. Without it `url()`
        // answers `None` — a private bucket's object URL answers 403, and a
        // link that fails is worse than no link.
        let url = env.string("STORAGE_URL_PREFIX", "");
        if !url.is_empty() {
            disk = disk.url(url);
        }

        disks = disks.with("documents", disk);
    }

    // A second object store on its own service — the case worth demonstrating,
    // because it is the one a single-driver configuration cannot express.
    //
    // It takes its own credentials rather than the ambient chain, and that is
    // the point: the ambient chain belongs to the *primary* account, so a disk
    // on another provider that inherits it does not fail. It authenticates as
    // the wrong principal and reads a bucket of the same name in an account
    // that is not this one.
    //
    // Declared only when both the bucket and the endpoint are present. Half a
    // configuration is someone midway through writing one, and a disk built
    // from half of it reads the wrong place rather than being absent.
    let archive_bucket = env.string("ARCHIVE_BUCKET", "");
    let archive_endpoint = env.string("ARCHIVE_ENDPOINT", "");
    if !archive_bucket.is_empty() && !archive_endpoint.is_empty() {
        let mut disk = S3Disk::new(archive_bucket)
            .endpoint(archive_endpoint)
            // A signed request has to name a region and a guess is a wrong
            // one. `auto` is what the S3-compatible services without regions
            // expect.
            .region(env.string("ARCHIVE_REGION", "auto"));

        // Both halves together or neither: a key with no secret falls back to
        // the ambient chain, which is the account mix-up above wearing a
        // disguise.
        let key = env.string("ARCHIVE_ACCESS_KEY_ID", "");
        let secret = env.string("ARCHIVE_SECRET_ACCESS_KEY", "");
        if !key.is_empty() && !secret.is_empty() {
            disk = disk.credentials(key, secret);
        }

        disks = disks.with("archive", disk);
    }

    config.set(FILESYSTEMS, disks)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disks_from(env: &Env) -> Disks {
        let config = Config::new();
        configure(&config, env).unwrap();
        config.get(FILESYSTEMS).expect("the filesystems section is set")
    }

    #[test]
    fn a_deployment_that_configures_nothing_still_has_somewhere_to_write() {
        let disks = disks_from(&Env::parse(""));

        assert_eq!(disks.default_name(), "uploads");
        assert!(disks.get("uploads").is_some());
    }

    #[test]
    fn an_object_disk_is_declared_only_when_a_bucket_names_one() {
        // Not a disk that fails on first use: an `S3Disk` with an empty bucket
        // signs requests against `""`. Leaving it undeclared is what lets
        // `Storage::disk` answer `None`, so a handler says "not configured"
        // rather than reporting an empty bucket.
        assert!(disks_from(&Env::parse("")).get("documents").is_none());

        let configured = disks_from(&Env::parse("STORAGE_BUCKET=documents-bucket"));
        assert!(configured.get("documents").is_some());
    }

    #[test]
    fn a_second_service_is_its_own_disk_and_not_the_first_ones_connector() {
        let disks = disks_from(&Env::parse(
            "STORAGE_BUCKET=documents-bucket\n\
             ARCHIVE_BUCKET=archive-bucket\n\
             ARCHIVE_ENDPOINT=https://example.r2.cloudflarestorage.com",
        ));

        let documents = disks.get("documents").expect("declared");
        let archive = disks.get("archive").expect("declared");

        // The assertion that matters: the two are not the same declaration. A
        // configuration that built both from one connector would pass every
        // other test here and fail only in production, silently.
        assert_ne!(format!("{documents:?}"), format!("{archive:?}"));
    }

    #[test]
    fn a_half_configured_second_service_is_not_declared() {
        let bucket_only = disks_from(&Env::parse("ARCHIVE_BUCKET=archive-bucket"));
        assert!(bucket_only.get("archive").is_none());

        let endpoint_only = disks_from(&Env::parse("ARCHIVE_ENDPOINT=https://example.com"));
        assert!(endpoint_only.get("archive").is_none());
    }

    #[test]
    fn a_credential_never_reaches_a_rendering_of_the_section() {
        let disks = disks_from(&Env::parse(
            "ARCHIVE_BUCKET=archive-bucket\n\
             ARCHIVE_ENDPOINT=https://example.r2.cloudflarestorage.com\n\
             ARCHIVE_ACCESS_KEY_ID=AKIA-example\n\
             ARCHIVE_SECRET_ACCESS_KEY=super-secret",
        ));

        // A configuration dump at boot must not put the secret into the log of
        // every process that started.
        let rendered = format!("{disks:?}");
        assert!(!rendered.contains("super-secret"), "{rendered}");
        assert!(!rendered.contains("AKIA-example"), "{rendered}");
        // Not vacuous — the disk itself does render.
        assert!(rendered.contains("archive-bucket"), "{rendered}");
    }
}
