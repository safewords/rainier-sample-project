//! `0003_index_posts_author` — an index, described once.

use rainier_framework::database::Step;

/// Index `posts.author_id`.
///
/// Nothing here is SQL. The three engines disagree about this exact statement
/// — SQLite and Postgres accept `CREATE INDEX IF NOT EXISTS`, MySQL rejects it
/// — and translating that is the builder's job, not yours.
///
/// The `down` is derived: an index that was created is an index that gets
/// dropped, and `DROP INDEX name` on SQLite and Postgres is
/// `DROP INDEX name ON table` on MySQL. Also not yours.
pub fn migration() -> Step {
    Step::table("0003_index_posts_author", "posts", |table| {
        table.index(["author_id"]);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::database::{Dialect, Migration};

    #[test]
    fn each_engine_gets_the_statement_it_accepts() {
        let sqlite = migration().up(Dialect::Sqlite).join("\n");
        let mysql = migration().up(Dialect::MySql).join("\n");

        assert!(sqlite.contains("IF NOT EXISTS"), "{sqlite}");
        assert!(!mysql.contains("IF NOT EXISTS"), "MySQL rejects it: {mysql}");
    }

    #[test]
    fn the_rollback_drops_what_the_migration_made() {
        // Derived from the change, so it cannot name a different index than
        // the one that was created — the mistake a hand-written `down` makes.
        for dialect in [Dialect::Sqlite, Dialect::MySql, Dialect::Postgres] {
            let down = migration()
                .down(dialect)
                .sql("0003_index_posts_author")
                .expect("reversible")
                .join("\n");

            assert!(down.contains("posts_author_id_index"), "{dialect:?}: {down}");
        }
    }

    #[test]
    fn mysql_needs_the_table_named_when_dropping_an_index() {
        let down = migration().down(Dialect::MySql).sql("x").expect("reversible").join("\n");

        assert!(down.contains("ON `posts`"), "{down}");
    }
}
