//! The console entry point — Laravel's `artisan`.
//!
//! ```sh
//! cargo run -- list                  # every command
//! cargo run -- serve                 # start the HTTP server
//! cargo run -- route:list            # the route table
//! cargo run -- migrate               # run pending migrations
//! cargo run -- queue:work --once     # drain the queue
//! cargo run -- app:seed              # your own command
//! ```

use app::{boot, Mode};
use rainier_framework::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Not `boot(..).await?`. Returning the error from `main` prints it with
    // `Debug`, and the commonest failure here is a misconfigured `.env` —
    // `CACHE_DRIVER=redys` deserves its own sentence, not
    // `Error { kind: Internal, message: "…", details: None, source: None }`.
    let application = match boot(Mode::Running).await {
        Ok(application) => application,
        Err(e) => {
            eprintln!("Rainier could not start: {}", e.message());
            std::process::exit(1);
        }
    };

    let code = app::routes::console::commands().run_from_env(&application).await;

    // Terminating hooks run after the command finishes — the place for work
    // that should not delay a response or a command's output.
    application.terminate();
    std::process::exit(code);
}
