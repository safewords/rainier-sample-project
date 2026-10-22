//! `BillingController` — sending members to Stripe, and a page they pay for.
//!
//! Card details never touch this application: Checkout and the billing portal
//! are pages Stripe hosts. What happens here is asking Stripe for one and
//! sending the browser to it. What comes *back* arrives as a webhook — see the
//! `/stripe/webhook` route — because a redirect back from Checkout proves
//! nothing on its own: anybody can type the success URL.

use rainier_framework::auth::AuthenticatedUser;
use rainier_framework::billing::{BillableExt, CheckoutParams};
use rainier_framework::prelude::*;

use crate::app::http::controllers::post_controller::resolve;
use crate::app::models::Member;
use crate::app::providers::addin_provider::Billing;
use crate::app::providers::AddIns;

fn billing(addins: &AddIns) -> Result<&Billing> {
    addins.billing.as_ref().ok_or_else(|| Error::not_found("Billing is not configured."))
}

/// `POST /billing/checkout` — a Checkout session for the subscription.
///
/// Answers with the URL rather than redirecting, so a front end can send the
/// browser there itself; a form post would want a `303` instead.
pub async fn checkout(member: AuthenticatedUser<Member>) -> Result<Response> {
    let addins = resolve::<AddIns>()?;
    let billing = billing(&addins)?;
    let mut member = member.get().clone();

    // The customer first, and remembered, so the next checkout and the portal
    // find the same one rather than Stripe accumulating a customer per click.
    if member.stripe_customer_id.is_none() {
        let customer = member.create_or_get_customer(&billing.stripe).await?;
        addins.store.remember_customer(&member, &customer.id).await?;
        member.stripe_customer_id = Some(customer.id);
    }

    let app = &addins.services.app_url;
    let session = member
        .checkout(
            &billing.stripe,
            CheckoutParams::subscription(
                billing.price.clone(),
                format!("{app}/billing/pro?session={{CHECKOUT_SESSION_ID}}"),
                format!("{app}/account"),
            )
            .trial_days(14)
            .allow_promotion_codes(),
        )
        .await?;

    Ok(Response::json(&serde_json::json!({ "url": session.url })))
}

/// `POST /billing/portal` — Stripe's page for cards, invoices and cancelling.
pub async fn portal(member: AuthenticatedUser<Member>) -> Result<Response> {
    let addins = resolve::<AddIns>()?;
    let billing = billing(&addins)?;

    let url = member
        .get()
        .billing_portal_url(&billing.stripe, &format!("{}/account", addins.services.app_url))
        .await?;
    Ok(Response::json(&serde_json::json!({ "url": url })))
}

/// `GET /billing/pro` — the page a subscription buys. Behind `EnsureSubscribed`.
pub async fn pro(member: AuthenticatedUser<Member>) -> Result<Response> {
    Ok(Response::json(&serde_json::json!({
        "message": format!("Welcome to Pro, {}.", member.get().name),
    })))
}
