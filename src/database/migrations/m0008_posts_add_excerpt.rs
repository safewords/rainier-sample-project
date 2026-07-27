//! `0008_posts_add_excerpt` — changing a table that already has rows.

use rainier_framework::database::Step;

/// Add `posts.excerpt`, and an index for listing by author.
///
/// The everyday migration, and the one where a hand-written `down` goes stale
/// first: add a second column later, forget the matching `DROP COLUMN`, and
/// the rollback silently leaves it behind. Here the `down` is **derived from
/// the change**, so it cannot disagree with the `up`.
///
/// Note `nullable()`. A `NOT NULL` column added to a table that already holds
/// rows has no value for them, and every engine refuses it — so a new column
/// is either nullable or carries a `default()`.
pub fn migration() -> Step {
    Step::table("0008_posts_add_excerpt", "posts", |table| {
        table.string_len("excerpt", 500).nullable();

        // Listing an author's posts newest-first reads both columns.
        table.index(["author_id", "created_at"]);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::database::{Dialect, Migration};

    #[test]
    fn the_column_is_nullable_because_the_table_already_has_rows() {
        let up = migration().up(Dialect::Sqlite).join("\n");
        assert!(up.contains("\"excerpt\" varchar(500) NULL"), "{up}");
    }

    #[test]
    fn the_rollback_undoes_both_changes_in_reverse() {
        // Reverse order matters: an index over a column has to go before the
        // column does, or the drop fails on an engine that checks.
        let down = migration()
            .down(Dialect::Sqlite)
            .sql("0008_posts_add_excerpt")
            .expect("reversible")
            .join("\n");

        let index = down.find("posts_author_id_created_at_index").expect("drops the index");
        let column = down.find("DROP COLUMN").expect("drops the column");

        assert!(index < column, "{down}");
    }

    #[test]
    fn it_is_reversible_and_says_so() {
        for dialect in [Dialect::Sqlite, Dialect::MySql, Dialect::Postgres] {
            assert!(migration().down(dialect).is_reversible(), "{dialect:?}");
        }
    }
}
