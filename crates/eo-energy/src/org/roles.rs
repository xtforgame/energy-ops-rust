//! 能源角色指派。兩道閘：一般閘 `energy:member:manage`（端點層）；
//! 特權角色（admin／CXO）另要 `energy:role:assign-privileged`，撤銷也一樣（否則 CXO 能把 admin 降級）。
//! 撤銷治理角色時再過「最後一位治理者」防呆（`energy:govern:org` ∪ `platform:rbac:manage`）。

use crate::audit::{self, AuditInput};
use crate::common::{err, parse_body, user_name, Ctx, RequestMeta, Tx};
use crate::gates;
use actix_web::{web, HttpResponse};
use eo_domain::energy_rbac::{get_energy_role_def, is_energy_role, is_privileged_energy_role, ENERGY_ROLE_DEFS, ORG_GOVERNANCE_PERMISSION};
use eo_domain::rbac::RbacScope;
use eo_domain::schemas::{RoleAssignmentInput, UserIdInput};
use eo_platform::auth::CurrentUser;
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use eo_platform::rbac::grant::{self, GrantInput};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignableRoleView {
    pub id: i64,
    pub unique_name: String,
    pub name: String,
    pub description: String,
    /// true → 指派它需要 `energy:role:assign-privileged`。
    pub privileged: bool,
}

/// 能源的五個具名角色（前端下拉的來源；不露出平台角色語彙）。
pub async fn assignable_roles(tx: &mut Tx<'_>) -> ApiResult<Vec<AssignableRoleView>> {
    let names: Vec<&str> = ENERGY_ROLE_DEFS.iter().map(|r| r.unique_name).collect();
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, unique_name FROM tbl_rbac_role WHERE scope = 'organization' AND unique_name = ANY($1) AND deleted_at IS NULL ORDER BY id",
    )
    .bind(&names)
    .fetch_all(&mut **tx)
    .await?;
    Ok(ENERGY_ROLE_DEFS
        .iter()
        .filter_map(|d| {
            rows.iter().find(|r| r.1 == d.unique_name).map(|r| AssignableRoleView {
                id: r.0,
                unique_name: d.unique_name.into(),
                name: d.name.into(),
                description: d.description.into(),
                privileged: is_privileged_energy_role(d.unique_name),
            })
        })
        .collect())
}

fn target_user(id: &UserIdInput) -> ApiResult<i64> {
    match id {
        UserIdInput::Id(n) => Ok(*n),
        UserIdInput::Text(s) => crate::common::numeric_id(s).ok_or_else(|| err(400, "invalid_user_id")),
    }
}

async fn check_role_request(ctx: &Ctx, role: &str) -> ApiResult<()> {
    if !is_energy_role(role) {
        return Err(err(400, "unknown_role"));
    }
    if is_privileged_energy_role(role) && !ctx.has(gates::ASSIGN_PRIVILEGED[0]).await? {
        return Err(err(403, "cannot_assign_privileged_role"));
    }
    Ok(())
}

/// 指派。回傳是否新建（已有相同授權時冪等）與受影響的使用者。
pub async fn assign_role(tx: &mut Tx<'_>, ctx: &Ctx, target: i64, role: &str, reason: Option<&str>) -> ApiResult<bool> {
    check_role_request(ctx, role).await?;
    let label = user_name(tx, target).await?;
    if label.is_none() {
        return Err(err(404, "user_not_found"));
    }
    let r = grant::role_by_unique_name(tx, RbacScope::Organization, role).await.map_err(|_| err(500, "role_not_seeded"))?;
    let before: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM tbl_user_rbac_role WHERE user_id = $1 AND role_id = $2 AND organization_id = $3
            AND project_id IS NULL AND resource_identifier = '' AND deleted_at IS NULL LIMIT 1",
    )
    .bind(target)
    .bind(r.id)
    .bind(ctx.org_id())
    .fetch_optional(&mut **tx)
    .await?;
    let row = grant::grant(
        tx,
        &GrantInput { user_id: target, role_id: r.id, organization_id: Some(ctx.org_id()), project_id: None, resource_identifier: None },
        ctx.user_id(),
    )
    .await?;
    let def = get_energy_role_def(role);
    let who = label.clone().unwrap_or_else(|| format!("#{target}"));
    audit::record(
        tx,
        ctx,
        AuditInput::new("member.role_changed", target, format!("授予「{who}」角色「{}」", def.map(|d| d.name).unwrap_or(role)))
            .label_opt(label)
            .payload(json!({
                "action": "granted", "roleUniqueName": role, "roleName": def.map(|d| d.name),
                "grantId": row.id, "reason": reason,
            })),
    )
    .await?;
    Ok(before.is_none())
}

/// 撤銷本組織（組織層）這個角色的所有有效授權。回傳撤了幾筆。
pub async fn revoke_role(tx: &mut Tx<'_>, ctx: &Ctx, target: i64, role: &str, reason: Option<&str>) -> ApiResult<u64> {
    check_role_request(ctx, role).await?;
    let def = get_energy_role_def(role).ok_or_else(|| err(400, "unknown_role"))?;
    let r = grant::role_by_unique_name(tx, RbacScope::Organization, role).await.map_err(|_| err(500, "role_not_seeded"))?;
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM tbl_user_rbac_role WHERE user_id = $1 AND role_id = $2 AND organization_id = $3
            AND project_id IS NULL AND resource_identifier = '' AND deleted_at IS NULL ORDER BY id",
    )
    .bind(target)
    .bind(r.id)
    .bind(ctx.org_id())
    .fetch_all(&mut **tx)
    .await?;
    let now = ctx.now();
    let fallback_reason = format!("energy-ops role revoke by {}", ctx.user_id().unwrap_or(0));
    for id in &ids {
        grant::revoke(tx, *id, ctx.user_id(), Some(reason.unwrap_or(&fallback_reason)), now).await?;
    }
    // 撤的是治理角色時：撤完若組織已無人持有治理權就整筆拒絕。
    if !ids.is_empty() && def.permissions.contains(&ORG_GOVERNANCE_PERMISSION) {
        grant::ensure_still_governed(tx, ctx.org_id(), gates::GOVERNANCE).await.map_err(|_| err(400, "cannot_remove_last_admin"))?;
    }
    if !ids.is_empty() {
        let label = user_name(tx, target).await?;
        let who = label.clone().unwrap_or_else(|| format!("#{target}"));
        audit::record(
            tx,
            ctx,
            AuditInput::new("member.role_changed", target, format!("撤銷「{who}」的角色「{}」", def.name))
                .label_opt(label)
                .payload(json!({ "action": "revoked", "roleUniqueName": role, "roleName": def.name, "reason": reason })),
        )
        .await?;
    }
    Ok(ids.len() as u64)
}

// ── handlers ──

pub async fn assignable(core: web::Data<CoreRef>, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::ASSIGN_ROLE).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    let roles = assignable_roles(&mut tx).await?;
    // 前端據此 disable 特權選項（純 UX，真正的閘在指派時）。
    let can = ctx.has(gates::ASSIGN_PRIVILEGED[0]).await?;
    Ok(HttpResponse::Ok().json(json!({ "items": roles, "canAssignPrivileged": can })))
}

pub async fn assign(
    core: web::Data<CoreRef>,
    user: CurrentUser,
    meta: RequestMeta,
    org: web::Path<i64>,
    body: web::Json<Value>,
) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::ASSIGN_ROLE).await?;
    let input: RoleAssignmentInput = parse_body(&body)?;
    let target = target_user(&input.user_id)?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    let created = assign_role(&mut tx, &ctx, target, &input.role_unique_name, input.reason.as_deref()).await?;
    core.rbac.commit(tx, &[target]).await?;
    Ok(HttpResponse::Ok().json(json!({ "created": created })))
}

pub async fn revoke(
    core: web::Data<CoreRef>,
    user: CurrentUser,
    meta: RequestMeta,
    org: web::Path<i64>,
    body: web::Json<Value>,
) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::ASSIGN_ROLE).await?;
    let input: RoleAssignmentInput = parse_body(&body)?;
    let target = target_user(&input.user_id)?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    let revoked = revoke_role(&mut tx, &ctx, target, &input.role_unique_name, input.reason.as_deref()).await?;
    core.rbac.commit(tx, &[target]).await?;
    Ok(HttpResponse::Ok().json(json!({ "revoked": revoked })))
}
