//! `routes/web.php` — pages a browser visits.

use rainier_framework::prelude::*;

use crate::app::http::controllers::{auth_controller, home_controller};
use crate::app::http::kernel;

/// Declare the web routes.
pub fn routes(router: &mut Router) {
    router.get("/", home_controller::index).name("home");

    // Vite's build output. Outside `kernel::web()` on purpose — a hashed
    // asset needs no session — and a 404 until `npm run build` has run.
    // No route for `/build/…`.
    //
    // There was one — an `AssetController` reading `public/build` with its own
    // traversal guard, because Rainier is the web server and somebody has to
    // serve the files Vite writes. The framework does it now: `public/` is
    // wired up at boot, and `public/build/assets/app-<hash>.js` is reachable
    // at `/build/assets/app-<hash>.js` without anybody saying so.
    //
    // Deleted rather than left beside it. Two things serving the same
    // directory means two traversal guards, and the second one is the one
    // nobody reviews.
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
