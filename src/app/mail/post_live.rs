//! `PostLiveMail` — sent by the [`NotifyAuthor`](crate::app::jobs::NotifyAuthor) job.

use rainier_framework::mail::{Content, Envelope, Mailable};
use rainier_framework::prelude::*;

/// Tells an author their post is live.
pub struct PostLiveMail {
    /// Who it is for.
    pub name: String,
    /// Where it goes.
    pub email: String,
    /// The post's headline.
    pub title: String,
    /// The post's slug, for the link.
    pub slug: String,
}

impl Mailable for PostLiveMail {
    fn envelope(&self) -> Envelope {
        Envelope::new(format!("“{}” is live", self.title)).to(self.email.clone())
    }

    fn content(&self) -> Result<Content> {
        // A named route rather than a hard-coded path, so the URL can change
        // in one place.
        let url = Url::instance()
            .absolute("api.posts.show", &[("post", &self.slug)])
            .unwrap_or_else(|_| format!("/api/posts/{}", self.slug));

        Content::view(
            "mail.post_live",
            serde_json::json!({ "name": self.name, "title": self.title, "url": url }),
        )
    }

    fn headers(&self) -> Vec<(String, String)> {
        vec![("X-Campaign".into(), "post-live".into())]
    }
}
