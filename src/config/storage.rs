//! `config/filesystems.php`.
//!
//! Where uploaded files live. `local` — one directory on this machine — is
//! the default, and its limitation is worth knowing before deploying: it
//! survives a restart and not a redeploy, and two instances each have their
//! own. `s3` (and everything that speaks its API — Cloudflare R2, MinIO,
//! Backblaze) is the shared answer, behind the `s3` cargo feature.

use rainier_framework::config::{Config, Env};
use rainier_framework::filesystem::FilesystemDriver;
use rainier_framework::prelude::*;

use crate::config::keys::{
    STORAGE_BUCKET, STORAGE_DRIVER, STORAGE_ENDPOINT, STORAGE_REGION, STORAGE_ROOT,
    STORAGE_URL_PREFIX,
};

/// Storage settings, read back by `bootstrap::storage`.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // A `FilesystemDriver`, not a string — `STORAGE_DRIVER=s3` without the
    // feature fails at boot naming it, and a misspelling lists the set.
    config.set(STORAGE_DRIVER, env.setting_or("STORAGE_DRIVER", FilesystemDriver::Local)?)?;

    config.set(STORAGE_ROOT, env.string("STORAGE_ROOT", "storage/app"))?;

    // The S3 half. An endpoint is what points the same driver at R2 or MinIO
    // instead of AWS; empty means AWS's own.
    config.set(STORAGE_BUCKET, env.string("STORAGE_BUCKET", ""))?;
    config.set(STORAGE_REGION, env.string("STORAGE_REGION", ""))?;
    config.set(STORAGE_ENDPOINT, env.string("STORAGE_ENDPOINT", ""))?;
    // A CloudFront distribution or an R2 custom domain. Without it `url()`
    // answers `None` — a private bucket's object URL answers 403, and a link
    // that fails is worse than no link.
    config.set(STORAGE_URL_PREFIX, env.string("STORAGE_URL_PREFIX", ""))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_local_until_a_deployment_says_otherwise() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.setting(STORAGE_DRIVER).unwrap(), FilesystemDriver::Local);
        assert_eq!(config.get(STORAGE_ROOT).as_deref(), Some("storage/app"));
    }

    #[test]
    fn a_misspelled_driver_stops_the_boot() {
        let err = configure(&Config::new(), &Env::parse("STORAGE_DRIVER=r2")).unwrap_err();

        assert!(err.message().contains("STORAGE_DRIVER"), "{}", err.message());
        assert!(
            err.message().contains("`s3`"),
            "the message should list the valid values, got {}",
            err.message()
        );
    }
}
