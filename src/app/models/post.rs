//! The `Post` model — `app/Models/Post.php`.

use chrono::{DateTime, Utc};
use rainier_framework::prelude::*;
use serde::Serialize;

use crate::app::models::{Tag, User};

/// A post, owned by a user.
#[derive(Entity, Clone, Debug, PartialEq, Serialize)]
#[orm(table = "posts")]
#[orm(index = "published, created_at")]
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
}
