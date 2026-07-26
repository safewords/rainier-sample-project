//! Migrations — `database/migrations`.
//!
//! Ordered, named, and idempotent. Each runs at most once, tracked in a
//! `rainier_migrations` table, so `migrate` is safe to run on every boot.
//!
//! Names are prefixed with a number because they run **in the order listed**
//! and never re-run: renaming one makes it run again, so treat an applied name
//! as permanent.

use rainier_framework::database::Migrator;

use crate::app::models::{Post, User};

/// Every migration, in order.
pub fn all() -> Migrator {
    Migrator::new()
        // `create_table` renders the DDL from the model's own metadata, so the
        // schema cannot drift from the struct that defines it.
        .create_table::<User>("0001_create_users")
        .create_table::<Post>("0002_create_posts")
        // A step can also be SQL you write. `raw` runs the same statement on
        // every backend; `step` takes a closure and renders per dialect, for
        // the cases where they genuinely differ.
        .raw(
            "0003_index_posts_author",
            vec!["CREATE INDEX IF NOT EXISTS idx_posts_author ON posts (author_id)".into()],
        )
    // Switching a driver to the database needs its tables; merge them in here
    // so `migrate` creates them:
    //
    // .merge(rainier_framework::queue::DatabaseQueue::migrations())
    // .merge(rainier_framework::session::DatabaseSessionStore::migrations())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_named_in_order() {
        assert_eq!(
            all().names(),
            vec!["0001_create_users", "0002_create_posts", "0003_index_posts_author"]
        );
    }

    #[test]
    fn the_schema_comes_from_the_models() {
        let ddl = rainier_framework::database::schema::schema_ddl::<Post>(
            rainier_framework::database::Dialect::Sqlite,
        )
        .join("\n");

        assert!(ddl.contains("posts"), "{ddl}");
        assert!(ddl.contains("slug"), "{ddl}");
        assert!(ddl.contains("users"), "the foreign key should be declared: {ddl}");
    }
}
