//! `RepositoryServiceProvider` — binds the [repositories](crate::app::repositories).
//!
//! Its own provider rather than a method on `AppServiceProvider`, for the same
//! reason the repositories got their own folder: the list of things this
//! application can query is worth being able to read on its own.

use std::sync::Arc;

use rainier_framework::events::Dispatcher;
use rainier_framework::prelude::*;

use crate::app::models::Tag;
use crate::app::repositories::{PostRepository, TagRepository, UserRepository};

/// Binds one repository per model.
pub struct RepositoryServiceProvider {
    /// The database, opened during bootstrap.
    pub database: Database,
}

impl ServiceProvider for RepositoryServiceProvider {
    fn name(&self) -> &'static str {
        "RepositoryServiceProvider"
    }

    fn register(&self, app: &Application) -> Result<()> {
        // `singleton`, not `instance`: the closure runs on first resolve, which
        // is after every provider has registered. `PostRepository` needs the
        // `Dispatcher`, and binding it eagerly here would resolve one before
        // the provider that binds it has run.
        let db = self.database.clone();
        app.singleton(move |container: &Container| {
            Ok(PostRepository::new(db.clone(), container.resolve::<Dispatcher>()?))
        });

        // No dispatcher: users have no lifecycle listeners in this application,
        // and wiring one that nothing listens to would clone every row for
        // nobody.
        let db = self.database.clone();
        app.singleton(move |_: &Container| Ok(UserRepository::new(db.clone())));

        // The bare `EntityRepository<Tag>`, still bound: a relationship loads
        // through the contract, and `Post::tags().load(..)` wants that one
        // rather than a newtype around it.
        let db = self.database.clone();
        app.singleton(move |_: &Container| Ok(EntityRepository::<Tag>::new(db.clone())));

        // And a newtype beside it, which this comment used to say would be
        // ceremony. It is not, any more: the tag cloud is a `GROUP BY` over the
        // `post_tag` pivot, and attaching a tag is an upsert on a two-column
        // conflict target. Neither is CRUD over `tags`, and neither belonged in
        // a controller.
        let db = self.database.clone();
        app.singleton(move |_: &Container| Ok(TagRepository::new(db.clone())));

        Ok(())
    }
}

/// The `Arc` the container hands back, spelled out because the closures above
/// are the only place the concrete types appear.
#[allow(dead_code, reason = "documentation of the bound types")]
type Bound =
    (Arc<PostRepository>, Arc<UserRepository>, Arc<EntityRepository<Tag>>, Arc<TagRepository>);

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::database::testing::{fake_database, MemoryConnection};
    use rainier_framework::database::Dialect;

    #[test]
    fn every_repository_resolves() {
        let (database, _) = fake_database(MemoryConnection::new(Dialect::Sqlite));

        let app = Application::new(".");
        app.instance(Dispatcher::new());
        RepositoryServiceProvider { database }.register(&app).unwrap();

        assert!(app.resolve::<PostRepository>().is_ok());
        assert!(app.resolve::<UserRepository>().is_ok());
        assert!(app.resolve::<EntityRepository<Tag>>().is_ok());
        assert!(app.resolve::<TagRepository>().is_ok());
    }

    #[test]
    fn a_repository_is_the_same_instance_every_time() {
        // `singleton`, not `bind`: a repository holding a connection pool that
        // was rebuilt per resolve would open one pool per request.
        let (database, _) = fake_database(MemoryConnection::new(Dialect::Sqlite));

        let app = Application::new(".");
        app.instance(Dispatcher::new());
        RepositoryServiceProvider { database }.register(&app).unwrap();

        let first = app.resolve::<UserRepository>().unwrap();
        let second = app.resolve::<UserRepository>().unwrap();
        assert!(Arc::ptr_eq(&first, &second));
    }
}
