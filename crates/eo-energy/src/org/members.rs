//! 能源視角的成員清單（成員＋能源角色＋部門）與移除成員。

use crate::audit::{self, AuditInput};
use crate::common::{err, Ctx, RequestMeta, Tx};
use crate::gates;
use actix_web::{web, HttpResponse};
use eo_domain::energy_rbac::{get_energy_role_def, ENERGY_ROLE_DEFS};
use eo_platform::auth::CurrentUser;
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use eo_platform::plugins::multitenancy::remove_member_in_tx;
use eo_platform::rbac::OrgAccess;
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberRoleView {
    pub unique_name: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberDepartmentView {
    pub id: i64,
    pub key: String,
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberView {
    pub user_id: i64,
    pub name: String,
    pub email: Option<String>,
    /// 組織成員關係上的角色（owner／member）。
    pub org_role: String,
    pub energy_roles: Vec<MemberRoleView>,
    pub departments: Vec<MemberDepartmentView>,
}

pub async fn list_members(tx: &mut Tx<'_>, access: &OrgAccess) -> ApiResult<Vec<MemberView>> {
    let members: Vec<(i64, String, Option<String>, String)> = sqlx::query_as(
        "SELECT m.user_id, u.name, u.email, m.role FROM mn_user_organization m
           JOIN tbl_user u ON u.id = m.user_id AND u.deleted_at IS NULL
          WHERE m.organization_id = $1 AND m.deleted_at IS NULL ORDER BY m.user_id",
    )
    .bind(access.org_id())
    .fetch_all(&mut **tx)
    .await?;
    let ids: Vec<i64> = members.iter().map(|m| m.0).collect();
    let energy: Vec<&str> = ENERGY_ROLE_DEFS.iter().map(|r| r.unique_name).collect();
    let grants: Vec<(i64, String)> = sqlx::query_as(
        "SELECT DISTINCT g.user_id, r.unique_name FROM tbl_user_rbac_role g
           JOIN tbl_rbac_role r ON r.id = g.role_id AND r.deleted_at IS NULL
          WHERE g.organization_id = $1 AND g.project_id IS NULL AND g.resource_identifier = ''
            AND g.deleted_at IS NULL AND g.user_id = ANY($2) AND r.unique_name = ANY($3)
          ORDER BY g.user_id, r.unique_name",
    )
    .bind(access.org_id())
    .bind(&ids)
    .bind(&energy)
    .fetch_all(&mut **tx)
    .await?;
    let depts: Vec<(i64, i64, String, String, String)> = sqlx::query_as(
        "SELECT dm.user_id, d.id, d.key, d.name, dm.role FROM mn_energy_department_member dm
           JOIN tbl_energy_department d ON d.id = dm.department_id AND d.organization_id = $1 AND d.deleted_at IS NULL
          WHERE dm.deleted_at IS NULL AND dm.user_id = ANY($2)
          ORDER BY d.sort_order, d.id",
    )
    .bind(access.org_id())
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    Ok(members
        .into_iter()
        .map(|(uid, name, email, org_role)| MemberView {
            energy_roles: grants
                .iter()
                .filter(|g| g.0 == uid)
                .map(|g| MemberRoleView {
                    unique_name: g.1.clone(),
                    name: get_energy_role_def(&g.1).map(|d| d.name.to_string()).unwrap_or_default(),
                })
                .collect(),
            departments: depts
                .iter()
                .filter(|d| d.0 == uid)
                .map(|d| MemberDepartmentView { id: d.1, key: d.2.clone(), name: d.3.clone(), role: d.4.clone() })
                .collect(),
            user_id: uid,
            name,
            email,
            org_role,
        })
        .collect())
}

/// 移出組織：撤掉所有授權、成員關係與本組織的部門成員列；撤完若沒人持有治理權就整筆拒絕。
pub async fn remove_member(tx: &mut Tx<'_>, ctx: &Ctx, member_id: i64) -> ApiResult<()> {
    let now = ctx.now();
    let revoked = remove_member_in_tx(tx, &ctx.access, member_id, gates::GOVERNANCE, now).await.map_err(|e| {
        if e.code == "last_governor" {
            err(400, "cannot_remove_last_admin")
        } else {
            e
        }
    })?;
    // 被移出的人不可再被部門規則解析成候選人。
    sqlx::query(
        "UPDATE mn_energy_department_member SET deleted_at = $3 WHERE user_id = $1 AND deleted_at IS NULL
           AND department_id IN (SELECT id FROM tbl_energy_department WHERE organization_id = $2)",
    )
    .bind(member_id)
    .bind(ctx.org_id())
    .bind(now)
    .execute(&mut **tx)
    .await?;
    let label = crate::common::user_name(tx, member_id).await?;
    let who = label.clone().unwrap_or_else(|| format!("#{member_id}"));
    for g in revoked {
        let role: Option<String> =
            sqlx::query_scalar("SELECT unique_name FROM tbl_rbac_role WHERE id = $1").bind(g.role_id).fetch_optional(&mut **tx).await?;
        let Some(def) = role.as_deref().and_then(get_energy_role_def) else { continue };
        audit::record(
            tx,
            ctx,
            AuditInput::new("member.role_changed", member_id, format!("移出組織：撤銷「{who}」的角色「{}」", def.name))
                .label_opt(label.clone())
                .payload(json!({ "action": "revoked", "roleUniqueName": def.unique_name, "roleName": def.name,
                                 "grantId": g.id, "reason": "member removed" })),
        )
        .await?;
    }
    Ok(())
}

pub async fn list(core: web::Data<CoreRef>, user: CurrentUser, org: web::Path<i64>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::ASSIGN_ROLE).await?;
    let mut tx = core.pool.begin().await?;
    let items = list_members(&mut tx, &access).await?;
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

pub async fn remove(core: web::Data<CoreRef>, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, member) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::ASSIGN_ROLE).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    remove_member(&mut tx, &ctx, member).await?;
    core.rbac.commit(tx, &[member]).await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}
