//! `PostLive` — `app/Notifications/PostLive.php`.

use rainier_framework::notifications::DatabaseChannel;
use rainier_framework::notify::MailChannel;
use rainier_framework::prelude::*;

use crate::app::mail::PostLiveMail;
use crate::app::models::{Post, User};

/// Tells an author their post is live.
///
/// Sent by the [`NotifyAuthor`](crate::app::jobs::NotifyAuthor) job, which is
/// queued by the listener for
/// [`PostPublished`](crate::app::models::PostPublished).
pub struct PostLive {
    /// The post that went live.
    pub post: Post,
}

impl Notification<User> for PostLive {
    /// Permanent once rows exist — the database channel stores it.
    fn notification_name(&self) -> &'static str {
        "post.live"
    }

    /// The channels, chosen **per recipient**.
    ///
    /// Selected by type, so deleting a channel is a compile error rather than
    /// a notification that quietly goes nowhere. The database channel is
    /// unconditional because it needs no address; a real application would ask
    /// the user's preferences here before adding mail.
    fn via(&self, _: &User) -> Channels {
        Channels::new().with::<DatabaseChannel>().with::<MailChannel>()
    }

    /// The email body — the mailable, reused.
    ///
    /// Note what it does *not* set: the address. The notification says what to
    /// say; [`Notifiable::route_for`](rainier_framework::notify::Notifiable)
    /// says where it goes. That split is what lets the same notification reach
    /// a user by email today and by Slack tomorrow.
    fn to_mail(&self, to: &User) -> Option<Box<dyn Mailable>> {
        Some(Box::new(PostLiveMail {
            name: to.name.clone(),
            title: self.post.title.clone(),
            slug: self.post.slug.clone(),
        }))
    }

    /// One line, for anything that only has one — the log channel, and an SMS
    /// channel if this application grew one.
    fn to_text(&self, _: &User) -> Option<String> {
        Some(format!("“{}” is now live.", self.post.title))
    }

    /// The payload the database channel stores, for the in-app list.
    ///
    /// Keep it small and stable: these rows outlive the deploy that wrote
    /// them, so a field removed here is a field missing from history.
    fn to_data(&self, _: &User) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "post_id": self.post.id,
            "slug": self.post.slug,
            "title": self.post.title,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::notify::Delivery;

    fn author() -> User {
        User::new("Ada", "ada@example.com", "hash".into())
    }

    fn post() -> Post {
        Post::draft("Hello, World!", "body", 1)
    }

    #[test]
    fn it_goes_out_on_both_channels() {
        assert_eq!(
            PostLive { post: post() }.via(&author()).names(),
            vec!["DatabaseChannel", "MailChannel"]
        );
    }

    #[test]
    fn the_email_is_addressed_by_the_recipient_not_the_notification() {
        // The property that makes this a notification rather than a mailable:
        // the body knows nothing about where it is going.
        let notification = PostLive { post: post() };
        let mailable = notification.to_mail(&author()).expect("it renders an email");

        assert!(mailable.envelope().to.is_empty(), "the channel fills this in");
        assert_eq!(author().route_for("mail").as_deref(), Some("ada@example.com"));
    }

    #[test]
    fn every_representation_is_rendered_once_per_send() {
        // All three forms come from one `Delivery`, so a notification that
        // implements all of them does not pay for them per channel.
        let delivery = Delivery::render(&PostLive { post: post() }, &author());

        assert!(delivery.mail().is_some());
        assert_eq!(delivery.text(), Some("“Hello, World!” is now live."));
        assert_eq!(delivery.data().unwrap()["slug"], "hello-world");
    }
}
