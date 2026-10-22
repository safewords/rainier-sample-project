//! Controllers — `app/Http/Controllers`.
//!
//! Each action is a plain `async fn`; the router points at it by path.

#[cfg(feature = "accounts")]
pub mod account_controller;
pub mod auth_controller;
#[cfg(feature = "billing-stripe")]
pub mod billing_controller;
pub mod home_controller;
pub mod notification_controller;
#[cfg(feature = "oauth-server")]
pub mod oauth_controller;
pub mod post_controller;
#[cfg(feature = "sso")]
pub mod sso_controller;
