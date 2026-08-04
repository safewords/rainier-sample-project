//! Seeders — `database/seeders`.
//!
//! Run with `cargo run -- app:seed`.

use rainier_framework::database::Repository;
use rainier_framework::prelude::*;

use crate::app::models::Post;
use crate::app::providers::register_user;
use crate::app::repositories::{PostRepository, UserRepository};

/// Seed a demo user and a few posts.
///
/// Idempotent: it checks before inserting, so running it twice does not
/// produce two of everything or trip a unique index.
pub async fn seed(app: &Application) -> Result<()> {
    let users = app.resolve::<UserRepository>()?;
    let posts = app.resolve::<PostRepository>()?;

    let author = match users.by_email("ada@example.com").await? {
        Some(existing) => existing,
        None => register_user(app, "Ada Lovelace", "ada@example.com", "correct-horse").await?,
    };

    let seeds = [
        ("Welcome to Rainier", "A Laravel-shaped MVC framework for Rust.", true),
        ("On request contracts", "Authorise, validate, and bind a typed payload.", true),
        ("A draft nobody can see", "Unpublished posts 404 rather than leak.", false),
    ];

    for (title, body, published) in seeds {
        if posts.first_by("title", title.into()).await?.is_some() {
            continue;
        }

        let post = posts.create_unique(Post::draft(title, body, author.id)).await?;
        if published {
            // The named write rather than `update(&post)`. Nothing is racing a
            // seeder, so this is not the concurrency argument — it is that the
            // seeder should demonstrate the call a request would make, and a
            // sample whose seeder reaches for the unsafe-but-obvious one is
            // teaching that call.
            posts.publish(post.id).await?;
        }
    }

    Ok(())
}
