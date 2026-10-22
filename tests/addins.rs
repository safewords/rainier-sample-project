//! Feature tests for the add-ins — each behind the cargo feature that brings it.
//!
//! ```sh
//! cargo test --features add-ins
//! cargo test --features compat-php
//! ```
//!
//! The same shape as `feature.rs`: boot the real application, drive the real
//! kernel. Two things differ, both because these routes are a browser's:
//!
//! - **a cookie jar.** The account routes run on a session, and the session is
//!   a cookie the next request has to carry back — along with the CSRF token,
//!   echoed from the `XSRF-TOKEN` cookie into `X-XSRF-TOKEN` the way a front
//!   end's HTTP client does. [`Browser`] keeps both.
//! - **third parties answer from a fake.** Stripe, GitHub, the captcha
//!   provider and Discord are all reached through the transport bound under
//!   test, so a test queues what the provider would say and reads back what
//!   was sent.

#![cfg(any(feature = "accounts", feature = "notify-discord", feature = "telemetry"))]
// The boot lock is held across the boot's awaits, as in `feature.rs`.
#![allow(clippy::await_holding_lock)]

use std::sync::{Arc, Mutex};

use app::{boot, Mode};
use rainier_framework::http::{Method, Request};
#[allow(unused_imports, reason = "not every feature combination calls out")]
use rainier_framework::http_client::FakeTransport;
use rainier_framework::server::Kernel;
use rainier_framework::testing::{TestApp, TestResponse};
#[allow(unused_imports, reason = "a telemetry-only build posts no JSON")]
use serde_json::{json, Value};

static BOOTING: Mutex<()> = Mutex::new(());

/// One browser: an application, and the cookies it has been given.
struct Browser {
    app: TestApp,
    kernel: Arc<Kernel>,
    cookies: Mutex<Vec<(String, String)>>,
}

#[allow(dead_code, reason = "not every feature combination uses every helper")]
impl Browser {
    async fn boot() -> Self {
        let app = {
            let _booting = BOOTING.lock().unwrap_or_else(|e| e.into_inner());
            boot(Mode::Testing).await.expect("the application should boot")
        };
        let kernel = app.resolve::<Kernel>().expect("a kernel");
        Self { app: TestApp::new(app).expect("a harness"), kernel, cookies: Mutex::new(Vec::new()) }
    }

    fn resolve<T: Send + Sync + 'static>(&self) -> Arc<T> {
        self.app.resolve::<T>().expect("bound")
    }

    /// The transport Stripe, GitHub and Discord are reached through.
    fn outbound(&self) -> Arc<FakeTransport> {
        self.resolve::<FakeTransport>()
    }

    async fn get(&self, uri: &str) -> TestResponse {
        self.send(Request::builder().method(Method::GET).uri(uri)).await
    }

    async fn post(&self, uri: &str, body: Value) -> TestResponse {
        self.send(Request::builder().method(Method::POST).uri(uri).json(&body)).await
    }

    /// Send with whatever cookies the application has set, echoing the CSRF
    /// cookie back as a header — and remember every cookie in the answer.
    ///
    /// Through the kernel directly rather than `TestApp::send`, because a
    /// `TestResponse` keeps one value per header name and a response that sets
    /// the session *and* the CSRF cookie sends `Set-Cookie` twice.
    async fn send(&self, mut request: rainier_framework::http::RequestBuilder) -> TestResponse {
        {
            let cookies = self.cookies.lock().unwrap();
            if !cookies.is_empty() {
                let header: Vec<String> =
                    cookies.iter().map(|(name, value)| format!("{name}={value}")).collect();
                request = request.header("cookie", &header.join("; "));
            }
            if let Some((_, token)) = cookies.iter().find(|(name, _)| name == "XSRF-TOKEN") {
                request = request.header("x-xsrf-token", token);
            }
        }
        let request = request.header("accept", "application/json").build();

        let response = self.kernel.handle_request(request).await;
        {
            let mut cookies = self.cookies.lock().unwrap();
            for set in response.headers().get_all("set-cookie") {
                let Some((name, value)) =
                    set.to_str().ok().and_then(|set| set.split(';').next()?.split_once('='))
                else {
                    continue;
                };
                cookies.retain(|(existing, _)| existing != name.trim());
                cookies.push((name.trim().to_string(), value.trim().to_string()));
            }
        }
        TestResponse::of(response).await
    }

    fn forget_cookie(&self, name: &str) {
        self.cookies.lock().unwrap().retain(|(existing, _)| existing != name);
    }
}

#[cfg(feature = "accounts")]
mod accounts {
    use super::*;
    use rainier_framework::http::StatusCode;

    /// What a captcha provider says about a solved widget, when the captcha is
    /// built in — every registration passes through it.
    pub(super) fn solve_captcha(browser: &Browser) {
        #[cfg(feature = "captcha")]
        browser
            .resolve::<app::app::providers::outbound::CaptchaFake>()
            .0
            .responding(200, r#"{"success":true,"action":"register"}"#);
        #[cfg(not(feature = "captcha"))]
        let _ = browser;
    }

    /// Register Ada, which also logs her in.
    pub(super) async fn register(browser: &Browser, email: &str) -> TestResponse {
        browser.get("/account/csrf").await;
        solve_captcha(browser);
        browser
            .post(
                "/account/register",
                json!({
                    "name": "Ada Lovelace",
                    "email": email,
                    "password": "correct horse battery",
                    "password_confirmation": "correct horse battery",
                    "cf-turnstile-response": "solved",
                }),
            )
            .await
    }

    /// The link in the last email sent, made relative.
    fn last_link(browser: &Browser) -> String {
        let sent = browser.resolve::<rainier_framework::mail::MemoryTransport>().sent();
        let text = sent.last().and_then(|message| message.text.clone()).expect("an email");
        let start = text.find("https://app.test").expect("a link to the application");
        let link = text[start..].split_whitespace().next().unwrap();
        link.trim_start_matches("https://app.test").to_string()
    }

    #[tokio::test]
    async fn registering_logs_in_and_sends_a_verification_link() {
        let browser = Browser::boot().await;

        let registered = register(&browser, "ada@example.com").await;
        assert_eq!(registered.status(), StatusCode::CREATED, "{}", registered.text());

        let subjects: Vec<String> = browser
            .resolve::<rainier_framework::mail::MemoryTransport>()
            .sent()
            .iter()
            .map(|message| message.envelope.subject.clone())
            .collect();
        assert!(subjects.iter().any(|subject| subject.starts_with("Welcome")), "{subjects:?}");
        assert!(subjects.iter().any(|subject| subject.contains("Verify")), "{subjects:?}");
    }

    #[tokio::test]
    async fn the_account_page_waits_for_a_verified_address() {
        let browser = Browser::boot().await;
        register(&browser, "ada@example.com").await;

        browser.get("/account").await.assert_forbidden();

        let link = last_link(&browser);
        let followed = browser.get(&link).await;
        assert!(
            followed.status().is_success() || followed.status().is_redirection(),
            "{}",
            followed.text()
        );

        browser.get("/account").await.assert_ok().assert_json_path("email", "ada@example.com");
    }

    #[tokio::test]
    async fn a_write_without_the_csrf_token_is_refused() {
        let browser = Browser::boot().await;
        browser.get("/account/csrf").await;
        browser.forget_cookie("XSRF-TOKEN");

        let refused = browser
            .post("/account/login", json!({ "email": "ada@example.com", "password": "x" }))
            .await;
        assert_eq!(refused.status().as_u16(), 419, "{}", refused.text());
    }

    #[tokio::test]
    async fn logging_out_and_back_in_through_the_session() {
        let browser = Browser::boot().await;
        register(&browser, "ada@example.com").await;

        let out = browser.post("/account/logout", json!({})).await;
        assert!(out.status().is_success(), "{}", out.text());

        let wrong = browser
            .post("/account/login", json!({ "email": "ada@example.com", "password": "nope" }))
            .await;
        assert_eq!(wrong.status(), StatusCode::UNPROCESSABLE_ENTITY, "{}", wrong.text());

        let right = browser
            .post(
                "/account/login",
                json!({ "email": "ada@example.com", "password": "correct horse battery" }),
            )
            .await;
        assert!(right.status().is_success(), "{}", right.text());
    }

    #[tokio::test]
    async fn a_forgotten_password_says_the_same_thing_for_everybody() {
        let browser = Browser::boot().await;
        register(&browser, "ada@example.com").await;
        browser.post("/account/logout", json!({})).await;

        let known =
            browser.post("/account/forgot-password", json!({ "email": "ada@example.com" })).await;
        let unknown = browser
            .post("/account/forgot-password", json!({ "email": "nobody@example.com" }))
            .await;

        assert_eq!(known.status(), unknown.status());
        assert_eq!(known.text(), unknown.text());
        assert!(last_link(&browser).starts_with("/reset-password/"), "{}", last_link(&browser));
    }

    #[tokio::test]
    async fn an_administrator_can_act_as_a_member_and_come_back() {
        use app::app::repositories::AccountStore;
        use app::config::services::TEST_ADMIN;
        use rainier_framework::accounts::AccountUsers;

        let browser = Browser::boot().await;
        register(&browser, "linus@example.com").await;
        let linus = browser
            .resolve::<AccountStore>()
            .find_by_email("linus@example.com")
            .await
            .unwrap()
            .expect("registered");
        browser.post("/account/logout", json!({})).await;

        register(&browser, TEST_ADMIN).await;
        let taken = browser.post(&format!("/admin/impersonate/{}", linus.id), json!({})).await;
        taken.assert_ok().assert_json_path("as", "Ada Lovelace");

        browser.post("/admin/impersonate/leave", json!({})).await.assert_ok();
    }

    #[tokio::test]
    async fn a_member_cannot_impersonate_anybody() {
        let browser = Browser::boot().await;
        register(&browser, "linus@example.com").await;

        browser.post("/admin/impersonate/1", json!({})).await.assert_forbidden();
    }

    #[cfg(feature = "captcha")]
    #[tokio::test]
    async fn registration_without_a_solved_captcha_is_refused() {
        let browser = Browser::boot().await;
        browser.get("/account/csrf").await;
        // Nothing queued: the provider's answer is an empty 200, which is not
        // a pass — and a check that cannot be read fails closed.
        let refused = browser
            .post(
                "/account/register",
                json!({
                    "name": "Mallory",
                    "email": "mallory@example.com",
                    "password": "correct horse battery",
                    "password_confirmation": "correct horse battery",
                    "cf-turnstile-response": "unsolved",
                }),
            )
            .await;
        refused.assert_invalid();
    }
}

#[cfg(feature = "two-factor")]
mod two_factor {
    use super::accounts::register;
    use super::*;
    use rainier_framework::mfa::{Secret, Totp};

    fn code(secret: &str, offset: u64) -> String {
        let now =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        Totp::new().code_at(&Secret::from_base32(secret).unwrap(), now + offset)
    }

    #[tokio::test]
    async fn a_password_alone_stops_at_the_challenge_once_enrolled() {
        let browser = Browser::boot().await;
        register(&browser, "ada@example.com").await;

        browser
            .post("/account/user/confirm-password", json!({ "password": "correct horse battery" }))
            .await;
        let enabled = browser.post("/account/user/two-factor-authentication", json!({})).await;
        let secret = enabled.json()["secret"].as_str().expect("a secret").to_string();
        browser
            .post(
                "/account/user/confirmed-two-factor-authentication",
                json!({ "code": code(&secret, 0) }),
            )
            .await
            .assert_ok();

        browser.post("/account/logout", json!({})).await;
        let login = browser
            .post(
                "/account/login",
                json!({ "email": "ada@example.com", "password": "correct horse battery" }),
            )
            .await;
        assert_eq!(login.json(), json!({ "two_factor": true }));

        // The step after the one spent confirming — a code works once.
        browser
            .post("/account/two-factor-challenge", json!({ "code": code(&secret, 30) }))
            .await
            .assert_ok()
            .assert_json_path("authenticated", true);
    }

    #[tokio::test]
    async fn the_secret_is_encrypted_at_rest() {
        use app::app::models::TwoFactorEnrolment;
        use rainier_framework::database::{EntityRepository, Repository};
        use rainier_framework::prelude::Database;

        let browser = Browser::boot().await;
        register(&browser, "ada@example.com").await;
        browser
            .post("/account/user/confirm-password", json!({ "password": "correct horse battery" }))
            .await;
        let enabled = browser.post("/account/user/two-factor-authentication", json!({})).await;
        let secret = enabled.json()["secret"].as_str().unwrap().to_string();

        let rows =
            EntityRepository::<TwoFactorEnrolment>::new((*browser.resolve::<Database>()).clone())
                .all()
                .await
                .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].secret.contains(&secret), "the stored secret is the plaintext");
    }
}

#[cfg(feature = "passkeys")]
mod passkeys {
    use super::accounts::register;
    use super::*;
    use rainier_framework::mfa::webauthn::testing::SoftwareAuthenticator;

    #[tokio::test]
    async fn a_registered_passkey_signs_in_without_a_password() {
        let browser = Browser::boot().await;
        register(&browser, "ada@example.com").await;
        let mut authenticator = SoftwareAuthenticator::es256("app.test", "https://app.test");

        browser
            .post("/account/user/confirm-password", json!({ "password": "correct horse battery" }))
            .await;
        let options = browser.post("/account/user/passkeys/options", json!({})).await.json();
        let credential = authenticator.register(&options);
        browser
            .post("/account/user/passkeys", json!({ "credential": credential, "name": "Laptop" }))
            .await
            .assert_created();

        browser.post("/account/logout", json!({})).await;

        let options = browser.post("/account/passkeys/login/options", json!({})).await.json();
        let assertion = authenticator.authenticate(&options);
        browser
            .post("/account/passkeys/login", json!({ "credential": assertion }))
            .await
            .assert_ok()
            .assert_json_path("authenticated", true);
    }
}

#[cfg(feature = "sso")]
mod sso {
    use super::*;

    /// GitHub's three answers: the token, the profile, the verified emails.
    fn github_answers(browser: &Browser, id: u64, verified_email: &str) {
        let fake = browser.outbound();
        fake.responding(
            200,
            r#"{"access_token":"gho_x","token_type":"bearer","scope":"read:user,user:email"}"#,
        );
        fake.responding(
            200,
            json!({ "id": id, "login": "octo", "name": "Octo", "email": null }).to_string(),
        );
        fake.responding(
            200,
            json!([{ "email": verified_email, "primary": true, "verified": true }]).to_string(),
        );
    }

    async fn sign_in(browser: &Browser) -> TestResponse {
        let redirect = browser.get("/auth/github/redirect").await;
        let location = redirect.header("location").expect("off to GitHub").to_string();
        let state = location
            .split_once('?')
            .and_then(|(_, query)| query.split('&').find_map(|pair| pair.strip_prefix("state=")))
            .expect("a state")
            .to_string();

        browser.get(&format!("/auth/github/callback?code=from-github&state={state}")).await
    }

    #[tokio::test]
    async fn signing_in_with_github_creates_then_finds_the_member() {
        let browser = Browser::boot().await;

        // Both visits' answers up front: a fake hands its queued answers out
        // in order and repeats the *last* one, so answers queued after a
        // round would sit behind the one it left repeating.
        github_answers(&browser, 1, "octo@example.com");
        github_answers(&browser, 1, "octo@example.com");

        sign_in(&browser).await.assert_ok().assert_json_path("outcome", "Created");
        sign_in(&browser).await.assert_ok().assert_json_path("outcome", "Existing");
    }

    #[tokio::test]
    async fn a_callback_with_a_forged_state_is_refused() {
        let browser = Browser::boot().await;
        browser.get("/auth/github/redirect").await;

        let forged = browser.get("/auth/github/callback?code=x&state=not-the-one").await;
        assert!(forged.status().is_client_error(), "{}", forged.text());
    }

    #[tokio::test]
    async fn an_undeclared_provider_is_a_404() {
        let browser = Browser::boot().await;
        browser.get("/auth/myspace/redirect").await.assert_not_found();
    }
}

#[cfg(feature = "billing-stripe")]
mod billing {
    use super::accounts::register;
    use super::*;
    use app::app::providers::addin_provider::TEST_WEBHOOK_SECRET;
    use rainier_framework::billing::sign_payload;
    use rainier_framework::http::StatusCode;

    fn subscription_event(customer: &str, status: &str) -> String {
        let now = chrono::Utc::now().timestamp();
        json!({
            "id": format!("evt_{now}"),
            "type": "customer.subscription.updated",
            "created": now,
            "livemode": false,
            "data": { "object": {
                "id": "sub_123",
                "customer": customer,
                "status": status,
                "cancel_at_period_end": false,
                "items": { "data": [{
                    "id": "si_1",
                    "price": { "id": "price_pro_monthly" },
                    "quantity": 1,
                    "current_period_end": now + 30 * 86_400
                }]}
            }}
        })
        .to_string()
    }

    async fn deliver(browser: &Browser, body: String, secret: &str) -> TestResponse {
        let signature = sign_payload(secret, chrono::Utc::now().timestamp(), body.as_bytes());
        browser
            .send(
                Request::builder()
                    .method(Method::POST)
                    .uri("/stripe/webhook")
                    .header("Stripe-Signature", &signature)
                    .body(body),
            )
            .await
    }

    #[tokio::test]
    async fn checkout_then_a_webhook_unlocks_the_paid_page() {
        let browser = Browser::boot().await;
        register(&browser, "ada@example.com").await;

        // Not yet.
        assert_eq!(browser.get("/billing/pro").await.status(), StatusCode::PAYMENT_REQUIRED);

        // Stripe creates the customer, then the Checkout session.
        let fake = browser.outbound();
        fake.responding(200, json!({ "id": "cus_123", "email": "ada@example.com" }).to_string());
        fake.responding(
            200,
            json!({ "id": "cs_1", "url": "https://checkout.stripe.com/c/pay/cs_1" }).to_string(),
        );
        browser
            .post("/billing/checkout", json!({}))
            .await
            .assert_ok()
            .assert_json_path("url", "https://checkout.stripe.com/c/pay/cs_1");
        fake.assert_sent(|request| request.url().ends_with("/v1/checkout/sessions"));

        // Paying happens at Stripe; the webhook is how this application hears.
        deliver(&browser, subscription_event("cus_123", "active"), TEST_WEBHOOK_SECRET)
            .await
            .assert_ok();

        browser.get("/billing/pro").await.assert_ok();
    }

    #[tokio::test]
    async fn a_forged_webhook_is_refused_and_changes_nothing() {
        let browser = Browser::boot().await;

        let forged =
            deliver(&browser, subscription_event("cus_123", "active"), "whsec_attacker").await;
        assert_eq!(forged.status(), StatusCode::BAD_REQUEST);

        let subscriptions = browser.resolve::<app::app::providers::AddIns>();
        let stored = subscriptions.billing.as_ref().unwrap().subscriptions.for_customer("cus_123");
        assert!(stored.await.unwrap().is_empty());
    }
}

#[cfg(feature = "oauth-server")]
mod oauth {
    use super::accounts::register;
    use super::*;
    use app::app::providers::AddIns;
    use rainier_framework::oauth_server::{grant, pkce, Client};

    const REDIRECT: &str = "http://127.0.0.1/callback";

    async fn mobile_client(browser: &Browser) {
        browser
            .resolve::<AddIns>()
            .oauth
            .store
            .register_client(
                &Client {
                    id: "mobile".into(),
                    name: "Mobile app".into(),
                    secret_hash: None,
                    redirect_uris: vec![REDIRECT.into()],
                    grant_types: vec![
                        grant::AUTHORIZATION_CODE.into(),
                        grant::REFRESH_TOKEN.into(),
                    ],
                    scopes: None,
                    provider: None,
                    owner_id: None,
                    revoked: false,
                },
                false,
            )
            .await
            .unwrap();
    }

    /// The whole dance: consent, approve, exchange — an access token for
    /// `scope`.
    async fn token_for(browser: &Browser, scope: &str) -> String {
        let verifier = pkce::verifier();
        let consent = browser
            .get(&format!(
                "/oauth/authorize?response_type=code&client_id=mobile&scope={scope}\
                 &redirect_uri=http%3A%2F%2F127.0.0.1%2Fcallback&state=xyz\
                 &code_challenge={}&code_challenge_method=S256",
                pkce::challenge(&verifier)
            ))
            .await;
        consent.assert_ok().assert_json_path("client", "Mobile app");
        let auth_token = consent.json()["auth_token"].as_str().unwrap().to_string();

        let approved = browser.post("/oauth/authorize", json!({ "auth_token": auth_token })).await;
        let location = approved.header("location").expect("back to the client").to_string();
        let code = location
            .split_once('?')
            .and_then(|(_, query)| query.split('&').find_map(|pair| pair.strip_prefix("code=")))
            .expect("a code")
            .to_string();

        let exchanged = browser
            .send(Request::builder().method(Method::POST).uri("/oauth/token").form(&[
                ("grant_type", "authorization_code"),
                ("client_id", "mobile"),
                ("redirect_uri", REDIRECT),
                ("code", &code),
                ("code_verifier", &verifier),
            ]))
            .await;
        exchanged.assert_ok();
        exchanged.json()["access_token"].as_str().unwrap().to_string()
    }

    async fn me(browser: &Browser, token: &str) -> TestResponse {
        browser
            .send(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/me")
                    .header("authorization", &format!("Bearer {token}")),
            )
            .await
    }

    #[tokio::test]
    async fn an_approved_client_reads_the_api_with_the_scope_it_was_granted() {
        let browser = Browser::boot().await;
        mobile_client(&browser).await;
        register(&browser, "ada@example.com").await;

        let token = token_for(&browser, "account").await;
        me(&browser, &token).await.assert_ok().assert_json_path("email", "ada@example.com");
    }

    #[tokio::test]
    async fn a_token_without_the_scope_is_refused() {
        let browser = Browser::boot().await;
        mobile_client(&browser).await;
        register(&browser, "ada@example.com").await;

        let token = token_for(&browser, "posts").await;
        me(&browser, &token).await.assert_forbidden();
    }

    #[tokio::test]
    async fn no_token_is_a_401() {
        let browser = Browser::boot().await;
        me(&browser, "junk").await.assert_unauthorized();
    }
}

#[cfg(feature = "notify-discord")]
mod discord {
    use super::*;
    use app::app::providers::register_user;
    use app::config::services::TEST_DISCORD_WEBHOOK;

    #[tokio::test]
    async fn publishing_a_post_announces_it_in_discord() {
        let browser = Browser::boot().await;
        register_user(browser.app.app(), "Ada", "ada@example.com", "correct-horse").await.unwrap();

        let token = browser
            .post("/login", json!({ "email": "ada@example.com", "password": "correct-horse" }))
            .await
            .json()["token"]
            .as_str()
            .unwrap()
            .to_string();
        let authed = |method: Method, uri: &str| {
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", &format!("Bearer {token}"))
        };

        browser
            .send(authed(Method::POST, "/api/posts").json(&json!({
                "title": "Hello, Discord",
                "body": "A post long enough to pass validation.",
            })))
            .await
            .assert_created();
        browser.send(authed(Method::POST, "/api/posts/hello-discord/publish")).await.assert_ok();

        let fake = browser.outbound();
        fake.assert_sent(|request| request.url().starts_with(TEST_DISCORD_WEBHOOK));
        let sent = fake.recorded();
        let body = sent
            .iter()
            .find(|r| r.url().starts_with(TEST_DISCORD_WEBHOOK))
            .unwrap()
            .json()
            .unwrap();
        assert_eq!(body["embeds"][0]["title"], "Hello, Discord");
        assert_eq!(body["allowed_mentions"]["parse"], json!([]));
    }
}

#[cfg(feature = "compat-php")]
mod compat {
    use super::*;
    use app::app::repositories::AccountStore;
    use app::config::services::TEST_PHP_APP_KEY;
    use rainier_framework::accounts::AccountUsers;
    use rainier_framework::compat::php::cache::PhpCache;
    use rainier_framework::compat::php::cookie::EncryptedCookies;
    use rainier_framework::compat::php::serialize::{serialize, Key, Value};
    use rainier_framework::crypt::KeyRing;

    /// The PHP application's signed-in session key — `login_web_` and the
    /// SHA-1 of its guard class; any forty hex digits are recognised.
    const GUARD_KEY: &str = "login_web_59ba36addc2b2f9401580f014c7f58ea4e30989d";

    #[tokio::test]
    async fn a_session_the_php_application_started_is_signed_in_here() {
        let browser = Browser::boot().await;
        super::accounts::register(&browser, "ada@example.com").await;
        let store = browser.resolve::<AccountStore>();
        let ada = store.find_by_email("ada@example.com").await.unwrap().unwrap();
        store.mark_email_verified(&ada).await.unwrap();

        // A fresh browser, holding only the PHP application's cookie.
        let browser_two = Browser {
            app: TestApp::new(Arc::clone(browser.app.app())).unwrap(),
            kernel: Arc::clone(&browser.kernel),
            cookies: Mutex::new(Vec::new()),
        };

        // What the PHP side wrote: the session in the shared cache…
        let php_id = "Ab1Cd2Ef3Gh4Ij5Kl6Mn7Op8Qr9St0Uv1Wx2Yz3A";
        let payload = serialize(&Value::Array(vec![
            (Key::str("_token"), Value::string("php-csrf-token")),
            (Key::str(GUARD_KEY), Value::Int(ada.id as i64)),
        ]));
        let cache = browser.resolve::<rainier_framework::cache::CacheManager>();
        PhpCache::new(Arc::clone(cache.store()), "")
            .put(php_id, &Value::String(payload), None)
            .await
            .unwrap();

        // …and its id, sealed into its cookie.
        let keys =
            KeyRing::from_base64(TEST_PHP_APP_KEY.trim_start_matches("base64:"), &[]).unwrap();
        let cookie = EncryptedCookies::new(keys).encrypt("php_session", php_id).unwrap();
        browser_two.cookies.lock().unwrap().push(("php_session".into(), cookie));

        browser_two.get("/account").await.assert_ok().assert_json_path("email", "ada@example.com");
    }
}

#[cfg(feature = "telemetry")]
mod telemetry {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Metadata, Subscriber};

    /// Records every span opened, by name — all a test needs to see that the
    /// instrumentation is there without a collector to send it to.
    #[derive(Default, Clone)]
    struct Spans {
        names: Arc<Mutex<Vec<String>>>,
        next: Arc<AtomicU64>,
    }

    impl Subscriber for Spans {
        fn enabled(&self, _: &Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, span: &Attributes<'_>) -> Id {
            self.names.lock().unwrap().push(span.metadata().name().to_string());
            Id::from_u64(self.next.fetch_add(1, Ordering::Relaxed) + 1)
        }
        fn record(&self, _: &Id, _: &Record<'_>) {}
        fn record_follows_from(&self, _: &Id, _: &Id) {}
        fn event(&self, _: &Event<'_>) {}
        fn enter(&self, _: &Id) {}
        fn exit(&self, _: &Id) {}
    }

    #[tokio::test]
    async fn a_request_is_traced_through_the_database_with_no_code_of_ours() {
        let browser = Browser::boot().await;

        // Installed for this thread only — `#[tokio::test]` is single-threaded,
        // so everything the request does is seen, and nothing from another test.
        let spans = Spans::default();
        let _guard = tracing::subscriber::set_default(spans.clone());

        browser.get("/api/posts").await.assert_ok();

        let names = spans.names.lock().unwrap().clone();
        assert!(names.iter().any(|name| name == "http.server.request"), "{names:?}");
        assert!(names.iter().any(|name| name == "db.query"), "{names:?}");
    }
}
