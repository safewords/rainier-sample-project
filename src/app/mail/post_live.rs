//! `PostLiveMail` — the body of the [`PostLive`](crate::app::notifications::PostLive)
//! notification.
//!
//! Unlike [`WelcomeMail`](crate::app::mail::WelcomeMail) it does **not**
//! address itself. It is sent by a notification, and a notification's recipient
//! supplies the address — see
//! [`Notifiable::route_for`](rainier_framework::notify::Notifiable). A mailable
//! that hard-coded `to` here would take that choice away.

use rainier_framework::mail::{Content, Envelope, Mailable};
use rainier_framework::prelude::*;

/// Tells an author their post is live.
pub struct PostLiveMail {
    /// Who it is for, for the greeting — not for the envelope.
    pub name: String,
    /// The post's headline.
    pub title: String,
    /// The post's slug, for the link.
    pub slug: String,
}

impl Mailable for PostLiveMail {
    fn envelope(&self) -> Envelope {
        // No `to`: the notification's channel addresses it from the recipient.
        Envelope::new(format!("“{}” is live", self.title))
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
