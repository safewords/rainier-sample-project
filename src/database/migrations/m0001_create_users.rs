//! `0001_create_users` — the `users` table, from the model's own metadata.

use rainier_framework::database::Step;

use crate::app::models::User;

/// Create `users`.
///
/// `create_table` renders the DDL from [`User`]'s `#[orm]` attributes, for
/// whichever dialect the connection speaks. **The schema cannot drift from the
/// struct that defines it** — add a column to the struct and the table it
/// creates has that column, with no second place to keep in step.
///
/// Its `down` is the matching `DROP TABLE IF EXISTS`, which you get without
/// writing it. That is the honest inverse, and also why rolling this back in
/// production destroys data: the operation is destructive, not the
/// implementation of it.
pub fn migration() -> Step {
    Step::create_table::<User>("0001_create_users")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::database::{Dialect, Down, Migration};

    #[test]
    fn it_creates_the_users_table() {
        let up = migration().up(Dialect::Sqlite).join("\n");

        assert!(up.contains("users"), "{up}");
        assert!(up.contains("email"), "{up}");
        assert!(up.contains("IF NOT EXISTS"), "re-running must be a no-op: {up}");
    }

    #[test]
    fn it_undoes_itself_by_dropping_the_table() {
        assert_eq!(
            migration().down(Dialect::Sqlite),
            Down::Statements(vec!["DROP TABLE IF EXISTS users".to_string()])
        );
    }

    #[test]
    fn the_ddl_follows_the_dialect() {
        let sqlite = migration().up(Dialect::Sqlite).join("");
        let postgres = migration().up(Dialect::Postgres).join("");

        assert_ne!(sqlite, postgres, "each backend should get its own types");
    }
}
