//! Local cache + outbound calls for orgs.
//!
//! v1 model: each org has exactly one default vault, named the same as
//! the org. When a user is invited to the org, the caller (an
//! owner/admin) also adds them as a vault member with a sealed-box
//! wrapped vault key.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::error::{ParaError, Result};
use crate::store::db::LocalStore;

#[derive(Debug, Clone, Serialize)]
pub struct LocalOrgRow {
    pub org_id: String,
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateOrgArgs {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InviteToOrgArgs {
    pub org_id: String,
    pub email: String,
}

pub fn list(store: &LocalStore) -> Result<Vec<LocalOrgRow>> {
    let rows: Vec<LocalOrgRow> = store
        .with_conn(|c| {
            let mut stmt = c.prepare(
                r#"SELECT org_id, name, role FROM sync_orgs ORDER BY created_at ASC"#,
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(LocalOrgRow {
                        org_id: r.get(0)?,
                        name: r.get(1)?,
                        role: r.get(2)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .map_err(|e| ParaError::Db(format!("list sync_orgs: {e}")))?;
    Ok(rows)
}

pub fn upsert(
    store: &LocalStore,
    org_id: &str,
    name: &str,
    role: &str,
    owner_user_id: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    let org_id = org_id.to_string();
    let name = name.to_string();
    let role = role.to_string();
    let owner = owner_user_id.map(|s| s.to_string());
    store
        .with_conn(move |c| {
            c.execute(
                r#"INSERT INTO sync_orgs
                       (org_id, name, role, owner_user_id, created_at, updated_at)
                       VALUES (?1, ?2, ?3, ?4, ?5, ?5)
                   ON CONFLICT(org_id) DO UPDATE SET
                       name = excluded.name,
                       role = excluded.role,
                       owner_user_id = excluded.owner_user_id,
                       updated_at = excluded.updated_at"#,
                params![org_id, name, role, owner, now],
            )
        })
        .map_err(|e| ParaError::Db(format!("upsert sync_org: {e}")))?;
    Ok(())
}
