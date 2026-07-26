//! `config/posts.php` — an example of an application's own section.

use rainier_framework::config::{Config, Env};
use rainier_framework::prelude::*;

use crate::config::keys::{POSTS_MAX_PER_PAGE, POSTS_PER_PAGE};

/// How posts are listed.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // Tunable per environment: a bigger page is reasonable on a fast database.
    config.set(POSTS_PER_PAGE, env.int("POSTS_PER_PAGE", 15) as u64)?;

    // A literal, because it is a property of the API contract rather than of a
    // deployment — and because without an upper bound a client asking for
    // `per_page=1000000` is a denial of service against your own database.
    config.set(POSTS_MAX_PER_PAGE, 100)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_size_is_tunable_and_bounded() {
        let config = Config::new();
        configure(&config, &Env::parse("POSTS_PER_PAGE=50")).unwrap();

        assert_eq!(config.get(POSTS_PER_PAGE), Some(50));
        assert_eq!(config.get(POSTS_MAX_PER_PAGE), Some(100));
    }

    #[test]
    fn the_maximum_is_not_environment_driven() {
        let config = Config::new();
        configure(&config, &Env::parse("POSTS_MAX_PER_PAGE=1000000")).unwrap();

        assert_eq!(
            config.get(POSTS_MAX_PER_PAGE),
            Some(100),
            "the bound is the contract, not a deployment setting"
        );
    }
}
