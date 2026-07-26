//! `UserRepository` — the queries this application asks about users.

use std::ops::Deref;

use rainier_framework::prelude::*;

use crate::app::models::User;

/// Access to users.
pub struct UserRepository {
    inner: EntityRepository<User>,
}

impl UserRepository {
    /// A repository over `db`.
    pub fn new(db: Database) -> Self {
        Self { inner: EntityRepository::<User>::new(db) }
    }

    /// The user with this address.
    pub async fn by_email(&self, email: &str) -> Result<Option<User>> {
        self.inner.first_by("email", email.into()).await
    }
}

impl Deref for UserRepository {
    type Target = EntityRepository<User>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
