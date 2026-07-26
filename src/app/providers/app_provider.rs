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
use rainier_framework::mail::{Address, FileTransport, LogTransport, Mailer, MemoryTransport, Transport};
use rainier_framework::prelude::*;
use rainier_framework::queue::{JobRegistry, MemoryQueue, Queue as QueueDriver, QueueManager};

use crate::app::jobs::NotifyAuthor;
use crate::app::models::{PostPublished, User};
use crate::app::providers::{PostRepository, UserRepository};
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
        self.repositories(app);
        self.authentication(app);
        self.mail(app)?;
        self.queue(app);

        app.instance(migrations::all());
        Ok(())
    }

    rainier_framework::container::boot_provider!(async |self, app| {
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

impl AppServiceProvider {
    fn hashing(&self, app: &Application) {
        // Production parameters are deliberately slow; a test suite that logs
        // in a few dozen times would crawl.
        app.instance(match self.mode {
            Mode::Testing => Argon2Hasher::insecure_for_tests(),
            _ => Argon2Hasher::new(),
        });
    }

    fn repositories(&self, app: &Application) {
        let db = self.database.clone();
        app.singleton(move |container: &Container| {
            Ok(PostRepository::new(db.clone(), container.resolve::<Dispatcher>()?))
        });

        let db = self.database.clone();
        app.singleton(move |_: &Container| Ok(UserRepository::new(db.clone())));
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
            Ok(AuthManager::<User>::new("api")
                .register(Arc::new(TokenGuard::new("api", provider))))
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
            Mode::Running => match Config::instance().get_or("mail.driver", "log".to_string()).as_str() {
                "file" => Arc::new(FileTransport::new(
                    Config::instance().get_or("mail.file_path", "storage/mail".to_string()),
                )?),
                _ => Arc::new(LogTransport),
            },
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
        events.listen(|event: Arc<PostPublished>| async move {
            tracing::info!(slug = %event.post.slug, title = %event.post.title, "post published");
            Ok(())
        });
    }
}
