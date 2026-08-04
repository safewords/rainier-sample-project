//! `routes/api.php` — the JSON API.
//!
//! Everything is under `/api` and inside the `api` middleware group (a rate
//! limit). The inner group adds the token guard.
//!
//! CORS is not on the group and is not declared here. It is global, because a
//! browser's preflight is an `OPTIONS` request against a path no route below
//! accepts `OPTIONS` for — so the router answers `405` and no group middleware
//! ever runs. See [`kernel::register`].
//!
//! Route names get the `api.` prefix from the group, so the URL generator
//! resolves `api.posts.show`.

use std::sync::Arc;

use rainier_framework::metrics::Metrics;
use rainier_framework::prelude::*;

use crate::app::http::controllers::{auth_controller, notification_controller, post_controller};
use crate::app::http::kernel;

/// Declare the API routes.
///
/// `metrics` is threaded in rather than resolved, because middleware is
/// **declared**: the group is built once, at boot, and cannot reach into the
/// container per request to find out whether it should be timing anything.
pub fn routes(router: &mut Router, metrics: Option<Arc<Metrics>>) {
    router.group(
        GroupAttributes::new().prefix("api").name("api.").middleware(kernel::api(metrics)),
        |router| {
            // --- observability --------------------------------------------
            //
            // Both answer `404` when their feature is off, so a deployment
            // that has not turned them on looks like one that has no such
            // endpoint — rather than one serving an empty document, or an
            // empty scrape that reads as an idle application.
            //
            // Neither is behind the guard here, which is a **sample's**
            // choice: a scrape endpoint tells a reader your traffic shape and
            // every route you serve, so a real deployment puts it behind
            // whatever its admin routes are behind, or binds it to an
            // interface only the scraper can reach.
            router
                .get("/metrics", rainier_framework::observability::metrics_endpoint)
                .name("metrics");
            router
                .get("/openapi.json", rainier_framework::observability::openapi_endpoint)
                .name("openapi");

            // --- public ---------------------------------------------------
            router.get("/posts", post_controller::index).name("posts.index");

            // Declared **before** `/posts/{post}`, and it has to be. Routes are
            // tried in declaration order and the first match wins, so
            // `/api/posts/trashed` would otherwise be read as a post whose slug
            // is `trashed` — and the `where_slug` constraint below does not
            // save it, because `trashed` *is* a slug. The symptom is a 404 from
            // a route that exists and is spelled correctly, which is a bad
            // afternoon.
            //
            // It carries its own guard here rather than joining the
            // authenticated group below, because that group is declared after
            // the parameter route — and moving the parameter route down would
            // order every public read behind an authenticated one.
            router.group(GroupAttributes::new().middleware(kernel::auth("api")), |router| {
                router.get("/posts/trashed", post_controller::trashed).name("posts.trashed");
            });

            router
                .get("/posts/{post}", post_controller::show)
                .name("posts.show")
                // A constraint, so `/api/posts/not a slug` is a clean 404
                // rather than a database round-trip.
                .where_slug("post");

            // --- authenticated --------------------------------------------
            // The guard names the user model it authenticates, which is the
            // thing `"auth:api"` could never say.
            router.group(GroupAttributes::new().middleware(kernel::auth("api")), |router| {
                router.get("/me", auth_controller::me).name("me");
                router.post("/logout", auth_controller::logout).name("logout");

                router
                    .post("/posts", post_controller::store)
                    .name("posts.store")
                    .middleware(kernel::throttle_writes(20));

                router
                    .post("/posts/{post}/publish", post_controller::publish)
                    .name("posts.publish")
                    .where_slug("post");

                router
                    .delete("/posts/{post}", post_controller::destroy)
                    .name("posts.destroy")
                    .where_slug("post");

                // The bin's other half is up above, before `/posts/{post}` —
                // see the note there. This one needs no such care: its literal
                // segment comes after the parameter, so nothing else matches
                // it.
                router
                    .post("/posts/{post}/restore", post_controller::restore)
                    .name("posts.restore")
                    .where_slug("post");

                // The in-app bell menu — what the database channel wrote.
                // `Broadcast::routes()`. Behind the guard, because it reads
                // the authenticated user to decide — without one every private
                // channel answers 401, which reads as a broken client.
                router
                    .post(
                        "/broadcasting/auth",
                        rainier_framework::broadcasting::authorize::<crate::app::models::User>,
                    )
                    .name("broadcasting.auth");

                router
                    .get("/notifications", notification_controller::index)
                    .name("notifications.index");
                router
                    .post("/notifications/read", notification_controller::read_all)
                    .name("notifications.read-all");
                router
                    .post("/notifications/{notification}/read", notification_controller::read)
                    .name("notifications.read");
            });
        },
    );
}
