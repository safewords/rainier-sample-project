//! Queued jobs — `app/Jobs`.
//!
//! Every job must also be registered on the `JobRegistry` in
//! `app/providers/queue_provider.rs`, or a worker cannot turn its name back
//! into code.

pub mod notify_author;

pub use notify_author::NotifyAuthor;
