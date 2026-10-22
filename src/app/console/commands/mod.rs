//! Your own commands — `app/Console/Commands`.
//!
//! Register them in `routes/console.rs`.

#[cfg(feature = "oauth-server")]
pub mod oauth_client;
pub mod seed;

#[cfg(feature = "oauth-server")]
pub use oauth_client::OAuthClientCommand;
pub use seed::SeedCommand;
