//! `PostController` — `app/Http/Controllers/PostController.php`.
//!
//! A controller action is a plain `async fn`. Its parameters say what it
//! needs and the framework supplies them, so an action is testable by calling
//! it — no request to assemble, no container to stand up.

use rainier_framework::auth::AuthenticatedUser;
use rainier_framework::prelude::*;

use crate::app::http::requests::{ListPostsRequest, StorePostRequest};
use crate::app::models::{Post, PostPublished, Tag, User};
use crate::app::policies::PostPolicy;
use crate::app::repositories::{PostRepository, UserRepository};

/// `GET /api/posts` — a page of published posts, with their authors and tags.
///
/// **Three queries, whatever the page size.** The posts, then one for every
/// author on the page, then the pivot and the tags. Twenty posts do not become
/// forty-one queries, and they cannot: a relationship is loaded for the whole
/// slice at once, so there is no per-post load to accidentally put in the loop
/// below.
pub async fn index(Validated(query): Validated<ListPostsRequest>) -> Result<Response> {
    let posts = resolve::<PostRepository>()?;
    let tag_rows = resolve::<EntityRepository<Tag>>()?;

    // `?tag=rust` names a tag rather than keying one, so it is resolved here.
    // An unknown name filters to nothing, which is the honest answer — falling
    // back to "no filter" would quietly answer a different question from the one
    // asked, and look like the tag matched everything.
    let tag_id = match query.tag.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => match tag_rows.find_by_route_key(Tag::named(name).name).await? {
            Some(tag) => Some(tag.id),
            None => return Ok(Response::json(&empty_page(query.page, query.per_page))),
        },
        None => None,
    };

    let page =
        posts.published_page(query.page, query.per_page, query.search.as_deref(), tag_id).await?;

    // `&**`: these repositories are newtypes that `Deref` to an
    // `EntityRepository`, and it is the inner one that implements the contract
    // a relationship loads through.
    let users = resolve::<UserRepository>()?;

    let authors = Post::author().load(&page.data, &**users).await?;
    let tags = Post::tags().load(&page.data, &*tag_rows).await?;

    let data: Vec<_> = page
        .data
        .iter()
        .map(|post| {
            serde_json::json!({
                "post": post,
                // `None` rather than an error: an author deleted between the
                // two queries is a race, not a broken response.
                "author": authors.one(post).map(|user| &user.name),
                "tags": tags.of(post).iter().map(|tag| &tag.name).collect::<Vec<_>>(),
            })
        })
        .collect();

    Ok(Response::json(&serde_json::json!({
        "data": data,
        "total": page.total,
        "current_page": page.current_page,
        "per_page": page.per_page,
    })))
}

/// The shape `index` answers with when a filter matches nothing.
///
/// Spelled once so the empty case and the full one cannot disagree about what a
/// page looks like — a client that special-cased a missing `total` would break
/// on exactly the request nobody tests.
fn empty_page(page: u64, per_page: u64) -> serde_json::Value {
    serde_json::json!({
        "data": Vec::<serde_json::Value>::new(),
        "total": 0,
        "current_page": page,
        "per_page": per_page,
    })
}

/// `GET /api/posts/{post}` — one post, by slug.
///
/// The lookup and the 404 are both gone from the body: `Bound<Post>` resolves
/// `{post}` through the model's [route
/// key](rainier_framework::database::Model::route_key_name), which is the slug.
/// Laravel's `public function show(Post $post)`.
///
/// Binding does not authorise, so the draft check is still this action's job —
/// and it is a check, not a filtered query, because the model is already here.
/// A 404 rather than a 403, so an unpublished slug is not confirmed to exist.
pub async fn show(Bound(post): Bound<Post>) -> Result<Response> {
    if !post.published {
        return Err(Error::not_found("No Post matches the given key."));
    }

    Ok(Response::json(&post))
}

/// `POST /api/posts` — create a draft. Behind `auth:api`.
pub async fn store(
    author: AuthenticatedUser<User>,
    Validated(input): Validated<StorePostRequest>,
) -> Result<Response> {
    let posts = resolve::<PostRepository>()?;

    let created = posts.create_unique(Post::draft(input.title, input.body, author.id)).await?;

    Ok(Response::json(&created).with_status(StatusCode::CREATED))
}

/// `POST /api/posts/{post}/publish` — publish a draft. Behind `auth:api`.
///
/// The interesting part is what it does *not* do: it does not send mail. It
/// writes the row, raises an event and queues a job, and the response goes out
/// immediately. Whether the author's notification succeeds is the worker's
/// problem, not this request's.
pub async fn publish(
    author: AuthenticatedUser<User>,
    Bound(mut post): Bound<Post>,
) -> Result<Response> {
    let posts = resolve::<PostRepository>()?;

    // Authorisation is a policy, not an `if` buried in the controller — and
    // it is still here, because binding finds the row and says nothing about
    // who may have it.
    PostPolicy::gate().authorize("posts.publish", author.get(), Some(&post))?;

    // One statement writes one column, and its own criteria carries the
    // "still a draft" guard — so publishing twice cannot send two
    // notifications even if two requests arrive at once. This used to be an
    // `if post.published` here plus `posts.update(&post)`, and both halves
    // were wrong: the check raced, and the write rewrote every column of the
    // post from the copy this request happened to load. See
    // `PostRepository::publish`.
    if !posts.publish(post.id).await? {
        return Ok(Response::json(&post));
    }

    // The in-process copy, brought in line with the one column that changed.
    post.published = true;

    // One dispatch, and the controller is done. Who reacts — the log line,
    // the queued notification to the author, whatever is added next — is the
    // listener list's business, declared in `EventServiceProvider`.
    Event::instance().dispatch(PostPublished { post: post.clone() }).await?;

    Ok(Response::json(&post))
}

/// `DELETE /api/posts/{post}` — bin your own post. Behind `auth:api`.
///
/// A soft delete: the row stays and gains a tombstone, so it stops appearing
/// anywhere without the author losing it. [`restore`] is the way back.
pub async fn destroy(
    author: AuthenticatedUser<User>,
    Bound(post): Bound<Post>,
) -> Result<Response> {
    PostPolicy::gate().authorize("posts.delete", author.get(), Some(&post))?;
    resolve::<PostRepository>()?.trash(post.id).await?;

    Ok(Response::no_content())
}

/// `GET /api/posts/trashed` — what you have in the bin. Behind `auth:api`.
pub async fn trashed(author: AuthenticatedUser<User>) -> Result<Response> {
    let posts = resolve::<PostRepository>()?.trashed_for_author(author.id).await?;

    Ok(Response::json(&serde_json::json!({ "data": posts })))
}

/// `POST /api/posts/{post}/restore` — take one back out. Behind `auth:api`.
///
/// The one action in this controller that cannot take a `Bound<Post>`, and the
/// reason is the feature working: route-model binding looks the row up through
/// the same scope every other read uses, so a binned post has no model to
/// bind and the action would answer 404 before running.
///
/// So it takes the slug, and authorisation is a query rather than a policy
/// check against a loaded row — the row has to be found before it can be
/// judged, and finding it is the part the scope refuses.
pub async fn restore(author: AuthenticatedUser<User>, request: Arc<Request>) -> Result<Response> {
    let slug = request
        .route_param("post")
        .ok_or_else(|| Error::internal("the route has no `{post}` parameter to restore"))?
        .to_string();

    let posts = resolve::<PostRepository>()?;

    // Scoped to the author in the *query*, so somebody else's binned post is
    // indistinguishable from one that never existed. A restore that reported
    // "not yours" would confirm the slug is taken.
    let mine = posts
        .trashed_for_author(author.id)
        .await?
        .into_iter()
        .find(|post| post.slug == slug)
        .ok_or_else(|| Error::not_found("No Post matches the given key."))?;

    posts.restore(&mine.slug).await?;

    Ok(Response::json(&serde_json::json!({ "restored": mine.slug })))
}

// --- helpers ---------------------------------------------------------------
//
// There used to be two more here: `current_user`, which dug the user out of
// the request's extensions, and `route_param`, which pulled `{post}` out and
// left the caller to look the row up. Both are gone, and neither was replaced
// by another helper — an action now *asks* for what it needs
// (`AuthenticatedUser<User>`, `Bound<Post>`) and the framework supplies it.
//
// That is the difference between a controller that starts with four lines of
// unpacking and one whose signature says what it is about.

/// Resolve a service from the container.
pub(crate) fn resolve<T: Send + Sync + 'static>() -> Result<Arc<T>> {
    rainier_framework::container::facade_application().resolve::<T>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_action_that_asks_for_the_user_gets_a_401_without_the_middleware() {
        // The extractor answers this now, so the guarantee is the framework's
        // rather than this application's — but it is the one this application
        // relies on, so it is still asserted here.
        use rainier_framework::http::FromRequest;

        let request = Arc::new(Request::builder().build());
        let extracted = AuthenticatedUser::<User>::from_request(request).await;

        assert_eq!(extracted.expect_err("no user on the request").status(), 401);
    }

    #[tokio::test]
    async fn binding_a_model_the_route_cannot_supply_is_a_wiring_error() {
        // Not a 404 and not a 400: the route has no `{post}` at all, which is
        // a mistake in `routes/api.rs` rather than anything the caller did.
        use rainier_framework::http::FromRequest;

        let request = Arc::new(Request::builder().build());
        let err = Bound::<Post>::from_request(request).await.err().expect("nothing to bind");

        assert_eq!(err.status(), 500);
        assert!(err.message().contains("{post}"), "{}", err.message());
    }
}
