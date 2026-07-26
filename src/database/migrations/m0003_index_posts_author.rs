//! `0003_index_posts_author` — raw SQL, and the SQL that undoes it.

use rainier_framework::database::Step;

/// Index `posts.author_id`.
///
/// `raw` runs the same statements on every backend, which is right when the SQL
/// genuinely is the same. You write both directions; there is no `down` to
/// forget, because the contract has no default for it.
///
/// Note `IF NOT EXISTS` on the way up and `IF EXISTS` on the way down. Neither
/// is required — a migration runs at most once — but both make the step safe to
/// re-run by hand against a database somebody has already touched, which is the
/// situation you are in when you are reaching for this at all.
pub fn migration() -> Step {
    Step::raw(
        "0003_index_posts_author",
        vec!["CREATE INDEX IF NOT EXISTS idx_posts_author ON posts (author_id)".into()],
        vec!["DROP INDEX IF EXISTS idx_posts_author".into()],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::database::{Dialect, Down, Migration};

    #[test]
    fn the_two_directions_name_the_same_index() {
        // The mistake this catches: renaming the index in `up` and not in
        // `down`, which leaves a rollback silently dropping nothing.
        let up = migration().up(Dialect::Sqlite).join("\n");
        let Down::Statements(down) = migration().down(Dialect::Sqlite) else {
            panic!("an index is reversible");
        };

        assert!(up.contains("idx_posts_author"), "{up}");
        assert!(down.join("\n").contains("idx_posts_author"), "{down:?}");
    }

    #[test]
    fn it_runs_the_same_sql_on_every_backend() {
        for dialect in [Dialect::Sqlite, Dialect::Postgres, Dialect::MySql] {
            assert_eq!(migration().up(dialect), migration().up(Dialect::Sqlite));
        }
    }
}
