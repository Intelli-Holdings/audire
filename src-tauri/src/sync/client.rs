//! Thin HTTP client for `audire-server`.
//!
//! Only the calls actually used by v1 are implemented; the WebSocket
//! sync stream is a separate worker (see `sync::worker`). All requests
//! send a Stack Auth bearer token and JSON.

use anyhow::{anyhow, Context};
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct SyncClient {
    base: String,
    access_token: String,
    http: Client,
}

impl SyncClient {
    pub fn new(base: &str, access_token: &str) -> Self {
        let base = base.trim_end_matches('/').to_string();
        Self {
            base,
            access_token: access_token.to_string(),
            http: Client::builder()
                .user_agent("audire-desktop")
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .expect("reqwest client build"),
        }
    }

    /// `POST /v1/users/me` — register or refresh the caller's profile.
    /// Idempotent on `(stack_user_id, public_key)`.
    pub async fn register_user(
        &self,
        email: &str,
        public_key: &[u8; 32],
        wrapped_kek_for_recovery: &[u8],
    ) -> anyhow::Result<RegisterUserResp> {
        #[derive(Serialize)]
        struct Body<'a> {
            email: &'a str,
            #[serde(serialize_with = "ser_hex")]
            public_key: &'a [u8],
            #[serde(serialize_with = "ser_hex")]
            wrapped_kek_for_recovery: &'a [u8],
        }
        let resp = self
            .http
            .post(format!("{}/v1/users/me", self.base))
            .bearer_auth(&self.access_token)
            .json(&Body {
                email,
                public_key,
                wrapped_kek_for_recovery,
            })
            .send()
            .await
            .context("POST /v1/users/me")?;
        let resp = ok_or_err(resp).await?;
        let user: ServerUserProfile = resp.json().await.context("decode user profile")?;
        Ok(RegisterUserResp { user_id: user.id })
    }

    /// `GET /v1/users/lookup?email=` — resolve another Audire user's
    /// public key.
    pub async fn lookup_user(&self, email: &str) -> anyhow::Result<UserLookupView> {
        let resp = self
            .http
            .get(format!("{}/v1/users/lookup", self.base))
            .bearer_auth(&self.access_token)
            .query(&[("email", email)])
            .send()
            .await
            .context("GET /v1/users/lookup")?;
        let resp = ok_or_err(resp).await?;
        Ok(resp.json().await.context("decode user lookup")?)
    }

    /// `POST /v1/orgs` — create an org. Caller becomes owner.
    pub async fn create_org(&self, name: &str) -> anyhow::Result<OrgView> {
        #[derive(Serialize)]
        struct Body<'a> {
            name: &'a str,
        }
        let resp = self
            .http
            .post(format!("{}/v1/orgs", self.base))
            .bearer_auth(&self.access_token)
            .json(&Body { name })
            .send()
            .await
            .context("POST /v1/orgs")?;
        let resp = ok_or_err(resp).await?;
        Ok(resp.json().await.context("decode org")?)
    }

    pub async fn list_orgs(&self) -> anyhow::Result<Vec<OrgView>> {
        let resp = self
            .http
            .get(format!("{}/v1/orgs", self.base))
            .bearer_auth(&self.access_token)
            .send()
            .await
            .context("GET /v1/orgs")?;
        let resp = ok_or_err(resp).await?;
        Ok(resp.json().await.context("decode orgs")?)
    }

    pub async fn add_org_member(
        &self,
        org_id: &str,
        user_id: &str,
        role: &str,
    ) -> anyhow::Result<()> {
        #[derive(Serialize)]
        struct Body<'a> {
            user_id: &'a str,
            role: &'a str,
        }
        let resp = self
            .http
            .post(format!("{}/v1/orgs/{org_id}/members", self.base))
            .bearer_auth(&self.access_token)
            .json(&Body { user_id, role })
            .send()
            .await
            .context("POST /v1/orgs/:id/members")?;
        ok_or_err(resp).await?;
        Ok(())
    }

    pub async fn list_org_members(&self, org_id: &str) -> anyhow::Result<Vec<OrgMemberView>> {
        let resp = self
            .http
            .get(format!("{}/v1/orgs/{org_id}/members", self.base))
            .bearer_auth(&self.access_token)
            .send()
            .await
            .context("GET /v1/orgs/:id/members")?;
        let resp = ok_or_err(resp).await?;
        Ok(resp.json().await.context("decode members")?)
    }

    pub async fn create_vault(
        &self,
        name_ciphertext: &[u8],
        wrapped_vault_key: &[u8],
        org_id: Option<&str>,
    ) -> anyhow::Result<VaultView> {
        #[derive(Serialize)]
        struct Body<'a> {
            #[serde(serialize_with = "ser_hex")]
            name_ciphertext: &'a [u8],
            #[serde(serialize_with = "ser_hex")]
            wrapped_vault_key: &'a [u8],
            #[serde(skip_serializing_if = "Option::is_none")]
            org_id: Option<&'a str>,
        }
        let resp = self
            .http
            .post(format!("{}/v1/vaults", self.base))
            .bearer_auth(&self.access_token)
            .json(&Body {
                name_ciphertext,
                wrapped_vault_key,
                org_id,
            })
            .send()
            .await
            .context("POST /v1/vaults")?;
        let resp = ok_or_err(resp).await?;
        Ok(resp.json().await.context("decode vault")?)
    }

    pub async fn list_vaults(&self) -> anyhow::Result<Vec<VaultView>> {
        let resp = self
            .http
            .get(format!("{}/v1/vaults", self.base))
            .bearer_auth(&self.access_token)
            .send()
            .await
            .context("GET /v1/vaults")?;
        let resp = ok_or_err(resp).await?;
        Ok(resp.json().await.context("decode vaults")?)
    }

    pub async fn add_vault_member(
        &self,
        vault_id: &str,
        user_id: &str,
        wrapped_vault_key: &[u8],
        role: &str,
    ) -> anyhow::Result<()> {
        #[derive(Serialize)]
        struct Body<'a> {
            user_id: &'a str,
            #[serde(serialize_with = "ser_hex")]
            wrapped_vault_key: &'a [u8],
            role: &'a str,
        }
        let resp = self
            .http
            .post(format!("{}/v1/vaults/{vault_id}/members", self.base))
            .bearer_auth(&self.access_token)
            .json(&Body {
                user_id,
                wrapped_vault_key,
                role,
            })
            .send()
            .await
            .context("POST /v1/vaults/:id/members")?;
        ok_or_err(resp).await?;
        Ok(())
    }
}

async fn ok_or_err(resp: reqwest::Response) -> anyhow::Result<reqwest::Response> {
    if resp.status().is_success() {
        Ok(resp)
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(anyhow!("{} — {}", status, body))
    }
}

fn ser_hex<S: serde::Serializer>(bytes: &&[u8], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&hex::encode(bytes))
}

#[derive(Debug)]
pub struct RegisterUserResp {
    pub user_id: String,
}

#[derive(Debug, Deserialize)]
struct ServerUserProfile {
    id: String,
    #[allow(dead_code)]
    email: String,
}

#[derive(Debug, Deserialize)]
pub struct UserLookupView {
    pub id: String,
    pub email: String,
    /// Hex-encoded X25519 public key (32 bytes).
    pub public_key: String,
}

#[derive(Debug, Deserialize)]
pub struct OrgView {
    pub id: String,
    pub name: String,
    pub owner_user_id: String,
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct OrgMemberView {
    pub user_id: String,
    pub email: String,
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct VaultView {
    pub id: String,
    /// Hex-encoded.
    pub name_ciphertext: String,
    pub owner_user_id: String,
    pub org_id: Option<String>,
    /// Hex-encoded.
    pub wrapped_vault_key: String,
    pub role: String,
    pub last_op_id: i64,
}
