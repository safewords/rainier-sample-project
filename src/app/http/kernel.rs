//! The HTTP kernel — `app/Http/Kernel.php`.
//!
//! Laravel's kernel is three maps: `$middleware` (global), `$routeMiddleware`
//! (alias → class) and `$middlewareGroups` (name → list of aliases). Two of
//! those exist only because PHP cannot put a class in a route file and have the
//! router mean it.
//!
//! Rust can, so this file has:
//!
//! - **global** middleware, registered on the [`MiddlewareRegistry`] — the one
//!   list that is genuinely a registry, since it belongs to no route;
//! - **groups**, which are plain functions returning a [`MiddlewareStack`].
//!
//! There are no aliases, because there is nothing to alias. A route attaches
//! `ThrottleRequests::per_minute(10)`, not `"throttle:10"`.
//!
//! ## What that buys
//!
//! | | With names | With values |
//! |---|---|---|
//! | `middleware("athu")` | boots; the route is unguarded | does not compile |
//! | Renaming a middleware | every route silently breaks | every route is a compile error naming it |
//! | "What does `web` do?" | grep the kernel | go to definition |
//! | A group taking a parameter | parse it back out of `"throttle:60,1"` | `api(60)` |
//!
//! The one thing an alias genuinely did — build middleware that needs the
//! container — is `MiddlewareStack::resolved`, which runs when the router
//! compiles. See [`auth`].

use rainier_framework::auth::Authenticate;
use rainier_framework::groups;
use rainier_framework::middleware::{
    AddHeaders, ConvertEmptyStringsToNull, HandleCors, MiddlewareRegistry, MiddlewareStack,
    ThrottleRequests, TrimStrings,
};

use crate::app::http::middleware::RequestIdMiddleware;
use crate::app::models::User;

/// Register this application's **global** middleware.
///
/// The framework has already registered its own (`TrimStrings` and
/// `ConvertEmptyStringsToNull`), so this adds what is ours.
pub fn register(registry: &MiddlewareRegistry) {
    // Stamped first so every log line and every error response can carry it.
    registry.global(RequestIdMiddleware::new());

    // Re-stated rather than assumed: these two are the reason an unfilled text
    // input arrives as `null` instead of `""`, and a stray space in an email
    // field does not create a second account. Restating them here costs a
    // second pass over the input and makes the list complete in one place.
    registry.global((
        TrimStrings::new().except(["password", "password_confirmation", "body"]),
        ConvertEmptyStringsToNull,
    ));
}

// --- groups ----------------------------------------------------------------
//
// Laravel's `$middlewareGroups`, as functions. Note what a function can do that
// a map entry cannot: take an argument, call another group, and be found by
// "go to definition".

/// Pages a browser visits: security headers plus a session.
///
/// Extends the framework's `web` rather than restating it. Laravel's
/// `$middlewareGroups['web'] = [...]` *replaces* the list, which is how a
/// deploy quietly loses `StartSession` while adding one thing.
pub fn web() -> MiddlewareStack {
    groups::web()
}

/// The JSON API: CORS, a rate limit, and no session.
///
/// A session row and a `Set-Cookie` per call would be pure overhead for a
/// client that authenticates with a token on every request.
pub fn api() -> MiddlewareStack {
    MiddlewareStack::new()
        .with(HandleCors::any_origin().allow_headers([
            "content-type",
            "authorization",
            "x-requested-with",
        ]))
        .with(ThrottleRequests::per_minute(60))
}

/// The API, authenticated with a bearer token.
///
/// A group built from another group — the composition Laravel spells by
/// listing `'api'` inside another array, except this one is checked.
pub fn api_authenticated() -> MiddlewareStack {
    api().with_stack(auth("api"))
}

/// Require an authenticated user, through the named guard.
///
/// The one place this application needs `resolved`: the `AuthManager<User>` is
/// bound by `AppServiceProvider`, and routes are declared before providers run.
/// The closure runs when the router compiles, which is after.
///
/// Note the `User` in the type. `"auth"` could never say which user model it
/// authenticates; this cannot avoid saying it.
pub fn auth(guard: &str) -> MiddlewareStack {
    Authenticate::<User>::resolved_with_guard(guard)
}

/// A stricter limiter for endpoints that create things.
pub fn throttle_writes(per_minute: u32) -> MiddlewareStack {
    MiddlewareStack::new().with(ThrottleRequests::per_minute(per_minute))
}

/// Security headers on their own, for a route outside `web`.
pub fn secure_headers() -> MiddlewareStack {
    MiddlewareStack::new().with(AddHeaders::security_defaults())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_web_group_still_starts_sessions() {
        // The mistake Laravel's replace-the-array model invites, and the
        // reason `web()` delegates instead of restating: dropping the session
        // fails nothing, and every page in the group quietly has none.
        assert!(
            web().labels().contains(&"StartSession"),
            "the `web` group should be security headers *and* the session, got {:?}",
            web().labels()
        );
    }

    #[test]
    fn the_api_group_has_no_session() {
        assert!(!api().labels().contains(&"StartSession"));
        assert_eq!(api().labels(), vec!["HandleCors", "ThrottleRequests"]);
    }

    #[test]
    fn the_authenticated_api_is_the_api_plus_a_guard() {
        assert_eq!(
            api_authenticated().labels(),
            vec!["HandleCors", "ThrottleRequests", "Authenticate"],
            "composition, not a second list to keep in step"
        );
    }

    #[test]
    fn a_group_can_take_an_argument() {
        // `"throttle:20"` parsed a number back out of a string. This is a
        // number.
        assert_eq!(throttle_writes(20).len(), 1);
    }

    #[test]
    fn the_global_stack_is_stamped_with_a_request_id_first() {
        // The order matters: everything after it can log the id.
        let registry = MiddlewareRegistry::new();
        register(&registry);

        // `RequestId` rather than `RequestIdMiddleware`: the middleware
        // overrides `name()`, and the label is what `route:list` prints.
        assert_eq!(
            registry.global_labels(),
            vec!["RequestId", "TrimStrings", "ConvertEmptyStringsToNull"]
        );
    }
}
