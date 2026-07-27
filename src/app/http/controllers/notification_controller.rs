//! `NotificationController` — the in-app bell menu.
//!
//! What the [database channel](rainier_framework::notifications::DatabaseChannel)
//! stores is only useful if something reads it. This is that something: the
//! rows one notification wrote, for the user they were written for.
//!
//! Note what is missing — any mention of `PostLive`. The list is of
//! *notifications*, whatever they turn out to be, so adding a second one is a
//! new file in `app/notifications` and no change here.

use rainier_framework::notifications::DatabaseChannel;
use rainier_framework::prelude::*;

use crate::app::http::controllers::post_controller::resolve;
use crate::app::http::requests::ListNotificationsRequest;
use crate::app::models::User;

/// `GET /api/notifications` — this user's notifications, newest first.
///
/// `?unread=1` narrows it to the ones they have not seen.
pub async fn index(
    user: AuthenticatedUser<User>,
    Validated(query): Validated<ListNotificationsRequest>,
) -> Result<Response> {
    let stored = resolve::<DatabaseChannel>()?;

    // Scoped to the caller, in the query. Fetching and then filtering would
    // make "whose is this?" a thing the controller has to remember, and one
    // day it would forget.
    let rows = if query.unread {
        stored.unread(user.notifiable_type(), &user.notifiable_id(), query.limit).await?
    } else {
        stored.for_recipient(user.notifiable_type(), &user.notifiable_id(), query.limit).await?
    };

    let unread = stored.unread_count(user.notifiable_type(), &user.notifiable_id()).await?;

    Ok(Response::json(&serde_json::json!({
        "unread": unread,
        "data": rows.iter().map(present).collect::<Vec<_>>(),
    })))
}

/// `POST /api/notifications/{notification}/read` — mark one as read.
pub async fn read(user: AuthenticatedUser<User>, Path(id): Path<String>) -> Result<Response> {
    let stored = resolve::<DatabaseChannel>()?;

    // Scoped to the caller in the lookup. An id is an opaque string, not a
    // secret, and marking someone else's notification read is a small thing
    // that should still be impossible. A 404 rather than a 403, so the id
    // does not confirm it exists.
    let row = stored
        .find_for(user.notifiable_type(), &user.notifiable_id(), &id)
        .await?
        .ok_or_else(|| Error::not_found("No notification matches the given key."))?;

    stored.mark_read(&row.id).await?;
    Ok(Response::no_content())
}

/// `POST /api/notifications/read` — mark them all as read.
pub async fn read_all(user: AuthenticatedUser<User>) -> Result<Response> {
    let stored = resolve::<DatabaseChannel>()?;

    let marked = stored.mark_all_read(user.notifiable_type(), &user.notifiable_id()).await?;

    Ok(Response::json(&serde_json::json!({ "marked": marked })))
}

/// One row, as the client sees it.
///
/// The stored payload is passed through rather than reshaped: it is whatever
/// `to_data` wrote, and the client that renders `post.live` knows its fields.
fn present(row: &rainier_framework::notifications::NotificationRow) -> serde_json::Value {
    serde_json::json!({
        "id": row.id,
        "type": row.notification,
        "data": row.data().unwrap_or(serde_json::Value::Null),
        "read_at": row.read_at,
        "created_at": row.created_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::models::User;

    #[test]
    fn a_row_is_presented_with_its_payload_parsed() {
        let row = rainier_framework::notifications::NotificationRow {
            id: "n1".into(),
            notification: "post.live".into(),
            notifiable_type: "User".into(),
            notifiable_id: "1".into(),
            payload: r#"{"slug":"going-live"}"#.into(),
            read_at: None,
            created_at: chrono::Utc::now(),
        };

        let json = present(&row);

        assert_eq!(json["type"], "post.live");
        assert_eq!(json["data"]["slug"], "going-live", "the payload is JSON, not a string");
        assert!(json["read_at"].is_null());
    }

    #[test]
    fn the_notifiable_identity_is_what_scopes_the_query() {
        let user = User::new("Ada", "ada@example.com", "hash".into());
        assert_eq!(user.notifiable_type(), "User");
        assert_eq!(user.notifiable_id(), "0", "unsaved: the database has not assigned a key yet");
    }
}
