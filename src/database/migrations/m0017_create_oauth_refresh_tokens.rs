//! `0017_create_oauth_refresh_tokens` — issued refresh tokens, for the add-ins.

use rainier_framework::database::Step;

use crate::app::models::OAuthRefreshToken;

/// Create `oauth_refresh_tokens` from [`OAuthRefreshToken`].
///
/// Run in every build, whether or not the add-in that writes it is compiled
/// in — the same reasoning as the queue's tables in [`all`](super::all): an
/// empty table costs nothing, and a feature that needed a migration to turn
/// on would make enabling it a deploy with a schema change.
pub fn migration() -> Step {
    Step::create_table::<OAuthRefreshToken>("0017_create_oauth_refresh_tokens")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::database::{Dialect, Migration};

    #[test]
    fn it_creates_its_table_and_can_drop_it() {
        let up = migration().up(Dialect::Sqlite).join(
            "
",
        );
        assert!(up.contains("oauth_refresh_tokens"), "{up}");
        assert!(migration().down(Dialect::Sqlite).is_reversible());
    }
}
