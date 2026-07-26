//! Repositories — where the domain's queries get names.
//!
//! `EntityRepository<M>` already implements every CRUD operation for any
//! model, so these newtypes exist for one reason: to give *this application's*
//! queries a name. `published_page` lives here rather than being a `Criteria`
//! each controller assembles, so "what published means" is defined once.
//!
//! `Deref` exposes everything the generic repository already does, so a
//! newtype costs nothing.

use std::ops::Deref;
use std::sync::Arc;

use rainier_framework::events::Dispatcher;
use rainier_framework::prelude::*;

use crate::app::models::{Post, User};

/// Access to posts.
pub struct PostRepository {
    inner: EntityRepository<Post>,
}

impl PostRepository {
    /// A repository over `db`, firing model hooks through `events`.
    pub fn new(db: Database, events: Arc<Dispatcher>) -> Self {
        Self { inner: EntityRepository::<Post>::new(db).with_events(events) }
    }

    /// A page of published posts, newest first, optionally filtered by title.
    pub async fn published_page(
        &self,
        page: u64,
        per_page: u64,
        search: Option<&str>,
    ) -> Result<Paginated<Post>> {
        let term = search.map(str::trim).filter(|term| !term.is_empty());

        let criteria = Criteria::new()
            .where_eq("published", true)
            .order_by_desc("created_at")
            // `when` keeps the optional filter declarative, instead of
            // branching around two nearly identical queries.
            .when(term.is_some(), |criteria| {
                criteria.where_like("title", format!("%{}%", term.unwrap_or_default()))
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

    /// Store a post, giving it a slug nothing else has taken.
    ///
    /// Two posts with the same title would otherwise collide on the unique
    /// index, and a constraint violation is a worse answer than a suffix.
    pub async fn create_unique(&self, mut post: Post) -> Result<Post> {
        let base = post.slug.clone();
        let mut suffix = 2;

        while self.inner.exists(Criteria::new().where_eq("slug", post.slug.clone())).await? {
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

/// Access to users.
pub struct UserRepository {
    inner: EntityRepository<User>,
}

impl UserRepository {
    /// A repository over `db`.
    pub fn new(db: Database) -> Self {
        Self { inner: EntityRepository::<User>::new(db) }
    }

    /// The user with this address.
    pub async fn by_email(&self, email: &str) -> Result<Option<User>> {
        self.inner.first_by("email", email.into()).await
    }
}

impl Deref for UserRepository {
    type Target = EntityRepository<User>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
