//! `LoginRequest` and `ListPostsRequest` — more `app/Http/Requests`.

use rainier_framework::prelude::*;
use serde::Deserialize;

/// The contract for logging in.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    /// The address to log in as.
    pub email: String,
    /// The password to check.
    pub password: String,
}

#[async_trait]
impl FormRequest for LoginRequest {
    fn rules() -> RuleSet {
        vec![
            field("email", [Rule::Required, Rule::Email]),
            field("password", [Rule::Required, Rule::Min(8.0)]),
        ]
    }
}

/// Query parameters for the post index.
#[derive(Debug, Deserialize)]
pub struct ListPostsRequest {
    /// Which page, 1-based.
    #[serde(default = "first_page")]
    pub page: u64,
    /// How many per page.
    #[serde(default = "default_per_page")]
    pub per_page: u64,
    /// An optional title search.
    #[serde(default)]
    pub search: Option<String>,
    /// An optional tag to filter by, named rather than keyed.
    #[serde(default)]
    pub tag: Option<String>,
}

fn first_page() -> u64 {
    1
}

fn default_per_page() -> u64 {
    15
}

#[async_trait]
impl FormRequest for ListPostsRequest {
    fn rules() -> RuleSet {
        vec![
            field("page", [Rule::Integer, Rule::Min(1.0)]),
            // Bounded, or a client could ask for every row in one request.
            field("per_page", [Rule::Integer, Rule::Between(1.0, 100.0)]),
            field("search", [Rule::String, Rule::Max(100.0)]),
            field("tag", [Rule::String, Rule::Max(50.0)]),
        ]
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

    #[tokio::test]
    async fn query_parameters_coerce_and_default() {
        let request = Request::builder().uri("/posts?page=2").build();
        let payload = ListPostsRequest::validate_request(&request).await.unwrap();

        assert_eq!(payload.page, 2, "a query string is text; the contract coerces it");
        assert_eq!(payload.per_page, 15, "absent, so the default applies");
    }

    #[tokio::test]
    async fn an_oversized_page_is_refused() {
        let request = Request::builder().uri("/posts?per_page=100000").build();
        let err = ListPostsRequest::validate_request(&request).await.unwrap_err();

        assert_eq!(err.status(), 422);
    }

    #[tokio::test]
    async fn login_requires_a_plausible_address() {
        let request = Request::builder()
            .json(&serde_json::json!({ "email": "nope", "password": "longenough" }))
            .build();

        let err = LoginRequest::validate_request(&request).await.unwrap_err();
        assert!(err.details().unwrap()["email"][0].as_str().unwrap().contains("valid email"));
    }
}
