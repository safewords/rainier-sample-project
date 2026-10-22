//! What the OAuth server stores: clients, and the tokens and codes it issued.
//!
//! Four tables, compiled into every build for the reason
//! [`account_state`](super::account_state) gives. With the `oauth-server`
//! feature on, `app/repositories/oauth_store.rs` implements the server's store
//! port over these models.
//!
//! List-valued fields — redirect URIs, grants, scopes — are JSON arrays in a
//! text column. They are read whole and never queried into, so a join table
//! each would be three more tables for nothing.

use chrono::{DateTime, Utc};
use rainier_framework::prelude::*;

/// An application allowed to ask for tokens.
#[derive(Entity, Clone, Debug, PartialEq)]
#[orm(table = "oauth_clients")]
pub struct OAuthClient {
    /// What it sends as `client_id`.
    #[orm(pk)]
    pub id: String,

    /// Shown on the consent screen.
    pub name: String,

    /// The **hash** of its secret, or `None` for a public client — a phone or
    /// browser app, which cannot keep one.
    pub secret_hash: Option<String>,

    /// Where an authorization may send the browser back to, as JSON.
    pub redirect_uris: String,

    /// The grants it may use, as JSON.
    pub grant_types: String,

    /// The scopes it may be granted, as JSON, or `None` for any.
    pub scopes: Option<String>,

    /// The user who registered it; `None` is first party.
    pub owner_id: Option<String>,

    /// Whether personal access tokens are issued through it.
    pub personal_access: bool,

    /// A revoked client is treated as unknown.
    pub revoked: bool,

    /// When it was registered.
    pub created_at: DateTime<Utc>,
}

impl Model for OAuthClient {}

/// An access token's row. The token is a signed JWT; this row is what lets it
/// be revoked before it expires.
#[derive(Entity, Clone, Debug, PartialEq)]
#[orm(table = "oauth_access_tokens")]
#[orm(index = "user_id, client_id")]
pub struct OAuthAccessToken {
    /// The JWT's `jti`.
    #[orm(pk)]
    pub id: String,

    /// The user it acts for, or `None` for a client-credentials token.
    pub user_id: Option<String>,

    /// The client it was issued to.
    pub client_id: String,

    /// A name, for a personal access token.
    pub name: Option<String>,

    /// The scopes it carries, as JSON.
    pub scopes: String,

    /// Whether it has been revoked.
    pub revoked: bool,

    /// When it was issued.
    pub created_at: DateTime<Utc>,

    /// When it expires.
    pub expires_at: DateTime<Utc>,
}

impl Model for OAuthAccessToken {}

/// A refresh token's row. The token itself is sealed and held by the client.
#[derive(Entity, Clone, Debug, PartialEq)]
#[orm(table = "oauth_refresh_tokens")]
pub struct OAuthRefreshToken {
    /// Its id.
    #[orm(pk)]
    pub id: String,

    /// The access token it was issued beside.
    #[orm(index)]
    pub access_token_id: String,

    /// Revoked, or spent: a refresh token works once.
    pub revoked: bool,

    /// When it expires.
    pub expires_at: DateTime<Utc>,
}

impl Model for OAuthRefreshToken {}

/// An authorization code's row — what makes a code single-use.
#[derive(Entity, Clone, Debug, PartialEq)]
#[orm(table = "oauth_auth_codes")]
pub struct OAuthAuthCode {
    /// Its id.
    #[orm(pk)]
    pub id: String,

    /// The user who approved it.
    pub user_id: String,

    /// The client it was issued to.
    pub client_id: String,

    /// The scopes approved, as JSON.
    pub scopes: String,

    /// Revoked, or spent.
    pub revoked: bool,

    /// When it expires.
    pub expires_at: DateTime<Utc>,
}

impl Model for OAuthAuthCode {}
