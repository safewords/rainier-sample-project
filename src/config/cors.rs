//! `config/cors.php` — which origins a browser may call this API from.
//!
//! # `*` is not the permissive end of this setting
//!
//! It is the most common thing to write here and it reads as "allow
//! everything", which is why it is worth spelling out what the three reachable
//! policies actually are. Two of them look like settings and are mistakes.
//!
//! | Written as | Answers | What it means |
//! |---|---|---|
//! | `any_origin()` | `Access-Control-Allow-Origin: *` | anyone may read a **public** response; no browser client can ever authenticate |
//! | `any_origin().allow_credentials(true)` | the caller's own origin, echoed | every page on the internet may make **authenticated** calls and read the answers |
//! | `allow_origins([…]).allow_credentials(true)` | the caller's origin if it is on the list | the policy an application means |
//!
//! The first row is the one this section was written to avoid, and its failure
//! is invisible from the configuration: **a browser does not attach a cookie to
//! a cross-origin request whose response omits
//! `Access-Control-Allow-Credentials`**, and it does not accept that header
//! alongside `Access-Control-Allow-Origin: *` either. So an API that
//! authenticates from a cookie and answers `*` has not been made lax. It has
//! been made impossible to log in to from a browser, while telling the whole
//! internet it is welcome. Every authenticated call arrives session-less and is
//! answered `401`, forever, and nothing in any log says why.
//!
//! The two halves are therefore one decision. Naming your origins is what makes
//! credentials possible, and credentials are what make the cookie arrive.
//!
//! The second row is what that discovery invites, because turning credentials on
//! and leaving the origins alone looks like the smaller change. In this
//! framework it does not fail: [`HandleCors::allowed_origin_for`] echoes the
//! caller's own origin rather than `*` when credentials are on, so the browser
//! is satisfied and the policy works. It works for everybody. Any page a user
//! visits may then call this API as them and read the response — which, for a
//! cookie, is the whole account. A bearer token is a smaller hole because an
//! attacker's page cannot produce the header, but the policy still answers yes
//! to every origin there is, which is not what anyone sat down to configure.
//!
//! # Why the development origins are declared in every environment
//!
//! They are only reachable by a browser already running on the developer's own
//! machine, so they give a deployment nothing. Gating them on `APP_ENV` would
//! make production's list differ from the list every developer tests against —
//! which is the arrangement that hides a CORS mistake until the deploy that
//! reveals it.
//!
//! There is no entry here for this application's *own* origin, and that is not
//! an omission. A page served from the same origin as the API is not making a
//! cross-origin request, so no rule in this file is consulted; adding it would
//! be a line that never does anything and reads as though it does.
//!
//! # This is a browser rule, and only a browser's
//!
//! Nothing here defends the API. `curl`, a mobile client, a server calling
//! another server — none of them consult a CORS policy, and none of them are
//! stopped by one. That is worth knowing before debugging with a tool that
//! ignores it, because "it works in Postman" is not evidence about the thing
//! that is broken. Authorisation is what the guard does; this decides which
//! *pages* a browser will hand a response to.
//!
//! # What naming origins costs
//!
//! A real thing: `GET /api/posts` is public, and under `*` a script on any page
//! anywhere could read it. Listing origins takes that away. If an application
//! genuinely wants a browser-readable public API, that is a second middleware
//! group with `any_origin()` and no credentials — and the rule for it is that no
//! cookie- or token-authenticated route may be in it, because the moment one is,
//! the first row of the table above is back.
//!
//! [`HandleCors::allowed_origin_for`]: rainier_framework::middleware::HandleCors::allowed_origin_for

use rainier_framework::config::{Config, Env};
use rainier_framework::prelude::*;

use crate::config::keys::CORS_ALLOWED_ORIGINS;

/// Where this application's own front end is served from.
///
/// `www` is listed beside the bare host because the two are **different
/// origins**. A browser compares scheme, host and port literally; it does not
/// know the two names reach the same server, and the symptom of leaving one out
/// is an application that works for whoever typed the URL one way.
const APPLICATION_ORIGINS: &[&str] = &["https://example.com", "https://www.example.com"];

/// Development servers, deliberately present in every environment — see the
/// module docs.
///
/// These two are Vite's own defaults, which is what `npm run dev` and
/// `npm run build && npx vite preview` in this repository actually listen on.
/// A list of plausible-looking ports nobody serves from would be decoration.
const DEVELOPMENT_ORIGINS: &[&str] = &["http://localhost:5173", "http://localhost:4173"];

/// The headers a browser may send on a cross-origin request.
///
/// `authorization` is the entry that has to be here. It is not one of the
/// handful of headers a browser will send without asking, so every API call
/// carrying a token triggers a preflight — and if the answer does not name it,
/// the browser does not strip the header and carry on, it refuses to send the
/// request at all. The failure is a request that never leaves, from a client
/// that is holding a perfectly good token.
pub const ALLOWED_HEADERS: &[&str] = &["content-type", "authorization", "x-requested-with"];

/// CORS settings, read by [`kernel`](crate::app::http::kernel) when it builds
/// the API middleware group.
///
/// Records a list; it builds no policy and installs no middleware. That is what
/// keeps "which origins are allowed" answerable from configuration alone,
/// including by a test that never boots an application.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    let mut origins: Vec<String> =
        APPLICATION_ORIGINS.iter().chain(DEVELOPMENT_ORIGINS).map(|o| (*o).to_string()).collect();

    // An escape hatch for an origin nobody anticipated — a preview deployment,
    // a partner embedding this application's widget. Comma-separated, and
    // **appended** rather than replacing what is above: a variable that replaced
    // the list would let one hurried entry lock the real front end out, and the
    // symptom is an application that is down for everybody except the person who
    // set it.
    let extra = env.string("CORS_ALLOWED_ORIGINS", "");
    for origin in extra.split(',').map(str::trim).filter(|o| !o.is_empty()) {
        // A wildcard is dropped rather than honoured, and it is warned about
        // rather than dropped quietly, because someone who wrote it wanted an
        // effect and needs to know they did not get one.
        //
        // It cannot be honoured. `*` is not a broader version of this policy —
        // the whole list exists so that credentials are possible, and a browser
        // rejects the two together. Appending it would either silently turn
        // authentication off for every browser client, or, since this framework
        // echoes the caller's origin instead, turn the policy into "yes, to
        // anyone who asks". See the module docs.
        if origin == "*" {
            tracing::warn!(
                "CORS_ALLOWED_ORIGINS contains `*`, which a credentialed policy cannot use; \
                 ignoring it — name the origins instead"
            );
            continue;
        }

        if !origins.iter().any(|o| o == origin) {
            origins.push(origin.to_string());
        }
    }

    config.set(CORS_ALLOWED_ORIGINS, origins)?;

    Ok(())
}

/// The declared origins, for the middleware to build a policy from.
pub fn allowed_origins(config: &Config) -> Vec<String> {
    config.get_or(CORS_ALLOWED_ORIGINS, Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `isolated` because this section reads a variable a developer may well
    /// have exported. Without it these assertions would pass or fail depending
    /// on whose shell ran them.
    fn origins_from(env: &Env) -> Vec<String> {
        let config = Config::new();
        configure(&config, env).unwrap();
        allowed_origins(&config)
    }

    #[test]
    fn every_declared_origin_is_present_with_nothing_configured() {
        // A missing origin is a browser failure on one deployment and nowhere
        // else, which is the hardest kind to reproduce — and the front end is
        // the one client that cannot work around it.
        let origins = origins_from(&Env::parse("").isolated());

        for declared in APPLICATION_ORIGINS.iter().chain(DEVELOPMENT_ORIGINS) {
            assert!(origins.iter().any(|o| o == declared), "`{declared}` is not allowed");
        }
    }

    #[test]
    fn the_development_origins_do_not_depend_on_the_environment() {
        // Deliberate, not an oversight. A production list that differs from the
        // one every developer tests against is the arrangement that hides a
        // CORS mistake until the deploy.
        let local = origins_from(&Env::parse("APP_ENV=local").isolated());
        let production = origins_from(&Env::parse("APP_ENV=production").isolated());

        assert_eq!(local, production);
        assert!(production.iter().any(|o| o == "http://localhost:5173"));
    }

    #[test]
    fn an_extra_origin_is_added_rather_than_replacing_the_list() {
        // Appending is what stops this variable from locking the real front end
        // out — a failure that looks like the API being down for everyone
        // except whoever set it.
        let origins =
            origins_from(&Env::parse("CORS_ALLOWED_ORIGINS=https://preview.example").isolated());

        assert!(origins.iter().any(|o| o == "https://preview.example"));
        assert!(origins.iter().any(|o| o == "https://example.com"));
    }

    #[test]
    fn several_extra_origins_are_read_from_one_variable() {
        let origins = origins_from(
            &Env::parse("CORS_ALLOWED_ORIGINS=https://a.example, https://b.example").isolated(),
        );

        assert!(origins.iter().any(|o| o == "https://a.example"));
        // Trimmed: the space after the comma is how a human writes a list, and
        // an untrimmed ` https://b.example` matches no `Origin` header ever
        // sent — which fails as a missing origin, several files from the typo.
        assert!(origins.iter().any(|o| o == "https://b.example"));
    }

    #[test]
    fn the_list_never_contains_a_wildcard() {
        // The assertion this section exists for. A `*` reaching the list would
        // not loosen the policy: it would either stop every browser client
        // authenticating, or hand a credentialed answer to whoever asked. Both
        // read, from here, as the most permissive setting available.
        let origins = origins_from(&Env::parse("CORS_ALLOWED_ORIGINS=*").isolated());

        assert!(!origins.iter().any(|o| o == "*"), "a wildcard is not a usable origin here");
        // And the rest of the list survived it, rather than the whole variable
        // being discarded on account of one bad entry.
        assert!(origins.iter().any(|o| o == "https://example.com"));
    }

    #[test]
    fn an_origin_is_never_listed_twice() {
        // Harmless in the response, which only ever names one origin — but a
        // duplicate is the visible half of a list that grew by appending, and
        // the invisible half is a `CORS_ALLOWED_ORIGINS` nobody can read.
        let origins =
            origins_from(&Env::parse("CORS_ALLOWED_ORIGINS=https://example.com").isolated());

        let count = origins.iter().filter(|o| *o == "https://example.com").count();
        assert_eq!(count, 1, "duplicated: {origins:?}");
    }

    #[test]
    fn a_token_is_never_the_only_thing_a_browser_needs() {
        // `authorization` is not a header a browser sends without asking. If it
        // is missing from this list the preflight answer omits it and the
        // request is never sent — so the client holds a valid token and every
        // call fails before it leaves the machine.
        assert!(ALLOWED_HEADERS.contains(&"authorization"));
    }
}
