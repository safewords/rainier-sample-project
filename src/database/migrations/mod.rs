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
//!   m0006_create_tags.rs          another table from a model
//!   m0007_create_post_tag.rs      a pivot: two keys, no model
//!   m0008_posts_add_excerpt.rs    an alter, with a derived rollback
//!   m0009 … m0018                 the add-ins' tables, created in every build
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
//!
//! ## And one consequence of the first rule
//!
//! A `create_table::<M>` step renders whatever the model says *today*, so a
//! field added to the model changes what an already-numbered migration does.
//! Which means a column the model carries cannot also arrive in a later alter:
//! a fresh database runs both, and `ADD COLUMN` has no portable "if not
//! exists". The trade-off is written up in `m0002_create_posts`, which is where
//! `posts.deleted_at` is created and where `m0008`'s model-less `excerpt` is
//! explained.

use rainier_framework::database::Migrator;

pub mod m0001_create_users;
pub mod m0002_create_posts;
pub mod m0003_index_posts_author;
pub mod m0004_add_post_search;
pub mod m0005_normalise_emails;
pub mod m0006_create_tags;
pub mod m0007_create_post_tag;
pub mod m0008_posts_add_excerpt;
pub mod m0009_create_email_verifications;
pub mod m0010_create_two_factor_enrolments;
pub mod m0011_create_passkeys;
pub mod m0012_create_social_accounts;
pub mod m0013_create_billing_customers;
pub mod m0014_create_subscriptions;
pub mod m0015_create_oauth_clients;
pub mod m0016_create_oauth_access_tokens;
pub mod m0017_create_oauth_refresh_tokens;
pub mod m0018_create_oauth_auth_codes;

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
        .add(m0006_create_tags::migration())
        .add(m0007_create_post_tag::migration())
        .add(m0008_posts_add_excerpt::migration())
        // The add-ins' tables. Created in every build — see `m0009`.
        .add(m0009_create_email_verifications::migration())
        .add(m0010_create_two_factor_enrolments::migration())
        .add(m0011_create_passkeys::migration())
        .add(m0012_create_social_accounts::migration())
        .add(m0013_create_billing_customers::migration())
        .add(m0014_create_subscriptions::migration())
        .add(m0015_create_oauth_clients::migration())
        .add(m0016_create_oauth_access_tokens::migration())
        .add(m0017_create_oauth_refresh_tokens::migration())
        .add(m0018_create_oauth_auth_codes::migration())
        // A framework component that needs a table brings its own migration.
        // The notification channel's rows are what the in-app list reads.
        .merge(rainier_framework::notifications::DatabaseChannel::migrations())
        // And the database queue's two tables, so `QUEUE_CONNECTION=database`
        // is one line in `.env` rather than a migration hunt. Creating tables a
        // deployment may not use costs two empty tables; the reverse costs a
        // boot that fails in production.
        .merge(rainier_framework::queue::DatabaseQueue::migrations())
    // Switching the session store to the database needs its table too:
    //
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
                "0006_create_tags",
                "0007_create_post_tag",
                "0008_posts_add_excerpt",
                "0009_create_email_verifications",
                "0010_create_two_factor_enrolments",
                "0011_create_passkeys",
                "0012_create_social_accounts",
                "0013_create_billing_customers",
                "0014_create_subscriptions",
                "0015_create_oauth_clients",
                "0016_create_oauth_access_tokens",
                "0017_create_oauth_refresh_tokens",
                "0018_create_oauth_auth_codes",
                "rainier_notify_0001_notifications",
                "rainier_queue_0001_jobs",
                "rainier_queue_0002_failed_jobs",
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
            "m0006_create_tags",
            "m0007_create_post_tag",
            "m0008_posts_add_excerpt",
            "m0009_create_email_verifications",
            "m0010_create_two_factor_enrolments",
            "m0011_create_passkeys",
            "m0012_create_social_accounts",
            "m0013_create_billing_customers",
            "m0014_create_subscriptions",
            "m0015_create_oauth_clients",
            "m0016_create_oauth_access_tokens",
            "m0017_create_oauth_refresh_tokens",
            "m0018_create_oauth_auth_codes",
        ];

        for (module, name) in modules.iter().zip(all().names()) {
            assert_eq!(module.trim_start_matches('m'), name, "`{module}.rs` declares `{name}`");
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
