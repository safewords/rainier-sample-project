//! `routes/web.php` — pages a browser visits.

use rainier_framework::prelude::*;

use crate::app::http::controllers::{auth_controller, home_controller};

/// Declare the web routes.
pub fn routes(router: &mut Router) {
    router.get("/", home_controller::index).name("home");
    router.get("/health", home_controller::health).name("health");

    // Public, but still behind the `web` group's security headers.
    router.group(GroupAttributes::new().middleware(["web"]), |router| {
        router.post("/login", auth_controller::login).name("login").middleware([
            // Login is the endpoint worth rate-limiting hardest: it is where
            // credential stuffing goes.
            "throttle-writes:10",
        ]);
    });
}
