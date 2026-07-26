//! Migrations — `database/migrations`.
//!
//! Laravel has one file per migration, named by timestamp, discovered from the
//! directory. This is one **module** per migration, named by prefix, listed in
//! [`all`] — because Rust does not autoload, and a list you can read beats a
//! directory scan you cannot.
//!
//! ```text
//! src/database/migrations/
//!   mod.rs                        the ordered list — this file
//!   m0001_create_users.rs         create_table from the model's metadata
//!   m0002_create_posts.rs         …and a foreign key
//!   m0003_index_posts_author.rs   raw SQL, with the SQL that undoes it
//!   m0004_add_post_search.rs      a step that differs per dialect
//!   m0005_normalise_emails.rs     a data migration that cannot be undone
//! ```
//!
//! Each module exposes one `pub fn` returning a
//! [`Step`](rainier_framework::database::Step), so the name, the `up` and the
//! `down` are in one file and the order is in this one.
//!
//! ## Two rules
//!
//! **Names are permanent.** A name is the identity of an applied migration, so
//! renaming one makes it run again. The `m` prefix is there because a Rust
//! module cannot start with a digit; the number after it is what makes the
//! order visible in the directory listing.
//!
//! **Every step declares how to undo itself.** That is the
//! [contract](rainier_framework::database::Migration), not a convention. Where
//! a step genuinely cannot be undone, say so with `Down::irreversible` and the
//! reason travels with it — see `m0005_normalise_emails`.

use rainier_framework::database::Migrator;

pub mod m0001_create_users;
pub mod m0002_create_posts;
pub mod m0003_index_posts_author;
pub mod m0004_add_post_search;
pub mod m0005_normalise_emails;

/// Every migration, in order.
///
/// Order is declaration order, and it matters twice: forwards, because
/// `posts` has a foreign key into `users`; and backwards, because a rollback
/// undoes a batch in reverse and would otherwise drop `users` while `posts`
/// still points at it.
pub fn all() -> Migrator {
    Migrator::new()
        .add(m0001_create_users::migration())
        .add(m0002_create_posts::migration())
        .add(m0003_index_posts_author::migration())
        .add(m0004_add_post_search::migration())
        .add(m0005_normalise_emails::migration())
    // Switching a driver to the database needs its tables; merge them in here
    // so `migrate` creates them:
    //
    // .merge(rainier_framework::queue::DatabaseQueue::migrations())
    // .merge(rainier_framework::session::DatabaseSessionStore::migrations())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::database::Dialect;

    #[test]
    fn migrations_are_named_in_order() {
        assert_eq!(
            all().names(),
            vec![
                "0001_create_users",
                "0002_create_posts",
                "0003_index_posts_author",
                "0004_add_post_search",
                "0005_normalise_emails",
            ]
        );
    }

    #[test]
    fn the_module_prefix_matches_the_migration_name() {
        // `m0003_index_posts_author.rs` must contain `0003_index_posts_author`,
        // or the directory listing stops being the running order.
        let modules = [
            "m0001_create_users",
            "m0002_create_posts",
            "m0003_index_posts_author",
            "m0004_add_post_search",
            "m0005_normalise_emails",
        ];

        for (module, name) in modules.iter().zip(all().names()) {
            assert_eq!(
                module.trim_start_matches('m'),
                name,
                "`{module}.rs` declares `{name}`"
            );
        }
    }

    #[test]
    fn only_the_data_migration_is_irreversible() {
        // Everything else must be undoable, and this is the assertion that
        // notices when a new step quietly is not.
        assert_eq!(
            all().irreversible(Dialect::Sqlite),
            vec!["0005_normalise_emails"],
            "a step that cannot be rolled back should be a deliberate, visible choice"
        );
    }

    #[test]
    fn the_schema_comes_from_the_models() {
        let ddl = rainier_framework::database::schema::schema_ddl::<crate::app::models::Post>(
            Dialect::Sqlite,
        )
        .join("\n");

        assert!(ddl.contains("posts"), "{ddl}");
        assert!(ddl.contains("slug"), "{ddl}");
        assert!(ddl.contains("users"), "the foreign key should be declared: {ddl}");
    }
}
