//! `0007_create_post_tag` — the pivot behind `Post::tags`.

use rainier_framework::database::Step;

/// Create `post_tag`.
///
/// Written as raw SQL rather than as an entity, because a pivot is two foreign
/// keys and no model — nothing reads a row of it on its own, and
/// [`BelongsToMany`](rainier_framework::database::BelongsToMany) fetches it as
/// two columns.
///
/// Three things worth having, none of which come for free:
///
/// - a **composite primary key**, so the same tag cannot be attached twice —
///   otherwise a double-click puts two links in and the tag appears twice;
/// - `ON DELETE CASCADE`, so deleting a post takes its links with it rather
///   than leaving rows pointing at nothing;
/// - an index on `tag_id`, because the pivot is read from **both** directions
///   and the primary key only helps the one that leads with `post_id`.
pub fn migration() -> Step {
    Step::raw(
        "0007_create_post_tag",
        vec![
            "CREATE TABLE IF NOT EXISTS post_tag (\
                 post_id BIGINT NOT NULL, \
                 tag_id BIGINT NOT NULL, \
                 PRIMARY KEY (post_id, tag_id), \
                 FOREIGN KEY (post_id) REFERENCES posts(id) ON DELETE CASCADE, \
                 FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE\
             )"
            .into(),
            "CREATE INDEX IF NOT EXISTS idx_post_tag_tag ON post_tag (tag_id)".into(),
        ],
        vec![
            "DROP INDEX IF EXISTS idx_post_tag_tag".into(),
            "DROP TABLE IF EXISTS post_tag".into(),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::database::{Dialect, Down, Migration};

    #[test]
    fn the_pair_is_the_primary_key() {
        // What stops the same tag being attached to the same post twice.
        let up = migration().up(Dialect::Sqlite).join("\n");
        assert!(up.contains("PRIMARY KEY (post_id, tag_id)"), "{up}");
    }

    #[test]
    fn the_index_is_dropped_before_the_table_it_is_on() {
        let Down::Statements(down) = migration().down(Dialect::Sqlite) else {
            panic!("a pivot is reversible");
        };

        let index = down.iter().position(|s| s.contains("INDEX")).expect("drops the index");
        let table = down.iter().position(|s| s.contains("TABLE")).expect("drops the table");
        assert!(index < table, "{down:?}");
    }
}
