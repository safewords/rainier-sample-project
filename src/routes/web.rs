//! `routes/web.php` — pages a browser visits.

use rainier_framework::prelude::*;

use crate::app::http::controllers::{auth_controller, home_controller};

/// Declare the web routes.
pub fn routes(router: &mut Router) {
    router.get("/", home_controller::index).name("home");
    router.get("/health", home_controller::health).name("health");

    // The `web` group is security headers plus `session`, so anything in here
    // has `request.session()`. A route outside it does not — which is the
    // point: an API endpoint should not be allocating session rows.
    router.group(GroupAttributes::new().middleware(["web"]), |router| {
        router.post("/login", auth_controller::login).name("login").middleware([
            // Login is the endpoint worth rate-limiting hardest: it is where
            // credential stuffing goes.
            "throttle-writes:10",
        ]);

        // An example of the session and its flash data.
        router.get("/visits", home_controller::visits).name("visits");
    });
}
