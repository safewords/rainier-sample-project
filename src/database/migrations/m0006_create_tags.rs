//! `0006_create_tags` — the `tags` table, from the model's own metadata.

use rainier_framework::database::Step;

use crate::app::models::Tag;

/// Create `tags`.
///
/// `create_table` reads the columns, the primary key and the unique index off
/// `#[derive(Entity)]`, so the schema cannot drift from the struct: adding a
/// field to the model and forgetting the migration is a compile-time-visible
/// mismatch rather than a runtime "no such column".
pub fn migration() -> Step {
    Step::create_table::<Tag>("0006_create_tags")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::database::{Dialect, Migration};

    #[test]
    fn it_creates_the_table_the_model_declares() {
        let up = migration().up(Dialect::Sqlite).join("\n");

        assert!(up.contains("tags"), "{up}");
        assert!(up.to_uppercase().contains("UNIQUE"), "the name is unique: {up}");
    }
}
