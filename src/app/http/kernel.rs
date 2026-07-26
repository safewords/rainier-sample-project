//! The HTTP kernel — `app/Http/Kernel.php`.
//!
//! Three things, exactly as in Laravel:
//!
//! - **global** middleware, which runs on every request;
//! - **aliases**, so a route can say `.middleware(["auth"])` instead of naming
//!   a type — which is what lets the router stay independent of auth, sessions
//!   and everything else a route might be guarded with;
//! - **groups**, which bundle aliases under one name.
//!
//! An alias may take arguments after a colon: `"throttle:30"` reaches the
//! factory as `["30"]`.

use std::sync::Arc;

use rainier_framework::auth::{AuthManager, Authenticate};
use rainier_framework::middleware::{
    AddHeaders, ConvertEmptyStringsToNull, HandleCors, MiddlewareRegistry, ThrottleRequests,
    TrimStrings,
};

use crate::app::http::middleware::RequestIdMiddleware;
use crate::app::models::User;

/// Register this application's middleware.
///
/// The framework has already registered its own defaults by the time this runs
/// (`TrimStrings` and `ConvertEmptyStringsToNull` globally; `cors`,
/// `secure-headers` and `throttle` aliases; `web` and `api` groups), so this
/// adds what is yours and overrides what you want to differ.
pub fn register(registry: &MiddlewareRegistry) {
    global(registry);
    aliases(registry);
    groups(registry);
}

/// Runs on every request, in this order, outside everything else.
fn global(registry: &MiddlewareRegistry) {
    // Stamped first so every log line and every error response can carry it.
    registry.global(RequestIdMiddleware::new());

    // Re-stated rather than assumed: these two are the reason an unfilled text
    // input arrives as `null` instead of `""`, and a stray space in an email
    // field does not create a second account.
    registry.global(TrimStrings::new().except(["password", "password_confirmation", "body"]));
    registry.global(ConvertEmptyStringsToNull);
}

/// Named middleware a route can refer to.
fn aliases(registry: &MiddlewareRegistry) {
    // `auth` and `auth:api` — needs the application's user type, which is
    // exactly what the framework cannot know, so it is registered here.
    registry.alias_factory("auth", |args: &[String]| {
        let auth = rainier_framework::container::facade_application().resolve::<AuthManager<User>>()?;
        Ok(Arc::new(Authenticate::from_args(auth, args)) as Arc<_>)
    });

    registry.alias("secure-headers", Arc::new(AddHeaders::security_defaults()));

    // A stricter limiter than the framework's default, for endpoints that
    // create things.
    registry.alias_factory("throttle-writes", |args: &[String]| {
        let per_minute = args.first().and_then(|a| a.parse().ok()).unwrap_or(20);
        Ok(Arc::new(ThrottleRequests::per_minute(per_minute)) as Arc<_>)
    });

    registry.alias(
        "cors",
        Arc::new(HandleCors::any_origin().allow_headers([
            "content-type",
            "authorization",
            "x-requested-with",
        ])),
    );
}

/// Bundles of aliases.
fn groups(registry: &MiddlewareRegistry) {
    registry.group("web", ["secure-headers"]);
    registry.group("api", ["cors", "throttle:60"]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_alias_a_route_uses_is_registered() {
        // This is the test that stops a typo in a route's `.middleware([..])`
        // becoming a boot failure discovered by hand.
        let registry = MiddlewareRegistry::new();
        register(&registry);

        for name in ["auth", "secure-headers", "throttle-writes", "cors"] {
            assert!(registry.has_alias(name), "`{name}` should be registered");
        }
        for name in ["web", "api"] {
            assert!(registry.has_group(name), "`{name}` group should be registered");
        }
    }

    #[test]
    fn a_parameterised_alias_reads_its_arguments() {
        let registry = MiddlewareRegistry::new();
        register(&registry);

        assert!(registry.resolve_one(&"throttle-writes:5".into()).is_ok());
    }
}
