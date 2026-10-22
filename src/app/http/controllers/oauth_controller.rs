//! `OAuthController` — this application as an OAuth 2.0 server.
//!
//! Other applications ask a member for access, the member approves on a
//! consent screen, and the application gets tokens for the API under
//! `/api/v1`, limited to the scopes approved.
//!
//! | Route | Who calls it |
//! |---|---|
//! | `GET /oauth/authorize` | the member's browser, sent by the client — answers the consent screen |
//! | `POST /oauth/authorize` | the consent screen's approve/deny |
//! | `POST /oauth/token` | the client, exchanging a code or a refresh token |
//! | `POST /oauth/revoke` | the client, giving a token back |
//!
//! The consent screen here is JSON — what a front end renders. The two
//! authorize routes are behind the browser session and the `web` guard,
//! because approving is something a logged-in member does; the token routes
//! are behind neither, because the client authenticates itself there.

use rainier_framework::auth::{Authenticatable, AuthenticatedUser};
use rainier_framework::oauth_server::AuthorizationSession;
use rainier_framework::prelude::*;

use crate::app::http::controllers::post_controller::resolve;
use crate::app::models::{Member, User};
use crate::app::providers::AddIns;

/// `GET /oauth/authorize` — validate the client's request, and describe it.
pub async fn authorize(request: Req, _member: AuthenticatedUser<Member>) -> Result<Response> {
    let addins = resolve::<AddIns>()?;

    // An invalid request is answered in the OAuth error shape — or, when the
    // client and its redirect URI check out, by redirecting the error back to
    // it. Never by redirecting anywhere unregistered.
    let authorization = match addins.oauth.server.validate_authorization_request(&request).await {
        Ok(authorization) => authorization,
        Err(error) => return Ok(error.into_response()),
    };

    // Held in the session under a one-time token, so the approval cannot be
    // for a request other than the one the member was shown.
    let auth_token = AuthorizationSession::stash(&request, &authorization)?;

    Ok(Response::json(&serde_json::json!({
        "client": authorization.client_name,
        "scopes": authorization.scopes,
        "auth_token": auth_token,
    })))
}

/// `POST /oauth/authorize` — the member's answer.
pub async fn approve(request: Req, member: AuthenticatedUser<Member>) -> Result<Response> {
    let addins = resolve::<AddIns>()?;

    let token = request.input_or("auth_token", "");
    let authorization = AuthorizationSession::take(&request, &token)?;
    let approved = request.input("approve").as_deref() != Some("false");

    Ok(addins
        .oauth
        .server
        .complete_authorization_request(&authorization, &member.get().auth_identifier(), approved)
        .await)
}

/// `POST /oauth/token` — every grant.
pub async fn token(request: Req) -> Result<Response> {
    Ok(resolve::<AddIns>()?.oauth.server.respond_to_token_request(&request).await)
}

/// `POST /oauth/revoke` — RFC 7009. Always `200`, whether or not the token
/// existed: saying otherwise would tell a guesser which ones do.
pub async fn revoke(request: Req) -> Result<Response> {
    Ok(resolve::<AddIns>()?.oauth.server.respond_to_revocation_request(&request).await)
}

/// `GET /api/v1/me` — the user an OAuth token acts for. Needs the `account`
/// scope.
pub async fn me(user: AuthenticatedUser<User>) -> Result<Response> {
    Ok(Response::json(user.get()))
}
