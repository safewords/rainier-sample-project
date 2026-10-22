//! `PostAnnounced` — a published post, posted to the community's Discord.

use rainier_framework::notify::{
    Channels, DiscordChannel, DiscordEmbed, DiscordMessage, Notifiable, Notification,
};

use crate::app::models::Post;

/// The community's Discord channel, as something to notify.
///
/// Not a user: a notification's recipient is anything with a route per
/// channel, and this one has exactly one — the webhook `DISCORD_WEBHOOK_URL`
/// names.
pub struct Community {
    /// The webhook URL.
    pub webhook: String,
}

impl Notifiable for Community {
    fn notifiable_id(&self) -> String {
        "community".into()
    }

    fn notifiable_type(&self) -> &'static str {
        "Community"
    }

    fn route_for(&self, channel: &str) -> Option<String> {
        (channel == "discord").then(|| self.webhook.clone())
    }
}

/// "A new post is live."
pub struct PostAnnounced {
    /// The post.
    pub post: Post,
    /// Where it can be read.
    pub url: String,
}

impl Notification<Community> for PostAnnounced {
    fn notification_name(&self) -> &'static str {
        "post.announced"
    }

    fn via(&self, _: &Community) -> Channels {
        Channels::new().with::<DiscordChannel>()
    }

    fn to_text(&self, _: &Community) -> Option<String> {
        Some(format!("New post: {}", self.post.title))
    }

    /// An embed with the title as its link.
    ///
    /// The title is the author's text, interpolated into a message the whole
    /// server sees — and the channel sends with mentions **off**, so a post
    /// titled "@everyone" pings nobody.
    fn to_data(&self, _: &Community) -> Option<serde_json::Value> {
        Some(
            DiscordMessage::new()
                .content("A new post is live")
                .embed(
                    DiscordEmbed::new()
                        .title(self.post.title.clone())
                        .url(self.url.clone())
                        .color(0x2E7D32),
                )
                .into_data(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_goes_to_discord_and_nowhere_else() {
        let community = Community { webhook: "https://discord.com/api/webhooks/1/x".into() };
        let notification = PostAnnounced {
            post: Post::draft("Hello", "body", 1),
            url: "https://app.test/p".into(),
        };

        assert_eq!(notification.via(&community).names(), vec!["DiscordChannel"]);
        assert_eq!(community.route_for("mail"), None);
    }
}
