//! Feature tests — Laravel's `tests/Feature`.
//!
//! These boot the real application and drive the real kernel: real routes,
//! real middleware, real database, real migrations. Only the mail transport is
//! a double, and only so a test can assert on what was sent.
//!
//! Each test boots its own application, so they must not run concurrently —
//! the facades are process-global. `SERIAL` enforces that.

// The serial guard is deliberately held across awaits: that is the point. Safe
// because `#[tokio::test]` runs on a current-thread runtime, so it never
// crosses a thread.
#![allow(clippy::await_holding_lock)]

use app::app::models::{Post, Tag, User};
use app::app::providers::register_user;
use app::app::repositories::PostRepository;
use app::{boot, Mode};
use rainier_framework::database::Repository;
use rainier_framework::http::{Method, Request, Response, StatusCode};
use rainier_framework::prelude::*;
use rainier_framework::server::Kernel;
use std::sync::Arc;

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A booted application plus the helpers a test needs to drive it.
struct App {
    app: Arc<Application>,
    kernel: Arc<Kernel>,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl App {
    async fn boot() -> Self {
        let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let app = boot(Mode::Testing).await.expect("the application should boot");
        let kernel = app.resolve::<Kernel>().expect("a kernel");
        Self { app, kernel, _guard: guard }
    }

    async fn send(&self, request: Request) -> Response {
        self.kernel.handle_request(request).await
    }

    async fn json(&self, request: Request) -> serde_json::Value {
        let bytes =
            self.send(request).await.into_http().into_body().collect().await.expect("a body");
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    }

    fn get(&self, uri: &str) -> Request {
        Request::builder().method(Method::GET).uri(uri).header("accept", "application/json").build()
    }

    /// Register a user and log in, returning the API token.
    async fn login(&self) -> String {
        register_user(&self.app, "Ada Lovelace", "ada@example.com", "correct-horse")
            .await
            .expect("the user should be created");

        let body = self
            .json(
                Request::builder()
                    .method(Method::POST)
                    .uri("/login")
                    .header("accept", "application/json")
                    .json(&serde_json::json!({
                        "email": "ada@example.com",
                        "password": "correct-horse",
                    }))
                    .build(),
            )
            .await;

        body["token"].as_str().expect("a token").to_string()
    }

    fn authed(
        &self,
        method: Method,
        uri: &str,
        token: &str,
    ) -> rainier_framework::http::RequestBuilder {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("accept", "application/json")
            .header("authorization", &format!("Bearer {token}"))
    }

    fn posts(&self) -> Arc<PostRepository> {
        self.app.resolve::<PostRepository>().expect("a post repository")
    }

    fn mail(&self) -> Arc<rainier_framework::mail::MemoryTransport> {
        self.app
            .resolve::<rainier_framework::mail::MemoryTransport>()
            .expect("the memory transport")
    }
}

// --- boot -------------------------------------------------------------------

#[tokio::test]
async fn the_application_boots_and_migrates() {
    let app = App::boot().await;
    // The migrator ran during boot, so a query works.
    assert_eq!(app.posts().count().await.unwrap(), 0);
}

#[tokio::test]
async fn the_health_check_responds() {
    let app = App::boot().await;
    let response = app.send(app.get("/health")).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn the_home_page_renders_html() {
    let app = App::boot().await;
    let request = Request::builder().method(Method::GET).uri("/").build();
    let response = app.send(request).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.header("content-type"), Some("text/html; charset=utf-8"));
}

// --- authentication ---------------------------------------------------------

#[tokio::test]
async fn logging_in_returns_a_token() {
    let app = App::boot().await;
    assert_eq!(app.login().await.len(), 64, "256 bits, hex-encoded");
}

#[tokio::test]
async fn a_wrong_password_and_an_unknown_address_look_identical() {
    // So the endpoint does not reveal which addresses are registered.
    let app = App::boot().await;
    register_user(&app.app, "Ada", "ada@example.com", "correct-horse").await.unwrap();

    let attempt = |email: &str, password: &str| {
        Request::builder()
            .method(Method::POST)
            .uri("/login")
            .header("accept", "application/json")
            .json(&serde_json::json!({ "email": email, "password": password }))
            .build()
    };

    let wrong_password = app.json(attempt("ada@example.com", "wrong-password")).await;
    let unknown_user = app.json(attempt("nobody@example.com", "correct-horse")).await;

    assert_eq!(wrong_password["message"], unknown_user["message"]);
}

#[tokio::test]
async fn a_guarded_route_refuses_an_anonymous_caller() {
    let app = App::boot().await;
    assert_eq!(app.send(app.get("/api/me")).await.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_guarded_route_accepts_a_token_and_hides_secrets() {
    let app = App::boot().await;
    let token = app.login().await;

    let body = app.json(app.authed(Method::GET, "/api/me", &token).build()).await;
    assert_eq!(body["email"], "ada@example.com");
    assert!(body.get("password").is_none(), "the hash must never be serialised");
    assert!(body.get("api_token").is_none());
}

#[tokio::test]
async fn logging_out_revokes_the_token() {
    let app = App::boot().await;
    let token = app.login().await;

    assert_eq!(
        app.send(app.authed(Method::POST, "/api/logout", &token).build()).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.send(app.authed(Method::GET, "/api/me", &token).build()).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

// --- request contracts ------------------------------------------------------

#[tokio::test]
async fn creating_a_post_validates_its_input() {
    let app = App::boot().await;
    let token = app.login().await;

    let body = app
        .json(
            app.authed(Method::POST, "/api/posts", &token)
                .json(&serde_json::json!({ "title": "Hi", "body": "short" }))
                .build(),
        )
        .await;

    assert_eq!(body["errors"]["body"][0], "A post needs at least a sentence.");
}

#[tokio::test]
async fn a_client_cannot_mass_assign_what_the_contract_did_not_declare() {
    let app = App::boot().await;
    let token = app.login().await;

    app.send(
        app.authed(Method::POST, "/api/posts", &token)
            .json(&serde_json::json!({
                "title": "Sneaky",
                "body": "A body long enough to clear the minimum.",
                "published": true,
                "author_id": 9999,
            }))
            .build(),
    )
    .await;

    let stored = &app.posts().all().await.unwrap()[0];
    assert!(!stored.published, "`published` has no rule, so it is not assignable");
    assert_eq!(stored.author_id, 1, "the author comes from the guard, not the body");
}

#[tokio::test]
async fn global_middleware_trims_input_before_the_contract_sees_it() {
    let app = App::boot().await;
    let token = app.login().await;

    app.send(
        app.authed(Method::POST, "/api/posts", &token)
            .json(&serde_json::json!({
                "title": "   Trimmed Title   ",
                "body": "A body long enough to clear the minimum.",
            }))
            .build(),
    )
    .await;

    assert_eq!(app.posts().all().await.unwrap()[0].title, "Trimmed Title");
}

// --- listing and showing ----------------------------------------------------

#[tokio::test]
async fn the_index_lists_only_published_posts() {
    let app = App::boot().await;
    let author = register_user(&app.app, "Ada", "ada@example.com", "correct-horse").await.unwrap();
    let posts = app.posts();

    posts.create_unique(Post::draft("A draft", "body", author.id)).await.unwrap();
    let mut live = posts.create_unique(Post::draft("Published", "body", author.id)).await.unwrap();
    live.published = true;
    posts.update(&live).await.unwrap();

    let body = app.json(app.get("/api/posts")).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["post"]["title"], "Published");
}

#[tokio::test]
async fn the_index_loads_the_author_of_every_post_it_returns() {
    // The `belongs_to`, over a page: one query for every author on it.
    let app = App::boot().await;
    let author = register_user(&app.app, "Ada", "ada@example.com", "correct-horse").await.unwrap();
    let posts = app.posts();

    for title in ["One", "Two", "Three"] {
        let mut post = posts.create_unique(Post::draft(title, "body", author.id)).await.unwrap();
        post.published = true;
        posts.update(&post).await.unwrap();
    }

    let body = app.json(app.get("/api/posts")).await;

    assert_eq!(body["total"], 3);
    for entry in body["data"].as_array().unwrap() {
        assert_eq!(entry["author"], "Ada");
    }
}

#[tokio::test]
async fn a_post_carries_the_tags_the_pivot_links_to_it() {
    use rainier_framework::database::{EntityRepository, Relation};

    let app = App::boot().await;
    let author = register_user(&app.app, "Ada", "ada@example.com", "correct-horse").await.unwrap();
    let posts = app.posts();

    let mut post = posts.create_unique(Post::draft("Tagged", "body", author.id)).await.unwrap();
    post.published = true;
    posts.update(&post).await.unwrap();

    let tags = app.app.resolve::<EntityRepository<Tag>>().unwrap();
    let rust = tags.create(Tag::named("Rust")).await.unwrap();
    let laravel = tags.create(Tag::named("laravel")).await.unwrap();

    let db = app.app.resolve::<rainier_framework::database::Database>().unwrap();
    for tag in [&rust, &laravel] {
        db.statement(&format!("INSERT INTO post_tag VALUES ({}, {})", post.id, tag.id))
            .await
            .unwrap();
    }

    let body = app.json(app.get("/api/posts")).await;
    let names = body["data"][0]["tags"].as_array().unwrap();

    assert_eq!(names.len(), 2);
    assert!(names.iter().any(|name| name == "rust"), "{names:?}");

    // And the inverse: the same pivot, read from the tag's side.
    let with_posts = Tag::posts().load(&[rust.clone()], &**posts).await.unwrap();
    assert_eq!(with_posts.of(&rust).len(), 1);
    assert_eq!(with_posts.one(&rust).unwrap().slug, "tagged");
    assert_eq!(Tag::route_key_name(), "name");
}

#[tokio::test]
async fn counting_a_relationship_does_not_load_it() {
    use rainier_framework::database::Relation;

    let app = App::boot().await;
    let author = register_user(&app.app, "Ada", "ada@example.com", "correct-horse").await.unwrap();
    let posts = app.posts();

    for title in ["One", "Two"] {
        posts.create_unique(Post::draft(title, "body", author.id)).await.unwrap();
    }

    let counts = User::posts().count(&[author.clone()], &**posts).await.unwrap();

    assert_eq!(counts.of(&author), 2);
    assert_eq!(counts.total(), 2);
}

#[tokio::test]
async fn an_unpublished_post_is_a_404_rather_than_a_leak() {
    let app = App::boot().await;
    let author = register_user(&app.app, "Ada", "ada@example.com", "correct-horse").await.unwrap();
    app.posts().create_unique(Post::draft("Secret Draft", "body", author.id)).await.unwrap();

    assert_eq!(app.send(app.get("/api/posts/secret-draft")).await.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_slug_collision_gets_a_suffix() {
    let app = App::boot().await;
    let author = register_user(&app.app, "Ada", "ada@example.com", "correct-horse").await.unwrap();
    let posts = app.posts();

    let first = posts.create_unique(Post::draft("Same Title", "b", author.id)).await.unwrap();
    let second = posts.create_unique(Post::draft("Same Title", "b", author.id)).await.unwrap();

    assert_eq!(first.slug, "same-title");
    assert_eq!(second.slug, "same-title-2");
}

#[tokio::test]
async fn a_slug_constraint_rejects_a_non_slug() {
    let app = App::boot().await;
    assert_eq!(
        app.send(app.get("/api/posts/not%20a%20slug")).await.status(),
        StatusCode::NOT_FOUND
    );
}

// --- publishing: policies, events, queues, mail -----------------------------

async fn create_post(app: &App, token: &str, title: &str) {
    app.send(
        app.authed(Method::POST, "/api/posts", token)
            .json(&serde_json::json!({
                "title": title,
                "body": "A body long enough to clear the minimum.",
            }))
            .build(),
    )
    .await;
}

#[tokio::test]
async fn publishing_queues_a_notification_instead_of_sending_it_inline() {
    let app = App::boot().await;
    let token = app.login().await;
    create_post(&app, &token, "Going live").await;

    let response =
        app.send(app.authed(Method::POST, "/api/posts/going-live/publish", &token).build()).await;
    assert_eq!(response.status(), StatusCode::OK);

    let queue = app.app.resolve::<rainier_framework::queue::QueueManager>().unwrap();
    assert_eq!(queue.queue().size("mail").await.unwrap(), 1);
}

/// Drain the `mail` queue, as `queue:work` would.
async fn work_the_mail_queue(app: &App) -> rainier_framework::queue::WorkerStats {
    let manager = app.app.resolve::<rainier_framework::queue::QueueManager>().unwrap();
    let worker = rainier_framework::queue::Worker::new(
        Arc::clone(manager.queue()),
        Arc::clone(manager.registry()),
        Arc::clone(app.app.container()),
    )
    .with_options(
        rainier_framework::queue::WorkerOptions::default().queues(["mail"]).stop_when_empty(),
    );

    worker.run().await.unwrap()
}

#[tokio::test]
async fn the_queued_notification_sends_the_mail_when_a_worker_runs() {
    let app = App::boot().await;
    let token = app.login().await;
    create_post(&app, &token, "Going live").await;
    app.send(app.authed(Method::POST, "/api/posts/going-live/publish", &token).build()).await;

    let before = app.mail().count();
    let stats = work_the_mail_queue(&app).await;

    assert_eq!(stats.processed, 1);
    assert_eq!(stats.failed, 0);
    assert_eq!(app.mail().count(), before + 1);
    assert!(app.mail().sent().last().unwrap().envelope.subject.contains("Going live"));
}

#[tokio::test]
async fn the_notification_is_addressed_by_the_recipient_not_by_the_message() {
    // `PostLiveMail` sets no `to`. The address comes from the author's
    // `route_for("mail")`, which is what makes it a notification.
    let app = App::boot().await;
    let token = app.login().await;
    create_post(&app, &token, "Going live").await;
    app.send(app.authed(Method::POST, "/api/posts/going-live/publish", &token).build()).await;

    work_the_mail_queue(&app).await;

    let sent = app.mail().sent();
    let message = sent.last().unwrap();
    assert_eq!(message.envelope.to.len(), 1);
    assert_eq!(message.envelope.to[0].email, "ada@example.com");
}

#[tokio::test]
async fn one_notification_reaches_every_channel_it_selected() {
    // `via()` chose mail *and* database, so one send produces an email and a
    // row. The job asked for neither by name.
    let app = App::boot().await;
    let token = app.login().await;
    create_post(&app, &token, "Going live").await;
    app.send(app.authed(Method::POST, "/api/posts/going-live/publish", &token).build()).await;

    work_the_mail_queue(&app).await;

    let stored = app.app.resolve::<rainier_framework::notifications::DatabaseChannel>().unwrap();
    let unread = stored.unread("User", "1", 10).await.unwrap();

    let emails =
        app.mail().sent().iter().filter(|m| m.envelope.subject.contains("Going live")).count();

    assert_eq!(emails, 1, "the mail channel");
    assert_eq!(unread.len(), 1, "the database channel");
    assert_eq!(unread[0].notification, "post.live");
    assert_eq!(unread[0].data().unwrap()["slug"], "going-live");
    assert_eq!(stored.unread_count("User", "1").await.unwrap(), 1);
}

#[tokio::test]
async fn the_bell_menu_lists_what_the_database_channel_stored() {
    let app = App::boot().await;
    let token = app.login().await;
    create_post(&app, &token, "Going live").await;
    app.send(app.authed(Method::POST, "/api/posts/going-live/publish", &token).build()).await;
    work_the_mail_queue(&app).await;

    let body = app.json(app.authed(Method::GET, "/api/notifications", &token).build()).await;

    assert_eq!(body["unread"], 1);
    assert_eq!(body["data"][0]["type"], "post.live");
    assert_eq!(body["data"][0]["data"]["title"], "Going live");
    assert!(body["data"][0]["read_at"].is_null());
}

#[tokio::test]
async fn reading_a_notification_clears_the_badge() {
    let app = App::boot().await;
    let token = app.login().await;
    create_post(&app, &token, "Going live").await;
    app.send(app.authed(Method::POST, "/api/posts/going-live/publish", &token).build()).await;
    work_the_mail_queue(&app).await;

    let listed = app.json(app.authed(Method::GET, "/api/notifications", &token).build()).await;
    let id = listed["data"][0]["id"].as_str().unwrap().to_string();

    let response = app
        .send(app.authed(Method::POST, &format!("/api/notifications/{id}/read"), &token).build())
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let after = app.json(app.authed(Method::GET, "/api/notifications", &token).build()).await;
    assert_eq!(after["unread"], 0);
    assert!(!after["data"][0]["read_at"].is_null(), "read, not deleted");
}

#[tokio::test]
async fn you_cannot_read_someone_elses_notification() {
    let app = App::boot().await;
    let token = app.login().await;
    create_post(&app, &token, "Going live").await;
    app.send(app.authed(Method::POST, "/api/posts/going-live/publish", &token).build()).await;
    work_the_mail_queue(&app).await;

    // A guessable id belonging to nobody in this session.
    let response = app
        .send(app.authed(Method::POST, "/api/notifications/not-yours/read", &token).build())
        .await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_bell_menu_is_empty_for_a_user_nothing_was_sent_to() {
    let app = App::boot().await;
    let token = app.login().await;

    let body = app.json(app.authed(Method::GET, "/api/notifications", &token).build()).await;

    assert_eq!(body["unread"], 0);
    assert_eq!(body["data"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn a_stored_notification_can_be_marked_read() {
    // The bell menu's other half. `unread_count` is what the badge shows.
    let app = App::boot().await;
    let token = app.login().await;
    create_post(&app, &token, "Going live").await;
    app.send(app.authed(Method::POST, "/api/posts/going-live/publish", &token).build()).await;
    work_the_mail_queue(&app).await;

    let stored = app.app.resolve::<rainier_framework::notifications::DatabaseChannel>().unwrap();
    let id = stored.unread("User", "1", 10).await.unwrap()[0].id.clone();

    assert!(stored.mark_read(&id).await.unwrap());
    assert_eq!(stored.unread_count("User", "1").await.unwrap(), 0);
    assert!(!stored.mark_read(&id).await.unwrap(), "marking it twice changes nothing");
    assert_eq!(stored.for_recipient("User", "1", 10).await.unwrap().len(), 1, "still there");
}

#[tokio::test]
async fn publishing_twice_does_not_queue_a_second_notification() {
    let app = App::boot().await;
    let token = app.login().await;
    create_post(&app, &token, "Going live").await;

    for _ in 0..2 {
        app.send(app.authed(Method::POST, "/api/posts/going-live/publish", &token).build()).await;
    }

    let queue = app.app.resolve::<rainier_framework::queue::QueueManager>().unwrap();
    assert_eq!(queue.queue().size("mail").await.unwrap(), 1);
}

#[tokio::test]
async fn the_policy_stops_you_touching_someone_elses_post() {
    let app = App::boot().await;
    let token = app.login().await;

    let other =
        register_user(&app.app, "Grace", "grace@example.com", "another-password").await.unwrap();
    app.posts().create_unique(Post::draft("Not Yours", "body", other.id)).await.unwrap();

    for uri in ["/api/posts/not-yours/publish"] {
        assert_eq!(
            app.send(app.authed(Method::POST, uri, &token).build()).await.status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        app.send(app.authed(Method::DELETE, "/api/posts/not-yours", &token).build()).await.status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn registering_sends_a_welcome_email() {
    let app = App::boot().await;
    register_user(&app.app, "Ada", "ada@example.com", "correct-horse").await.unwrap();

    let sent = app.mail().sent();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].envelope.subject.contains("Ada"));
    assert_eq!(sent[0].envelope.from.as_ref().unwrap().email, "hello@example.com");
}

// --- routing and middleware -------------------------------------------------

#[tokio::test]
async fn named_routes_generate_urls() {
    let app = App::boot().await;
    let urls = app.app.resolve::<rainier_framework::routing::UrlGenerator>().unwrap();

    assert_eq!(urls.route("api.posts.index", &[]).unwrap(), "/api/posts");
    assert_eq!(urls.route("api.posts.show", &[("post", "hello")]).unwrap(), "/api/posts/hello");
    assert_eq!(urls.route("login", &[]).unwrap(), "/login");
}

#[tokio::test]
async fn the_api_group_adds_cors_and_rate_limit_headers() {
    let app = App::boot().await;
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/posts")
        .header("origin", "https://app.example")
        .header("accept", "application/json")
        .build();

    let response = app.send(request).await;
    assert_eq!(response.header("access-control-allow-origin"), Some("*"));
    assert!(response.header("x-ratelimit-limit").is_some());
}

#[tokio::test]
async fn every_response_carries_a_request_id() {
    let app = App::boot().await;
    assert!(app.send(app.get("/health")).await.header("x-request-id").is_some());
}

#[tokio::test]
async fn an_incoming_request_id_is_preserved_end_to_end() {
    let app = App::boot().await;
    let request = Request::builder()
        .method(Method::GET)
        .uri("/health")
        .header("x-request-id", "trace-abc")
        .build();

    assert_eq!(app.send(request).await.header("x-request-id"), Some("trace-abc"));
}

#[tokio::test]
async fn the_wrong_method_is_a_405_that_says_what_is_allowed() {
    let app = App::boot().await;
    let request = Request::builder().method(Method::DELETE).uri("/login").build();
    let response = app.send(request).await;

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(response.header("allow").unwrap().contains("POST"));
}

#[tokio::test]
async fn a_browser_gets_html_and_an_api_client_gets_json_for_the_same_error() {
    let app = App::boot().await;

    let browser = app.send(Request::builder().method(Method::GET).uri("/nope").build()).await;
    assert_eq!(browser.header("content-type"), Some("text/html; charset=utf-8"));

    let api = app.send(app.get("/nope")).await;
    assert_eq!(api.header("content-type"), Some("application/json; charset=utf-8"));
}

// --- seeding ----------------------------------------------------------------

#[tokio::test]
async fn seeding_is_idempotent() {
    let app = App::boot().await;

    app::database::seeders::seed(&app.app).await.unwrap();
    let after_first = app.posts().count().await.unwrap();
    assert!(after_first > 0);

    app::database::seeders::seed(&app.app).await.unwrap();
    assert_eq!(app.posts().count().await.unwrap(), after_first, "running twice adds nothing");
}

// --- sessions and encryption -------------------------------------------------

/// The session cookie a response set, if any.
fn session_cookie(response: &Response) -> Option<String> {
    response
        .header("set-cookie")?
        .split(';')
        .next()?
        .strip_prefix("rainier_session=")
        .map(str::to_string)
}

#[tokio::test]
async fn a_session_counts_visits_across_requests() {
    let app = App::boot().await;

    let first = app.send(app.get("/visits")).await;
    let cookie = session_cookie(&first).expect("the session should be persisted");

    let with_cookie = || {
        Request::builder()
            .method(Method::GET)
            .uri("/visits")
            .header("accept", "application/json")
            .header("cookie", &format!("rainier_session={cookie}"))
            .build()
    };

    assert_eq!(app.json(with_cookie()).await["visits"], 1);
    assert_eq!(app.json(with_cookie()).await["visits"], 2);
}

#[tokio::test]
async fn flash_data_survives_exactly_one_request() {
    let app = App::boot().await;

    let first = app.send(app.get("/visits")).await;
    let cookie = session_cookie(&first).expect("a session cookie");

    let with_cookie = || {
        Request::builder()
            .method(Method::GET)
            .uri("/visits")
            .header("accept", "application/json")
            .header("cookie", &format!("rainier_session={cookie}"))
            .build()
    };

    // The first request flashed a greeting; this one reads it…
    let second = app.json(with_cookie()).await;
    assert!(second["flashed_last_time"].is_string(), "{second}");

    // …and it re-flashes, so the next one reads the *new* one rather than the
    // stale one. What matters is that it is never the same value twice.
    let third = app.json(with_cookie()).await;
    assert_ne!(third["flashed_last_time"], second["flashed_last_time"]);
}

#[tokio::test]
async fn a_route_outside_the_web_group_gets_no_session_cookie() {
    // `/api/posts` is not behind `session`, and should not be allocating rows
    // or setting cookies for every anonymous API call.
    let app = App::boot().await;
    let response = app.send(app.get("/api/posts")).await;

    assert!(session_cookie(&response).is_none());
}

#[tokio::test]
async fn a_forged_session_cookie_is_replaced_rather_than_trusted() {
    let app = App::boot().await;

    let response = app
        .send(
            Request::builder()
                .method(Method::GET)
                .uri("/visits")
                .header("accept", "application/json")
                .header("cookie", "rainier_session=chosen-by-the-client")
                .build(),
        )
        .await;

    let issued = session_cookie(&response).expect("a fresh session");
    assert_ne!(issued, "chosen-by-the-client", "a client must not pick its own session id");
}

#[tokio::test]
async fn the_csrf_token_is_stable_within_a_session() {
    let app = App::boot().await;

    let first = app.send(app.get("/visits")).await;
    let cookie = session_cookie(&first).expect("a session cookie");
    let token = app
        .json(
            Request::builder()
                .method(Method::GET)
                .uri("/visits")
                .header("accept", "application/json")
                .header("cookie", &format!("rainier_session={cookie}"))
                .build(),
        )
        .await["csrf_token"]
        .clone();

    let again = app
        .json(
            Request::builder()
                .method(Method::GET)
                .uri("/visits")
                .header("accept", "application/json")
                .header("cookie", &format!("rainier_session={cookie}"))
                .build(),
        )
        .await["csrf_token"]
        .clone();

    assert_eq!(token, again, "rotating it per request would break every form");
}

#[tokio::test]
async fn encryption_is_wired_and_round_trips() {
    use rainier_framework::crypt::Encryption;

    let app = App::boot().await;
    let crypt = app.app.resolve::<Encryption>().expect("encryption should be bound");

    let sealed = crypt.encrypt("a card number").unwrap();
    assert!(!sealed.contains("card"), "{sealed}");
    assert_eq!(crypt.decrypt(&sealed).unwrap(), "a card number");

    let signed = crypt.sign("unsubscribe-42").unwrap();
    assert!(signed.starts_with("unsubscribe-42."), "signing leaves the value readable");
    assert_eq!(crypt.verify(&signed).unwrap(), "unsubscribe-42");
}
