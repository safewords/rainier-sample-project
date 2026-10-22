//! `SubscriptionRepository` — the local copy of each Stripe subscription.

use std::ops::Deref;

use chrono::Utc;
use rainier_framework::billing::SubscriptionSnapshot;
use rainier_framework::database::Repository;
use rainier_framework::prelude::*;

use crate::app::models::Subscription;

/// Access to `subscriptions`.
pub struct SubscriptionRepository {
    inner: EntityRepository<Subscription>,
}

impl SubscriptionRepository {
    /// A repository over `db`.
    pub fn new(db: Database) -> Self {
        Self { inner: EntityRepository::new(db) }
    }

    /// Write what a webhook said, replacing what was there.
    ///
    /// An upsert keyed by Stripe's id, so a redelivered or out-of-order event
    /// lands on the same row instead of beside it.
    pub async fn record(&self, snapshot: &SubscriptionSnapshot) -> Result<()> {
        let row = Subscription {
            id: snapshot.id.clone(),
            stripe_customer_id: snapshot.customer.clone().unwrap_or_default(),
            status: format!("{:?}", snapshot.status).to_lowercase(),
            snapshot: serde_json::to_string(snapshot)?,
            updated_at: Utc::now(),
        };
        self.inner
            .upsert(&row, &["id"], &["stripe_customer_id", "status", "snapshot", "updated_at"])
            .await?;
        Ok(())
    }

    /// Every subscription a customer holds, as snapshots.
    pub async fn for_customer(&self, customer_id: &str) -> Result<Vec<SubscriptionSnapshot>> {
        self.inner
            .find_by("stripe_customer_id", customer_id.into())
            .await?
            .iter()
            .map(|row| Ok(serde_json::from_str(&row.snapshot)?))
            .collect()
    }

    /// Whether the customer holds any subscription that grants access now —
    /// active, trialing, or cancelled but paid up to the end of the period.
    pub async fn is_subscribed(&self, customer_id: &str) -> Result<bool> {
        let now = Utc::now();
        Ok(self.for_customer(customer_id).await?.iter().any(|snapshot| snapshot.valid(now)))
    }
}

impl Deref for SubscriptionRepository {
    type Target = EntityRepository<Subscription>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
