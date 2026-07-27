//! `config/server.php` — how the HTTP server itself behaves.
//!
//! Laravel has no equivalent: PHP's `max_execution_time` and nginx's
//! `gzip on;` live outside the application. Rainier *is* the server, so the
//! two settings that would have belonged to nginx belong here.

use rainier_framework::config::{Config, Env};
use rainier_framework::prelude::*;

use crate::config::keys::{SERVER_COMPRESSION, SERVER_REQUEST_TIMEOUT_SECS};

/// Server settings, on top of the host and port the framework already read.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // A ceiling on how long a handler may take before the request is cancelled
    // and answered `408`. Without one, a handler that never returns holds its
    // connection and its task for as long as the process lives — and enough of
    // those is a service that has stopped answering anything.
    //
    // Thirty seconds is generous for this application: every route here is a
    // database read or a template render. A route that legitimately takes
    // longer should carry its own `Timeout`, rather than this being raised for
    // everything.
    //
    // It bounds the *handler*, not the response body, so the server-sent
    // events route is unaffected — it returns immediately and streams
    // afterwards.
    config.set(SERVER_REQUEST_TIMEOUT_SECS, env.int("SERVER_REQUEST_TIMEOUT", 30).max(0) as u64)?;

    // Off by default, because the usual deployment has nginx or a CDN in
    // front and compressing twice is CPU spent to produce the same bytes.
    // Turn it on when Rainier is what clients talk to directly.
    config.set(SERVER_COMPRESSION, env.bool("SERVER_COMPRESSION", false))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_is_a_timeout_by_default() {
        // The framework defaults this to off, because it cannot know what is
        // reasonable. An application can.
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.get(SERVER_REQUEST_TIMEOUT_SECS), Some(30));
    }

    #[test]
    fn a_deployment_can_turn_the_timeout_off() {
        let config = Config::new();
        configure(&config, &Env::parse("SERVER_REQUEST_TIMEOUT=0")).unwrap();

        assert_eq!(config.get(SERVER_REQUEST_TIMEOUT_SECS), Some(0));
    }

    #[test]
    fn a_negative_timeout_is_off_rather_than_enormous() {
        // `-1` as a u64 would be 18 quintillion seconds, which is off with
        // extra steps and a much worse log line.
        let config = Config::new();
        configure(&config, &Env::parse("SERVER_REQUEST_TIMEOUT=-1")).unwrap();

        assert_eq!(config.get(SERVER_REQUEST_TIMEOUT_SECS), Some(0));
    }

    #[test]
    fn compression_is_off_until_a_deployment_asks() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();
        assert_eq!(config.get(SERVER_COMPRESSION), Some(false));

        let config = Config::new();
        configure(&config, &Env::parse("SERVER_COMPRESSION=true")).unwrap();
        assert_eq!(config.get(SERVER_COMPRESSION), Some(true));
    }
}
