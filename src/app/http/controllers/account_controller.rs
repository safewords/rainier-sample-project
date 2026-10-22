//! `AccountController` — the account pages the scaffolding does not own.
//!
//! The scaffolding mounts login, logout, password reset, email verification
//! and the second factor itself. What is here is what this application wanted
//! to do differently, or in addition:
//!
//! - **registration**, mounted by the application so a captcha can stand in
//!   front of it — the flow itself is still the scaffolding's;
//! - **the account page**, which only a verified address reaches;
//! - **impersonation**, for administrators.

use rainier_framework::auth::{Authenticatable, AuthenticatedUser};
use rainier_framework::prelude::*;

use crate::app::http::controllers::post_controller::resolve;
use crate::app::models::Member;
use crate::app::providers::AddIns;

/// `GET /account/csrf` — nothing, with the CSRF cookie set.
///
/// What a front end calls once before its first form post, when no page
/// rendered by this application handed it the `XSRF-TOKEN` cookie already.
pub async fn csrf() -> Response {
    Response::no_content()
}

/// `POST /account/register` — create an account and log it in.
pub async fn register(request: Req) -> Result<Response> {
    resolve::<AddIns>()?.accounts.register(&request).await
}

/// `GET /account` — who is logged in. Behind `EnsureEmailIsVerified`.
pub async fn show(member: AuthenticatedUser<Member>) -> Result<Response> {
    Ok(Response::json(member.get()))
}

/// `POST /admin/impersonate/{user}` — act as somebody else.
///
/// Who may is the policy in `AddInServiceProvider`: an administrator, as
/// anybody who is not one. Everything else — refusing a second impersonation
/// on top of a first, rotating the session — is the framework's.
pub async fn impersonate(request: Req, staff: AuthenticatedUser<Member>) -> Result<Response> {
    let addins = resolve::<AddIns>()?;

    let id: u64 = request
        .route_param("user")
        .and_then(|id| id.parse().ok())
        .ok_or_else(|| Error::not_found("No such user."))?;
    let target =
        addins.store.member_by_id(id).await?.ok_or_else(|| Error::not_found("No such user."))?;

    addins.impersonation.take(&request, staff.get(), &target)?;

    Ok(Response::json(&serde_json::json!({
        "impersonating": target.auth_identifier(),
        "as": target.name,
    })))
}

/// `POST /admin/impersonate/leave` — back to yourself.
pub async fn leave(request: Req) -> Result<Response> {
    let back_to = resolve::<AddIns>()?.impersonation.leave(&request)?;
    Ok(Response::json(&serde_json::json!({ "user": back_to })))
}
