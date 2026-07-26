//! Configuration — Laravel's `config/*.php`.
//!
//! Laravel has one file per concern; Rust has one function per concern, all
//! writing into the same dotted tree. Values come from the environment with a
//! sensible fallback, so a fresh clone runs with no `.env` at all.
//!
//! Read it back anywhere with the `Config` facade:
//!
//! ```ignore
//! let name: String = Config::instance().get_or("app.name", "Rainier".into());
//! ```

use rainier_framework::config::{Config, Env};
use rainier_framework::prelude::*;

/// Apply every configuration section.
///
/// The framework has already filled in its own defaults (`app.*`, `server.*`,
/// `database.*`, `queue.*`, `mail.*`) from the environment by the time this
/// runs, so this only sets what is specific to your application — and
/// overrides anything you want to differ.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    app(config, env)?;
    posts(config, env)?;
    Ok(())
}

/// `config/app.php`
fn app(config: &Config, env: &Env) -> Result<()> {
    config.set("app.name", env.string("APP_NAME", "Rainier Sample"))?;
    config.set("app.locale", env.string("APP_LOCALE", "en"))?;

    // Where mail written by the `file` transport lands.
    config.set("mail.file_path", env.string("MAIL_FILE_PATH", "storage/mail"))?;
    Ok(())
}

/// `config/posts.php` — an example of an application's own section.
fn posts(config: &Config, env: &Env) -> Result<()> {
    config.set("posts.per_page", env.int("POSTS_PER_PAGE", 15))?;
    // Bounded, so a client cannot ask for every row in one request.
    config.set("posts.max_per_page", env.int("POSTS_MAX_PER_PAGE", 100))?;
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
    }

    #[test]
    fn the_environment_overrides_a_default() {
        let config = Config::new();
        configure(&config, &Env::parse("APP_NAME=Custom\nPOSTS_PER_PAGE=50")).unwrap();

        assert_eq!(config.string("app.name").as_deref(), Some("Custom"));
        assert_eq!(config.int("posts.per_page"), Some(50));
    }
}
