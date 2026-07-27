//! `routes/web.php` — pages a browser visits.

use rainier_framework::prelude::*;

use crate::app::http::controllers::{auth_controller, home_controller};
use crate::app::http::kernel;

/// Declare the web routes.
pub fn routes(router: &mut Router) {
    router.get("/", home_controller::index).name("home");
    router.get("/health", home_controller::health).name("health");
    router.get("/health/version", home_controller::version).name("health.version");

    // `kernel::web()` is security headers plus the session, so anything in here
    // has `request.session()`. A route outside it does not — which is the
    // point: an API endpoint should not be allocating session rows.
    //
    // It is a function call, not a name. Renaming it in the kernel breaks this
    // line in the compiler; misspelling it does not compile at all.
    router.group(GroupAttributes::new().middleware(kernel::web()), |router| {
        router
            .post("/login", auth_controller::login)
            .name("login")
            // Login is the endpoint worth rate-limiting hardest: it is where
            // credential stuffing goes.
            .middleware(kernel::throttle_writes(10));

        // An example of the session and its flash data.
        router.get("/visits", home_controller::visits).name("visits");
    });
}
