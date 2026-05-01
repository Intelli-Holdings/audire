//! Environment-driven configuration. Everything secret is required at
//! startup so misconfigured deploys fail fast.

use anyhow::{anyhow, Result};

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bind_addr: String,
    pub database_url: String,
    pub stack_auth_project_id: String,
    pub stack_auth_jwks_url: String,
    /// Audience claim we expect on incoming JWTs. Set this to whatever
    /// Stack Auth issues for the project; usually the project_id.
    pub stack_auth_audience: String,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            bind_addr: env_or("BIND_ADDR", "0.0.0.0:8080"),
            database_url: req("DATABASE_URL")?,
            stack_auth_project_id: req("STACK_AUTH_PROJECT_ID")?,
            stack_auth_jwks_url: req("STACK_AUTH_JWKS_URL")?,
            stack_auth_audience: env_or("STACK_AUTH_AUDIENCE", "audire-server"),
        })
    }
}

fn req(name: &str) -> Result<String> {
    std::env::var(name).map_err(|_| anyhow!("missing env: {}", name))
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}
