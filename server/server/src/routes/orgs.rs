//! `/v1/orgs/*` endpoints. Orgs are optional scoping above vaults: a
//! user can be a member of multiple orgs, and any vault can be
//! associated with at most one org. Org admins can invite users; org
//! membership doesn't grant access to vaults — vault membership is
//! still per-vault — but it does make invites cheaper UX-wise (the
//! Account UI shows org members as one-click vault invitees).

use audire_shared::{AddOrgMemberRequest, CreateOrgRequest, OrgMemberView, OrgRole, OrgView};
use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;

use crate::auth::AuthCtx;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// `GET /v1/orgs` — every org the caller belongs to.
pub async fn list(
    State(state): State<AppState>,
    auth: AuthCtx,
) -> ApiResult<Json<Vec<OrgView>>> {
    let rows = sqlx::query!(
        r#"
        SELECT o.id, o.name, o.owner_user_id, o.created_at, m.role
        FROM audire.orgs o
        JOIN audire.org_members m ON m.org_id = o.id
        WHERE m.user_id = $1
        ORDER BY o.created_at ASC
        "#,
        auth.user_id
    )
    .fetch_all(&state.db)
    .await?;

    let views = rows
        .into_iter()
        .map(|r| OrgView {
            id: r.id,
            name: r.name,
            owner_user_id: r.owner_user_id,
            role: parse_role(&r.role),
            created_at: r.created_at,
        })
        .collect();
    Ok(Json(views))
}

/// `POST /v1/orgs` — create an org. Caller becomes the owner.
pub async fn create(
    State(state): State<AppState>,
    auth: AuthCtx,
    Json(body): Json<CreateOrgRequest>,
) -> ApiResult<Json<OrgView>> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("org name cannot be empty".into()));
    }
    let org_id = Uuid::new_v4();
    let mut tx = state.db.begin().await?;
    let row = sqlx::query!(
        r#"INSERT INTO audire.orgs (id, name, owner_user_id)
           VALUES ($1, $2, $3)
           RETURNING id, name, owner_user_id, created_at"#,
        org_id,
        name,
        auth.user_id,
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query!(
        r#"INSERT INTO audire.org_members (org_id, user_id, role)
           VALUES ($1, $2, 'owner')"#,
        org_id,
        auth.user_id,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(OrgView {
        id: row.id,
        name: row.name,
        owner_user_id: row.owner_user_id,
        role: OrgRole::Owner,
        created_at: row.created_at,
    }))
}

/// `POST /v1/orgs/:id/members` — invite a user. Caller must be an
/// owner or admin of the org.
pub async fn add_member(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(org_id): Path<Uuid>,
    Json(body): Json<AddOrgMemberRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&state, auth.user_id, org_id).await?;
    sqlx::query!(
        r#"INSERT INTO audire.org_members (org_id, user_id, role)
           VALUES ($1, $2, $3)
           ON CONFLICT (org_id, user_id) DO UPDATE SET role = EXCLUDED.role"#,
        org_id,
        body.user_id,
        role_str(body.role),
    )
    .execute(&state.db)
    .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `GET /v1/orgs/:id/members` — list members. Caller must be a member.
pub async fn list_members(
    State(state): State<AppState>,
    auth: AuthCtx,
    Path(org_id): Path<Uuid>,
) -> ApiResult<Json<Vec<OrgMemberView>>> {
    require_member(&state, auth.user_id, org_id).await?;
    let rows = sqlx::query!(
        r#"SELECT m.user_id, u.email, m.role
           FROM audire.org_members m
           JOIN audire.users u ON u.id = m.user_id
           WHERE m.org_id = $1
           ORDER BY m.role, u.email"#,
        org_id,
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| OrgMemberView {
                user_id: r.user_id,
                email: r.email,
                role: parse_role(&r.role),
            })
            .collect(),
    ))
}

async fn require_member(state: &AppState, user_id: Uuid, org_id: Uuid) -> ApiResult<()> {
    let role: Option<String> = sqlx::query_scalar!(
        r#"SELECT role FROM audire.org_members WHERE org_id = $1 AND user_id = $2"#,
        org_id,
        user_id
    )
    .fetch_optional(&state.db)
    .await?;
    match role {
        Some(_) => Ok(()),
        None => Err(ApiError::NotFound("org not found or no access".into())),
    }
}

async fn require_admin(state: &AppState, user_id: Uuid, org_id: Uuid) -> ApiResult<()> {
    let role: Option<String> = sqlx::query_scalar!(
        r#"SELECT role FROM audire.org_members WHERE org_id = $1 AND user_id = $2"#,
        org_id,
        user_id
    )
    .fetch_optional(&state.db)
    .await?;
    match role.as_deref() {
        Some("owner") | Some("admin") => Ok(()),
        Some(_) => Err(ApiError::Forbidden("admin role required".into())),
        None => Err(ApiError::NotFound("org not found or no access".into())),
    }
}

fn parse_role(s: &str) -> OrgRole {
    match s {
        "owner" => OrgRole::Owner,
        "admin" => OrgRole::Admin,
        _ => OrgRole::Member,
    }
}

fn role_str(r: OrgRole) -> &'static str {
    match r {
        OrgRole::Owner => "owner",
        OrgRole::Admin => "admin",
        OrgRole::Member => "member",
    }
}
