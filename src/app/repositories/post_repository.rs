//! `PostRepository` — the queries this application asks about posts.
//!
//! # Every read here is scoped, and none of them says so
//!
//! [`Post`] marks a `#[orm(soft_delete)]` column, so the reads below append
//! `deleted_at IS NULL` without being asked. That is the point: the predicate a
//! human has to remember is the predicate a human eventually forgets, and
//! forgetting it raises nothing. The page renders, with the deleted posts on
//! it.
//!
//! The methods that mean to see tombstoned rows say so out loud — see
//! [`PostRepository::trashed_for_author`] — because that direction is the hazard
//! the scope introduces, and it is exactly as quiet: a bin view under the scope
//! is not an error, it is an empty page.

use chrono::Utc;
use std::ops::Deref;
use std::sync::Arc;

use rainier_framework::database::Subquery;
use rainier_framework::events::Dispatcher;
use rainier_framework::prelude::*;

use crate::app::models::Post;

/// Access to posts.
pub struct PostRepository {
    inner: EntityRepository<Post>,
}

impl PostRepository {
    /// A repository over `db`, firing model hooks through `events`.
    pub fn new(db: Database, events: Arc<Dispatcher>) -> Self {
        Self { inner: EntityRepository::<Post>::new(db).with_events(events) }
    }

    /// A page of published posts, newest first, optionally filtered by title
    /// and by tag.
    ///
    /// The tag filter is a **correlated subquery**, and it is the one shape a
    /// `column = value` builder cannot express. A post's tags live in
    /// `post_tag`, so "posts carrying this tag" compares a column of one table
    /// against a column of another — `post_tag.post_id = posts.id` — which is
    /// not a bound value.
    ///
    /// `EXISTS` rather than a join, because a join over a many-to-many
    /// multiplies the rows: a post with three tags comes back three times, and
    /// the paginator's `total` counts those duplicates. `EXISTS` asks whether
    /// there is at least one match and stops there, so a page of fifteen posts
    /// is fifteen posts.
    ///
    /// Note that a `Subquery` cannot be built without correlating it. That is
    /// the type refusing the worst version of this query rather than the docs
    /// warning about it: `EXISTS (SELECT 1 FROM post_tag)` is true for *every*
    /// post the moment any post anywhere carries any tag, so the filter matches
    /// the whole table. Nothing errors and the SQL reads plausibly.
    pub async fn published_page(
        &self,
        page: u64,
        per_page: u64,
        search: Option<&str>,
        tag_id: Option<u64>,
    ) -> Result<Paginated<Post>> {
        let term = search.map(str::trim).filter(|term| !term.is_empty());

        let criteria = Criteria::new()
            .where_eq("published", true)
            .order_by_desc("created_at")
            // `when` keeps the optional filter declarative, instead of
            // branching around two nearly identical queries.
            .when(term.is_some(), |criteria| {
                criteria.where_like("title", format!("%{}%", term.unwrap_or_default()))
            })
            .when(tag_id.is_some(), |criteria| {
                criteria.where_exists(
                    Subquery::count("post_tag")
                        // The tie to the outer row. Without it this reads
                        // "there exists a link to this tag anywhere", which is
                        // true for every post as soon as one post has it.
                        .correlate("post_id", "id")
                        .where_eq("tag_id", tag_id.unwrap_or_default()),
                )
            });

        self.inner.paginate_matching(criteria, page, per_page).await
    }

    /// The published post with this slug.
    pub async fn published_by_slug(&self, slug: &str) -> Result<Option<Post>> {
        self.inner
            .first_matching(Criteria::new().where_eq("slug", slug).where_eq("published", true))
            .await
    }

    /// Every post by an author — traversing the `author_id` foreign key.
    pub async fn for_author(&self, author_id: u64) -> Result<Vec<Post>> {
        self.inner.find_by("author_id", author_id.into()).await
    }

    /// Publish a draft — one column, and no others.
    ///
    /// Deliberately **not** `update(&post)`, which is the obvious call and the
    /// wrong one. `update` writes every non-key column from a struct that was
    /// read at some earlier moment, so an edit that landed in between is
    /// written back to what it was then:
    ///
    /// ```text
    /// t0  the publish request loads the post   (title = "Draft title")
    /// t1  the author renames it                (title = "Real title")
    /// t2  publish sets `published` and calls `update`
    ///       → UPDATE posts SET title = 'Draft title', body = …, published = 1
    /// ```
    ///
    /// The rename is gone. Nothing errored, one row was affected, and the
    /// return value is the same `1` a correct write produces.
    ///
    /// The criteria carries the guard as well as the key. `where_eq("published",
    /// false)` makes this "publish it, but only if it is still a draft" in one
    /// statement, so two requests racing cannot both come away believing they
    /// were the one that published it — and only one of them dispatches the
    /// author's notification. Under `update` that check has to happen in the
    /// process, between the read and the write, which is where the race lives.
    ///
    /// Returns whether this call was the one that published it.
    pub async fn publish(&self, id: u64) -> Result<bool> {
        let published = self
            .inner
            .update_column(
                Criteria::new().where_eq("id", id).where_eq("published", false),
                "published",
                true,
            )
            .await?;

        Ok(published == 1)
    }

    /// Move a post to the bin.
    ///
    /// A soft delete is an `UPDATE`, not a `DELETE`, and this application writes
    /// it rather than the ORM: `#[orm(soft_delete)]` scopes **reads**, and
    /// leaves `delete` meaning delete. That is deliberate — a purge job needs a
    /// `DELETE` that actually removes the tombstoned rows it names, and one that
    /// had been quietly rewritten into another tombstone write would leave the
    /// table growing forever with nothing to say so.
    ///
    /// The write is unscoped, which is what lets it reach a row a read cannot
    /// see. `where_null("deleted_at")` is therefore this method's own guard and
    /// not the framework's: without it, binning an already-binned post would
    /// move its tombstone forward and reset the retention clock somebody is
    /// counting on.
    ///
    /// Returns whether this call was the one that binned it.
    pub async fn trash(&self, id: u64) -> Result<bool> {
        let trashed = self
            .inner
            .update_column(
                Criteria::new().where_eq("id", id).where_null("deleted_at"),
                "deleted_at",
                Utc::now(),
            )
            .await?;

        Ok(trashed == 1)
    }

    /// Take a post back out of the bin.
    ///
    /// The mirror of [`trash`](Self::trash), and it exists to make the point
    /// that a write is not scoped. Under a scoped write this would match no rows
    /// — the row it means to reach is exactly the one a read hides — and it
    /// would report success while restoring nothing, forever.
    ///
    /// It is named by slug rather than handed a `Post`, and that is not a
    /// convenience. Route-model binding reads through the scope, so
    /// `Bound<Post>` answers 404 for a binned post: there is no `Post` for a
    /// restore endpoint to be given.
    pub async fn restore(&self, slug: &str) -> Result<bool> {
        let restored = self
            .inner
            .update_column(
                Criteria::new().where_eq("slug", slug).where_not_null("deleted_at"),
                "deleted_at",
                None::<chrono::DateTime<Utc>>,
            )
            .await?;

        Ok(restored == 1)
    }

    /// What an author has in the bin, newest first.
    ///
    /// `only_trashed()` is the whole method. Without it this returns an empty
    /// list — not an error, not a warning, an empty bin — because the scope
    /// every other read here relies on is the scope this one has to turn off.
    /// That is the hazard soft deletes introduce, and it is why the opt-out is
    /// a named method rather than a predicate somebody remembers to write.
    pub async fn trashed_for_author(&self, author_id: u64) -> Result<Vec<Post>> {
        self.inner
            .matching(
                Criteria::new()
                    .where_eq("author_id", author_id)
                    .order_by_desc("deleted_at")
                    .only_trashed(),
            )
            .await
    }

    /// Store a post, giving it a slug nothing else has taken.
    ///
    /// Two posts with the same title would otherwise collide on the unique
    /// index, and a constraint violation is a worse answer than a suffix.
    ///
    /// `with_trashed()` is load-bearing here, and it is the least obvious use of
    /// it in this file. The unique index does not know about the scope: a binned
    /// post still occupies its slug in the database. A probe that read through
    /// the scope would find nothing, conclude the slug was free, and hand the
    /// insert straight into the constraint violation this loop exists to avoid —
    /// and it would do it only for titles that had been used and binned, which
    /// is the kind of bug that reaches production because nobody tests it.
    pub async fn create_unique(&self, mut post: Post) -> Result<Post> {
        let base = post.slug.clone();
        let mut suffix = 2;

        while self
            .inner
            .exists(Criteria::new().where_eq("slug", post.slug.clone()).with_trashed())
            .await?
        {
            post.slug = format!("{base}-{suffix}");
            suffix += 1;
        }

        self.inner.create(post).await
    }
}

impl Deref for PostRepository {
    type Target = EntityRepository<Post>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
