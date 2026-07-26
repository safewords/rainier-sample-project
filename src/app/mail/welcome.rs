//! `WelcomeMail` — `app/Mail/WelcomeMail.php`.

use rainier_framework::mail::{Content, Envelope, Mailable};
use rainier_framework::prelude::*;

/// The email a new user receives.
///
/// A mailable *describes* the message and does no I/O, so what matters about
/// it — does it address the right person, does the template render, does the
/// subject read well — is testable without a mail server.
pub struct WelcomeMail {
    /// Who it is for.
    pub name: String,
    /// Where it goes.
    pub email: String,
}

impl Mailable for WelcomeMail {
    fn envelope(&self) -> Envelope {
        // No `from`: the mailer fills in `mail.from` for us.
        Envelope::new(format!("Welcome, {}", self.name)).to(self.email.clone())
    }

    fn content(&self) -> Result<Content> {
        Content::view("mail.welcome", serde_json::json!({ "name": self.name }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::view::MemoryEngine;

    #[test]
    fn it_renders_and_addresses_correctly() {
        let views = MemoryEngine::new().with("mail.welcome", "<p>Hi {{ name }}!</p>");
        let message = WelcomeMail { name: "Ada".into(), email: "ada@example.com".into() }
            .build(&views)
            .unwrap();

        assert_eq!(message.envelope.subject, "Welcome, Ada");
        assert_eq!(message.envelope.to[0].email, "ada@example.com");
        assert_eq!(message.html.as_deref(), Some("<p>Hi Ada!</p>"));
    }

    #[test]
    fn view_data_is_escaped() {
        let views = MemoryEngine::new().with("mail.welcome", "<p>Hi {{ name }}!</p>");
        let message =
            WelcomeMail { name: "<script>alert(1)</script>".into(), email: "a@b.co".into() }
                .build(&views)
                .unwrap();

        assert!(!message.html.unwrap().contains("<script>"));
    }
}
