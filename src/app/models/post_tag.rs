//! The `PostTag` pivot — a table keyed on two columns.

use rainier_framework::prelude::*;
use serde::Serialize;

/// One link between a post and a tag.
///
/// # A pivot is a table with no single key, and that used to mean no model
///
/// `post_tag` is `(post_id, tag_id)` and nothing else: neither column is
/// unique, only the pair is. That is why `m0007_create_post_tag` builds it from
/// a blueprint rather than from an entity, and why this file's docblock used to
/// live there as "a table no model describes".
///
/// It describes one now. Two `#[orm(pk)]` columns and the derive generates the
/// composite `PRIMARY KEY (post_id, tag_id)`, a `WHERE post_id = ? AND tag_id =
/// ?` for every keyed operation, and a refusal for a partial key — naming one
/// column of a two-column key is an error rather than a statement that matches
/// every row sharing that half.
///
/// # Neither column auto-increments, and that is not an oversight
///
/// Both values arrive from somewhere else: they are the keys of rows that
/// already exist. There is nothing for the database to mint. A pivot is the
/// clearest case of a key an application supplies rather than receives.
///
/// # What having a model buys, given [`BelongsToMany`] already reads the pivot
///
/// Loading a post's tags does not need this — `Post::tags()` reads the pivot
/// directly and always could. What needs it is asking the pivot a question
/// *about itself*: how many posts carry each tag, which is one `GROUP BY` over
/// this table and the only cheap way to build a tag cloud.
///
/// See
/// [`TagRepository::cloud`](crate::app::repositories::TagRepository::cloud).
/// Without an entity here that count is raw SQL, or every link loaded into the
/// process and tallied there — which is the same table scan, moved somewhere it
/// cannot be indexed.
#[derive(Entity, Clone, Debug, PartialEq, Serialize)]
#[orm(table = "post_tag")]
pub struct PostTag {
    /// The post half of the key.
    #[orm(pk)]
    pub post_id: u64,

    /// The tag half of the key.
    #[orm(pk)]
    pub tag_id: u64,
}

impl PostTag {
    /// A link between a post and a tag.
    pub fn link(post_id: u64, tag_id: u64) -> Self {
        Self { post_id, tag_id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::database::Dialect;

    #[test]
    fn the_pair_is_the_key_and_neither_column_is() {
        assert_eq!(PostTag::primary_key_columns(), ["post_id", "tag_id"]);
    }

    #[test]
    fn the_entitys_own_ddl_matches_the_migration_that_built_the_table() {
        // The two have to agree or the model describes a table nobody created.
        // `m0007_create_post_tag` writes one `PRIMARY KEY` over both columns;
        // so must this.
        let ddl =
            rainier_framework::database::schema::schema_ddl::<PostTag>(Dialect::Sqlite).join("\n");

        assert_eq!(ddl.matches("PRIMARY KEY").count(), 1, "{ddl}");
        assert!(ddl.contains("post_id"), "{ddl}");
        assert!(ddl.contains("tag_id"), "{ddl}");
        assert!(!ddl.contains("AUTOINCREMENT"), "a pivot's keys come from its neighbours: {ddl}");
    }
}
