//! Your application.
//!
//! The layout is the usual MVC one:
//!
//! ```text
//! src/
//!   bootstrap.rs      — assembles and boots the app
//!   config/           — one module per concern
//!   app/
//!     models/         — entities the framework manages
//!     http/
//!       kernel.rs     — global middleware, aliases, groups
//!       controllers/
//!       middleware/
//!       requests/     — request contracts (form requests)
//!     providers/      — service registration
//!     jobs/
//!     mail/
//!     policies/       — authorisation
//!     console/commands/
//!   database/
//!     migrations.rs
//!     seeders.rs
//!   routes/
//!     web.rs
//!     api.rs
//!     console.rs
//! resources/views/    — `*.view.html` templates
//! ```
//!
//! Two differences worth knowing, both because Rust has no autoloading:
//!
//! - **Every directory needs a `mod.rs`** listing its files. Adding a
//!   controller means adding one line to `src/app/http/controllers/mod.rs`.
//! - **Nothing is discovered by name.** A provider is registered in
//!   `bootstrap.rs`, a route in `routes/`, a job in the provider's job
//!   registry. The wiring is explicit, and the compiler tells you when you
//!   forget.

pub mod app;
pub mod bootstrap;
pub mod config;
pub mod database;
pub mod routes;

pub use bootstrap::{boot, Mode};
