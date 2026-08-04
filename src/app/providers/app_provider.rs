//! `AppServiceProvider` — `app/Providers/AppServiceProvider.php`.
//!
//! A provider has two passes, and the split is load-bearing:
//!
//! - [`register`](ServiceProvider::register) binds factories. It must **not**
//!   resolve anything, because the providers after it have not registered yet.
//! - [`boot`](ServiceProvider::boot) runs once every provider has registered,
//!   so it may resolve freely. It is `async`, which is what lets a provider
//!   open a pool or run migrations.

use std::sync::Arc;

use rainier_framework::auth::{
    AuthManager, Hasher, RepositoryUserProvider, TokenGuard, UserProvider,
};
use rainier_framework::broadcast::{Broadcasting, MemoryBroadcaster};
use rainier_framework::broadcasting::BroadcastChannel;
use rainier_framework::crypt::hash::{HashDriver, HashManager};
use rainier_framework::database::{EntityRepository, Migrator, Repository};
use rainier_framework::mail::{self, Mailer, MemoryTransport, Transport};
use rainier_framework::notifications::DatabaseChannel;
use rainier_framework::notify::MailChannel;
use rainier_framework::prelude::*;

use crate::app::jobs::NotifyAuthor;
use crate::app::models::{PostPublished, User};
use crate::bootstrap::Mode;
use crate::database::migrations;

/// Binds everything this application needs.
pub struct AppServiceProvider {
    /// How the application is wired — see [`Mode`].
    pub mode: Mode,
    /// The database, opened during bootstrap.
    pub database: Database,
}

impl ServiceProvider for AppServiceProvider {
    fn name(&self) -> &'static str {
        "AppServiceProvider"
    }

    fn register(&self, app: &Application) -> Result<()> {
        self.hashing(app);
        self.authentication(app);
        self.mail(app)?;
        self.broadcasting(app);
        self.notifications(app);
        // No `self.queue(app)`. See the note at the bottom of this file.

        app.instance(migrations::all());
        Ok(())
    }

    rainier_framework::container::boot_provider!(async |self, app| {
        // Migrating on boot is what makes a fresh clone run with no setup step.
        // It is a development convenience: in production you want one migration
        // step per deploy, not a race between starting instances.
        //
        // Skipped for the migration commands themselves, and that skip is not
        // cosmetic — without it `migrate:rollback` would re-apply everything
        // during boot and then undo the batch it had just created, and
        // `migrate` would always report "Nothing to migrate" because boot had
        // already done it.
        if running_a_migration_command() {
            return Ok(());
        }

        // `on_one_server` over a per-process cache is every machine holding
        // its own claim and each concluding it is the one. Saying so at boot
        // beats finding out from a digest that went to everybody three times.
        let locks = app.resolve::<rainier_framework::cache::LockManager>()?;
        if !locks.is_shared() {
            tracing::warn!(
                "the cache is per-process, so `on_one_server` guarantees nothing — set REDIS_URL, which declares the `shared` store in `config/cache.rs`, and point CACHE_STORE at it before running the scheduler on more than one machine"
            );
        }

        // Resolving is legal here and nowhere earlier.
        let database = app.resolve::<Database>()?;
        let migrator = app.resolve::<Migrator>()?;

        let applied = migrator.run(&database).await?;
        if !applied.is_empty() {
            tracing::info!(count = applied.len(), "applied migrations");
        }
        Ok(())
    });
}

/// Is the command being run one that manages migrations itself?
///
/// Reading `argv` in a provider is not something to make a habit of — a
/// provider that behaves differently depending on how the process was started
/// is a provider you cannot reason about. It earns its place here because the
/// thing it guards is *itself* the convenience, and the alternative is a
/// command that lies about what it did.
fn running_a_migration_command() -> bool {
    std::env::args().nth(1).is_some_and(|command| command.starts_with("migrate"))
}

impl AppServiceProvider {
    fn hashing(&self, app: &Application) {
        let mode = self.mode;

        // The manager, not a bare driver: `HASH_DRIVER` names what `hash`
        // writes, and verification dispatches on the stored hash's own prefix
        // regardless — so changing algorithm is a deploy, and rows convert on
        // their next successful login. The `bcrypt` cargo feature is what
        // puts that driver on the manager; selecting it without the feature
        // fails at boot naming it.
        //
        // A singleton rather than an instance because the driver comes from
        // configuration, and `register` must not resolve.
        app.singleton(move |container: &Container| {
            match mode {
                // Production parameters are deliberately slow; a test suite
                // that logs in a few dozen times would crawl.
                Mode::Testing => HashManager::insecure_for_tests(HashDriver::Argon2id),
                Mode::Running => {
                    let config = container.resolve::<rainier_framework::config::Config>()?;
                    HashManager::new(config.setting(crate::config::keys::HASH_DRIVER)?)
                }
            }
        });
    }

    fn authentication(&self, app: &Application) {
        let db = self.database.clone();
        app.singleton(move |container: &Container| {
            let users: Arc<dyn Repository<User>> =
                Arc::new(EntityRepository::<User>::new(db.clone()));
            let hasher: Arc<dyn Hasher> = container.resolve::<HashManager>()?;

            let provider: Arc<dyn UserProvider<User>> =
                Arc::new(RepositoryUserProvider::new(users, hasher));

            // One guard here. Add a `SessionGuard` under the name "web" for a
            // cookie-based front end, and routes can pick with `auth:web`.
            Ok(AuthManager::<User>::new("api").register(Arc::new(TokenGuard::new("api", provider))))
        });
    }

    fn mail(&self, app: &Application) -> Result<()> {
        // In testing the memory transport is *also* bound on its own, so a
        // test can resolve it and assert on what was sent — the mailer only
        // exposes it as `dyn Transport`.
        let captured: Option<Arc<MemoryTransport>> = match self.mode {
            Mode::Testing => {
                let memory = Arc::new(MemoryTransport::new());
                app.instance_arc(Arc::clone(&memory));
                Some(memory)
            }
            Mode::Running => None,
        };

        app.singleton(move |container: &Container| {
            let views = container.resolve::<rainier_framework::Views>()?;
            let config = container.resolve::<rainier_framework::config::Config>()?;
            let engine = Arc::clone(views.engine());

            // Running mode builds whatever `MAIL_DRIVER` names, from the
            // settings `config/mail.rs` read — the exhaustive match lives in
            // the framework, and a sender this build did not enable fails
            // here naming its cargo feature. Testing swaps in the captured
            // transport and keeps everything else the same.
            let mailer = match &captured {
                Some(memory) => {
                    mail::mailer_over(&config, engine, Arc::clone(memory) as Arc<dyn Transport>)
                }
                None => mail::mailer(&config, engine)?,
            };

            Ok(mailer.with_events(container.resolve::<Dispatcher>()?))
        });
        Ok(())
    }

    fn broadcasting(&self, app: &Application) {
        // The channel table. Without one every private channel is denied, so
        // a missing registration looks like a WebSocket that never connects
        // rather than like a leak.
        app.instance(crate::routes::channels::channels());

        match self.mode {
            // In testing the memory broadcaster is *also* bound on its own, so
            // a test can resolve it and assert on what was published — the
            // manager only exposes it as `dyn Broadcaster`.
            Mode::Testing => {
                let memory = Arc::new(MemoryBroadcaster::new());
                app.instance_arc(Arc::clone(&memory));
                app.instance(Broadcasting::new(memory));
            }
            // Otherwise the log, which reaches no browser. Swap in
            // `RedisBroadcaster::connect(..)` and point soketi at the same
            // Redis to go live — that one needs to connect, so it belongs in
            // `Rainier::with_broadcasting(..)` in `bootstrap.rs` rather than
            // here, where `register` may not await.
            Mode::Running => app.instance(Broadcasting::log()),
        }
    }

    fn notifications(&self, app: &Application) {
        // Bound on its own as well as into the notifier: reading the bell menu
        // — `unread`, `mark_read` — is the other half of storing rows, and a
        // controller needs a handle to do it.
        let database = self.database.clone();
        app.singleton(move |_: &Container| Ok(DatabaseChannel::new(database.clone())));

        app.singleton(move |container: &Container| {
            // The **same channels in every mode**, deliberately. A test that
            // ran a different set would not be testing what production does;
            // what differs is the mail transport underneath, which is already
            // in memory when testing.
            // Three channels, and each answers a different question. The
            // database one is what survives a reload; the broadcast one is
            // what makes the bell move without one; mail is what reaches
            // someone who has closed the tab.
            Ok(Notifier::new()
                .with_arc(container.resolve::<DatabaseChannel>()?)
                .with(BroadcastChannel::new(container.resolve::<Broadcasting>()?))
                .with(MailChannel::new(container.resolve::<Mailer>()?)))
        });
    }
}

// There is no `queue()` builder here any more, and its absence is the point —
// the same point the missing `storage()` in `bootstrap.rs` makes, and it was
// wrong here for one more reason.
//
// It read `QUEUE_DRIVER` out of the configuration and matched on it, arm by
// `#[cfg]`-gated arm, to build one connection. Two things were wrong with that.
//
// It could describe only a single destination, so `config/queue.rs` declaring
// `sync`, `database` and `bulk` had nowhere to land. And — this is the part
// that made it a defect rather than a limitation — **a provider binding a
// `QueueManager` silently wins over the declared section.** The framework
// builds the section's connections at boot and binds them; providers register
// afterwards, because a provider may legitimately resolve a queue. So the
// section was built, its connections opened, and then thrown away by this
// singleton.
//
// The failure that produced is the one `config/queue.rs` opens by describing:
// `QUEUE_CONNECTION=database` declared the default, the framework built it, and
// then every dispatch went inline through a `SyncQueue` anyway, because that is
// what `QUEUE_DRIVER` defaulted to. Nothing errored. Jobs ran in the request
// that dispatched them, and the only symptom was latency nobody attributed to a
// queue that everybody believed existed.
//
// `config/queue.rs` now declares what exists and the framework builds each
// connection from its own settings. The job *registry* moved with it, to
// `Rainier::with_jobs` in `bootstrap.rs`, because registering jobs in a
// provider does not reach the framework's queue either — for the same ordering
// reason.

/// `EventServiceProvider` — `app/Providers/EventServiceProvider.php`.
///
/// Listeners are registered during bootstrap rather than discovered, so the
/// wiring is explicit and the compiler checks it.
pub struct EventServiceProvider;

impl EventServiceProvider {
    /// Register this application's listeners.
    pub fn register_listeners(events: &Dispatcher) {
        // Two listeners on one event, and neither knows about the other —
        // which is the point of an event. The controller that published the
        // post knows about neither.
        events.listen(|event: Arc<PostPublished>| async move {
            tracing::info!(slug = %event.post.slug, title = %event.post.title, "post published");
            Ok(())
        });

        // The same fact, pushed to any browser watching. A separate listener
        // because it is a separate concern, and because a WebSocket relay
        // being down must not stop the author's notification being queued.
        events.listen(|event: Arc<PostPublished>| async move {
            Broadcast::instance().event(event.as_ref()).await
        });

        // Telling the author is *a reaction to* the fact, not the fact itself.
        // Queued rather than sent here, so a slow mail server cannot slow the
        // request that published the post — and so a failure is retried
        // instead of lost.
        events.listen(|event: Arc<PostPublished>| async move {
            Queue::instance().dispatch(NotifyAuthor { post_id: event.post.id }).await?;
            Ok(())
        });
    }
}
