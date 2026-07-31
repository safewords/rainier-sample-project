//! Service providers — `app/Providers`.
//!
//! Where services are bound into the container. Registered in
//! `src/bootstrap.rs`, in order.

pub mod app_provider;
pub mod repository_provider;

pub use app_provider::{AppServiceProvider, EventServiceProvider};
pub use repository_provider::RepositoryServiceProvider;

/// Create a user with a hashed password, and welcome them.
///
/// Lives here rather than in a controller because both the `app:seed` command
/// and the registration endpoint need it, and hashing a password in two places
/// is how one of them ends up wrong.
pub async fn register_user(
    app: &rainier_framework::Application,
    name: &str,
    email: &str,
    password: &str,
) -> rainier_framework::support::Result<crate::app::models::User> {
    use rainier_framework::auth::Hasher;
    use rainier_framework::crypt::hash::HashManager;
    use rainier_framework::database::Repository;

    let users = app.resolve::<crate::app::repositories::UserRepository>()?;
    let hasher = app.resolve::<HashManager>()?;

    let user =
        users.create(crate::app::models::User::new(name, email, hasher.hash(password)?)).await?;

    app.resolve::<rainier_framework::mail::Mailer>()?
        .send(&crate::app::mail::WelcomeMail { name: user.name.clone(), email: user.email.clone() })
        .await?;

    Ok(user)
}
