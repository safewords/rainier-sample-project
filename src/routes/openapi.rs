//! The OpenAPI document — what the router cannot know on its own.
//!
//! Half of the document is generated: every path, every method, the path
//! parameters, and a `401` on anything behind the guard. That half is read
//! from the compiled router, so it cannot go stale.
//!
//! This file is the other half — the summaries, the tags, and which request
//! contract each endpoint takes. Rust erases a handler's parameter types by the
//! time the router holds it, so there is nothing to introspect and it has to be
//! said.
//!
//! # The contracts are not restated
//!
//! `accepts(StorePostRequest::rules())` hands the document **the same rules the
//! validator runs**. Add a rule and the schema changes; delete a field and it
//! leaves both at once. That is the whole reason to generate a document rather
//! than write one.

use rainier_framework::openapi::{Endpoint, OpenApi};
use rainier_framework::validation::FormRequest;

use crate::app::http::requests::{ListNotificationsRequest, ListPostsRequest, StorePostRequest};

/// Describe this application's endpoints.
///
/// The title, the version and the server URL come from `config/openapi.rs` —
/// they are deployment facts, not code.
pub fn document() -> OpenApi {
    OpenApi::new("Rainier Sample API", "1.0.0")
        .description("The API the starter application serves.")
        .describe(
            "api.posts.index",
            Endpoint::new()
                .summary("List published posts")
                .tag("Posts")
                .accepts(ListPostsRequest::rules())
                .returns(200, "A page of posts, each with its author and tags"),
        )
        .describe(
            "api.posts.show",
            Endpoint::new()
                .summary("One post, by slug")
                .tag("Posts")
                .returns(200, "The post")
                .returns(404, "No such post, or it is still a draft"),
        )
        .describe(
            "api.posts.store",
            Endpoint::new()
                .summary("Create a draft")
                .description("The post is created unpublished. Publishing is a separate call.")
                .tag("Posts")
                .accepts(StorePostRequest::rules())
                .returns(201, "The created post"),
        )
        .describe(
            "api.posts.publish",
            Endpoint::new()
                .summary("Publish a draft")
                .description(
                    "Raises `PostPublished`, which queues the author's notification. \
                     Publishing twice is not an error and sends nothing the second time.",
                )
                .tag("Posts")
                .returns(200, "The published post")
                .returns(403, "Not your post"),
        )
        .describe(
            "api.posts.destroy",
            Endpoint::new()
                .summary("Move your own post to the bin")
                .description(
                    "A soft delete: the post stops appearing anywhere, and \
                     `/api/posts/{post}/restore` brings it back.",
                )
                .tag("Posts")
                .returns(204, "Binned")
                .returns(403, "Not your post"),
        )
        .describe(
            "api.posts.trashed",
            Endpoint::new()
                .summary("What you have in the bin")
                .tag("Posts")
                .returns(200, "Your binned posts, most recently binned first"),
        )
        .describe(
            "api.posts.restore",
            Endpoint::new()
                .summary("Take one of your posts back out of the bin")
                .description(
                    "It comes back as it was, published flag included. A slug that is not \
                     yours and one that does not exist answer the same 404.",
                )
                .tag("Posts")
                .returns(200, "The slug that was restored")
                .returns(404, "Nothing of yours in the bin under that slug"),
        )
        .describe(
            "api.notifications.index",
            Endpoint::new()
                .summary("The bell menu")
                .tag("Notifications")
                .accepts(ListNotificationsRequest::rules())
                .returns(200, "Notifications, newest first, with an unread count"),
        )
        .describe(
            "api.notifications.read",
            Endpoint::new()
                .summary("Mark one as read")
                .tag("Notifications")
                .returns(204, "Marked")
                .returns(404, "No such notification, or it is not yours"),
        )
        .describe(
            "api.notifications.read-all",
            Endpoint::new()
                .summary("Mark them all as read")
                .tag("Notifications")
                .returns(200, "How many changed"),
        )
        .describe(
            "api.me",
            Endpoint::new().summary("The authenticated user").tag("Auth").returns(200, "You"),
        )
        .describe(
            "api.logout",
            Endpoint::new().summary("Revoke the current token").tag("Auth").returns(204, "Done"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::container::Container;
    use rainier_framework::prelude::*;

    fn router() -> rainier_framework::routing::CompiledRouter {
        let mut router = Router::new();
        crate::routes::api::routes(&mut router, None);

        // The guard is resolved when the route is compiled, so the container
        // needs one. A manager with no guards registered is enough: nothing
        // here dispatches a request, it only reads the shape of the routes.
        //
        // Only the guard, because only the guard is deferred *by a group*. The
        // CORS policy is deferred too and is not needed here — it is registered
        // globally, and the global stack is built by the kernel rather than by
        // `compile`. Which is also how this test noticed the policy moving: it
        // failed, naming the service the router could not resolve, rather than
        // compiling a route whose policy came from somewhere else.
        let container = Container::new();
        container
            .instance(rainier_framework::auth::AuthManager::<crate::app::models::User>::new("api"));

        router.compile(&container).expect("compiles")
    }

    #[test]
    fn every_description_points_at_a_route_that_exists() {
        // The one way this file rots: renaming a route orphans its
        // documentation, silently. This is the whole of the fix.
        assert_eq!(document().dangling(&router()), Vec::<String>::new());
    }

    #[test]
    fn the_request_schema_comes_from_the_contract_itself() {
        let built = document().build(&router());
        let schema = &built["paths"]["/api/posts"]["post"]["requestBody"]["content"]
            ["application/json"]["schema"];

        // `StorePostRequest` says `Between(3, 120)`, and nobody typed 120 here.
        assert_eq!(schema["properties"]["title"]["maxLength"], 120);
        assert_eq!(schema["required"], serde_json::json!(["title", "body"]));
    }

    #[test]
    fn a_guarded_route_documents_its_401_and_its_scheme() {
        let built = document().build(&router());

        assert!(built["paths"]["/api/posts"]["post"]["responses"]["401"].is_object());
        assert!(built["paths"]["/api/posts"]["post"]["security"].is_array());
        assert!(built["components"]["securitySchemes"]["bearerAuth"].is_object());
    }

    #[test]
    fn a_public_route_is_not_marked_as_requiring_a_token() {
        let built = document().build(&router());

        assert!(built["paths"]["/api/posts"]["get"]["security"].is_null());
    }

    #[test]
    fn a_path_parameter_is_documented_without_anyone_declaring_it() {
        let built = document().build(&router());
        let parameters = &built["paths"]["/api/posts/{post}"]["get"]["parameters"];

        assert_eq!(parameters[0]["name"], "post");
    }
}
