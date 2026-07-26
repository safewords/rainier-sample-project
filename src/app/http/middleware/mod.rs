//! Your own middleware — `app/Http/Middleware`.
//!
//! Register anything here in `app/http/kernel.rs`, either globally or as an
//! alias a route can name.

pub mod request_id;

pub use request_id::{RequestId, RequestIdMiddleware};
