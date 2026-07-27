//! `ListNotificationsRequest` — the contract for reading the bell menu.

use rainier_framework::auth::AuthenticatedUser;
use rainier_framework::prelude::*;
use serde::Deserialize;

use crate::app::models::User;

/// The query for `GET /api/notifications`.
///
/// A query string is input like any other, and this is what stops it being
/// read with `request.query()["limit"]` and a cast: `?limit=-1` and
/// `?limit=999999` are both refused here, before the action exists, rather
/// than becoming a database round trip that returns everything.
#[derive(Debug, Deserialize)]
pub struct ListNotificationsRequest {
    /// Narrow to the unread ones.
    #[serde(default)]
    pub unread: bool,

    /// How many to return. A bell menu is a preview, not an archive.
    #[serde(default = "default_limit")]
    pub limit: u64,
}

fn default_limit() -> u64 {
    20
}

#[async_trait]
impl FormRequest for ListNotificationsRequest {
    fn rules() -> RuleSet {
        vec![
            field("unread", [Rule::Boolean]),
            // The ceiling is the point: without it a client picks the size of
            // a query against a table that grows forever.
            field("limit", [Rule::Integer, Rule::Between(1.0, 100.0)]),
        ]
    }

    /// Reading your notifications requires being someone.
    async fn authorize(request: &Request) -> bool {
        request.extension::<AuthenticatedUser<User>>().is_some()
    }

    /// Only the query string. This is a `GET`, and reading a body here would
    /// be surprising.
    fn validation_data(request: &Request) -> serde_json::Value {
        request.query().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::http::Method;

    fn authenticated(query: &str) -> Request {
        Request::builder()
            .method(Method::GET)
            .uri(format!("/api/notifications?{query}"))
            .build()
            .with_extension(AuthenticatedUser(Arc::new(User::new(
                "Ada",
                "ada@example.com",
                String::new(),
            ))))
    }

    #[tokio::test]
    async fn the_defaults_apply_when_nothing_is_asked_for() {
        let payload = ListNotificationsRequest::validate_request(&authenticated("")).await.unwrap();

        assert!(!payload.unread);
        assert_eq!(payload.limit, 20);
    }

    #[tokio::test]
    async fn a_limit_nobody_should_be_able_to_ask_for_is_refused() {
        let err = ListNotificationsRequest::validate_request(&authenticated("limit=100000"))
            .await
            .unwrap_err();

        assert_eq!(err.status(), 422);
    }

    #[tokio::test]
    async fn an_anonymous_caller_never_reaches_validation() {
        let request = Request::builder().method(Method::GET).uri("/api/notifications").build();
        let err = ListNotificationsRequest::validate_request(&request).await.unwrap_err();

        assert_eq!(err.status(), 403);
    }
}
