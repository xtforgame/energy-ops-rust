//! 一個帳號的整體關係總覽（跨組織，只准超級使用者看）：
//! `GET /api/v1/admin/users/{userId}/relations`。
//! 不走 `E/` 前綴——這支本來就是跨組織查的，不綁單一組織的 `OrgAccess`。
//! 字串（部門完整路徑、授權範圍名稱）一律由後端組好，前端只負責印。

use crate::common::err;
use actix_web::{web, HttpResponse};
use eo_platform::auth::CurrentUser;
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use serde::Serialize;
use sqlx::PgPool;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RelationOrg {
    pub organization_id: i64,
    pub organization_name: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RelationProject {
    pub project_id: i64,
    pub project_name: String,
    pub organization_id: i64,
    pub organization_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationDepartment {
    pub department_id: i64,
    /// 完整路徑，例「總管理部 › 會計部門」。
    pub path: String,
    pub organization_name: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RelationGrant {
    pub role_unique_name: String,
    pub role_name: String,
    /// organization／project／resource。
    pub scope: String,
    pub scope_name: String,
    pub granted_at: chrono::DateTime<chrono::Utc>,
    pub granted_by: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserRelations {
    pub organizations: Vec<RelationOrg>,
    pub projects: Vec<RelationProject>,
    pub departments: Vec<RelationDepartment>,
    pub grants: Vec<RelationGrant>,
}

async fn require_super(core: &CoreRef, user: &CurrentUser) -> ApiResult<()> {
    if core.rbac.is_super_user(user.id).await? {
        Ok(())
    } else {
        Err(err(403, "super_user_required"))
    }
}

/// 遞迴 CTE 由下往上拼出完整路徑；depth 由大到小排＝從根到葉。
async fn department_path(pool: &PgPool, dept_id: i64) -> ApiResult<String> {
    let rows: Vec<(i32, String)> = sqlx::query_as(
        "WITH RECURSIVE anc AS (
           SELECT id, name, parent_id, 0 AS depth FROM tbl_energy_department WHERE id = $1
           UNION ALL
           SELECT d.id, d.name, d.parent_id, anc.depth + 1
             FROM tbl_energy_department d JOIN anc ON d.id = anc.parent_id
         )
         SELECT depth, name FROM anc ORDER BY depth DESC",
    )
    .bind(dept_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(_, name)| name).collect::<Vec<_>>().join(" › "))
}

pub async fn user_relations(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>) -> ApiResult<HttpResponse> {
    require_super(&core, &user).await?;
    let target = path.into_inner();
    let pool = &core.pool;

    let organizations: Vec<RelationOrg> = sqlx::query_as(
        "SELECT o.id AS organization_id, o.name AS organization_name, m.role
           FROM mn_user_organization m JOIN tbl_organization o ON o.id = m.organization_id AND o.deleted_at IS NULL
          WHERE m.user_id = $1 AND m.deleted_at IS NULL ORDER BY o.id",
    )
    .bind(target)
    .fetch_all(pool)
    .await?;

    let projects: Vec<RelationProject> = sqlx::query_as(
        "SELECT p.id AS project_id, p.name AS project_name, o.id AS organization_id, o.name AS organization_name
           FROM mn_user_project mp
           JOIN tbl_project p ON p.id = mp.project_id AND p.deleted_at IS NULL
           JOIN tbl_organization o ON o.id = p.organization_id
          WHERE mp.user_id = $1 AND mp.deleted_at IS NULL ORDER BY p.id",
    )
    .bind(target)
    .fetch_all(pool)
    .await?;

    let dept_rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT dm.department_id, dm.role, o.name
           FROM mn_energy_department_member dm
           JOIN tbl_energy_department d ON d.id = dm.department_id AND d.deleted_at IS NULL
           JOIN tbl_organization o ON o.id = d.organization_id
          WHERE dm.user_id = $1 AND dm.deleted_at IS NULL ORDER BY dm.department_id",
    )
    .bind(target)
    .fetch_all(pool)
    .await?;
    let mut departments = Vec::with_capacity(dept_rows.len());
    for (dept_id, role, org_name) in dept_rows {
        let path = department_path(pool, dept_id).await?;
        departments.push(RelationDepartment { department_id: dept_id, path, organization_name: org_name, role });
    }

    let grants: Vec<RelationGrant> = sqlx::query_as(
        "SELECT r.unique_name AS role_unique_name, r.name AS role_name, r.scope,
                COALESCE(p.name, o.name, g.resource_identifier) AS scope_name,
                g.created_at AS granted_at, gu.name AS granted_by
           FROM tbl_user_rbac_role g
           JOIN tbl_rbac_role r ON r.id = g.role_id
           LEFT JOIN tbl_organization o ON o.id = g.organization_id
           LEFT JOIN tbl_project p ON p.id = g.project_id
           LEFT JOIN tbl_user gu ON gu.id = g.added_by_user_id
          WHERE g.user_id = $1 AND g.deleted_at IS NULL AND g.revoked_at IS NULL
          ORDER BY g.id",
    )
    .bind(target)
    .fetch_all(pool)
    .await?;

    Ok(HttpResponse::Ok().json(UserRelations { organizations, projects, departments, grants }))
}
