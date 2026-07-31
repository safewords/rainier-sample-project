//! Every configuration key this application names, in one place.
//!
//! Laravel answers "what settings are there?" by reading `config/`. Here the
//! answer is this file, and the compiler enforces it: a key that is not
//! declared cannot be written with a typed call, and a key whose type changes
//! breaks every reader at once rather than one of them at runtime.
//!
//! The framework's own keys are re-exported at the bottom, so a section imports
//! this module and gets both:
//!
//! ```ignore
//! use crate::config::keys::{self, POSTS_PER_PAGE};
//!
//! config.set(POSTS_PER_PAGE, 15)?;
//! config.set(keys::APP_NAME, "Rainier Sample".to_string())?;
//! ```

use rainier_framework::config::config_keys;

config_keys! {
    /// The locale templates and validation messages render in.
    pub APP_LOCALE: String = "app.locale";

    /// `SameSite` for the session cookie: `lax` or `strict`.
    ///
    /// A `String` rather than a setting enum because the value is handed
    /// straight to `SameSite`, which is the framework's own type and already
    /// has the closed set.
    pub SESSION_SAME_SITE: String = "session.same_site";

    // The mail keys live in `rainier_framework::keys` — the framework owns
    // the whole `mail.*` section now that it builds the transports. See
    // `config/mail.rs` for where this application reads them.

    /// How many posts a listing shows by default.
    pub POSTS_PER_PAGE: u64 = "posts.per_page";

    /// The largest page a client may ask for.
    pub POSTS_MAX_PER_PAGE: u64 = "posts.max_per_page";
}

// The framework's keys — `APP_NAME`, `CACHE_DRIVER`, `SESSION_LIFETIME`, and
// the rest. Glob-imported deliberately: a name that collided with one declared
// above would be a compile error here, which is the right place to find out.
pub use rainier_framework::keys::*;

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::config::Config;

    #[test]
    fn the_application_and_framework_keys_share_one_namespace() {
        // Both spellings work from one import, which is the point of the glob.
        let config = Config::new();
        config.set(POSTS_PER_PAGE, 15).unwrap();
        config.set(APP_NAME, "Rainier Sample".to_string()).unwrap();

        assert_eq!(config.get(POSTS_PER_PAGE), Some(15));
        assert_eq!(config.get(APP_NAME).as_deref(), Some("Rainier Sample"));
    }

    #[test]
    fn the_applications_own_keys_stay_out_of_the_frameworks_sections() {
        // `posts.*` is ours. Anything the application invents belongs under a
        // prefix the framework will never claim, or a future upgrade renames a
        // setting out from under it.
        assert!(POSTS_PER_PAGE.path().starts_with("posts."));
        assert!(POSTS_MAX_PER_PAGE.path().starts_with("posts."));
    }
}
