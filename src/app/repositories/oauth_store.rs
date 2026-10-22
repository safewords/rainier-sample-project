//! `DatabaseOAuthStore` — the OAuth server's records, in this application's
//! tables.
//!
//! The server ships a memory store for tests and nothing that restarts; a
//! deployment brings its own over its own schema. This is that, over the four
//! `oauth_*` tables `m0015`–`m0018` create.
//!
//! # Revoking is claiming
//!
//! A code and a refresh token are each spent by revoking them, and the server
//! issues only if the revoke reports it changed something. So
//! [`revoke_auth_code`](OAuthStore::revoke_auth_code) is `UPDATE … SET revoked
//! = true WHERE id = ? AND revoked = false` and answers with the affected-row
//! count — two requests racing with one code get one token between them,
//! because only one of their updates matches a row.

use chrono::Utc;
use rainier_framework::database::Repository;
use rainier_framework::oauth_server::{
    AccessTokenRecord, AuthCodeRecord, Client, OAuthStore, PurgeOptions, Purged, RefreshTokenRecord,
};
use rainier_framework::prelude::*;

use crate::app::models::{OAuthAccessToken, OAuthAuthCode, OAuthClient, OAuthRefreshToken};

/// The OAuth server's store, over `oauth_*`.
pub struct DatabaseOAuthStore {
    clients: EntityRepository<OAuthClient>,
    access: EntityRepository<OAuthAccessToken>,
    refresh: EntityRepository<OAuthRefreshToken>,
    codes: EntityRepository<OAuthAuthCode>,
}

impl DatabaseOAuthStore {
    /// A store over `database`.
    pub fn new(database: Database) -> Self {
        Self {
            clients: EntityRepository::new(database.clone()),
            access: EntityRepository::new(database.clone()),
            refresh: EntityRepository::new(database.clone()),
            codes: EntityRepository::new(database),
        }
    }

    /// Register a client — what `oauth:client` calls.
    pub async fn register_client(&self, client: &Client, personal_access: bool) -> Result<()> {
        self.clients
            .create(OAuthClient {
                id: client.id.clone(),
                name: client.name.clone(),
                secret_hash: client.secret_hash.clone(),
                redirect_uris: json(&client.redirect_uris)?,
                grant_types: json(&client.grant_types)?,
                scopes: client.scopes.as_ref().map(json).transpose()?,
                owner_id: client.owner_id.clone(),
                personal_access,
                revoked: client.revoked,
                created_at: Utc::now(),
            })
            .await?;
        Ok(())
    }
}

fn json(list: &Vec<String>) -> Result<String> {
    Ok(serde_json::to_string(list)?)
}

fn list(text: &str) -> Result<Vec<String>> {
    Ok(serde_json::from_str(text)?)
}

fn client(row: OAuthClient) -> Result<Client> {
    Ok(Client {
        id: row.id,
        name: row.name,
        secret_hash: row.secret_hash,
        redirect_uris: list(&row.redirect_uris)?,
        grant_types: list(&row.grant_types)?,
        scopes: row.scopes.as_deref().map(list).transpose()?,
        provider: None,
        owner_id: row.owner_id,
        revoked: row.revoked,
    })
}

fn access_record(row: OAuthAccessToken) -> Result<AccessTokenRecord> {
    Ok(AccessTokenRecord {
        id: row.id,
        user_id: row.user_id,
        client_id: row.client_id,
        name: row.name,
        scopes: list(&row.scopes)?,
        revoked: row.revoked,
        created_at: row.created_at,
        expires_at: row.expires_at,
    })
}

/// `WHERE id = ? AND revoked = false` — the claim.
fn live(id: &str) -> Criteria {
    Criteria::new().where_eq("id", id).where_eq("revoked", false)
}

#[async_trait]
impl OAuthStore for DatabaseOAuthStore {
    async fn find_client(&self, id: &str) -> Result<Option<Client>> {
        self.clients.find_key(id).await?.map(client).transpose()
    }

    async fn personal_access_client(&self, _provider: Option<&str>) -> Result<Option<Client>> {
        // One user provider in this application, so one personal access
        // client; the newest live one, if somebody registered a replacement.
        self.clients
            .first_matching(
                Criteria::new()
                    .where_eq("personal_access", true)
                    .where_eq("revoked", false)
                    .order_by_desc("created_at"),
            )
            .await?
            .map(client)
            .transpose()
    }

    async fn create_access_token(&self, token: &AccessTokenRecord) -> Result<()> {
        self.access
            .create(OAuthAccessToken {
                id: token.id.clone(),
                user_id: token.user_id.clone(),
                client_id: token.client_id.clone(),
                name: token.name.clone(),
                scopes: json(&token.scopes)?,
                revoked: token.revoked,
                created_at: token.created_at,
                expires_at: token.expires_at,
            })
            .await?;
        Ok(())
    }

    async fn find_access_token(&self, id: &str) -> Result<Option<AccessTokenRecord>> {
        self.access.find_key(id).await?.map(access_record).transpose()
    }

    async fn revoke_access_token(&self, id: &str) -> Result<bool> {
        Ok(self.access.update_column(live(id), "revoked", true).await? > 0)
    }

    async fn create_refresh_token(&self, token: &RefreshTokenRecord) -> Result<()> {
        self.refresh
            .create(OAuthRefreshToken {
                id: token.id.clone(),
                access_token_id: token.access_token_id.clone(),
                revoked: token.revoked,
                expires_at: token.expires_at,
            })
            .await?;
        Ok(())
    }

    async fn find_refresh_token(&self, id: &str) -> Result<Option<RefreshTokenRecord>> {
        Ok(self.refresh.find_key(id).await?.map(|row| RefreshTokenRecord {
            id: row.id,
            access_token_id: row.access_token_id,
            revoked: row.revoked,
            expires_at: row.expires_at,
        }))
    }

    async fn revoke_refresh_token(&self, id: &str) -> Result<bool> {
        Ok(self.refresh.update_column(live(id), "revoked", true).await? > 0)
    }

    async fn revoke_refresh_tokens_for(&self, access_token_id: &str) -> Result<u64> {
        self.refresh
            .update_column(
                Criteria::new()
                    .where_eq("access_token_id", access_token_id)
                    .where_eq("revoked", false),
                "revoked",
                true,
            )
            .await
    }

    async fn create_auth_code(&self, code: &AuthCodeRecord) -> Result<()> {
        self.codes
            .create(OAuthAuthCode {
                id: code.id.clone(),
                user_id: code.user_id.clone(),
                client_id: code.client_id.clone(),
                scopes: json(&code.scopes)?,
                revoked: code.revoked,
                expires_at: code.expires_at,
            })
            .await?;
        Ok(())
    }

    async fn revoke_auth_code(&self, id: &str) -> Result<bool> {
        Ok(self.codes.update_column(live(id), "revoked", true).await? > 0)
    }

    async fn is_auth_code_revoked(&self, id: &str) -> Result<bool> {
        // Unknown and revoked are one answer: a code this table never saw is
        // not one to honour.
        Ok(self.codes.find_key(id).await?.is_none_or(|row| row.revoked))
    }

    async fn tokens_for(
        &self,
        user_id: &str,
        client_id: Option<&str>,
    ) -> Result<Vec<AccessTokenRecord>> {
        let mut criteria = Criteria::new()
            .where_eq("user_id", user_id)
            .where_eq("revoked", false)
            .where_gt("expires_at", Utc::now())
            .order_by_desc("created_at");
        if let Some(client_id) = client_id {
            criteria = criteria.where_eq("client_id", client_id);
        }

        self.access.matching(criteria).await?.into_iter().map(access_record).collect()
    }

    async fn purge(&self, options: PurgeOptions) -> Result<Purged> {
        let mut purged = Purged::default();

        if options.revoked {
            let revoked = || Criteria::new().where_eq("revoked", true);
            purged.access_tokens += self.access.delete_matching(revoked()).await?;
            purged.refresh_tokens += self.refresh.delete_matching(revoked()).await?;
            purged.auth_codes += self.codes.delete_matching(revoked()).await?;
        }

        if let Some(before) = options.expired_before {
            let expired = || Criteria::new().where_lt("expires_at", before);
            purged.access_tokens += self.access.delete_matching(expired()).await?;
            purged.refresh_tokens += self.refresh.delete_matching(expired()).await?;
            purged.auth_codes += self.codes.delete_matching(expired()).await?;
        }

        Ok(purged)
    }
}
