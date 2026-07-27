//! `AuthController` — logging in and reading the current user.

use rainier_framework::auth::{generate_session_id, Argon2Hasher, Hasher};
use rainier_framework::prelude::*;

use crate::app::models::User;

use crate::app::http::controllers::post_controller::resolve;
use crate::app::http::requests::LoginRequest;
use crate::app::repositories::UserRepository;

/// `POST /login` — exchange credentials for an API token.
pub async fn login(Validated(input): Validated<LoginRequest>) -> Result<Response> {
    let users = resolve::<UserRepository>()?;
    let hasher = resolve::<Argon2Hasher>()?;

    // One message and one status for every failure mode, so the endpoint does
    // not reveal which addresses are registered.
    let invalid = || Error::unauthenticated("Those credentials do not match our records.");

    let mut user = users.by_email(&input.email).await?.ok_or_else(invalid)?;
    if !hasher.verify(&input.password, &user.password) {
        return Err(invalid());
    }

    let token = generate_session_id();
    user.api_token = Some(token.clone());
    users.update(&user).await?;

    Ok(Response::json(&serde_json::json!({ "token": token })))
}

/// `POST /logout` — revoke the current token. Behind `auth:api`.
pub async fn logout(user: AuthenticatedUser<User>) -> Result<Response> {
    let users = resolve::<UserRepository>()?;

    // A clone because the extractor hands out a shared handle: two requests
    // for the same user hold the same `Arc`, so this one revokes its own copy
    // and writes that.
    let mut user = user.get().clone();
    user.api_token = None;
    users.update(&user).await?;

    Ok(Response::no_content())
}

/// `GET /api/me` — the authenticated user. Behind `auth:api`.
pub async fn me(user: AuthenticatedUser<User>) -> Result<Response> {
    Ok(Response::json(user.get()))
}
