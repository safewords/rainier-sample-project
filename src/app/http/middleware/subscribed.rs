//! `EnsureSubscribed` — a route only paying members reach.

use std::sync::Arc;

use rainier_framework::auth::AuthenticatedUser;
use rainier_framework::http::{Request, Response, StatusCode};
use rainier_framework::middleware::{Middleware, MiddlewareStack, Next};
use rainier_framework::prelude::*;

use crate::app::models::Member;
use crate::app::providers::AddIns;

/// Refuses a member with no subscription that grants access right now.
///
/// Reads the **local copy** the Stripe webhooks keep, never Stripe itself: a
/// page that asked Stripe on every view would be as slow as Stripe, limited by
/// Stripe's rate limits, and down whenever Stripe is. "Grants access" is the
/// snapshot's own [`valid`](rainier_framework::billing::SubscriptionSnapshot::valid)
/// — active, trialing, or cancelled but paid to the end of the period.
///
/// After the `web` guard, which is what puts the member on the request.
pub struct EnsureSubscribed {
    addins: Arc<AddIns>,
}

impl EnsureSubscribed {
    /// Resolved when the router compiles, like every guard here.
    pub fn resolved() -> MiddlewareStack {
        MiddlewareStack::new().resolved(|addins: Arc<AddIns>| Self { addins })
    }
}

#[async_trait]
impl Middleware for EnsureSubscribed {
    async fn handle(&self, request: Request, next: Next) -> Response {
        let Some(member) = request.extension::<AuthenticatedUser<Member>>().cloned() else {
            return Error::unauthenticated("Unauthenticated.").into_response();
        };

        let subscribed = match (&self.addins.billing, &member.get().stripe_customer_id) {
            (Some(billing), Some(customer)) => billing.subscriptions.is_subscribed(customer).await,
            _ => Ok(false),
        };

        match subscribed {
            Ok(true) => next.run(request).await,
            // 402 rather than 403: the answer to "may I?" is "yes, once you
            // pay", and a front end can route a 402 to the checkout.
            Ok(false) => Response::json(&serde_json::json!({
                "message": "This needs a subscription.",
                "checkout": "/billing/checkout",
            }))
            .with_status(StatusCode::PAYMENT_REQUIRED),
            Err(e) => e.into_response(),
        }
    }

    fn name(&self) -> &'static str {
        "EnsureSubscribed"
    }
}
