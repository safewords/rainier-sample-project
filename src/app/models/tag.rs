//! The `Tag` model — `app/Models/Tag.php`.

use rainier_framework::prelude::*;
use serde::Serialize;

/// A label a post can carry, and that many posts can share.
///
/// The other side of a [many-to-many](Tag::posts): tags do not belong to a
/// post, they are attached to any number of them through the `post_tag` pivot.
#[derive(Entity, Clone, Debug, PartialEq, Serialize)]
#[orm(table = "tags")]
pub struct Tag {
    /// The primary key.
    #[orm(pk, auto_increment)]
    pub id: u64,

    /// The label, lower-cased and unique.
    #[orm(unique)]
    pub name: String,
}

impl Model for Tag {
    /// `/tags/{tag}` binds by name.
    fn route_key_name() -> &'static str {
        "name"
    }
}

impl Tag {
    /// A new, unsaved tag. Names are normalised so `Rust` and `rust` are one.
    pub fn named(name: impl AsRef<str>) -> Self {
        Self { id: 0, name: name.as_ref().trim().to_lowercase() }
    }

    /// The posts carrying this tag — the inverse of
    /// [`Post::tags`](crate::app::models::Post::tags).
    ///
    /// The same pivot, read the other way round. Declaring both sides is not
    /// duplication: which is the "near" side decides what you can look up by,
    /// and an application usually needs both.
    pub fn posts() -> BelongsToMany<Tag, crate::app::models::Post> {
        BelongsToMany::new("post_tag")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_normalised_so_the_unique_index_means_something() {
        assert_eq!(Tag::named("  Rust ").name, "rust");
    }

    #[test]
    fn the_pivot_columns_follow_the_convention() {
        // `post_tag(post_id, tag_id)` — no configuration needed for the
        // conventional shape.
        assert_eq!(
            BelongsToMany::<crate::app::models::Post, Tag>::conventional_pivot(),
            "post_tag"
        );
    }
}
