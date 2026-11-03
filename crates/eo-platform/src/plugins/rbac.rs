//! RBAC 的 HTTP 介面與開機時的 seed。
//!
//! · `/api/v1/me/permissions|check|orgs-with-permission`：前端 gating 用。
//! · `/api/v1/orgs/{orgId}/rbac/*`：組織內的角色、permission、grant 管理（要 `platform:rbac:manage`）。
//! · `/api/v1/admin/rbac/*`：全域自訂角色與 permission（超級使用者）。

use crate::auth::CurrentUser;
use crate::azldi::Plugin;
use crate::core::CoreRef;
use crate::error::{ApiError, ApiResult};
use crate::plugin::{AppPlugin, RouteRegistrar, StartContext, StartError};
use crate::rbac::custom::{self, CreatePermissionInput, CreateRoleInput, UpdatePermissionInput, UpdateRoleInput};
use crate::rbac::grant::{self, GrantInput, GrantRow};
use crate::rbac::{
    seed, ScopeRef, PLATFORM_MEMBER_MANAGE, PLATFORM_ORG_ADMIN_ROLE, PLATFORM_ORG_MANAGE, PLATFORM_PROJECT_ADMIN_ROLE,
    PLATFORM_PROJECT_MANAGE, PLATFORM_RBAC_MANAGE,
};
use crate::tenancy;
use actix_web::{web, HttpResponse};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use eo_domain::rbac::{RbacScope, RbacSeeds};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub struct RbacPlugin {
    core: CoreRef,
}

impl RbacPlugin {
    pub fn new(core: CoreRef) -> Self {
        Self { core }
    }
}

pub fn platform_seeds() -> RbacSeeds {
    use RbacScope::{Organization as O, Project as P};
    RbacSeeds::new("rbac")
        .simple(O, PLATFORM_RBAC_MANAGE, "平台：RBAC 管理（授權／撤銷／自訂角色）")
        .simple(O, PLATFORM_ORG_MANAGE, "平台：組織設定")
        .simple(O, PLATFORM_MEMBER_MANAGE, "平台：成員與邀請管理（組織）")
        .simple(O, PLATFORM_PROJECT_MANAGE, "平台：專案管理（組織）")
        .simple(P, PLATFORM_MEMBER_MANAGE, "平台：成員與邀請管理（專案）")
        .simple(P, PLATFORM_PROJECT_MANAGE, "平台：專案設定（專案）")
        .role(
            O,
            PLATFORM_ORG_ADMIN_ROLE,
            "組織管理員",
            &[PLATFORM_RBAC_MANAGE, PLATFORM_ORG_MANAGE, PLATFORM_MEMBER_MANAGE, PLATFORM_PROJECT_MANAGE],
        )
        .role(P, PLATFORM_PROJECT_ADMIN_ROLE, "專案管理員", &[PLATFORM_MEMBER_MANAGE, PLATFORM_PROJECT_MANAGE])
}

// ── /me ──

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScopeQuery {
    org_id: Option<i64>,
    project_id: Option<i64>,
    resource_identifier: Option<String>,
}

/// 專案的組織一律由 server 推導。
async fn scope_from(core: &CoreRef, org: Option<i64>, project: Option<i64>, resource: Option<String>) -> ApiResult<ScopeRef> {
    let mut org = org;
    if let Some(p) = project {
        let p_org: i64 = sqlx::query_scalar("SELECT organization_id FROM tbl_project WHERE id = $1 AND deleted_at IS NULL")
            .bind(p)
            .fetch_optional(&core.pool)
            .await?
            .ok_or_else(|| ApiError::not_found("project_not_found", "project not found"))?;
        if org.is_some() && org != Some(p_org) {
            return Err(ApiError::bad_request("scope_mismatch", "project does not belong to organization"));
        }
        org = Some(p_org);
    }
    let org = org.ok_or_else(|| ApiError::bad_request("invalid_scope", "orgId or projectId is required"))?;
    Ok(match (project, resource.filter(|r| !r.is_empty())) {
        (p, Some(r)) => ScopeRef::resource(org, p, &r),
        (Some(p), None) => ScopeRef::project(org, p),
        (None, None) => ScopeRef::org(org),
    })
}

async fn me_permissions(core: web::Data<CoreRef>, user: CurrentUser, q: web::Query<ScopeQuery>) -> ApiResult<HttpResponse> {
    let q = q.into_inner();
    let scope = scope_from(&core, q.org_id, q.project_id, q.resource_identifier).await?;
    let eff = core.rbac.effective_permissions(user.id, &scope).await?;
    Ok(HttpResponse::Ok().json(eff))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CheckBody {
    permissions: Vec<String>,
    org_id: Option<i64>,
    project_id: Option<i64>,
    resource_identifier: Option<String>,
}

async fn me_check(core: web::Data<CoreRef>, user: CurrentUser, body: web::Json<CheckBody>) -> ApiResult<HttpResponse> {
    let b = body.into_inner();
    let scope = scope_from(&core, b.org_id, b.project_id, b.resource_identifier).await?;
    let perms: Vec<&str> = b.permissions.iter().map(String::as_str).collect();
    let r = core.rbac.check(user.id, &scope, &perms).await?;
    Ok(HttpResponse::Ok().json(r))
}

#[derive(Deserialize)]
struct PermQuery {
    permission: String,
}

/// 我在哪些組織持有某 permission（組織層判定）。
async fn me_orgs_with_permission(core: web::Data<CoreRef>, user: CurrentUser, q: web::Query<PermQuery>) -> ApiResult<HttpResponse> {
    let orgs: Vec<tenancy::OrgRow> = if core.rbac.is_super_user(user.id).await? {
        sqlx::query_as(&format!("SELECT {} FROM tbl_organization WHERE deleted_at IS NULL ORDER BY id", tenancy::ORG_COLS))
            .fetch_all(&core.pool)
            .await?
    } else {
        let grants = core.rbac.load_grants(user.id).await?;
        let mut ids: Vec<i64> = grants
            .iter()
            .filter(|g| g.project.is_none() && g.resource.is_empty() && g.permissions.contains(&q.permission))
            .filter_map(|g| g.org)
            .collect();
        ids.sort();
        ids.dedup();
        sqlx::query_as(&format!(
            "SELECT {} FROM tbl_organization WHERE deleted_at IS NULL AND id = ANY($1) ORDER BY id",
            tenancy::ORG_COLS
        ))
        .bind(&ids)
        .fetch_all(&core.pool)
        .await?
    };
    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": orgs })))
}

// ── 組織內的角色與 permission ──

/// 列表：要能管 RBAC 或管成員（邀請、指派時要看得到角色）。
async fn org_roles(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>) -> ApiResult<HttpResponse> {
    let org = path.into_inner();
    core.rbac.authorize_org(&user, org, &[PLATFORM_MEMBER_MANAGE]).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": custom::list_roles(&core.pool, Some(org)).await? })))
}

async fn org_permissions(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>) -> ApiResult<HttpResponse> {
    let org = path.into_inner();
    core.rbac.authorize_org(&user, org, &[PLATFORM_MEMBER_MANAGE]).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": custom::list_permissions(&core.pool, Some(org)).await? })))
}

async fn org_create_role(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>, body: web::Json<CreateRoleInput>) -> ApiResult<HttpResponse> {
    let org = path.into_inner();
    core.rbac.authorize_org(&user, org, &[PLATFORM_RBAC_MANAGE]).await?;
    let r = custom::create_role(&core.pool, &body, Some(org)).await?;
    Ok(HttpResponse::Created().json(r))
}

async fn org_update_role(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<(i64, i64)>, body: web::Json<UpdateRoleInput>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    core.rbac.authorize_org(&user, org, &[PLATFORM_RBAC_MANAGE]).await?;
    let r = custom::update_role(&core.pool, id, &body, Some(org)).await?;
    core.rbac.invalidate_all();
    Ok(HttpResponse::Ok().json(r))
}

async fn org_delete_role(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    core.rbac.authorize_org(&user, org, &[PLATFORM_RBAC_MANAGE]).await?;
    custom::delete_role(&core.pool, id, Some(org), core.clock.now()).await?;
    core.rbac.invalidate_all();
    Ok(HttpResponse::NoContent().finish())
}

async fn org_create_permission(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>, body: web::Json<CreatePermissionInput>) -> ApiResult<HttpResponse> {
    let org = path.into_inner();
    core.rbac.authorize_org(&user, org, &[PLATFORM_RBAC_MANAGE]).await?;
    let r = custom::create_permission(&core.pool, &body, Some(org)).await?;
    Ok(HttpResponse::Created().json(r))
}

async fn org_update_permission(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<(i64, i64)>, body: web::Json<UpdatePermissionInput>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    core.rbac.authorize_org(&user, org, &[PLATFORM_RBAC_MANAGE]).await?;
    let r = custom::update_permission(&core.pool, id, &body, Some(org)).await?;
    core.rbac.invalidate_all();
    Ok(HttpResponse::Ok().json(r))
}

async fn org_delete_permission(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    core.rbac.authorize_org(&user, org, &[PLATFORM_RBAC_MANAGE]).await?;
    custom::delete_permission(&core.pool, id, Some(org), core.clock.now()).await?;
    core.rbac.invalidate_all();
    Ok(HttpResponse::NoContent().finish())
}

// ── grant ──

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct GrantView {
    id: i64,
    user_id: i64,
    user_name: String,
    user_email: Option<String>,
    role_id: i64,
    role_unique_name: String,
    role_name: String,
    role_scope: String,
    organization_id: Option<i64>,
    project_id: Option<i64>,
    project_name: Option<String>,
    resource_identifier: String,
    added_by_user_id: Option<i64>,
    revoked_by_user_id: Option<i64>,
    revoked_at: Option<DateTime<Utc>>,
    revoked_reason: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GrantQuery {
    user_id: Option<i64>,
    role_id: Option<i64>,
    project_id: Option<i64>,
    #[serde(default)]
    include_revoked: bool,
}

/// 組織內（含其下專案、資源）的 grant。
pub async fn list_org_grants(
    pool: &sqlx::PgPool,
    org: i64,
    user_id: Option<i64>,
    role_id: Option<i64>,
    project_id: Option<i64>,
    include_revoked: bool,
) -> ApiResult<serde_json::Value> {
    let rows: Vec<GrantView> = sqlx::query_as(
        "SELECT g.id, g.user_id, u.name AS user_name, u.email AS user_email, g.role_id, r.unique_name AS role_unique_name,
                r.name AS role_name, r.scope AS role_scope, g.organization_id, g.project_id, p.name AS project_name,
                g.resource_identifier, g.added_by_user_id, g.revoked_by_user_id, g.revoked_at, g.revoked_reason, g.created_at
           FROM tbl_user_rbac_role g
           JOIN tbl_user u ON u.id = g.user_id
           JOIN tbl_rbac_role r ON r.id = g.role_id
           LEFT JOIN tbl_project p ON p.id = g.project_id
          WHERE (g.organization_id = $1 OR p.organization_id = $1)
            AND ($2::bigint IS NULL OR g.user_id = $2)
            AND ($3::bigint IS NULL OR g.role_id = $3)
            AND ($4::bigint IS NULL OR g.project_id = $4)
            AND ($5 OR g.deleted_at IS NULL)
          ORDER BY g.id",
    )
    .bind(org)
    .bind(user_id)
    .bind(role_id)
    .bind(project_id)
    .bind(include_revoked)
    .fetch_all(pool)
    .await?;
    Ok(serde_json::json!({ "items": rows }))
}

async fn org_grants(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>, q: web::Query<GrantQuery>) -> ApiResult<HttpResponse> {
    let org = path.into_inner();
    core.rbac.authorize_org(&user, org, &[PLATFORM_MEMBER_MANAGE]).await?;
    Ok(HttpResponse::Ok().json(list_org_grants(&core.pool, org, q.user_id, q.role_id, q.project_id, q.include_revoked).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateGrantBody {
    user_id: i64,
    role_id: Option<i64>,
    role_unique_name: Option<String>,
    project_id: Option<i64>,
    resource_identifier: Option<String>,
}

/// 授權。被授權者必須是組織成員（專案層還要是專案成員）。
async fn org_create_grant(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>, body: web::Json<CreateGrantBody>) -> ApiResult<HttpResponse> {
    let org = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, &[PLATFORM_RBAC_MANAGE]).await?;
    let b = body.into_inner();
    let mut tx = core.pool.begin().await?;
    if !tenancy::is_org_member(&mut tx, b.user_id, org).await? {
        return Err(ApiError::bad_request("not_a_member", "grantee is not a member of this organization"));
    }
    let role_id = match (b.role_id, &b.role_unique_name) {
        (Some(id), _) => id,
        (None, Some(n)) => {
            let scope = if b.resource_identifier.as_deref().is_some_and(|r| !r.is_empty()) {
                RbacScope::Resource
            } else if b.project_id.is_some() {
                RbacScope::Project
            } else {
                RbacScope::Organization
            };
            grant::role_by_unique_name(&mut tx, scope, n).await?.id
        }
        (None, None) => return Err(ApiError::bad_request("role_required", "roleId or roleUniqueName is required")),
    };
    if let Some(p) = b.project_id {
        let member: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM mn_user_project WHERE user_id = $1 AND project_id = $2 AND deleted_at IS NULL)",
        )
        .bind(b.user_id)
        .bind(p)
        .fetch_one(&mut *tx)
        .await?;
        if !member {
            return Err(ApiError::bad_request("not_a_project_member", "grantee is not a member of this project"));
        }
    }
    let row: GrantRow = grant::grant(
        &mut tx,
        &GrantInput {
            user_id: b.user_id,
            role_id,
            organization_id: Some(org),
            project_id: b.project_id,
            resource_identifier: b.resource_identifier,
        },
        access.user_id(),
    )
    .await?;
    core.rbac.commit(tx, &[b.user_id]).await?;
    Ok(HttpResponse::Created().json(row))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RevokeBody {
    reason: Option<String>,
}

/// 撤銷。撤完若組織已沒有人能管 RBAC 就拒絕（「最後一位管理者」）。
async fn org_revoke_grant(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<(i64, i64)>, body: Option<web::Json<RevokeBody>>) -> ApiResult<HttpResponse> {
    let (org, grant_id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, &[PLATFORM_RBAC_MANAGE]).await?;
    let mut tx = core.pool.begin().await?;
    let g = grant::grant_by_id(&mut tx, grant_id).await?;
    let g_org = match (g.organization_id, g.project_id) {
        (Some(o), _) => o,
        (None, Some(p)) => sqlx::query_scalar("SELECT organization_id FROM tbl_project WHERE id = $1").bind(p).fetch_one(&mut *tx).await?,
        _ => -1,
    };
    if g_org != org {
        return Err(ApiError::not_found("grant_not_found", "grant not found"));
    }
    let reason = body.as_ref().and_then(|b| b.reason.clone());
    let row = grant::revoke(&mut tx, grant_id, access.user_id(), reason.as_deref(), core.clock.now()).await?;
    grant::ensure_still_governed(&mut tx, org, grant::PLATFORM_GOVERNANCE).await?;
    core.rbac.commit(tx, &[row.user_id]).await?;
    Ok(HttpResponse::Ok().json(row))
}

async fn role_holders(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, role_id) = path.into_inner();
    core.rbac.authorize_org(&user, org, &[PLATFORM_MEMBER_MANAGE]).await?;
    Ok(HttpResponse::Ok().json(list_org_grants(&core.pool, org, None, Some(role_id), None, false).await?))
}

// ── 全域（超級使用者） ──

async fn require_super(core: &CoreRef, user: &CurrentUser) -> ApiResult<()> {
    if core.rbac.is_super_user(user.id).await? {
        Ok(())
    } else {
        Err(ApiError::forbidden("super_user_required", "super user only"))
    }
}

async fn admin_roles(core: web::Data<CoreRef>, user: CurrentUser) -> ApiResult<HttpResponse> {
    require_super(&core, &user).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": custom::list_roles(&core.pool, None).await? })))
}

async fn admin_permissions(core: web::Data<CoreRef>, user: CurrentUser) -> ApiResult<HttpResponse> {
    require_super(&core, &user).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": custom::list_permissions(&core.pool, None).await? })))
}

async fn admin_create_role(core: web::Data<CoreRef>, user: CurrentUser, body: web::Json<CreateRoleInput>) -> ApiResult<HttpResponse> {
    require_super(&core, &user).await?;
    Ok(HttpResponse::Created().json(custom::create_role(&core.pool, &body, None).await?))
}

async fn admin_update_role(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>, body: web::Json<UpdateRoleInput>) -> ApiResult<HttpResponse> {
    require_super(&core, &user).await?;
    let r = custom::update_role(&core.pool, path.into_inner(), &body, None).await?;
    core.rbac.invalidate_all();
    Ok(HttpResponse::Ok().json(r))
}

async fn admin_delete_role(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>) -> ApiResult<HttpResponse> {
    require_super(&core, &user).await?;
    custom::delete_role(&core.pool, path.into_inner(), None, core.clock.now()).await?;
    core.rbac.invalidate_all();
    Ok(HttpResponse::NoContent().finish())
}

async fn admin_create_permission(core: web::Data<CoreRef>, user: CurrentUser, body: web::Json<CreatePermissionInput>) -> ApiResult<HttpResponse> {
    require_super(&core, &user).await?;
    Ok(HttpResponse::Created().json(custom::create_permission(&core.pool, &body, None).await?))
}

async fn admin_update_permission(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>, body: web::Json<UpdatePermissionInput>) -> ApiResult<HttpResponse> {
    require_super(&core, &user).await?;
    let r = custom::update_permission(&core.pool, path.into_inner(), &body, None).await?;
    core.rbac.invalidate_all();
    Ok(HttpResponse::Ok().json(r))
}

async fn admin_delete_permission(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>) -> ApiResult<HttpResponse> {
    require_super(&core, &user).await?;
    custom::delete_permission(&core.pool, path.into_inner(), None, core.clock.now()).await?;
    core.rbac.invalidate_all();
    Ok(HttpResponse::NoContent().finish())
}

impl Plugin for RbacPlugin {
    fn name(&self) -> &str {
        "rbac"
    }
    crate::plugin_boilerplate!();

    fn deps(&self, method: &str) -> Vec<&str> {
        match method {
            "start" => vec!["database"],
            _ => vec![],
        }
    }

    /// seed 要在其他 plugin 開機之前寫好（它們可能要查角色）。
    fn run_before(&self, method: &str) -> Vec<&str> {
        match method {
            "start" => vec!["storage", "user", "multitenancy", "health", "static_web"],
            _ => vec![],
        }
    }
}

#[async_trait]
impl AppPlugin for RbacPlugin {
    fn rbac_seeds(&self) -> Option<RbacSeeds> {
        Some(platform_seeds())
    }

    /// 收集所有 plugin 的 seed，驗證後寫入。
    async fn on_start(&self, ctx: &StartContext) -> Result<(), StartError> {
        let seeds = ctx.system.collect_rbac_seeds();
        let (p, r) = seed::apply(&self.core.pool, &seeds).await.map_err(|e| StartError::new(e.to_string()))?;
        self.core.rbac.invalidate_all();
        log::info!("[rbac] seeded {p} permissions, {r} roles from {} plugins", seeds.len());
        Ok(())
    }

    fn configure_routes(&self, cfg: &mut RouteRegistrar<'_>, _self_arc: &Arc<dyn AppPlugin>) {
        cfg.get("/api/v1/me/permissions", web::get().to(me_permissions));
        cfg.post("/api/v1/me/check", web::post().to(me_check));
        cfg.get("/api/v1/me/orgs-with-permission", web::get().to(me_orgs_with_permission));

        cfg.get("/api/v1/orgs/{orgId}/rbac/roles", web::get().to(org_roles));
        cfg.post("/api/v1/orgs/{orgId}/rbac/roles", web::post().to(org_create_role));
        cfg.patch("/api/v1/orgs/{orgId}/rbac/roles/{roleId}", web::patch().to(org_update_role));
        cfg.delete("/api/v1/orgs/{orgId}/rbac/roles/{roleId}", web::delete().to(org_delete_role));
        cfg.get("/api/v1/orgs/{orgId}/rbac/roles/{roleId}/holders", web::get().to(role_holders));
        cfg.get("/api/v1/orgs/{orgId}/rbac/permissions", web::get().to(org_permissions));
        cfg.post("/api/v1/orgs/{orgId}/rbac/permissions", web::post().to(org_create_permission));
        cfg.patch("/api/v1/orgs/{orgId}/rbac/permissions/{permissionId}", web::patch().to(org_update_permission));
        cfg.delete("/api/v1/orgs/{orgId}/rbac/permissions/{permissionId}", web::delete().to(org_delete_permission));
        cfg.get("/api/v1/orgs/{orgId}/rbac/grants", web::get().to(org_grants));
        cfg.post("/api/v1/orgs/{orgId}/rbac/grants", web::post().to(org_create_grant));
        cfg.post("/api/v1/orgs/{orgId}/rbac/grants/{grantId}/revoke", web::post().to(org_revoke_grant));

        cfg.get("/api/v1/admin/rbac/roles", web::get().to(admin_roles));
        cfg.post("/api/v1/admin/rbac/roles", web::post().to(admin_create_role));
        cfg.patch("/api/v1/admin/rbac/roles/{roleId}", web::patch().to(admin_update_role));
        cfg.delete("/api/v1/admin/rbac/roles/{roleId}", web::delete().to(admin_delete_role));
        cfg.get("/api/v1/admin/rbac/permissions", web::get().to(admin_permissions));
        cfg.post("/api/v1/admin/rbac/permissions", web::post().to(admin_create_permission));
        cfg.patch("/api/v1/admin/rbac/permissions/{permissionId}", web::patch().to(admin_update_permission));
        cfg.delete("/api/v1/admin/rbac/permissions/{permissionId}", web::delete().to(admin_delete_permission));
    }
}
