//! `StorePostRequest` — `app/Http/Requests/StorePostRequest.php`.

use rainier_framework::auth::AuthenticatedUser;
use rainier_framework::prelude::*;
use serde::Deserialize;

use crate::app::models::User;

/// The contract for creating a post.
///
/// Three jobs in one type, in this order:
///
/// 1. **authorise** — may this caller do this at all?
/// 2. **validate** — is the input well formed?
/// 3. **bind** — hand the action a struct it can trust.
///
/// The third is quietly the most important: the payload deserialises from the
/// *validated subset*, so a field with no rule never reaches it. A client
/// sending `"published": true` or `"author_id": 9999` gets neither, without
/// the controller having to remember to strip them.
#[derive(Debug, Deserialize)]
pub struct StorePostRequest {
    /// The headline.
    pub title: String,
    /// The body.
    pub body: String,
}

#[async_trait]
impl FormRequest for StorePostRequest {
    fn rules() -> RuleSet {
        vec![
            field("title", [Rule::Required, Rule::String, Rule::Between(3.0, 120.0)]),
            field("body", [Rule::Required, Rule::String, Rule::Min(10.0)]),
        ]
    }

    /// Runs **before** validation, so an unauthorised caller cannot use the
    /// error messages to probe what the endpoint expects.
    async fn authorize(request: &Request) -> bool {
        request.extension::<AuthenticatedUser<User>>().is_some()
    }

    /// Override the default message for one `field.rule` pair.
    fn messages() -> Vec<(&'static str, &'static str)> {
        vec![
            ("body.min", "A post needs at least a sentence."),
            ("title.between", "Give it a title between 3 and 120 characters."),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::http::Method;

    fn authenticated(body: serde_json::Value) -> Request {
        Request::builder().method(Method::POST).json(&body).build().with_extension(
            AuthenticatedUser(Arc::new(User::new("Ada", "ada@example.com", String::new()))),
        )
    }

    #[tokio::test]
    async fn valid_input_becomes_a_typed_payload() {
        let request = authenticated(serde_json::json!({
            "title": "Hello there",
            "body": "Long enough to pass the minimum.",
        }));

        let payload = StorePostRequest::validate_request(&request).await.unwrap();
        assert_eq!(payload.title, "Hello there");
    }

    #[tokio::test]
    async fn an_anonymous_caller_is_refused_before_validation() {
        let request = Request::builder().method(Method::POST).json(&serde_json::json!({})).build();
        let err = StorePostRequest::validate_request(&request).await.unwrap_err();

        assert_eq!(err.status(), 403, "authorisation runs first");
    }

    #[tokio::test]
    async fn a_short_body_gets_the_custom_message() {
        let request = authenticated(serde_json::json!({ "title": "Hello", "body": "short" }));
        let err = StorePostRequest::validate_request(&request).await.unwrap_err();

        assert_eq!(err.status(), 422);
        assert_eq!(err.details().unwrap()["body"][0], "A post needs at least a sentence.");
    }
}
