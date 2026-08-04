//! The `Post` model — `app/Models/Post.php`.

use chrono::{DateTime, Utc};
use rainier_framework::prelude::*;
use serde::Serialize;

use crate::app::models::{Tag, User};

/// A post, owned by a user.
///
/// # It soft-deletes, and that is declared rather than inferred
///
/// [`deleted_at`](Post::deleted_at) carries `#[orm(soft_delete)]`, and one line
/// changes every read of this table: `find`, `first_matching`,
/// `paginate_matching`, the counts, the aggregates and the relation loads all
/// append `deleted_at IS NULL` themselves. A caller cannot forget it, which
/// matters because forgetting it raises nothing — the query runs, the rows
/// decode, the page renders, with the deleted posts on it.
///
/// Nothing sniffs for a column *named* `deleted_at`, and the difference is not
/// pedantry. A table that records a deletion date as **domain data** — when an
/// author retracted something, when an account was closed — would silently stop
/// returning most of its rows, on the upgrade that introduced the inference
/// rather than on a change anybody wrote. A marker costs one line and cannot
/// guess.
///
/// The other direction needs saying just as loudly, because turning the scope
/// on flips deliberately-trashed reads from working to returning nothing, and
/// just as silently. An admin bin, a restore endpoint, a purge job all mean to
/// see tombstoned rows and all come back empty under the scope. `with_trashed()`
/// and `only_trashed()` are how they opt out — see
/// [`PostRepository::trashed_for_author`](crate::app::repositories::PostRepository::trashed_for_author).
///
/// Note what the marker does *not* do: it scopes reads, it does not turn a
/// `DELETE` into an `UPDATE`. Writing the tombstone is this application's job
/// and it is one visible line — see
/// [`PostRepository::trash`](crate::app::repositories::PostRepository::trash).
#[derive(Entity, Clone, Debug, PartialEq, Serialize)]
#[orm(table = "posts")]
#[orm(index = "published, created_at")]
// For the direction the scope does *not* serve. Listing an author's bin is
// `deleted_at IS NOT NULL`, which no index on the other columns helps, and it is
// a page a person waits on.
#[orm(index = "author_id, deleted_at")]
pub struct Post {
    /// The primary key.
    #[orm(pk, auto_increment)]
    pub id: u64,

    /// The URL-safe identifier routes bind by.
    #[orm(unique)]
    pub slug: String,

    /// The headline.
    pub title: String,

    /// The body.
    pub body: String,

    /// Whether it is visible to the public.
    pub published: bool,

    /// Who wrote it.
    ///
    /// A flat column. The relationship over it is
    /// [`Post::author`](Post::author), which is a value you **load** rather
    /// than a property that queries itself — see
    /// [relationships](rainier_framework::database::relation).
    #[orm(index, references = "users(id)", on_delete = "cascade")]
    pub author_id: u64,

    /// When the row was created.
    pub created_at: DateTime<Utc>,

    /// When it was moved to the bin, or `None` while it is live.
    ///
    /// The tombstone, and `Option` is not a style choice: `NULL` is what "not
    /// deleted" means to the scope this marker installs, so a non-`Option`
    /// field would make every scoped read of `posts` return nothing. The derive
    /// refuses it at compile time rather than letting that ship.
    #[orm(soft_delete)]
    pub deleted_at: Option<DateTime<Utc>>,
}

impl Model for Post {
    /// Bind `/posts/{post}` by slug rather than by id, so URLs read well.
    fn route_key_name() -> &'static str {
        "slug"
    }
}

impl Post {
    /// The author — Eloquent's `belongsTo(User::class)`.
    ///
    /// The foreign key is not the convention (`user_id`), so it is named. The
    /// owner key still defaults to the user's primary key.
    ///
    /// ```ignore
    /// let authors = Post::author().load(&posts, &*users).await?;   // one query
    /// let name = &authors.one(&post).unwrap().name;
    /// ```
    pub fn author() -> BelongsTo<Post, User> {
        BelongsTo::new().foreign_key("author_id")
    }

    /// The tags attached to it — Eloquent's `belongsToMany(Tag::class)`.
    ///
    /// Through `post_tag(post_id, tag_id)`, which is the conventional name and
    /// so needs no column configuration.
    pub fn tags() -> BelongsToMany<Post, Tag> {
        BelongsToMany::new("post_tag")
    }

    /// A new, unsaved draft. The database assigns the key on insert.
    pub fn draft(title: impl Into<String>, body: impl Into<String>, author_id: u64) -> Self {
        let title = title.into();
        Self {
            id: 0,
            slug: rainier_framework::support::str::slug(&title),
            title,
            body: body.into(),
            published: false,
            author_id,
            created_at: Utc::now(),
            deleted_at: None,
        }
    }

    /// Whether `user_id` wrote this post.
    pub fn belongs_to(&self, user_id: u64) -> bool {
        self.author_id == user_id
    }
}

/// Raised when a post is published.
///
/// Listeners react without the controller knowing they exist — see
/// [`crate::app::providers::EventServiceProvider`].
#[derive(Debug, Clone, Serialize)]
pub struct PostPublished {
    /// The post that went live.
    pub post: Post,
}

/// The same fact, pushed to any browser watching — Laravel's `ShouldBroadcast`.
///
/// One type, dispatched in-process *and* broadcast. Nothing is discovered
/// though: implementing this makes it broadcast**able**, and a listener in
/// [`EventServiceProvider`](crate::app::providers::EventServiceProvider) still
/// has to broadcast it.
impl Broadcastable for PostPublished {
    /// A public channel, because a published post is public.
    ///
    /// The author's own view of it is `private-posts.{slug}`, declared in
    /// `routes/channels.rs` — a private channel for the same subject, gated by
    /// the policy.
    fn broadcast_on(&self) -> Vec<BroadcastChannelName> {
        vec![BroadcastChannelName::public("posts")]
    }

    /// Pinned, because a JavaScript client listens for this string. Renaming
    /// the struct would otherwise rename the event and the listener would go
    /// quiet rather than error.
    fn broadcast_as(&self) -> String {
        "post.published".into()
    }

    /// **Not** the whole post.
    ///
    /// The default would serialise every field, body included, to anyone
    /// subscribed to a public channel. A broadcast payload is a notification
    /// that something changed; the client fetches what it needs.
    fn broadcast_with(&self) -> Result<serde_json::Value> {
        Ok(serde_json::json!({
            "slug": self.post.slug,
            "title": self.post.title,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_draft_derives_its_slug_and_starts_unpublished() {
        let post = Post::draft("Hello, World!", "body", 1);

        assert_eq!(post.slug, "hello-world");
        assert!(!post.published);
        assert_eq!(post.id, 0, "the database assigns the key");
    }

    #[test]
    fn ownership_is_by_author_id() {
        let post = Post::draft("Mine", "body", 7);
        assert!(post.belongs_to(7));
        assert!(!post.belongs_to(8));
    }

    #[test]
    fn the_route_key_is_the_slug() {
        assert_eq!(Post::route_key_name(), "slug");
        assert_eq!(Post::primary_key(), "id");
    }

    #[test]
    fn the_tombstone_column_is_declared_and_not_guessed() {
        // The one line that scopes every read of this table. Asserting it here
        // is asserting the *declaration*, which is what the scope is built
        // from — a column of the same name without the marker would leave every
        // deleted post visible on every surface, with nothing to report.
        assert_eq!(Post::soft_delete_column(), Some("deleted_at"));

        // And it is a `Tag`, not this model, that shows what an unmarked entity
        // still does: exactly what it did before soft deletes existed.
        assert_eq!(crate::app::models::Tag::soft_delete_column(), None);
    }
}
