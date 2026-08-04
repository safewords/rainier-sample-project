//! Repositories — where the domain's queries get names.
//!
//! Laravel has no counterpart, because Eloquent puts scopes on the model and a
//! query anywhere it likes. Rainier separates the two: a [model](super::models)
//! describes a row, and a repository is the only thing that knows how to fetch
//! one.
//!
//! ```text
//! src/app/repositories/
//!   mod.rs                the module, and what a repository is for
//!   post_repository.rs    PostRepository
//!   tag_repository.rs     TagRepository
//!   user_repository.rs    UserRepository
//! ```
//!
//! ## Why a newtype rather than a trait
//!
//! `EntityRepository<M>` already implements every CRUD operation for any
//! model, so these exist for one reason: to give **this application's** queries
//! a name.
//!
//! ```rust,ignore
//! // Assembled in a controller: "published" is defined wherever it is used.
//! posts.paginate_matching(
//!     Criteria::new().where_eq("published", true).order_by_desc("created_at"),
//!     page,
//!     per_page,
//! ).await?
//!
//! // Named here: defined once.
//! posts.published_page(page, per_page, search, tag_id).await?
//! ```
//!
//! The second is not shorter by accident. When "published" grows a second
//! condition — not scheduled for the future, carrying this tag — the first has
//! to be found in every controller that assembled it and the second has one
//! place to change.
//!
//! "Not in the bin" is the exception, and it is worth knowing why. [`Post`]
//! marks a `#[orm(soft_delete)]` column, so that condition is appended by the
//! ORM to every read here whether or not anybody remembered it — which is the
//! stronger form of the same argument, because a predicate a repository has to
//! carry is one a new method can still be written without.
//!
//! [`Post`]: super::models::Post
//!
//! `Deref` exposes everything the generic repository already does, so the
//! newtype costs nothing: `posts.find(id)` and `posts.published_page(..)` are
//! both available and neither is forwarded by hand.
//!
//! ## They are bound, not constructed
//!
//! A controller resolves one from the container rather than building it, so
//! the database handle and the event dispatcher are wired in exactly one place
//! — see [`RepositoryServiceProvider`](super::providers::RepositoryServiceProvider).

pub mod post_repository;
pub mod tag_repository;
pub mod user_repository;

pub use post_repository::PostRepository;
pub use tag_repository::{TagCount, TagRepository};
pub use user_repository::UserRepository;
