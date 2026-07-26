//! `PostController` — `app/Http/Controllers/PostController.php`.
//!
//! A controller action is a plain `async fn`. Its parameters say what it
//! needs and the framework supplies them, so an action is testable by calling
//! it — no request to assemble, no container to stand up.

use rainier_framework::auth::AuthenticatedUser;
use rainier_framework::prelude::*;

use crate::app::http::requests::{ListPostsRequest, StorePostRequest};
use crate::app::jobs::NotifyAuthor;
use crate::app::models::{Post, PostPublished, User};
use crate::app::policies::PostPolicy;
use crate::app::repositories::PostRepository;

/// `GET /api/posts` — a page of published posts.
pub async fn index(Validated(query): Validated<ListPostsRequest>) -> Result<Response> {
    let posts = resolve::<PostRepository>()?;
    let page = posts.published_page(query.page, query.per_page, query.search.as_deref()).await?;

    Ok(Response::json(&page))
}

/// `GET /api/posts/{post}` — one post, by slug.
pub async fn show(request: Req) -> Result<Response> {
    let slug = route_param(&request, "post")?;
    let posts = resolve::<PostRepository>()?;

    // Filtered on `published` in the query rather than fetched and then
    // checked, so an unpublished post is a 404 and not a hint that it exists.
    let post = posts
        .published_by_slug(&slug)
        .await?
        .ok_or_else(|| Error::not_found("No Post matches the given key."))?;

    Ok(Response::json(&post))
}

/// `POST /api/posts` — create a draft. Behind `auth:api`.
pub async fn store(
    request: Req,
    Validated(input): Validated<StorePostRequest>,
) -> Result<Response> {
    let author = current_user(&request)?;
    let posts = resolve::<PostRepository>()?;

    let created =
        posts.create_unique(Post::draft(input.title, input.body, author.id)).await?;

    Ok(Response::json(&created).with_status(StatusCode::CREATED))
}

/// `POST /api/posts/{post}/publish` — publish a draft. Behind `auth:api`.
///
/// The interesting part is what it does *not* do: it does not send mail. It
/// writes the row, raises an event and queues a job, and the response goes out
/// immediately. Whether the author's notification succeeds is the worker's
/// problem, not this request's.
pub async fn publish(request: Req) -> Result<Response> {
    let author = current_user(&request)?;
    let slug = route_param(&request, "post")?;
    let posts = resolve::<PostRepository>()?;

    let mut post = posts
        .first_by("slug", slug.into())
        .await?
        .ok_or_else(|| Error::not_found("No Post matches the given key."))?;

    // Authorisation is a policy, not an `if` buried in the controller.
    PostPolicy::gate().authorize("posts.publish", &author, Some(&post))?;

    if post.published {
        // Publishing twice must not send a second notification.
        return Ok(Response::json(&post));
    }

    post.published = true;
    posts.update(&post).await?;

    Event::instance().dispatch(PostPublished { post: post.clone() }).await?;
    Queue::instance().dispatch(NotifyAuthor { post_id: post.id }).await?;

    Ok(Response::json(&post))
}

/// `DELETE /api/posts/{post}` — delete your own post. Behind `auth:api`.
pub async fn destroy(request: Req) -> Result<Response> {
    let author = current_user(&request)?;
    let slug = route_param(&request, "post")?;
    let posts = resolve::<PostRepository>()?;

    let post = posts
        .first_by("slug", slug.into())
        .await?
        .ok_or_else(|| Error::not_found("No Post matches the given key."))?;

    PostPolicy::gate().authorize("posts.delete", &author, Some(&post))?;
    posts.delete(post.id.into()).await?;

    Ok(Response::no_content())
}

// --- helpers ---------------------------------------------------------------

/// The user the `auth` middleware resolved.
///
/// A `401` rather than a panic if it is absent: moving the route out from
/// behind `auth` should be a wrong answer, not a crash.
pub(crate) fn current_user(request: &Request) -> Result<User> {
    request
        .extension::<AuthenticatedUser<User>>()
        .map(|user| user.get().clone())
        .ok_or_else(|| Error::unauthenticated("Unauthenticated."))
}

/// A route parameter the router captured.
pub(crate) fn route_param(request: &Request, name: &str) -> Result<String> {
    request
        .route_param(name)
        .map(str::to_string)
        .ok_or_else(|| Error::bad_request(format!("the route is missing its `{name}` parameter")))
}

/// Resolve a service from the container.
pub(crate) fn resolve<T: Send + Sync + 'static>() -> Result<Arc<T>> {
    rainier_framework::container::facade_application().resolve::<T>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_that_skipped_the_auth_middleware_is_a_401_not_a_panic() {
        let err = current_user(&Request::builder().build()).unwrap_err();
        assert_eq!(err.status(), 401);
    }

    #[test]
    fn a_missing_route_parameter_is_a_400() {
        let err = route_param(&Request::builder().build(), "post").unwrap_err();
        assert_eq!(err.status(), 400);
        assert!(err.message().contains("post"));
    }
}
