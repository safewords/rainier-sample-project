//! `0002_create_posts` — the `posts` table, and the foreign key into `users`.

use rainier_framework::database::Step;

use crate::app::models::Post;

/// Create `posts`.
///
/// After `0001` on purpose. [`Post`] declares
/// `#[orm(references = "users(id)", on_delete = "cascade")]`, so the generated
/// DDL contains a `FOREIGN KEY` clause and the table it points at has to exist
/// first.
///
/// That ordering constraint is also why a rollback undoes a batch in
/// **reverse**: dropping `users` while `posts` still references it fails on
/// every backend that enforces foreign keys.
///
/// # The price of deriving a table from a model
///
/// This step has no fixed shape. It renders whatever [`Post`] declares *today*,
/// so adding a field to the model changes what an already-numbered migration
/// does — and the rule that follows from that is worth stating, because it is
/// not obvious and it bites the first time somebody tries it.
///
/// **A column this table needs after `0002` has shipped cannot be added by an
/// alter if it is also on the model.** A fresh database would run `0002` — which
/// now creates the column — and then the alter, which would try to add it again;
/// `ALTER TABLE … ADD COLUMN` has no portable "if not exists", and SQLite has
/// none at all. So it is one or the other:
///
/// | The column is… | Where it goes |
/// |---|---|
/// | read or written by [`Post`] | a field on the model, created here |
/// | not on the model | its own alter — see `m0008_posts_add_excerpt`, which adds an `excerpt` no field reads |
///
/// `deleted_at` is the first kind. `#[orm(soft_delete)]` is a *model* marker —
/// the scope it installs comes from the entity, not from the schema — so the
/// column has to be a field, and this migration is where it is created.
///
/// The corollary is the part to keep in mind: an application already running
/// against a migrated database gets no new column from a model change, and
/// nothing reports it. Deriving the schema from the model is right while the
/// model is the newest thing in the repository, and stops being right the moment
/// somebody else's database has run `0002`.
pub fn migration() -> Step {
    Step::create_table::<Post>("0002_create_posts")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::database::{Dialect, Down, Migration};

    #[test]
    fn it_declares_the_foreign_key_into_users() {
        // The property that makes the order load-bearing. If this stops being
        // true, `0001` and `0002` become independent and the comment above is
        // a lie.
        let up = migration().up(Dialect::Sqlite).join("\n");

        assert!(up.contains("posts"), "{up}");
        assert!(up.contains("users"), "the foreign key should be declared: {up}");
    }

    #[test]
    fn the_tombstone_column_comes_from_the_model_rather_than_a_later_alter() {
        // The assertion behind the rule in this file's docs. `#[orm(soft_delete)]`
        // is a marker on the entity, so the column has to be a field — and a
        // field is created *here*. An alter adding it as well would fail on a
        // fresh database, which runs both.
        let up = migration().up(Dialect::Sqlite).join("\n");

        assert!(up.contains("deleted_at"), "{up}");
    }

    #[test]
    fn it_undoes_itself_by_dropping_the_table() {
        assert_eq!(
            migration().down(Dialect::Sqlite),
            Down::Statements(vec!["DROP TABLE IF EXISTS posts".to_string()])
        );
    }
}
