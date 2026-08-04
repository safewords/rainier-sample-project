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

    /// Every origin a browser may call this application's API from.
    ///
    /// A `Vec<String>` and not a comma-separated `String`, so the one place
    /// that splits it is `config/cors.rs` — a reader that forgot to trim would
    /// hold ` https://example.com`, which matches no `Origin` header a browser
    /// has ever sent.
    ///
    /// There is deliberately no `CORS_ALLOW_CREDENTIALS` beside it. Credentials
    /// and the origin list are one decision, not two settings that can disagree:
    /// a deployment that turned credentials off would not get a more permissive
    /// API, it would get one no browser can authenticate against. See
    /// `config/cors.rs`.
    pub CORS_ALLOWED_ORIGINS: Vec<String> = "cors.allowed_origins";

    /// How many posts a listing shows by default.
    pub POSTS_PER_PAGE: u64 = "posts.per_page";

    /// The largest page a client may ask for.
    pub POSTS_MAX_PER_PAGE: u64 = "posts.max_per_page";

    // --- queue ---------------------------------------------------------------
    //
    // `QUEUE_SQS_URL` was here. It is gone for the same reason the storage keys
    // below are: a queue URL belongs to a *connection*, and `config/queue.rs`
    // declares it there. A scalar beside the section would have been a second
    // place to say the same thing, and the one nothing reads.

    // --- storage -------------------------------------------------------------
    //
    // Nothing here any more. Storage used to be six scalars in this
    // application's own namespace — one driver, one bucket, one region, one
    // endpoint — because the framework's `Storage` took a built filesystem
    // rather than reading configuration.
    //
    // It reads `rainier_framework::keys::FILESYSTEMS` now: a default disk plus
    // a map of declarations, each naming its own driver and settings. Six
    // scalars could describe exactly one disk, which is the shape that cannot
    // express a second one on another service — see `config/storage.rs`.
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
