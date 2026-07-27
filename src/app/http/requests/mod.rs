//! Request contracts — `app/Http/Requests`.
//!
//! Laravel's form requests: authorise, validate, and bind a typed payload the
//! action can trust. A field with no rule never reaches the payload, which is
//! the mass-assignment protection `$fillable` gives you — obtained from the
//! rules you already wrote.

pub mod list_notifications;
pub mod login;
pub mod store_post;

pub use list_notifications::ListNotificationsRequest;
pub use login::{ListPostsRequest, LoginRequest};
pub use store_post::StorePostRequest;
