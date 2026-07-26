//! `NotifyAuthor` — `app/Jobs/NotifyAuthor.php`.

use rainier_framework::prelude::*;
use serde::{Deserialize, Serialize};

use crate::app::notifications::PostLive;
use crate::app::repositories::{PostRepository, UserRepository};

/// Tells an author their post is live.
///
/// Queued by the listener for
/// [`PostPublished`](crate::app::models::PostPublished), and it sends a
/// [notification](crate::app::notifications) rather than an email: the job
/// decides *when*, the notification decides *how*. Adding a channel is then a
/// change to one `via()` and nothing here.
///
/// The payload is an **id**, not a snapshot of the row. A queued job may run
/// minutes later, by which time the post could have been edited or deleted —
/// so it re-reads and acts on what is true then.
#[derive(Debug, Serialize, Deserialize)]
pub struct NotifyAuthor {
    /// Which post went live.
    pub post_id: u64,
}

#[async_trait]
impl Job for NotifyAuthor {
    /// The name on the wire. It must stay stable once jobs of this type exist
    /// in a queue — renaming the struct is fine, renaming this strands them.
    const NAME: &'static str = "app.notify-author";

    /// A separate queue, so slow mail cannot delay time-sensitive work.
    const QUEUE: &'static str = "mail";

    const TRIES: u32 = 5;

    async fn handle(&self, context: &JobContext) -> Result<()> {
        // A job cannot capture its dependencies — it was serialised — so it
        // resolves them from the container instead.
        let posts = context.resolve::<PostRepository>()?;
        let users = context.resolve::<UserRepository>()?;
        let notifier = context.resolve::<Notifier>()?;

        // Gone between publishing and now. Not a failure: there is nothing to
        // notify about, and retrying would never succeed.
        let Some(post) = posts.find(self.post_id.into()).await? else {
            tracing::info!(post_id = self.post_id, "post is gone; nothing to notify about");
            return Ok(());
        };
        let Some(author) = users.find(post.author_id.into()).await? else {
            tracing::info!(post_id = self.post_id, "author is gone; nothing to notify about");
            return Ok(());
        };

        // The receipt says what actually happened on each channel. A user
        // with no email still gets the database row, and the send does not
        // fail — a missing address is a skip, not an error.
        let receipt = notifier.send(&author, &PostLive { post }).await?;
        tracing::info!(
            post_id = self.post_id,
            delivered = ?receipt.delivered(),
            "notified the author"
        );

        Ok(())
    }

    /// Called once the last attempt has failed. Record it somewhere a human
    /// will look, rather than losing it.
    async fn failed(&self, _context: &JobContext, error: &Error) {
        tracing::error!(post_id = self.post_id, error = %error, "gave up notifying the author");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::queue::QueuedJob;

    #[test]
    fn the_payload_is_an_id_not_a_snapshot() {
        let queued = QueuedJob::from_job(&NotifyAuthor { post_id: 7 }).unwrap();

        assert_eq!(queued.payload, serde_json::json!({ "post_id": 7 }));
        assert_eq!(queued.name, "app.notify-author");
        assert_eq!(queued.queue, "mail");
        assert_eq!(queued.max_attempts, 5);
    }
}
