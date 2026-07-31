//! `config/database.php`.
//!
//! One value, and it is the whole persistence story: point `DATABASE_URL` at
//! SQLite, MySQL or Postgres and nothing else in the application changes.
//! In memory by default, so a fresh clone runs with no setup and is wiped on
//! exit.

use rainier_framework::config::{Config, Env};
use rainier_framework::prelude::*;

use crate::config::keys;

/// Database settings, read back by `bootstrap::connect`.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    config.set(keys::DATABASE_URL, env.string("DATABASE_URL", "sqlite::memory:"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_clone_gets_an_in_memory_database() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.get(keys::DATABASE_URL).as_deref(), Some("sqlite::memory:"));
    }

    #[test]
    fn a_deployment_names_its_own() {
        let config = Config::new();
        configure(&config, &Env::parse("DATABASE_URL=mysql://app@db/app")).unwrap();

        assert_eq!(config.get(keys::DATABASE_URL).as_deref(), Some("mysql://app@db/app"));
    }
}
