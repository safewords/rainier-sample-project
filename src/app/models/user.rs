//! The `User` model — `app/Models/User.php`.

use chrono::{DateTime, Utc};
use rainier_framework::auth::Authenticatable;
use rainier_framework::prelude::*;
use serde::Serialize;

/// Someone who can log in.
#[derive(Entity, Clone, Debug, PartialEq, Serialize)]
#[orm(table = "users")]
pub struct User {
    /// The primary key.
    #[orm(pk, auto_increment)]
    pub id: u64,

    /// Their display name.
    pub name: String,

    /// Their address, and their login.
    #[orm(unique)]
    pub email: String,

    /// The Argon2 hash of their password.
    ///
    /// `#[serde(skip)]` is doing real work: a `User` is returned straight from
    /// controllers, and without it the hash would be in the JSON.
    #[serde(skip)]
    pub password: String,

    /// Their API token, once they log in.
    #[serde(skip)]
    #[orm(index)]
    pub api_token: Option<String>,

    /// When the row was created.
    pub created_at: DateTime<Utc>,
}

impl Model for User {}

/// What the framework needs to know about a user to authenticate one.
impl Authenticatable for User {
    fn auth_identifier(&self) -> String {
        self.id.to_string()
    }

    fn auth_password_hash(&self) -> Option<&str> {
        Some(&self.password)
    }
}

/// What the framework needs to know to send a user a notification.
///
/// The half an [event](crate::app::models::PostPublished) does not have: an
/// identity, and an address per channel. Returning `None` **skips** that
/// channel rather than failing the send, so a user with no phone number still
/// gets the email.
impl Notifiable for User {
    fn notifiable_id(&self) -> String {
        self.id.to_string()
    }

    /// Stored beside the id, because id `7` is only unique within a type.
    fn notifiable_type(&self) -> &'static str {
        "User"
    }

    fn route_for(&self, channel: &str) -> Option<String> {
        match channel {
            "mail" => Some(self.email.clone()),
            // No phone column yet. Adding one is all an SMS channel needs from
            // this side.
            _ => None,
        }
    }
}

impl User {
    /// A new, unsaved user. `password` must already be hashed — see
    /// [`crate::app::providers::register_user`].
    pub fn new(name: impl Into<String>, email: impl Into<String>, password: String) -> Self {
        Self {
            id: 0,
            name: name.into(),
            email: email.into(),
            password,
            api_token: None,
            created_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_never_serialised() {
        let user = User::new("Ada", "ada@example.com", "$argon2id$secret".into());
        let json = serde_json::to_string(&user).unwrap();

        assert!(json.contains("ada@example.com"));
        assert!(!json.contains("argon2"), "{json}");
    }
}
