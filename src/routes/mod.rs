//! Route and command declarations — Laravel's `routes/`.
//!
//! Split the same way Laravel splits them, and for the same reason: the groups
//! want different middleware. `web` gets session-shaped defaults, `api` gets
//! CORS and rate limiting, and `console` is not routing at all.

pub mod api;
pub mod channels;
pub mod console;
pub mod web;
