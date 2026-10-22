//! `oauth:client` — register an application allowed to ask for tokens.

use rainier_framework::auth::{generate_session_id, Hasher};
use rainier_framework::console_kernel::{exit, Arguments, Command};
use rainier_framework::crypt::hash::HashManager;
use rainier_framework::oauth_server::{grant, Client};
use rainier_framework::prelude::*;

use crate::app::providers::AddIns;

/// Registers an OAuth client in `oauth_clients`.
#[derive(Debug, Default)]
pub struct OAuthClientCommand;

#[async_trait]
impl Command for OAuthClientCommand {
    fn name(&self) -> &str {
        "oauth:client"
    }

    fn description(&self) -> &str {
        "Register an OAuth client"
    }

    fn help(&self) -> Option<&str> {
        Some(
            "Usage:\n  oauth:client --name=<name> [--redirect=<uri>] [--public] [--personal]\n\n\
             Options:\n  \
             --name      What the consent screen calls it\n  \
             --redirect  Where an authorization returns to (required unless --personal)\n  \
             --public    No secret: a phone or browser app, which cannot keep one.\n              \
             PKCE is required for it instead.\n  \
             --personal  The client personal access tokens are issued through\n\n\
             A confidential client's secret is printed once and stored only as a hash.",
        )
    }

    async fn handle(&self, args: &Arguments, app: &Application) -> Result<i32> {
        let Some(name) = args.option("name") else {
            eprintln!("--name is required. See `oauth:client --help`.");
            return Ok(exit::FAILURE);
        };
        let personal = args.flag("personal");
        let public = args.flag("public");
        let redirect = args.option("redirect").map(str::to_string);

        if redirect.is_none() && !personal {
            eprintln!("--redirect is required for a client that sends people to authorize.");
            return Ok(exit::FAILURE);
        }

        let secret = (!public).then(generate_session_id);
        let hasher = app.resolve::<HashManager>()?;

        let grant_types = if personal {
            vec![grant::PERSONAL_ACCESS.to_string()]
        } else if public {
            vec![grant::AUTHORIZATION_CODE.into(), grant::REFRESH_TOKEN.into()]
        } else {
            vec![
                grant::AUTHORIZATION_CODE.into(),
                grant::REFRESH_TOKEN.into(),
                grant::CLIENT_CREDENTIALS.into(),
            ]
        };

        let client = Client {
            id: generate_session_id()[..32].to_string(),
            name: name.to_string(),
            secret_hash: secret.as_deref().map(|secret| hasher.hash(secret)).transpose()?,
            redirect_uris: redirect.into_iter().collect(),
            grant_types,
            scopes: None,
            provider: None,
            owner_id: None,
            revoked: false,
        };

        app.resolve::<AddIns>()?.oauth.store.register_client(&client, personal).await?;

        println!("Client registered.");
        println!("  client_id:     {}", client.id);
        match secret {
            Some(secret) => {
                println!("  client_secret: {secret}");
                println!(
                    "Store the secret now: only its hash is kept, so it cannot be shown again."
                );
            }
            None => println!("  public client: no secret, PKCE required"),
        }

        Ok(exit::SUCCESS)
    }
}
