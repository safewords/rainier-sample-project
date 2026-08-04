//! Models — `app/Models`.
//!
//! Rust has no autoloading, so every model is listed here. Adding one means
//! adding a file and a line.

pub mod post;
pub mod post_tag;
pub mod tag;
pub mod user;

pub use post::{Post, PostPublished};
pub use post_tag::PostTag;
pub use tag::Tag;
pub use user::User;
