//! `0005_normalise_emails` — a data migration that cannot be undone.

use rainier_framework::database::Step;

/// Lower-case every stored address.
///
/// A data migration rather than a schema one, and the example of the case
/// `Down::irreversible` exists for: the original casing is not stored anywhere
/// after this runs, so there is nothing a `down` could restore.
///
/// The alternatives are both worse:
///
/// | | |
/// |---|---|
/// | an empty `down` | a rollback reports success and changes nothing |
/// | `UPDATE users SET email = …` | there is no expression that undoes `lower` |
///
/// Saying so makes `migrate:rollback` refuse the **whole batch** this is in,
/// before running anything, with the reason in the message. Which is right: a
/// batch that half-comes-off leaves a schema no migration describes.
///
/// `migrate` prints it too, at the moment it applies — so the constraint is
/// known at deploy time rather than discovered at rollback time.
pub fn migration() -> Step {
    Step::raw_irreversible(
        "0005_normalise_emails",
        vec![
            // `lower` is in every backend Rainier speaks, which is why this can
            // be `raw_irreversible` rather than a per-dialect `step`.
            "UPDATE users SET email = lower(email) WHERE email <> lower(email)".into(),
        ],
        "the original casing is not recorded anywhere, so there is nothing to restore",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::database::{Dialect, Migration};

    #[test]
    fn it_only_touches_the_rows_that_need_it() {
        // The `WHERE` is not decoration: without it this rewrites every row in
        // the table and every one of them lands in the write-ahead log.
        let up = migration().up(Dialect::Sqlite).join("\n");

        assert!(up.contains("lower(email)"), "{up}");
        assert!(up.contains("WHERE"), "{up}");
    }

    #[test]
    fn it_refuses_to_be_rolled_back_and_says_why() {
        let down = migration().down(Dialect::Sqlite);
        assert!(!down.is_reversible());

        let err = down.sql("0005_normalise_emails").unwrap_err();
        assert!(err.message().contains("0005_normalise_emails"), "{}", err.message());
        assert!(err.message().contains("nothing to restore"), "{}", err.message());
    }
}
