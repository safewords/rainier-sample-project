//! Configuration — Laravel's `config/` directory.
//!
//! Laravel has one file per concern returning an array. This is one **module**
//! per concern, each writing into the same dotted tree — so the keys are the
//! same (`app.name`, `session.lifetime`, `posts.per_page`) and so is the
//! reason to split them: a file you can read in one screen.
//!
//! ```text
//! src/config/
//!   mod.rs        the entry point, calling each section
//!   app.rs        config/app.php
//!   session.rs    config/session.php
//!   mail.rs       config/mail.php
//!   posts.rs      config/posts.php — an application's own section
//! ```
//!
//! Read any of it back with the `Config` facade:
//!
//! ```ignore
//! let per_page: u64 = Config::instance().get_or("posts.per_page", 15);
//! ```
//!
//! ## Where a value belongs
//!
//! | The value… | Goes… |
//! |---|---|
//! | differs per environment (URLs, credentials, drivers) | in `.env`, read with `env.string(..)` |
//! | differs per environment but has a safe default | in `.env` with a fallback |
//! | is the same everywhere in this application | a **literal** here |
//! | is the same for every application | not configuration — a constant |
//!
//! Not every setting needs an environment variable, and pretending otherwise
//! turns `.env` into a dumping ground. A literal here is still discoverable in
//! one place, readable through the facade, and overridable in a test.

use rainier_framework::config::{Config, Env};
use rainier_framework::prelude::*;

pub mod app;
pub mod mail;
pub mod posts;
pub mod session;

/// Apply every configuration section.
///
/// The framework has already filled in its own defaults (`app.*`, `server.*`,
/// `database.*`, `queue.*`, `mail.*`) from the environment by the time this
/// runs, so each section only sets what is specific to this application — and
/// overrides anything it wants to differ.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    app::configure(config, env)?;
    session::configure(config, env)?;
    mail::configure(config, env)?;
    posts::configure(config, env)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_apply_with_no_environment() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.string("app.name").as_deref(), Some("Rainier Sample"));
        assert_eq!(config.int("posts.per_page"), Some(15));
        assert_eq!(config.int("session.lifetime"), Some(7200));
    }

    #[test]
    fn the_environment_overrides_a_default() {
        let config = Config::new();
        configure(&config, &Env::parse("APP_NAME=Custom\nPOSTS_PER_PAGE=50")).unwrap();

        assert_eq!(config.string("app.name").as_deref(), Some("Custom"));
        assert_eq!(config.int("posts.per_page"), Some(50));
    }

    #[test]
    fn every_section_is_reached() {
        // A section added to the directory but not to `configure` would
        // silently never apply, and the symptom is a fallback used everywhere.
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        for key in ["app.name", "session.driver", "mail.file_path", "posts.per_page"] {
            assert!(config.has(key), "`{key}` was not set — is its section wired into configure?");
        }
    }
}
