//! `routes/api.php` — the JSON API.
//!
//! Everything is under `/api` and inside the `api` middleware group (CORS plus
//! a rate limit). The inner group adds the token guard.
//!
//! Route names get the `api.` prefix from the group, so the URL generator
//! resolves `api.posts.show`.

use rainier_framework::prelude::*;

use crate::app::http::controllers::{auth_controller, post_controller};
use crate::app::http::kernel;

/// Declare the API routes.
pub fn routes(router: &mut Router) {
    router.group(
        GroupAttributes::new().prefix("api").name("api.").middleware(kernel::api()),
        |router| {
            // --- public ---------------------------------------------------
            router.get("/posts", post_controller::index).name("posts.index");
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
            });
        },
    );
}
