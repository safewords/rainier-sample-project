//! `routes/channels.php` — who may subscribe to what.
//!
//! A private channel is only private because something refuses the
//! subscription, and this is that something. Every pattern here is consulted by
//! `POST /broadcasting/auth`, which is what a browser calls before its
//! WebSocket server will let it listen.
//!
//! # It fails closed
//!
//! A channel with no matching pattern is **denied**. So the mistake this file
//! invites — a typo in a pattern, or forgetting to add one — makes a feature
//! stop working, rather than making a private channel readable by anyone who
//! guesses its name.
//!
//! # What does not belong here
//!
//! **Per-session channels.** Every rule in this file answers "may this *user*
//! subscribe", so a channel authorised here is one every device that user is
//! signed in on may join. For notifications that is right: an unread count is
//! a property of the account and identical on every device.
//!
//! For anything answering a request it is wrong — a reply built because a
//! phone asked would be delivered to the laptop as well. Those are named after
//! a key held in the session and authorised by
//! [`rainier_framework::broadcasting::authorize_session`], on its own route
//! with no guard. Adding a pattern here for one would hand it to every device
//! on the account, which is the thing that shape exists to avoid.

use rainier_framework::broadcast::{ChannelAccess, ChannelParams, ChannelRegistry};
use rainier_framework::broadcasting::authorize_notifications;
use rainier_framework::prelude::*;

use crate::app::models::User;
use crate::app::repositories::PostRepository;

/// Declare this application's channels.
pub fn channels() -> ChannelRegistry<User> {
    let mut channels = ChannelRegistry::new();

    // A user's own notifications — `private-notifications.User.7`. The
    // framework's rule, because the framework's channel publishes to it.
    authorize_notifications(&mut channels, "User", |user: &User| user.id.to_string());

    // The live view of one post. Only its author, because a draft is not
    // public — the same rule `PostPolicy` applies to the HTTP route, and the
    // duplication is the point: a WebSocket is a second way in.
    channels.channel("posts.{post}", |user: &User, params: &ChannelParams| {
        let user_id = user.id;
        let slug = params.get("post").unwrap_or_default().to_string();

        Box::pin(async move {
            let posts =
                rainier_framework::container::facade_application().resolve::<PostRepository>()?;

            // Absent is denied, not an error: a subscription to a post that no
            // longer exists is a stale browser tab, not a bug worth a 500.
            let Some(post) = posts.first_by("slug", slug.into()).await? else {
                return Ok(ChannelAccess::Denied);
            };

            Ok(ChannelAccess::allowed_if(post.belongs_to(user_id)))
        })
    });

    channels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_channel_this_application_publishes_to_has_a_rule() {
        // The assertion that notices a channel added to a publisher and not
        // here — which would be a feature that silently never connects.
        assert_eq!(channels().patterns(), vec!["notifications.{type}.{id}", "posts.{post}"]);
    }

    #[tokio::test]
    async fn an_undeclared_channel_is_denied() {
        let user = User::new("Ada", "ada@example.com", "hash".into());
        let secret = rainier_framework::broadcast::Channel::private("payroll.1");

        assert_eq!(channels().authorize(&user, &secret).await.unwrap(), ChannelAccess::Denied);
    }
}
