//! Feature tests — Laravel's `tests/Feature`.
//!
//! These boot the real application and drive the real kernel: real routes,
//! real middleware, real database, real migrations. Only the mail transport is
//! a double, and only so a test can assert on what was sent.
//!
//! ```ignore
//! let app = App::boot().await;
//!
//! app.send(app.get("/health")).await.assert_ok().assert_json_path("status", "ok");
//! ```
//!
//! [`TestApp`] does the driving. What is left here is what is specific to
//! *this* application: how to boot it, how to log in, and which repositories a
//! test wants to look at afterwards.
//!
//! # Why the boot is still serialised
//!
//! `TestApp` scopes the facades to the thread it runs on, so two tests no
//! longer resolve out of each other's containers. Booting is the exception:
//! the bootstrap installs its application globally *before* the providers run,
//! because a provider legitimately reaches for a facade while it is being
//! registered — so two boots at the same instant can still cross. The lock is
//! therefore held for the boot and released immediately after, rather than
//! held for the whole test.

// The boot lock is deliberately held across the boot's awaits: serialising
// the boot is the whole point of it. Safe because `#[tokio::test]` runs on a
// current-thread runtime, so the guard never crosses a thread.
#![allow(clippy::await_holding_lock)]

use app::app::models::{Post, Tag, User};
use app::app::providers::register_user;
use app::app::repositories::{PostRepository, TagRepository};
use app::{boot, Mode};
use rainier_framework::database::Repository;
use rainier_framework::http::{Method, Request};
use rainier_framework::prelude::*;
use rainier_framework::testing::{TestApp, TestResponse};
use std::sync::Arc;

static BOOTING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A booted application plus the helpers a test needs to drive it.
struct App {
    app: TestApp,
}

impl App {
    async fn boot() -> Self {
        let app = {
            // Held across the boot only — see the module docs.
            let _booting = BOOTING.lock().unwrap_or_else(|e| e.into_inner());
            boot(Mode::Testing).await.expect("the application should boot")
        };

        Self { app: TestApp::new(app).expect("a kernel") }
    }

    /// The container, for resolving something to assert on.
    fn container(&self) -> &Arc<Application> {
        self.app.app()
    }

    /// Resolve a service out of this application.
    fn resolve<T: Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        self.app.resolve::<T>()
    }

    async fn send(&self, request: Request) -> TestResponse {
        self.app.send(request).await
    }

    async fn json(&self, request: Request) -> serde_json::Value {
        self.send(request).await.json()
    }

    fn get(&self, uri: &str) -> Request {
        Request::builder().method(Method::GET).uri(uri).header("accept", "application/json").build()
    }

    /// Register a user and log in, returning the API token.
    async fn login(&self) -> String {
        register_user(self.container(), "Ada Lovelace", "ada@example.com", "correct-horse")
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

    fn tags(&self) -> Arc<TagRepository> {
        self.app.resolve::<TagRepository>().expect("a tag repository")
    }

    fn broadcasts(&self) -> Arc<rainier_framework::broadcast::MemoryBroadcaster> {
        self.app.resolve().expect("the testing broadcaster")
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
    response.assert_ok();
}

#[tokio::test]
async fn the_health_check_says_what_it_is() {
    let app = App::boot().await;

    app.send(app.get("/health")).await.assert_ok().assert_json_path("status", "ok");
}

#[tokio::test]
async fn the_version_endpoint_names_the_build() {
    // The first question of every incident. `build_info!()` expands in this
    // application's crate, so the name is this application's.
    let app = App::boot().await;

    let response = app.send(app.get("/health/version")).await;

    response.assert_ok().assert_json_path("name", "app").assert_json_path("profile", "debug");

    // Absent rather than null when nothing set `GIT_SHA` — which is every
    // local build, and is the honest answer.
    if option_env!("GIT_SHA").is_none() && option_env!("GITHUB_SHA").is_none() {
        response.assert_json_missing("commit");
    }
}

#[tokio::test]
async fn the_home_page_renders_html() {
    let app = App::boot().await;
    let request = Request::builder().method(Method::GET).uri("/").build();
    let response = app.send(request).await;

    response.assert_ok();
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
    register_user(app.container(), "Ada", "ada@example.com", "correct-horse").await.unwrap();

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
    app.send(app.get("/api/me")).await.assert_unauthorized();
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

    app.send(app.authed(Method::POST, "/api/logout", &token).build()).await.assert_no_content();
    app.send(app.authed(Method::GET, "/api/me", &token).build()).await.assert_unauthorized();
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
    let author =
        register_user(app.container(), "Ada", "ada@example.com", "correct-horse").await.unwrap();
    let posts = app.posts();

    posts.create_unique(Post::draft("A draft", "body", author.id)).await.unwrap();
    let live = posts.create_unique(Post::draft("Published", "body", author.id)).await.unwrap();
    posts.publish(live.id).await.unwrap();

    let body = app.json(app.get("/api/posts")).await;
    assert_eq!(body["total"], 1);
    assert_eq!(body["data"][0]["post"]["title"], "Published");
}

#[tokio::test]
async fn the_index_loads_the_author_of_every_post_it_returns() {
    // The `belongs_to`, over a page: one query for every author on it.
    let app = App::boot().await;
    let author =
        register_user(app.container(), "Ada", "ada@example.com", "correct-horse").await.unwrap();
    let posts = app.posts();

    for title in ["One", "Two", "Three"] {
        let post = posts.create_unique(Post::draft(title, "body", author.id)).await.unwrap();
        posts.publish(post.id).await.unwrap();
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
    let author =
        register_user(app.container(), "Ada", "ada@example.com", "correct-horse").await.unwrap();
    let posts = app.posts();

    let post = posts.create_unique(Post::draft("Tagged", "body", author.id)).await.unwrap();
    posts.publish(post.id).await.unwrap();

    let tags = app.resolve::<EntityRepository<Tag>>().unwrap();
    let rust = tags.create(Tag::named("Rust")).await.unwrap();
    let laravel = tags.create(Tag::named("laravel")).await.unwrap();

    // This used to be `db.statement("INSERT INTO post_tag VALUES (…)")`, hand
    // formatted, because the pivot had no model to insert through. It has one
    // now — see `PostTag` — and `attach` is an upsert on the pair, so the second
    // call below is a no-op rather than a constraint violation.
    let links = app.tags();
    for tag in [&rust, &laravel] {
        links.attach(post.id, tag.id).await.unwrap();
        links.attach(post.id, tag.id).await.expect("attaching twice is not an error");
    }

    let body = app.json(app.get("/api/posts")).await;
    let names = body["data"][0]["tags"].as_array().unwrap();

    assert_eq!(names.len(), 2);
    assert!(names.iter().any(|name| name == "rust"), "{names:?}");

    // And the inverse: the same pivot, read from the tag's side.
    let with_posts = Tag::posts().load(std::slice::from_ref(&rust), &**posts).await.unwrap();
    assert_eq!(with_posts.of(&rust).len(), 1);
    assert_eq!(with_posts.one(&rust).unwrap().slug, "tagged");
    assert_eq!(Tag::route_key_name(), "name");
}

#[tokio::test]
async fn counting_a_relationship_does_not_load_it() {
    use rainier_framework::database::Relation;

    let app = App::boot().await;
    let author =
        register_user(app.container(), "Ada", "ada@example.com", "correct-horse").await.unwrap();
    let posts = app.posts();

    for title in ["One", "Two"] {
        posts.create_unique(Post::draft(title, "body", author.id)).await.unwrap();
    }

    let counts = User::posts().count(std::slice::from_ref(&author), &**posts).await.unwrap();

    assert_eq!(counts.of(&author), 2);
    assert_eq!(counts.total(), 2);
}

#[tokio::test]
async fn an_unpublished_post_is_a_404_rather_than_a_leak() {
    let app = App::boot().await;
    let author =
        register_user(app.container(), "Ada", "ada@example.com", "correct-horse").await.unwrap();
    app.posts().create_unique(Post::draft("Secret Draft", "body", author.id)).await.unwrap();

    app.send(app.get("/api/posts/secret-draft")).await.assert_not_found();
}

#[tokio::test]
async fn a_slug_collision_gets_a_suffix() {
    let app = App::boot().await;
    let author =
        register_user(app.container(), "Ada", "ada@example.com", "correct-horse").await.unwrap();
    let posts = app.posts();

    let first = posts.create_unique(Post::draft("Same Title", "b", author.id)).await.unwrap();
    let second = posts.create_unique(Post::draft("Same Title", "b", author.id)).await.unwrap();

    assert_eq!(first.slug, "same-title");
    assert_eq!(second.slug, "same-title-2");
}

#[tokio::test]
async fn a_slug_constraint_rejects_a_non_slug() {
    let app = App::boot().await;
    app.send(app.get("/api/posts/not%20a%20slug")).await.assert_not_found();
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
    response.assert_ok();

    let queue = app.resolve::<rainier_framework::queue::QueueManager>().unwrap();
    assert_eq!(queue.queue().size("mail").await.unwrap(), 1);
}

/// Drain the `mail` queue, as `queue:work` would.
async fn work_the_mail_queue(app: &App) -> rainier_framework::queue::WorkerStats {
    let manager = app.resolve::<rainier_framework::queue::QueueManager>().unwrap();
    let worker = rainier_framework::queue::Worker::new(
        Arc::clone(manager.queue()),
        Arc::clone(manager.registry()),
        Arc::clone(app.container().container()),
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

    let stored = app.resolve::<rainier_framework::notifications::DatabaseChannel>().unwrap();
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
    response.assert_no_content();

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

    response.assert_not_found();
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

    let stored = app.resolve::<rainier_framework::notifications::DatabaseChannel>().unwrap();
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

    let queue = app.resolve::<rainier_framework::queue::QueueManager>().unwrap();
    assert_eq!(queue.queue().size("mail").await.unwrap(), 1);
}

#[tokio::test]
async fn publishing_broadcasts_the_fact_to_the_public_channel() {
    let app = App::boot().await;
    let token = app.login().await;
    create_post(&app, &token, "Going live").await;

    app.send(app.authed(Method::POST, "/api/posts/going-live/publish", &token).build()).await;

    app.broadcasts().assert_broadcast("post.published", "posts");
    let sent = app.broadcasts().sent();
    assert_eq!(sent[0].payload["slug"], "going-live");
    assert!(sent[0].payload.get("body").is_none(), "a public channel gets no body");
}

#[tokio::test]
async fn a_notification_is_broadcast_to_its_recipients_own_channel() {
    let app = App::boot().await;
    let token = app.login().await;
    create_post(&app, &token, "Going live").await;
    app.send(app.authed(Method::POST, "/api/posts/going-live/publish", &token).build()).await;

    work_the_mail_queue(&app).await;

    app.broadcasts().assert_broadcast("post.live", "private-notifications.User.1");
}

#[tokio::test]
async fn the_auth_endpoint_signs_a_channel_you_are_allowed_on() {
    let app = App::boot().await;
    let token = app.login().await;

    let response = app
        .send(
            app.authed(Method::POST, "/api/broadcasting/auth", &token)
                .json(&serde_json::json!({
                    "socket_id": "1234.5678",
                    "channel_name": "private-notifications.User.1",
                }))
                .build(),
        )
        .await;

    // The grant is the 200. The memory driver signs nothing — there is no
    // relay to convince — so the body is empty; with a Pusher-protocol relay
    // configured it would carry the HMAC.
    response.assert_ok();
}

#[tokio::test]
async fn the_auth_endpoint_refuses_someone_elses_channel() {
    let app = App::boot().await;
    let token = app.login().await;

    let response = app
        .send(
            app.authed(Method::POST, "/api/broadcasting/auth", &token)
                .json(&serde_json::json!({
                    "socket_id": "1234.5678",
                    "channel_name": "private-notifications.User.999",
                }))
                .build(),
        )
        .await;

    response.assert_forbidden();
}

#[tokio::test]
async fn the_auth_endpoint_refuses_a_channel_nobody_declared() {
    // Failing closed: a pattern that does not exist is denied, not allowed.
    let app = App::boot().await;
    let token = app.login().await;

    let response = app
        .send(
            app.authed(Method::POST, "/api/broadcasting/auth", &token)
                .json(&serde_json::json!({
                    "socket_id": "1234.5678",
                    "channel_name": "private-payroll.1",
                }))
                .build(),
        )
        .await;

    response.assert_forbidden();
}

#[tokio::test]
async fn the_auth_endpoint_is_behind_the_guard() {
    let app = App::boot().await;

    let response = app
        .send(
            Request::builder()
                .method(Method::POST)
                .uri("/api/broadcasting/auth")
                .json(&serde_json::json!({ "socket_id": "1.1", "channel_name": "private-x.1" }))
                .build(),
        )
        .await;

    response.assert_unauthorized();
}

#[tokio::test]
async fn the_policy_stops_you_touching_someone_elses_post() {
    let app = App::boot().await;
    let token = app.login().await;

    let other = register_user(app.container(), "Grace", "grace@example.com", "another-password")
        .await
        .unwrap();
    app.posts().create_unique(Post::draft("Not Yours", "body", other.id)).await.unwrap();

    for uri in ["/api/posts/not-yours/publish"] {
        app.send(app.authed(Method::POST, uri, &token).build()).await.assert_forbidden();
    }
    app.send(app.authed(Method::DELETE, "/api/posts/not-yours", &token).build())
        .await
        .assert_forbidden();
}

#[tokio::test]
async fn registering_sends_a_welcome_email() {
    let app = App::boot().await;
    register_user(app.container(), "Ada", "ada@example.com", "correct-horse").await.unwrap();

    let sent = app.mail().sent();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].envelope.subject.contains("Ada"));
    assert_eq!(sent[0].envelope.from.as_ref().unwrap().email, "hello@example.com");
}

// --- routing and middleware -------------------------------------------------

#[tokio::test]
async fn named_routes_generate_urls() {
    let app = App::boot().await;
    let urls = app.resolve::<rainier_framework::routing::UrlGenerator>().unwrap();

    assert_eq!(urls.route("api.posts.index", &[]).unwrap(), "/api/posts");
    assert_eq!(urls.route("api.posts.show", &[("post", "hello")]).unwrap(), "/api/posts/hello");
    assert_eq!(urls.route("login", &[]).unwrap(), "/login");
}

#[tokio::test]
async fn the_api_group_adds_cors_and_rate_limit_headers() {
    // A declared origin, answered with itself rather than with `*`. This
    // asserted `Some("*")` until `config/cors.rs` existed, and the assertion
    // passed for exactly as long as no browser client tried to authenticate.
    let app = App::boot().await;
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/posts")
        .header("origin", "https://example.com")
        .header("accept", "application/json")
        .build();

    let response = app.send(request).await;

    assert_eq!(response.header("access-control-allow-origin"), Some("https://example.com"));
    // The half that makes the first half worth having: without this a browser
    // sends no cookie, so a session-authenticated call arrives anonymous.
    assert_eq!(response.header("access-control-allow-credentials"), Some("true"));
    // And a cache must not hand one origin's answer to another, now that the
    // answer differs per origin.
    assert!(response.header("vary").unwrap_or_default().contains("Origin"));
    assert!(response.header("x-ratelimit-limit").is_some());
}

#[tokio::test]
async fn an_undeclared_origin_gets_no_cors_headers_and_is_served_anyway() {
    // Both halves are the point. The missing `Access-Control-Allow-Origin` is
    // what makes a browser refuse to hand the body to the calling page — and
    // the `200` is the reminder that nothing on the server refused anything.
    // CORS is a browser rule, so a tool that ignores it sees a working
    // endpoint, which is why "it works in curl" settles nothing here.
    let app = App::boot().await;
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/posts")
        .header("origin", "https://somewhere.example")
        .header("accept", "application/json")
        .build();

    let response = app.send(request).await;

    response.assert_status(StatusCode::OK);
    assert_eq!(response.header("access-control-allow-origin"), None);
    assert_eq!(response.header("access-control-allow-credentials"), None);
}

#[tokio::test]
async fn a_route_that_does_not_exist_still_answers_with_the_cors_headers() {
    // The other thing a global policy buys, and it is worth an assertion
    // because of how the failure is read. A `404` with no
    // `Access-Control-Allow-Origin` is reported by the browser as a CORS
    // error, so a mistyped path sends whoever is debugging it into
    // `config/cors.rs` — where everything is correct.
    let app = App::boot().await;
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/postz")
        .header("origin", "https://example.com")
        .header("accept", "application/json")
        .build();

    let response = app.send(request).await;

    response.assert_status(StatusCode::NOT_FOUND);
    assert_eq!(response.header("access-control-allow-origin"), Some("https://example.com"));
}

#[tokio::test]
async fn a_preflight_is_answered_by_the_middleware_and_allows_the_token_header() {
    // `OPTIONS /api/posts` matches no route — the middleware short-circuits it,
    // which is why a preflight never reaches a controller.
    //
    // `authorization` is the entry that has to be in the answer. A browser will
    // not send it uninvited, and a preflight that omits it does not make the
    // client drop the header and carry on: the request is never sent at all.
    let app = App::boot().await;
    let request = Request::builder()
        .method(Method::OPTIONS)
        .uri("/api/posts")
        .header("origin", "https://example.com")
        .header("access-control-request-method", "POST")
        .header("access-control-request-headers", "authorization")
        .build();

    let response = app.send(request).await;

    response.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(response.header("access-control-allow-origin"), Some("https://example.com"));
    assert_eq!(response.header("access-control-allow-credentials"), Some("true"));
    assert!(response.header("access-control-allow-headers").unwrap().contains("authorization"));
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

    response.assert_status(StatusCode::METHOD_NOT_ALLOWED);
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

    app::database::seeders::seed(app.container()).await.unwrap();
    let after_first = app.posts().count().await.unwrap();
    assert!(after_first > 0);

    app::database::seeders::seed(app.container()).await.unwrap();
    assert_eq!(app.posts().count().await.unwrap(), after_first, "running twice adds nothing");
}

// --- sessions and encryption -------------------------------------------------

/// The session cookie a response set, if any.
fn session_cookie(response: &TestResponse) -> Option<String> {
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
    let crypt = app.resolve::<Encryption>().expect("encryption should be bound");

    let sealed = crypt.encrypt("a card number").unwrap();
    assert!(!sealed.contains("card"), "{sealed}");
    assert_eq!(crypt.decrypt(&sealed).unwrap(), "a card number");

    let signed = crypt.sign("unsubscribe-42").unwrap();
    assert!(signed.starts_with("unsubscribe-42."), "signing leaves the value readable");
    assert_eq!(crypt.verify(&signed).unwrap(), "unsubscribe-42");
}

// --- soft deletes -----------------------------------------------------------

#[tokio::test]
async fn binning_a_post_hides_it_everywhere_without_losing_it() {
    // The whole point of the scope, and the reason it is worth an automatic
    // predicate rather than a remembered one: *every* read has to hide the row,
    // and a listing that forgot would look exactly like one that did not.
    let app = App::boot().await;
    let token = app.login().await;

    let created = app
        .json(
            app.authed(Method::POST, "/api/posts", &token)
                .json(&serde_json::json!({ "title": "Binned", "body": "Long enough to pass." }))
                .build(),
        )
        .await;
    let slug = created["slug"].as_str().unwrap().to_string();

    app.send(app.authed(Method::POST, &format!("/api/posts/{slug}/publish"), &token).build())
        .await
        .assert_ok();

    // Visible: on the listing and by its own URL.
    assert_eq!(app.json(app.get("/api/posts")).await["total"], 1);
    app.send(app.get(&format!("/api/posts/{slug}"))).await.assert_ok();

    app.send(app.authed(Method::DELETE, &format!("/api/posts/{slug}"), &token).build())
        .await
        .assert_status(StatusCode::NO_CONTENT);

    // Gone from the listing, and gone from route-model binding — which is a
    // read like any other, so `Bound<Post>` finds nothing and answers 404.
    assert_eq!(app.json(app.get("/api/posts")).await["total"], 0);
    app.send(app.get(&format!("/api/posts/{slug}"))).await.assert_not_found();

    // And from the count, which is a different builder and would have needed
    // its own predicate under a manual scheme.
    assert_eq!(app.posts().count().await.unwrap(), 0);

    // The row is still there. `with_trashed` is what proves the delete was
    // soft: without it this assertion cannot be written at all.
    let surviving = app
        .posts()
        .matching(Criteria::new().where_eq("slug", slug.clone()).with_trashed())
        .await
        .unwrap();
    assert_eq!(surviving.len(), 1, "a soft delete leaves the row");
    assert!(surviving[0].deleted_at.is_some(), "and stamps it");
}

#[tokio::test]
async fn the_bin_lists_what_is_in_it_and_a_restore_brings_it_back() {
    // The direction that turning the scope on breaks. Under the scope this
    // listing returns nothing — not an error, an empty page — so `only_trashed`
    // is the whole method, and the restore reaches a row no read can see.
    let app = App::boot().await;
    let token = app.login().await;

    let created = app
        .json(
            app.authed(Method::POST, "/api/posts", &token)
                .json(&serde_json::json!({ "title": "Recoverable", "body": "Long enough here." }))
                .build(),
        )
        .await;
    let slug = created["slug"].as_str().unwrap().to_string();

    // Nothing in the bin yet.
    assert_eq!(
        app.json(app.authed(Method::GET, "/api/posts/trashed", &token).build()).await["data"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    app.send(app.authed(Method::DELETE, &format!("/api/posts/{slug}"), &token).build())
        .await
        .assert_status(StatusCode::NO_CONTENT);

    let bin = app.json(app.authed(Method::GET, "/api/posts/trashed", &token).build()).await;
    assert_eq!(bin["data"].as_array().unwrap().len(), 1);
    assert_eq!(bin["data"][0]["slug"], slug.as_str());

    app.send(app.authed(Method::POST, &format!("/api/posts/{slug}/restore"), &token).build())
        .await
        .assert_ok();

    // Back to being an ordinary draft: readable by the repository, and out of
    // the bin.
    assert_eq!(app.posts().count().await.unwrap(), 1);
    assert_eq!(
        app.json(app.authed(Method::GET, "/api/posts/trashed", &token).build()).await["data"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn a_binned_slug_is_still_taken() {
    // The least obvious consequence of the scope, and the one that would have
    // shipped: the unique index does not know about it, so a probe that read
    // through the scope would find the slug free and hand the insert into a
    // constraint violation. `create_unique` uses `with_trashed` for exactly
    // this.
    let app = App::boot().await;
    let author =
        register_user(app.container(), "Ada", "ada@example.com", "correct-horse").await.unwrap();
    let posts = app.posts();

    let first = posts.create_unique(Post::draft("Same title", "body", author.id)).await.unwrap();
    assert_eq!(first.slug, "same-title");

    posts.trash(first.id).await.unwrap();

    // A read cannot see the first post at all, and the insert still has to
    // avoid its slug.
    let second = posts.create_unique(Post::draft("Same title", "body", author.id)).await.unwrap();
    assert_eq!(second.slug, "same-title-2", "a binned row still holds its slug");
}

#[tokio::test]
async fn binning_the_same_post_twice_does_not_move_its_tombstone() {
    // The write is unscoped, so the second call *can* reach the row. What stops
    // it is the criteria's own `where_null`, and without it a retried request
    // would push the retention clock forward every time.
    let app = App::boot().await;
    let author =
        register_user(app.container(), "Ada", "ada@example.com", "correct-horse").await.unwrap();
    let posts = app.posts();

    let post = posts.create_unique(Post::draft("Twice", "body", author.id)).await.unwrap();

    assert!(posts.trash(post.id).await.unwrap(), "the first call bins it");
    let stamped = posts.trashed_for_author(author.id).await.unwrap()[0].deleted_at;

    assert!(!posts.trash(post.id).await.unwrap(), "the second call is a no-op");
    assert_eq!(
        posts.trashed_for_author(author.id).await.unwrap()[0].deleted_at,
        stamped,
        "the tombstone did not move"
    );
}

// --- partial updates --------------------------------------------------------

#[tokio::test]
async fn publishing_writes_one_column_and_leaves_a_concurrent_edit_alone() {
    // The failure `update(&model)` produces, demonstrated rather than described:
    // the publish request holds a copy of the post from before the rename, and
    // a full-row write would put the old title back. Nothing would error and one
    // row would be reported affected, which is what a correct write reports too.
    let app = App::boot().await;
    let token = app.login().await;

    let created = app
        .json(
            app.authed(Method::POST, "/api/posts", &token)
                .json(&serde_json::json!({ "title": "Draft title", "body": "Long enough here." }))
                .build(),
        )
        .await;
    let slug = created["slug"].as_str().unwrap().to_string();
    let id = created["id"].as_u64().unwrap();

    // Somebody renames it between the read the publish request did and the
    // write it is about to do.
    let posts = app.posts();
    posts.update_column(Criteria::new().where_eq("id", id), "title", "Real title").await.unwrap();

    app.send(app.authed(Method::POST, &format!("/api/posts/{slug}/publish"), &token).build())
        .await
        .assert_ok();

    let stored = posts.find(id.into()).await.unwrap().expect("the post");
    assert!(stored.published, "the column the caller named was written");
    assert_eq!(stored.title, "Real title", "the column it did not name was left alone");
}

#[tokio::test]
async fn publishing_twice_publishes_once() {
    // The guard lives in the statement's own criteria rather than in an `if`
    // between a read and a write, so two requests cannot both conclude they
    // were the one that published it — and only one dispatches the author's
    // notification.
    let app = App::boot().await;
    let token = app.login().await;

    let created = app
        .json(
            app.authed(Method::POST, "/api/posts", &token)
                .json(&serde_json::json!({ "title": "Once", "body": "Long enough to pass." }))
                .build(),
        )
        .await;
    let slug = created["slug"].as_str().unwrap().to_string();
    let id = created["id"].as_u64().unwrap();

    assert!(app.posts().publish(id).await.unwrap(), "the first call publishes");
    assert!(!app.posts().publish(id).await.unwrap(), "the second finds nothing to do");

    // And through the endpoint, which is what a double-click actually hits.
    app.send(app.authed(Method::POST, &format!("/api/posts/{slug}/publish"), &token).build())
        .await
        .assert_ok();
}

// --- correlated subqueries and the pivot ------------------------------------

#[tokio::test]
async fn filtering_by_tag_returns_only_the_tagged_posts_and_counts_them_once() {
    // Two claims, and the second is why this is `EXISTS` rather than a join. A
    // join over a many-to-many multiplies rows, so a post with two tags would
    // appear twice and `total` would count the duplicates.
    let app = App::boot().await;
    let author =
        register_user(app.container(), "Ada", "ada@example.com", "correct-horse").await.unwrap();
    let posts = app.posts();
    let tags = app.tags();

    let tagged = posts.create_unique(Post::draft("Tagged one", "body", author.id)).await.unwrap();
    let untagged = posts.create_unique(Post::draft("Plain one", "body", author.id)).await.unwrap();
    for post in [&tagged, &untagged] {
        posts.publish(post.id).await.unwrap();
    }

    let rust = tags.create(Tag::named("rust")).await.unwrap();
    let orm = tags.create(Tag::named("orm")).await.unwrap();
    // Two tags on one post: the multiplication a join would produce.
    tags.attach(tagged.id, rust.id).await.unwrap();
    tags.attach(tagged.id, orm.id).await.unwrap();

    let filtered = app.json(app.get("/api/posts?tag=rust")).await;
    assert_eq!(filtered["total"], 1, "one post, not one row per link");
    assert_eq!(filtered["data"][0]["post"]["slug"], "tagged-one");

    // Unfiltered still sees both, so the subquery is a filter rather than a
    // join that dropped a row.
    assert_eq!(app.json(app.get("/api/posts")).await["total"], 2);

    // A tag nobody used filters to nothing rather than to everything, which is
    // what a dropped correlation would produce.
    assert_eq!(app.json(app.get("/api/posts?tag=nothing-here")).await["total"], 0);
}

#[tokio::test]
async fn the_tag_cloud_counts_only_posts_a_reader_could_reach() {
    // A `GROUP BY` over a table keyed on two columns — reachable only because
    // the pivot is an entity and `aggregate_rows` is `Entity`-bound. The
    // `EXISTS` is what keeps drafts and binned posts out of the count, so a tag
    // cannot advertise a number that leads to an empty page.
    let app = App::boot().await;
    let author =
        register_user(app.container(), "Ada", "ada@example.com", "correct-horse").await.unwrap();
    let posts = app.posts();
    let tags = app.tags();

    let live = posts.create_unique(Post::draft("Live", "body", author.id)).await.unwrap();
    let draft = posts.create_unique(Post::draft("Draft", "body", author.id)).await.unwrap();
    let binned = posts.create_unique(Post::draft("Binned", "body", author.id)).await.unwrap();

    posts.publish(live.id).await.unwrap();
    posts.publish(binned.id).await.unwrap();
    posts.trash(binned.id).await.unwrap();

    let rust = tags.create(Tag::named("rust")).await.unwrap();
    for post in [&live, &draft, &binned] {
        tags.attach(post.id, rust.id).await.unwrap();
    }

    let cloud = tags.cloud().await.unwrap();

    assert_eq!(cloud.len(), 1, "one tag is in use");
    assert_eq!(cloud[0].tag_id, rust.id);
    assert_eq!(cloud[0].posts, 1, "the draft and the binned post are not reachable");
}

// ---- public/ -------------------------------------------------------------

#[tokio::test]
async fn a_file_in_public_is_served_without_a_route() {
    // The whole point of a document root: `public/robots.txt` is reachable at
    // `/robots.txt` because it is there, not because anything declared it.
    let app = App::boot().await;

    let response = app.send(app.get("/robots.txt")).await;

    response.assert_ok();
}

#[tokio::test]
async fn the_built_frontend_is_served_from_public() {
    // This used to be `AssetController`, a route with its own traversal guard,
    // because Rainier is the web server and somebody had to serve what Vite
    // writes. The framework does it now and the URL is unchanged:
    // `public/build/manifest.json` answers at `/build/manifest.json`.
    let app = App::boot().await;

    let response = app.send(app.get("/build/manifest.json")).await;

    response.assert_ok();
}

#[tokio::test]
async fn a_route_wins_over_a_file_of_the_same_name() {
    // The one behavioural difference from Laravel, asserted rather than
    // described: `public/` is the router's fallback, so a declared route is
    // reached first and a file cannot shadow it.
    let app = App::boot().await;

    let response = app.send(app.get("/health")).await;

    response.assert_ok().assert_json_path("status", "ok");
}

#[tokio::test]
async fn nothing_under_public_climbs_out_of_it() {
    // Through the real router and the real kernel, not just the resolver's
    // unit tests: a percent-encoded traversal has to survive whatever the
    // HTTP layer does to a path before this ever sees it.
    let app = App::boot().await;

    for hostile in ["/../Cargo.toml", "/%2e%2e/Cargo.toml", "/build/../../Cargo.toml"] {
        let response = app.send(app.get(hostile)).await;
        assert_ne!(response.status(), 200, "{hostile} was served");
    }
}

#[tokio::test]
async fn a_dotfile_in_public_is_not_served() {
    // `.env` sits in the project root rather than `public/`, so this asserts
    // the rule at the door: a request naming one is refused before anything
    // looks for it.
    let app = App::boot().await;

    let response = app.send(app.get("/.env")).await;

    assert_ne!(response.status(), 200);
}
