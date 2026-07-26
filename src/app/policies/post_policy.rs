//! `PostPolicy` — `app/Policies/PostPolicy.php`.

use rainier_framework::auth::Gate;


use crate::app::models::{Post, User};

/// Who may do what to a post.
///
/// A [`Gate`] rather than scattered `if` statements: the rules live in one
/// place, and an **undefined** ability is denied — so a typo in an ability
/// name fails closed rather than opening a hole.
pub struct PostPolicy;

impl PostPolicy {
    /// Build the gate.
    ///
    /// Cheap to construct, so controllers call it per request. Move it into a
    /// container singleton if a policy ever grows expensive to build.
    pub fn gate() -> Gate<User> {
        Gate::new()
            // Runs before every ability: one place for "an admin may do
            // anything", instead of the same clause at the top of each rule.
            // Returning `None` defers to the ability.
            .before(|_user: &User, _ability: &str| None)
            .define("posts.update", |user: &User, post: Option<&Post>| {
                post.is_some_and(|post| post.belongs_to(user.id))
            })
            .define("posts.publish", |user: &User, post: Option<&Post>| {
                post.is_some_and(|post| post.belongs_to(user.id))
            })
            .define("posts.delete", |user: &User, post: Option<&Post>| {
                post.is_some_and(|post| post.belongs_to(user.id))
            })
            .define_simple("posts.create", |_user: &User| true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: u64) -> User {
        let mut user = User::new("Ada", "ada@example.com", String::new());
        user.id = id;
        user
    }

    #[test]
    fn only_the_author_may_change_a_post() {
        let gate = PostPolicy::gate();
        let mine = Post::draft("Mine", "body", 1);

        assert!(gate.allows("posts.publish", &user(1), Some(&mine)));
        assert!(gate.denies("posts.publish", &user(2), Some(&mine)));
        assert!(gate.denies("posts.delete", &user(2), Some(&mine)));
    }

    #[test]
    fn a_denial_is_a_403() {
        let theirs = Post::draft("Theirs", "body", 99);
        let err = PostPolicy::gate()
            .authorize("posts.publish", &user(1), Some(&theirs))
            .unwrap_err();

        assert_eq!(err.status(), 403);
    }

    #[test]
    fn an_undefined_ability_is_denied() {
        // Fails closed: a typo must not grant anything.
        assert!(PostPolicy::gate().denies::<Post>("posts.teleport", &user(1), None));
    }

    #[test]
    fn anyone_authenticated_may_create() {
        assert!(PostPolicy::gate().allows_any("posts.create", &user(1)));
    }
}
