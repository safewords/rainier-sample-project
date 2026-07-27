//! `config/openapi.php` — the generated API document.

use rainier_framework::config::{Config, Env};
use rainier_framework::prelude::*;

use crate::config::keys::{
    OPENAPI_ENABLED, OPENAPI_PATH, OPENAPI_SERVER, OPENAPI_TITLE, OPENAPI_VERSION,
};

/// OpenAPI settings, read by `bootstrap.rs`.
pub fn configure(config: &Config, env: &Env) -> Result<()> {
    // Off in production by default. The document is a map of your API — every
    // route, every field, every constraint — which is exactly what you want
    // your own developers to have and exactly what you may not want published.
    config.set(OPENAPI_ENABLED, env.bool("OPENAPI_ENABLED", false))?;
    config.set(OPENAPI_PATH, env.string("OPENAPI_PATH", "/openapi.json"))?;

    // The **API's** version, not the framework's and not the crate's. It is
    // what a client pins against, so it changes when the API's contract does.
    config.set(OPENAPI_TITLE, "Rainier Sample API".to_string())?;
    config.set(OPENAPI_VERSION, "1.0.0".to_string())?;

    // Only when a deployment knows its own public URL. Absent is better than
    // wrong: a client that follows `servers[0]` to the wrong host fails in a
    // way that looks like the API is down.
    if let Some(url) = env.get("APP_URL") {
        config.set(OPENAPI_SERVER, url)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_off_unless_asked_for() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.get(OPENAPI_ENABLED), Some(false));
    }

    #[test]
    fn the_server_is_absent_rather_than_guessed() {
        let config = Config::new();
        configure(&config, &Env::new()).unwrap();

        assert_eq!(config.get(OPENAPI_SERVER), None);
    }

    #[test]
    fn a_known_public_url_is_advertised() {
        let config = Config::new();
        configure(&config, &Env::parse("APP_URL=https://api.example.com")).unwrap();

        assert_eq!(config.get(OPENAPI_SERVER).as_deref(), Some("https://api.example.com"));
    }
}
