//! `SsoController` — "sign in with …".
//!
//! Two routes per provider, and the provider is a route parameter rather than
//! a route each: `Sso::driver` answers 404 for a name nobody configured, so
//! `/auth/gitlab/redirect` on a deployment without GitLab is a clean 404 with
//! no checking here.

use rainier_framework::auth::Authenticatable;
use rainier_framework::prelude::*;
use rainier_framework::sso::login_with;

use crate::app::http::controllers::post_controller::resolve;
use crate::app::providers::AddIns;

/// `GET /auth/{provider}/redirect` — off to the provider.
///
/// Stores `state`, the PKCE verifier and (for OpenID Connect) a nonce in the
/// session first, which is why this route is inside the session middleware.
pub async fn redirect(request: Req) -> Result<Response> {
    let addins = resolve::<AddIns>()?;
    let provider = addins.sso.driver(request.route_param("provider").unwrap_or_default())?;
    provider.redirect(&request).await
}

/// `GET /auth/{provider}/callback` — back from it, and logged in.
///
/// `login_with` finds the member already linked to this identity, or links it
/// to the member with the same address — **only** when the provider says it
/// verified that address, because otherwise anybody could register a provider
/// account in someone else's name and walk into their account — or creates
/// one.
pub async fn callback(request: Req) -> Result<Response> {
    let addins = resolve::<AddIns>()?;
    let provider = addins.sso.driver(request.route_param("provider").unwrap_or_default())?;

    let social = provider.user(&request).await?;
    let (member, outcome) = login_with(&*addins.guard, &*addins.store, &request, &social).await?;

    Ok(Response::json(&serde_json::json!({
        "outcome": format!("{outcome:?}"),
        "user": member.auth_identifier(),
        "email": member.email,
    })))
}
