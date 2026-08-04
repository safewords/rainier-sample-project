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

use std::sync::Arc;

use rainier_framework::auth::Authenticate;
use rainier_framework::config::Config;
use rainier_framework::groups;
use rainier_framework::metrics::{Metrics, RecordMetrics};
use rainier_framework::telemetry::Trace;

use rainier_framework::middleware::{
    AddHeaders, ConvertEmptyStringsToNull, HandleCors, MiddlewareRegistry, MiddlewareStack,
    ThrottleRequests, TrimStrings,
};

use crate::app::http::middleware::RequestIdMiddleware;
use crate::app::models::User;
use crate::config;

/// Register this application's **global** middleware.
///
/// The framework has already registered its own (`TrimStrings` and
/// `ConvertEmptyStringsToNull`), so this adds what is ours.
pub fn register(registry: &MiddlewareRegistry, trace: Option<Trace>) {
    // Outermost, when it is on at all: everything logged while handling the
    // request — including by the middleware below, and by one that rejects it
    // — carries the trace id. A trace registered after something else would
    // miss exactly the lines you go looking for.
    if let Some(trace) = trace {
        registry.global(trace);
    }

    // Stamped next so every log line and every error response can carry it.
    registry.global(RequestIdMiddleware::new());

    // CORS, and it is **global** rather than on the `api` group. That is not a
    // preference, and the group is where it started — `cors` below is what the
    // policy says, and this comment is where it has to say it.
    //
    // A browser does not send a cross-origin `POST` of JSON, or anything
    // carrying `Authorization`, without first asking permission with
    // `OPTIONS /api/posts`. No route accepts `OPTIONS`, so the router matches
    // the path, rejects the method, and answers `405` — and a group's
    // middleware belongs to the route's own pipeline, which never ran. The
    // preflight is refused, with no CORS headers on the refusal, so the request
    // it was asking about is never sent.
    //
    // The reach is the whole authenticated surface: every write, and every call
    // carrying a token. What survives is exactly the requests that need no
    // preflight — a plain `GET` — which is why a group-mounted policy looks
    // like it works, and why the test that covered this asserted a `GET`.
    //
    // Global middleware wraps the router instead of living inside it, so it
    // sees the preflight before routing does and answers it itself. It also
    // means a `404` and a `405` carry the headers, which matters more than it
    // sounds: without them a browser reports a CORS failure for a URL that is
    // simply a typo, and the afternoon goes into this file.
    //
    // `resolved` because the origin list comes from `config/cors.rs` and this
    // function runs while the application is still being assembled. The stack
    // is built here and the policy at boot, from the container.
    registry.global(MiddlewareStack::new().resolved(|settings: Arc<Config>| cors(&settings)));

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

/// The JSON API: a rate limit, and no session.
///
/// A session row and a `Set-Cookie` per call would be pure overhead for a
/// client that authenticates with a token on every request.
///
/// CORS is **not** here, and used to be. It is global now, because a preflight
/// matches no route and a group's middleware only runs for a route that
/// matched — see [`register`].
pub fn api(metrics: Option<Arc<Metrics>>) -> MiddlewareStack {
    let stack = MiddlewareStack::new();

    // Here rather than in the global stack, and that is not a detail: the
    // router attaches the matched route just before a route's own pipeline
    // runs, so a group-level middleware can label a series `/posts/{post}`
    // and a global one can only say `<unmatched>`.
    //
    // Which is the same fact that moved CORS the other way, read from the other
    // end. A group sees the matched route and nothing that failed to match; the
    // global stack sees everything and can name none of it. Metrics want the
    // first, a preflight needs the second.
    let stack = match metrics {
        Some(metrics) => stack.with(RecordMetrics::new(metrics)),
        None => stack,
    };

    stack.with(ThrottleRequests::per_minute(60))
}

/// The CORS policy, from `config/cors.rs`. Installed by [`register`].
///
/// **Credentialed, and therefore origin-listed.** This was
/// `HandleCors::any_origin()` with no credentials, which is not the lax version
/// of this policy but a different one — see the module docs on
/// [`config::cors`], which set out what the three reachable policies actually
/// do. The short version is that a browser will not attach a cookie to a
/// cross-origin request whose response omits
/// `Access-Control-Allow-Credentials`, and will not accept that header beside
/// `Access-Control-Allow-Origin: *`. Naming the origins is what makes
/// credentials possible; credentials are what make the cookie arrive.
///
/// The builder starts at `any_origin()` because that is the constructor — read
/// the chain, not the first call: `allow_origins` is what the policy ends up
/// being, and the type carries a list from there on.
///
/// # Why credentials are on when the guard reads a bearer token
///
/// [`auth`] resolves a `TokenGuard` today, so nothing here reads a cookie and
/// the flag changes no behaviour for the clients that exist. It is on because
/// of which direction the mistake runs. `AppServiceProvider` already
/// notes that a `SessionGuard` under the name `web` is what a cookie front end
/// adds, and the `web` group already issues a session cookie — so that day is
/// one guard away. On it, an uncredentialed policy answers `401` to every
/// authenticated cross-origin call, with a correct guard, a correct stack, and
/// nothing in any log naming CORS. This file is the last place anybody looks.
///
/// Turning it on now costs a token client one response header it ignores.
fn cors(settings: &Config) -> HandleCors {
    HandleCors::any_origin()
        .allow_origins(config::cors::allowed_origins(settings))
        .allow_headers(config::cors::ALLOWED_HEADERS.iter().copied())
        .allow_credentials(true)
}

/// The API, authenticated with a bearer token.
///
/// A group built from another group — the composition Laravel spells by
/// listing `'api'` inside another array, except this one is checked.
pub fn api_authenticated(metrics: Option<Arc<Metrics>>) -> MiddlewareStack {
    api(metrics).with_stack(auth("api"))
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

    use rainier_framework::config::Env;

    /// Configuration as the running application has it, without booting one.
    fn settings() -> Config {
        let settings = Config::new();
        config::configure(&settings, &Env::parse("").isolated()).unwrap();
        settings
    }

    #[test]
    fn the_cors_policy_answers_a_declared_origin_with_itself() {
        // Not `*`, and the difference is the whole section: a browser refuses
        // `Access-Control-Allow-Credentials` beside a wildcard, so a policy
        // that answered one could carry no cookie and no session.
        let policy = cors(&settings());

        assert_eq!(
            policy.allowed_origin_for(Some("https://example.com")),
            Some("https://example.com".to_string())
        );
    }

    #[test]
    fn an_origin_nobody_declared_is_answered_with_no_header_at_all() {
        // The half `any_origin()` could not express. An omitted
        // `Access-Control-Allow-Origin` is what makes the browser refuse to
        // hand the response to the calling page.
        let policy = cors(&settings());

        assert_eq!(policy.allowed_origin_for(Some("https://somewhere.example")), None);
    }

    #[test]
    fn no_request_can_make_this_policy_answer_a_wildcard() {
        // Including one with no `Origin` at all, which is the shape a
        // same-origin request and a non-browser client both take.
        let policy = cors(&settings());

        for origin in [Some("https://example.com"), Some("https://somewhere.example"), None] {
            assert_ne!(policy.allowed_origin_for(origin), Some("*".to_string()), "{origin:?}");
        }
    }

    #[test]
    fn the_policy_reads_its_origins_from_configuration_and_not_from_here() {
        // The reason the stage is `resolved`. A policy built where it is
        // registered would have to hardcode its list, and the escape hatch in
        // `config/cors.rs` would then set a key nothing reads — which is a
        // deployment adding an origin, restarting, and seeing no change.
        let settings = Config::new();
        config::configure(&settings, &Env::parse("CORS_ALLOWED_ORIGINS=https://later.example"))
            .unwrap();

        assert_eq!(
            cors(&settings).allowed_origin_for(Some("https://later.example")),
            Some("https://later.example".to_string())
        );
    }

    #[test]
    fn cors_is_global_and_not_on_the_api_group() {
        // Where this runs is the whole of whether it runs at all. A preflight
        // is `OPTIONS` against a path no route accepts `OPTIONS` for, so the
        // router answers 405 and no group pipeline is ever entered — see
        // `register`. The feature suite proves the consequence end to end; this
        // is the cheap assertion that notices someone moving it back.
        let registry = MiddlewareRegistry::new();
        register(&registry, None);

        assert!(registry.global_labels().contains(&"HandleCors"));
        assert!(!api(None).labels().contains(&"HandleCors"), "{:?}", api(None).labels());
    }

    #[test]
    fn cors_is_stamped_before_anything_reads_the_body() {
        // A preflight has no body to trim and is answered without one. Putting
        // the policy ahead of the input middleware keeps that work off a
        // request that short-circuits, and keeps it behind `RequestId` so the
        // answer is logged with the same id as everything else.
        let registry = MiddlewareRegistry::new();
        register(&registry, None);

        let labels = registry.global_labels();
        let cors = labels.iter().position(|l| *l == "HandleCors").expect("registered");
        let trim = labels.iter().position(|l| *l == "TrimStrings").expect("registered");
        let id = labels.iter().position(|l| *l == "RequestId").expect("registered");

        assert!(id < cors, "{labels:?}");
        assert!(cors < trim, "{labels:?}");
    }

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
        assert!(!api(None).labels().contains(&"StartSession"));
        assert_eq!(api(None).labels(), vec!["ThrottleRequests"]);
    }

    #[test]
    fn metrics_are_only_in_the_stack_when_they_are_configured_on() {
        // An application that does not scrape should not be timing every
        // request, so the middleware is absent rather than recording into a
        // registry nobody reads.
        assert!(!api(None).labels().contains(&"RecordMetrics"));

        let metrics = Some(Arc::new(Metrics::new()));
        assert!(api(metrics).labels().contains(&"RecordMetrics"));
    }

    #[test]
    fn metrics_come_before_anything_that_can_short_circuit() {
        // The rate limiter can answer 429 without calling `next`. Timing it
        // from outside is the only way that request is counted at all.
        let labels = api(Some(Arc::new(Metrics::new()))).labels();

        let metrics = labels.iter().position(|l| *l == "RecordMetrics").expect("present");
        let throttle = labels.iter().position(|l| *l == "ThrottleRequests").expect("present");

        assert!(metrics < throttle, "{labels:?}");
    }

    #[test]
    fn the_authenticated_api_is_the_api_plus_a_guard() {
        assert_eq!(
            api_authenticated(None).labels(),
            vec!["ThrottleRequests", "Authenticate"],
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
        register(&registry, None);

        // `RequestId` rather than `RequestIdMiddleware`: the middleware
        // overrides `name()`, and the label is what `route:list` prints.
        //
        // `HandleCors` is named here even though nothing has built it yet — a
        // deferred stage takes its label from the type it will produce, so the
        // list a reader sees is the list that will run.
        assert_eq!(
            registry.global_labels(),
            vec!["RequestId", "HandleCors", "TrimStrings", "ConvertEmptyStringsToNull"]
        );
    }
}
