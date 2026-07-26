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
    fn it_undoes_itself_by_dropping_the_table() {
        assert_eq!(
            migration().down(Dialect::Sqlite),
            Down::Statements(vec!["DROP TABLE IF EXISTS posts".to_string()])
        );
    }
}
