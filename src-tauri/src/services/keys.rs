use serde::Serialize;

use crate::error::Result;
use crate::keyvault::vault::KeyVault;
use crate::store::db::{LocalStore, OrgSharedKeyStatusRow};

#[derive(Debug, Clone, Serialize)]
pub struct KeyResolutionStatus {
    pub provider: String,
    pub source: String,
    pub org_id: Option<i64>,
    pub has_personal_key: bool,
    pub has_org_key: bool,
}

pub fn resolve_provider_source(
    keyvault: &KeyVault,
    provider: &str,
    org_id: Option<i64>,
) -> KeyResolutionStatus {
    let has_personal_key = keyvault.has_provider_key(provider);
    let has_org_key = org_id
        .map(|org_id| keyvault.has_org_provider_key(&org_id.to_string(), provider))
        .unwrap_or(false);

    let source = if has_personal_key {
        "personal"
    } else if has_org_key {
        "org_shared"
    } else {
        "missing"
    };

    KeyResolutionStatus {
        provider: provider.to_string(),
        source: source.to_string(),
        org_id,
        has_personal_key,
        has_org_key,
    }
}

pub fn save_org_key(
    store: &LocalStore,
    keyvault: &KeyVault,
    org_id: i64,
    provider: &str,
    key: &str,
) -> Result<()> {
    keyvault
        .set_org_provider_key(&org_id.to_string(), provider, key)
        .map_err(|e| crate::error::ParaError::KeyVault(e.to_string()))?;
    store.upsert_org_shared_key_status(org_id, provider)?;
    Ok(())
}

pub fn delete_org_key(
    store: &LocalStore,
    keyvault: &KeyVault,
    org_id: i64,
    provider: &str,
) -> Result<()> {
    keyvault
        .delete_org_provider_key(&org_id.to_string(), provider)
        .map_err(|e| crate::error::ParaError::KeyVault(e.to_string()))?;
    store.delete_org_shared_key_status(org_id, provider)?;
    Ok(())
}

pub fn list_org_key_statuses(
    store: &LocalStore,
    org_id: i64,
) -> Result<Vec<OrgSharedKeyStatusRow>> {
    store.list_org_shared_key_statuses(org_id)
}
