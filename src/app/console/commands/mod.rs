//! Your own commands — `app/Console/Commands`.
//!
//! Register them in `routes/console.rs`.

pub mod seed;

pub use seed::SeedCommand;
