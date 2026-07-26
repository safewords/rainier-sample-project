//! Your application.
//!
//! The layout mirrors a Laravel project, so if you know where something lives
//! there, you know where it lives here:
//!
//! ```text
//! src/
//!   bootstrap.rs        bootstrap/app.php   — assembles and boots the app
//!   config/             config/*.php        — one module per concern
//!   app/
//!     models/           app/Models          — entities the framework manages
//!     http/
//!       kernel.rs       app/Http/Kernel.php — global middleware, aliases, groups
//!       controllers/    app/Http/Controllers
//!       middleware/     app/Http/Middleware
//!       requests/       app/Http/Requests   — request contracts (form requests)
//!     providers/        app/Providers       — service registration
//!     jobs/             app/Jobs
//!     mail/             app/Mail
//!     policies/         app/Policies        — authorisation
//!     console/commands/ app/Console/Commands
//!   database/
//!     migrations.rs     database/migrations
//!     seeders.rs        database/seeders
//!   routes/
//!     web.rs            routes/web.php
//!     api.rs            routes/api.php
//!     console.rs        routes/console.php
//! resources/views/      resources/views     — Blade-style templates
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
