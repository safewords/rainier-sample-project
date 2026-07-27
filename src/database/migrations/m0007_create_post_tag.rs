//! `0007_create_post_tag` — the pivot behind `Post::tags`, from a blueprint.

use rainier_framework::database::Step;

/// Create `post_tag`.
///
/// A table no model describes — a pivot is two foreign keys and nothing else —
/// so it is built rather than derived from an
/// [`Entity`](rainier_framework::database::Entity). What it is *not* is three
/// hand-written `CREATE TABLE` statements, one per engine.
///
/// Three things worth having, and none of them are automatic:
///
/// - a **composite primary key**, so the same tag cannot be attached twice —
///   otherwise a double-click puts two links in and the tag appears twice;
/// - **cascades**, so deleting a post takes its links rather than leaving rows
///   pointing at nothing;
/// - an **index on `tag_id`** — `foreign_id` adds one to each side, which
///   matters because the pivot is read from both directions and a composite
///   primary key only helps the direction that leads with `post_id`.
pub fn migration() -> Step {
    Step::create("0007_create_post_tag", "post_tag", |table| {
        table.foreign_id("post_id").constrained_on("posts").cascade_on_delete();
        table.foreign_id("tag_id").constrained_on("tags").cascade_on_delete();

        table.primary(["post_id", "tag_id"]);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::database::{Dialect, Migration};

    #[test]
    fn the_pair_is_the_key_on_every_engine() {
        // What stops the same tag being attached to the same post twice.
        for dialect in [Dialect::Sqlite, Dialect::MySql, Dialect::Postgres] {
            let up = migration().up(dialect).join("\n");

            assert_eq!(up.matches("PRIMARY KEY").count(), 1, "{dialect:?}: {up}");
            assert!(up.contains("post_id"), "{dialect:?}: {up}");
            assert!(up.contains("tag_id"), "{dialect:?}: {up}");
        }
    }

    #[test]
    fn deleting_a_post_takes_its_links() {
        let up = migration().up(Dialect::Sqlite).join("\n");
        assert_eq!(up.matches("ON DELETE CASCADE").count(), 2, "{up}");
    }

    #[test]
    fn both_directions_are_indexed() {
        // The pivot is read from the post's side *and* the tag's.
        let up = migration().up(Dialect::Sqlite).join("\n");

        assert!(up.contains("post_tag_post_id_index"), "{up}");
        assert!(up.contains("post_tag_tag_id_index"), "{up}");
    }

    #[test]
    fn the_rollback_drops_the_table() {
        let down = migration().down(Dialect::Sqlite).sql("x").expect("reversible").join("\n");
        assert_eq!(down, "DROP TABLE IF EXISTS \"post_tag\"");
    }
}
