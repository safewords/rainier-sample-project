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
    Argon2Hasher, AuthManager, Hasher, RepositoryUserProvider, TokenGuard, UserProvider,
};
use rainier_framework::database::{EntityRepository, Migrator, Repository};
use rainier_framework::mail::{
    Address, FileTransport, LogTransport, Mailer, MemoryTransport, Transport,
};
use rainier_framework::notifications::DatabaseChannel;
use rainier_framework::notify::MailChannel;
use rainier_framework::prelude::*;
use rainier_framework::queue::{JobRegistry, MemoryQueue, Queue as QueueDriver, QueueManager};

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
        self.notifications(app);
        self.queue(app);

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
                "the cache is per-process, so `on_one_server` guarantees nothing — set CACHE_DRIVER to something shared before running the scheduler on more than one machine"
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
        // Production parameters are deliberately slow; a test suite that logs
        // in a few dozen times would crawl.
        app.instance(match self.mode {
            Mode::Testing => Argon2Hasher::insecure_for_tests(),
            _ => Argon2Hasher::new(),
        });
    }

    fn authentication(&self, app: &Application) {
        let db = self.database.clone();
        app.singleton(move |container: &Container| {
            let users: Arc<dyn Repository<User>> =
                Arc::new(EntityRepository::<User>::new(db.clone()));
            let hasher: Arc<dyn Hasher> = container.resolve::<Argon2Hasher>()?;

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
        let transport: Arc<dyn Transport> = match self.mode {
            Mode::Testing => {
                let memory = Arc::new(MemoryTransport::new());
                app.instance_arc(Arc::clone(&memory));
                memory
            }
            Mode::Running => {
                match Config::instance().get_or("mail.driver", "log".to_string()).as_str() {
                    "file" => Arc::new(FileTransport::new(
                        Config::instance().get_or("mail.file_path", "storage/mail".to_string()),
                    )?),
                    _ => Arc::new(LogTransport),
                }
            }
        };

        app.singleton(move |container: &Container| {
            let views = container.resolve::<rainier_framework::Views>()?;
            let config = container.resolve::<rainier_framework::config::Config>()?;

            Ok(Mailer::new(Arc::clone(views.engine()), Arc::clone(&transport))
                .with_events(container.resolve::<Dispatcher>()?)
                .with_default_from(Address::named(
                    config.get_or("mail.from.address", "hello@example.com".to_string()),
                    config.get_or("mail.from.name", "Rainier".to_string()),
                )))
        });
        Ok(())
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
            Ok(Notifier::new()
                .with_arc(container.resolve::<DatabaseChannel>()?)
                .with(MailChannel::new(container.resolve::<Mailer>()?)))
        });
    }

    fn queue(&self, app: &Application) {
        app.singleton(move |_: &Container| {
            // Every job the worker must be able to run. A job missing from
            // here fails at run time with "no job is registered as …", so add
            // to this list whenever you add a job.
            let registry = Arc::new(JobRegistry::new().with::<NotifyAuthor>());

            // In-memory: jobs are lost on restart and invisible to another
            // process. Swap for `DatabaseQueue::new(db)` — and add
            // `DatabaseQueue::migrations()` to `database/migrations.rs` — when
            // you want them to survive.
            let driver: Arc<dyn QueueDriver> = Arc::new(MemoryQueue::new());
            Ok(QueueManager::new(driver, registry))
        });
    }
}

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
