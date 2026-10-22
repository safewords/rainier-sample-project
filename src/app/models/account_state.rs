//! What the account add-ins remember about a user, beside the `users` row.
//!
//! # Why tables of their own, and not columns on `users`
//!
//! `users` is created from the [`User`](super::User) model by `0001`, so a
//! field added there would change what an already-shipped migration does —
//! and an alter adding it as well would fail on a fresh database, which runs
//! both. `m0002_create_posts` sets that rule out. A table per concern, keyed
//! by the user, sidesteps it: each is a new migration creating a new table,
//! and a deployment that never turns the feature on carries an empty one.
//!
//! These models are compiled in every build. Only the code that *uses* them
//! sits behind a cargo feature, so turning an add-in on is never a schema
//! change.

use chrono::{DateTime, Utc};
use rainier_framework::prelude::*;
use serde::Serialize;

/// When a user proved they receive mail at their address.
///
/// A row exists only once they have. "Unverified" is the absence of one,
/// which is also what every user who registered before this table existed
/// reads as.
#[derive(Entity, Clone, Debug, PartialEq, Serialize)]
#[orm(table = "email_verifications")]
pub struct EmailVerification {
    /// The user — one row each.
    #[orm(pk, references = "users(id)", on_delete = "cascade")]
    pub user_id: u64,

    /// When the link was followed.
    pub verified_at: DateTime<Utc>,
}

impl Model for EmailVerification {}

/// A user's authenticator-app enrolment.
#[derive(Entity, Clone, Debug, PartialEq)]
#[orm(table = "two_factor_enrolments")]
pub struct TwoFactorEnrolment {
    /// The user — one enrolment each.
    #[orm(pk, references = "users(id)", on_delete = "cascade")]
    pub user_id: u64,

    /// The shared secret, **encrypted** with the application key. A database
    /// dump holding it in the clear would hold every user's second factor.
    pub secret: String,

    /// Whether the user has proven their app produces codes for it. Only a
    /// confirmed enrolment is challenged at login.
    pub confirmed: bool,

    /// Unspent recovery codes, as a JSON array of their hashes.
    pub recovery_codes: String,

    /// The last TOTP step accepted — what refuses the same code twice.
    pub last_used_step: Option<i64>,
}

impl Model for TwoFactorEnrolment {}

/// A passkey a user registered.
#[derive(Entity, Clone, Debug, PartialEq)]
#[orm(table = "passkeys")]
pub struct Passkey {
    /// The primary key.
    #[orm(pk, auto_increment)]
    pub id: u64,

    /// Whose it is.
    #[orm(index, references = "users(id)", on_delete = "cascade")]
    pub user_id: u64,

    /// The credential id, base64url — what a sign-in looks the row up by.
    #[orm(unique)]
    pub credential_id: String,

    /// What the user called it: "this laptop".
    pub name: String,

    /// The credential — public key, counter, flags — as JSON.
    pub credential: String,

    /// When it was registered.
    pub created_at: DateTime<Utc>,
}

impl Model for Passkey {}

/// An identity at another provider, linked to a user.
///
/// Unique on the pair: one GitHub account belongs to one user, which is what
/// makes "sign in with GitHub" mean anything.
#[derive(Entity, Clone, Debug, PartialEq)]
#[orm(table = "social_accounts")]
#[orm(unique = "provider, provider_id")]
pub struct SocialAccount {
    /// The primary key.
    #[orm(pk, auto_increment)]
    pub id: u64,

    /// The user it signs in as.
    #[orm(index, references = "users(id)", on_delete = "cascade")]
    pub user_id: u64,

    /// `github`, or the name an OpenID Connect issuer was declared under.
    pub provider: String,

    /// The provider's id for them — never their email, which can change hands.
    pub provider_id: String,

    /// When it was linked.
    pub created_at: DateTime<Utc>,
}

impl Model for SocialAccount {}

/// The Stripe customer behind a user.
#[derive(Entity, Clone, Debug, PartialEq)]
#[orm(table = "billing_customers")]
pub struct BillingCustomer {
    /// The user — one customer each.
    #[orm(pk, references = "users(id)", on_delete = "cascade")]
    pub user_id: u64,

    /// Stripe's id: `cus_…`.
    #[orm(unique)]
    pub stripe_customer_id: String,
}

impl Model for BillingCustomer {}

/// The application's copy of a Stripe subscription, kept current by webhooks.
///
/// A copy because the alternative is asking Stripe on every request whether
/// somebody may see a page — slow, rate-limited, and down whenever Stripe is.
#[derive(Entity, Clone, Debug, PartialEq)]
#[orm(table = "subscriptions")]
pub struct Subscription {
    /// Stripe's id: `sub_…`.
    #[orm(pk)]
    pub id: String,

    /// The customer it bills: `cus_…`.
    #[orm(index)]
    pub stripe_customer_id: String,

    /// Stripe's status string, for reading the table by eye. Decisions are
    /// made from [`snapshot`](Self::snapshot).
    pub status: String,

    /// The whole snapshot, as JSON.
    pub snapshot: String,

    /// When a webhook last wrote it.
    pub updated_at: DateTime<Utc>,
}

impl Model for Subscription {}
