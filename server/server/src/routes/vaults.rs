//! `/v1/vaults/*` endpoints.

use audire_shared::{
    AddMemberRequest, CreateVaultRequest, RotateVaultKeyRequest, VaultRole, VaultView,
};
use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;

use crate::auth::AuthCtx;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// `GET /v1/vaults` — every vault the caller is a member of, with their
/// own copy of the wrapped vault key.
pub async fn list(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> ApiResult<Json<Vec<VaultView>>> {
    let rows = sqlx::query!(
        r#"
        SELECT v.id, v.name_ciphertext, v.owner_user_id, v.org_id,
               v.last_op_id, v.created_at,
               m.wrapped_vault_key, m.role
        FROM audire.vaults v
        JOIN audire.vault_members m ON m.vault_id = v.id
        WHERE m.user_id = $1
        ORDER BY v.created_at ASC
        "#,
        auth.user_id
    )
    .fetch_all(&state.db)
    .await?;

    let views = rows
        .into_iter()
        .map(|r| VaultView {
            id: r.id,
            name_ciphertext: r.name_ciphertext,
            owner_user_id: r.owner_user_id,
            org_id: r.org_id,
            wrapped_vault_key: r.wrapped_vault_key,
            role: parse_role(&r.role),
            last_op_id: r.last_op_id,
            created_at: r.created_at,
        })
        .collect();
    Ok(Json(views))
}

/// `POST /v1/vaults` — create a new vault. The caller becomes its owner.
/// Inserts the vault row and the owner's `vault_members` row in a single
/// transaction.
pub async fn create(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateVaultRequest>,
) -> ApiResult<Json<VaultView>> {
    let vault_id = Uuid::new_v4();
    let mut tx = state.db.begin().await?;

    let vault = sqlx::query!(
        r#"
        INSERT INTO audire.vaults (id, name_ciphertext, owner_user_id, org_id)
        VALUES ($1, $2, $3, $4)
        RETURNING id, name_ciphertext, owner_user_id, org_id, last_op_id, created_at
        "#,
        vault_id,
        &body.name_ciphertext,
        auth.user_id,
        body.org_id,
    )
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query!(
        r#"
        INSERT INTO audire.vault_members (vault_id, user_id, wrapped_vault_key, role, accepted_at)
        VALUES ($1, $2, $3, 'owner', now())
        "#,
        vault_id,
        auth.user_id,
        &body.wrapped_vault_key,
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(VaultView {
        id: vault.id,
        name_ciphertext: vault.name_ciphertext,
        owner_user_id: vault.owner_user_id,
        org_id: vault.org_id,
        wrapped_vault_key: body.wrapped_vault_key,
        role: VaultRole::Owner,
        last_op_id: vault.last_op_id,
        created_at: vault.created_at,
    }))
}

/// `GET /v1/vaults/:id` — single vault with the caller's wrapped key.
pub async fn get_one(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<VaultView>> {
    let row = sqlx::query!(
        r#"
        SELECT v.id, v.name_ciphertext, v.owner_user_id, v.org_id,
               v.last_op_id, v.created_at,
               m.wrapped_vault_key, m.role
        FROM audire.vaults v
        JOIN audire.vault_members m ON m.vault_id = v.id
        WHERE v.id = $1 AND m.user_id = $2
        "#,
        id,
        auth.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| ApiError::NotFound("vault not found or no access".into()))?;

    Ok(Json(VaultView {
        id: row.id,
        name_ciphertext: row.name_ciphertext,
        owner_user_id: row.owner_user_id,
        org_id: row.org_id,
        wrapped_vault_key: row.wrapped_vault_key,
        role: parse_role(&row.role),
        last_op_id: row.last_op_id,
        created_at: row.created_at,
    }))
}

/// `POST /v1/vaults/:id/members` — invite a user. Phase 2 feature; the
/// route is wired in v1 so the desktop client can be developed against
/// it.
pub async fn add_member(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(vault_id): Path<Uuid>,
    Json(body): Json<AddMemberRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    require_owner(&state, auth.user_id, vault_id).await?;
    sqlx::query!(
        r#"
        INSERT INTO audire.vault_members (vault_id, user_id, wrapped_vault_key, role, invited_by)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (vault_id, user_id) DO UPDATE
            SET wrapped_vault_key = EXCLUDED.wrapped_vault_key,
                role = EXCLUDED.role
        "#,
        vault_id,
        body.user_id,
        &body.wrapped_vault_key,
        role_str(body.role),
        auth.user_id,
    )
    .execute(&state.db)
    .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `POST /v1/vaults/:id/rotate-key` — owner re-wraps the vault key for
/// every remaining member after a removal. Atomically replaces every
/// `wrapped_vault_key` for the vault and re-encrypts the vault name.
pub async fn rotate_key(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(vault_id): Path<Uuid>,
    Json(body): Json<RotateVaultKeyRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    require_owner(&state, auth.user_id, vault_id).await?;

    if !body.wrapped_keys.iter().any(|k| k.user_id == auth.user_id) {
        return Err(ApiError::BadRequest(
            "rotate must include the caller's own wrapped key".into(),
        ));
    }

    let mut tx = state.db.begin().await?;

    sqlx::query!(
        r#"UPDATE audire.vaults SET name_ciphertext = $1, updated_at = now() WHERE id = $2"#,
        &body.name_ciphertext,
        vault_id,
    )
    .execute(&mut *tx)
    .await?;

    // Replace wrapped keys for every member listed; delete anyone not
    // listed (they've been removed).
    let listed_ids: Vec<Uuid> = body.wrapped_keys.iter().map(|k| k.user_id).collect();

    for k in &body.wrapped_keys {
        sqlx::query!(
            r#"
            UPDATE audire.vault_members
               SET wrapped_vault_key = $1
             WHERE vault_id = $2 AND user_id = $3
            "#,
            &k.wrapped_vault_key,
            vault_id,
            k.user_id,
        )
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query!(
        r#"
        DELETE FROM audire.vault_members
         WHERE vault_id = $1
           AND user_id <> ALL($2::uuid[])
        "#,
        vault_id,
        &listed_ids,
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn require_owner(state: &AppState, user_id: Uuid, vault_id: Uuid) -> ApiResult<()> {
    let row = sqlx::query!(
        r#"SELECT role FROM audire.vault_members WHERE vault_id = $1 AND user_id = $2"#,
        vault_id,
        user_id
    )
    .fetch_optional(&state.db)
    .await?;
    match row {
        Some(r) if r.role == "owner" => Ok(()),
        Some(_) => Err(ApiError::Forbidden("owner role required".into())),
        None => Err(ApiError::NotFound("vault not found or no access".into())),
    }
}

fn parse_role(s: &str) -> VaultRole {
    match s {
        "owner" => VaultRole::Owner,
        "editor" => VaultRole::Editor,
        _ => VaultRole::Reader,
    }
}

fn role_str(r: VaultRole) -> &'static str {
    match r {
        VaultRole::Owner => "owner",
        VaultRole::Editor => "editor",
        VaultRole::Reader => "reader",
    }
}
