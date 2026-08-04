//! `TagRepository` — the queries this application asks about tags.

use std::ops::Deref;

use rainier_framework::database::{statement, Subquery};
use rainier_framework::prelude::*;
use rainier_orm::Row as _;

use crate::app::models::{PostTag, Tag};

/// How many published posts carry a tag.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TagCount {
    /// The tag's key.
    pub tag_id: u64,
    /// How many published, un-binned posts carry it.
    pub posts: u64,
}

/// Access to tags, and to the pivot that attaches them.
pub struct TagRepository {
    inner: EntityRepository<Tag>,
    links: EntityRepository<PostTag>,
}

impl TagRepository {
    /// A repository over `db`.
    pub fn new(db: Database) -> Self {
        Self {
            inner: EntityRepository::<Tag>::new(db.clone()),
            links: EntityRepository::<PostTag>::new(db),
        }
    }

    /// Attach a tag to a post, or do nothing if it is already attached.
    ///
    /// An upsert whose conflict target is the **pair**, which is what the
    /// pivot's composite primary key constrains. A plain insert would be a
    /// constraint violation the second time — and the second time is a
    /// double-click, or a retried request, or an author re-saving a post
    /// without changing its tags. None of those is an error the person made.
    ///
    /// The update list is empty, which renders as insert-or-ignore: there is no
    /// third column on this table to bring up to date. A link either exists or
    /// does not.
    pub async fn attach(&self, post_id: u64, tag_id: u64) -> Result<()> {
        let db = self.links.database();

        // The statement layer rather than `Repository::upsert`, and that is the
        // composite key showing through: `Repository` is bounded on `Model`,
        // which is `Entity + SingleKey`, because most of its methods name a row
        // by *the* key. A pivot has two, so its route to a statement is the
        // layer below — which is `Entity`-bound and needs no such assumption.
        let prepared = statement::upsert::<PostTag>(
            db.dialect(),
            &PostTag::link(post_id, tag_id),
            &["post_id", "tag_id"],
            &[],
        );

        db.execute(prepared).await?;
        Ok(())
    }

    /// How many published posts carry each tag — the tag cloud.
    ///
    /// One `GROUP BY` over the pivot, and it is reachable only because
    /// [`PostTag`] is an entity: `aggregate_rows` is on the `Entity`-bound
    /// repository rather than the `Model`-bound trait, so a table keyed on two
    /// columns can still be counted. The alternative for a composite-key table
    /// is raw SQL, or loading every link into the process and tallying it
    /// there — which is the same table scan moved somewhere no index reaches.
    ///
    /// The `EXISTS` is doing real work and is not a scope in disguise. A
    /// subquery names a **table**, not an entity, so nothing appends
    /// `deleted_at IS NULL` to it for us — the pivot has no such column and
    /// `posts` is only reached through a raw table name here. Both halves are
    /// therefore written out: without `published` the cloud counts drafts, and
    /// without `deleted_at IS NULL` it counts what is in the bin, so a tag can
    /// show a count and lead to an empty page.
    pub async fn cloud(&self) -> Result<Vec<TagCount>> {
        let rows = self
            .links
            .aggregate_rows(
                Criteria::new()
                    .select(Projection::Column("tag_id".into()), "tag_id")
                    .select(Projection::CountAll, "posts")
                    .where_exists(
                        Subquery::count("posts")
                            .correlate("id", "post_id")
                            .where_eq("published", true)
                            .where_null("deleted_at"),
                    )
                    .group_by(Projection::Column("tag_id".into()))
                    .order_by_alias("posts", true),
            )
            .await?;

        rows.iter()
            .map(|row| {
                Ok(TagCount {
                    tag_id: row.get_i64("tag_id")?.unwrap_or_default() as u64,
                    posts: row.get_i64("posts")?.unwrap_or_default() as u64,
                })
            })
            .collect()
    }
}

impl Deref for TagRepository {
    type Target = EntityRepository<Tag>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
