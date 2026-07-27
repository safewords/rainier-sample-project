//! `0004_add_post_search` — the exception: SQL that genuinely differs per
//! backend.

use rainier_framework::database::{Dialect, Down, Step};

/// Full-text search over posts, however this backend spells it.
///
/// **The last resort, and the only migration here that writes SQL.** Every
/// other one in this directory describes what it wants and lets the builder
/// render it — see `m0003` for an index, `m0007` for a table, `m0008` for an
/// alter. If you are writing SQL, check first that
/// [`Step::create`](rainier_framework::database::Step::create) or
/// [`Step::table`](rainier_framework::database::Step::table) cannot say it.
///
/// This one they cannot. Full-text search is not one feature spelled three
/// ways — it is three different features: Postgres wants a GIN index over a
/// `tsvector`, SQLite wants an FTS5 **virtual table**, and MySQL wants a
/// `FULLTEXT` index. There is no portable form to translate to, so the
/// migration takes a closure per direction and answers per dialect.
///
/// Both matches are **exhaustive** — no `_` arm. That is deliberate: a dialect
/// added to the framework should make this a compile error here, because a
/// backend silently getting no search index is the kind of thing nobody
/// notices until a query is slow in production.
///
/// Returning an empty vector *is* a legal no-op, for a backend that genuinely
/// needs nothing. Note that the empty `down` for such an arm would be
/// `Down::statements([])`, not `Down::irreversible`: "nothing to undo" and
/// "cannot be undone" are different answers, and a rollback treats them
/// differently — the first succeeds, the second refuses.
pub fn migration() -> Step {
    Step::new("0004_add_post_search", up, down)
}

fn up(dialect: Dialect) -> Vec<String> {
    match dialect {
        Dialect::Postgres => vec!["CREATE INDEX IF NOT EXISTS posts_search \
             ON posts USING gin (to_tsvector('english', title || ' ' || body))"
            .into()],

        // A virtual table rather than an index, so the `down` is a DROP TABLE.
        Dialect::Sqlite => {
            vec!["CREATE VIRTUAL TABLE IF NOT EXISTS posts_fts USING fts5(title, body)".into()]
        }

        Dialect::MySql => {
            vec!["CREATE FULLTEXT INDEX posts_search ON posts (title, body)".into()]
        }
    }
}

fn down(dialect: Dialect) -> Down {
    match dialect {
        Dialect::Postgres => Down::statements(["DROP INDEX IF EXISTS posts_search".to_string()]),
        Dialect::Sqlite => Down::statements(["DROP TABLE IF EXISTS posts_fts".to_string()]),
        Dialect::MySql => Down::statements(["DROP INDEX posts_search ON posts".to_string()]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::database::Migration;

    const ALL: [Dialect; 3] = [Dialect::Postgres, Dialect::Sqlite, Dialect::MySql];

    #[test]
    fn each_backend_gets_its_own_spelling() {
        assert!(migration().up(Dialect::Postgres)[0].contains("gin"));
        assert!(migration().up(Dialect::Sqlite)[0].contains("fts5"));
        assert!(migration().up(Dialect::MySql)[0].contains("FULLTEXT"));
    }

    #[test]
    fn every_backend_that_creates_something_can_drop_it() {
        // The mistake this catches: adding an `up` arm for a new dialect and
        // leaving `down` returning nothing, so a rollback leaves the index
        // behind and the *next* `up` fails on the duplicate.
        for dialect in ALL {
            let creates = !migration().up(dialect).is_empty();
            let drops = !migration()
                .down(dialect)
                .sql("0004_add_post_search")
                .expect("reversible")
                .is_empty();

            assert_eq!(creates, drops, "{dialect:?} should undo exactly what it did");
        }
    }

    #[test]
    fn the_two_directions_name_the_same_object() {
        for dialect in ALL {
            let up = migration().up(dialect).join("\n");
            let down = migration()
                .down(dialect)
                .sql("0004_add_post_search")
                .expect("reversible")
                .join("\n");

            let object = if dialect == Dialect::Sqlite { "posts_fts" } else { "posts_search" };
            assert!(up.contains(object), "{dialect:?} up: {up}");
            assert!(down.contains(object), "{dialect:?} down: {down}");
        }
    }

    #[test]
    fn it_is_reversible_on_every_backend() {
        for dialect in ALL {
            assert!(migration().down(dialect).is_reversible(), "{dialect:?}");
        }
    }
}
