//! 授權寫入：所有 grant／revoke 都經過這裡。
//!
//! 函式都收呼叫端的交易（稽核紀錄要跟授權寫在同一個交易），回傳受影響的使用者；
//! 呼叫端用 `Rbac::commit` 提交後才讓快取失效——commit 前失效，併發的讀者會把舊狀態寫回快取。

use super::{Rbac, PLATFORM_RBAC_MANAGE};
use crate::error::{ApiError, ApiResult};
use chrono::{DateTime, Utc};
use eo_domain::rbac::RbacScope;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, Transaction};

pub type Tx<'a> = Transaction<'a, Postgres>;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantInput {
    pub user_id: i64,
    pub role_id: i64,
    pub organization_id: Option<i64>,
    pub project_id: Option<i64>,
    #[serde(default)]
    pub resource_identifier: Option<String>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct GrantRow {
    pub id: i64,
    pub user_id: i64,
    pub role_id: i64,
    pub organization_id: Option<i64>,
    pub project_id: Option<i64>,
    pub resource_identifier: String,
    pub added_by_user_id: Option<i64>,
    pub revoked_by_user_id: Option<i64>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoked_reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

const GRANT_COLS: &str = "id, user_id, role_id, organization_id, project_id, resource_identifier, added_by_user_id, revoked_by_user_id, revoked_at, revoked_reason, created_at";

#[derive(Debug, Clone, FromRow)]
pub struct RoleInfo {
    pub id: i64,
    pub scope: String,
    pub unique_name: String,
    pub name: String,
    pub owning_organization_id: Option<i64>,
}

pub async fn role_by_id(tx: &mut Tx<'_>, role_id: i64) -> ApiResult<RoleInfo> {
    sqlx::query_as("SELECT id, scope, unique_name, name, owning_organization_id FROM tbl_rbac_role WHERE id = $1 AND deleted_at IS NULL")
        .bind(role_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ApiError::not_found("role_not_found", "role not found"))
}

pub async fn role_by_unique_name(tx: &mut Tx<'_>, scope: RbacScope, unique_name: &str) -> ApiResult<RoleInfo> {
    sqlx::query_as(
        "SELECT id, scope, unique_name, name, owning_organization_id FROM tbl_rbac_role WHERE scope = $1 AND unique_name = $2 AND deleted_at IS NULL",
    )
    .bind(scope.as_str())
    .bind(unique_name)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| ApiError::not_found("role_not_found", format!("role {unique_name} not found")))
}

/// scope 形狀要對：組織層只有 org；專案層有 project（org 由 server 從專案推導）；資源層有 resource。
fn grant_scope(input: &GrantInput) -> ApiResult<RbacScope> {
    let res = input.resource_identifier.as_deref().unwrap_or("");
    if !res.is_empty() {
        if input.organization_id.is_none() {
            return Err(ApiError::bad_request("invalid_scope", "resource grant needs organizationId"));
        }
        return Ok(RbacScope::Resource);
    }
    if input.project_id.is_some() {
        return Ok(RbacScope::Project);
    }
    if input.organization_id.is_some() {
        return Ok(RbacScope::Organization);
    }
    Err(ApiError::bad_request("invalid_scope", "grant needs organizationId, projectId or resourceIdentifier"))
}

/// 授權（冪等：已有相同的有效 grant 就回那一列）。
pub async fn grant(tx: &mut Tx<'_>, input: &GrantInput, added_by: Option<i64>) -> ApiResult<GrantRow> {
    let scope = grant_scope(input)?;
    let role = role_by_id(tx, input.role_id).await?;
    if role.scope != scope.as_str() {
        return Err(ApiError::bad_request(
            "scope_mismatch",
            format!("role {} is {}-scoped but the grant is {}", role.unique_name, role.scope, scope.as_str()),
        ));
    }
    let mut org = input.organization_id;
    if let Some(p) = input.project_id {
        let p_org: i64 = sqlx::query_scalar("SELECT organization_id FROM tbl_project WHERE id = $1 AND deleted_at IS NULL")
            .bind(p)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(|| ApiError::not_found("project_not_found", "project not found"))?;
        if org.is_some() && org != Some(p_org) {
            return Err(ApiError::bad_request("scope_mismatch", "project does not belong to organization"));
        }
        org = Some(p_org);
    }
    if let (Some(owner), Some(o)) = (role.owning_organization_id, org) {
        if owner != o {
            return Err(ApiError::forbidden("role_not_available", "role belongs to another organization"));
        }
    }
    let res = input.resource_identifier.clone().unwrap_or_default();
    let existing: Option<GrantRow> = sqlx::query_as(&format!(
        "SELECT {GRANT_COLS} FROM tbl_user_rbac_role
          WHERE user_id = $1 AND role_id = $2 AND COALESCE(organization_id, 0) = COALESCE($3, 0)
            AND COALESCE(project_id, 0) = COALESCE($4, 0) AND resource_identifier = $5 AND deleted_at IS NULL"
    ))
    .bind(input.user_id)
    .bind(input.role_id)
    .bind(org)
    .bind(input.project_id)
    .bind(&res)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(e) = existing {
        return Ok(e);
    }
    let row = sqlx::query_as(&format!(
        "INSERT INTO tbl_user_rbac_role (user_id, role_id, organization_id, project_id, resource_identifier, added_by_user_id)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING {GRANT_COLS}"
    ))
    .bind(input.user_id)
    .bind(input.role_id)
    .bind(org)
    .bind(input.project_id)
    .bind(&res)
    .bind(added_by)
    .fetch_one(&mut **tx)
    .await?;
    Ok(row)
}

pub async fn grant_by_id(tx: &mut Tx<'_>, grant_id: i64) -> ApiResult<GrantRow> {
    sqlx::query_as(&format!("SELECT {GRANT_COLS} FROM tbl_user_rbac_role WHERE id = $1 AND deleted_at IS NULL"))
        .bind(grant_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ApiError::not_found("grant_not_found", "grant not found"))
}

/// 撤銷＝軟刪除並記下誰、何時、為什麼。
pub async fn revoke(tx: &mut Tx<'_>, grant_id: i64, by: Option<i64>, reason: Option<&str>, now: DateTime<Utc>) -> ApiResult<GrantRow> {
    let row: Option<GrantRow> = sqlx::query_as(&format!(
        "UPDATE tbl_user_rbac_role
            SET revoked_by_user_id = $2, revoked_at = $3, revoked_reason = $4, deleted_at = $3
          WHERE id = $1 AND deleted_at IS NULL RETURNING {GRANT_COLS}"
    ))
    .bind(grant_id)
    .bind(by)
    .bind(now)
    .bind(reason)
    .fetch_optional(&mut **tx)
    .await?;
    row.ok_or_else(|| ApiError::not_found("grant_not_found", "grant not found"))
}

/// 撤掉某人在某組織（含其下專案、資源）的所有 grant。移除成員時用。
pub async fn revoke_all_in_org(
    tx: &mut Tx<'_>,
    user_id: i64,
    org_id: i64,
    by: Option<i64>,
    reason: &str,
    now: DateTime<Utc>,
) -> ApiResult<Vec<GrantRow>> {
    let rows = sqlx::query_as(&format!(
        "UPDATE tbl_user_rbac_role g
            SET revoked_by_user_id = $3, revoked_at = $4, revoked_reason = $5, deleted_at = $4
          WHERE g.user_id = $1 AND g.deleted_at IS NULL
            AND (g.organization_id = $2 OR g.project_id IN (SELECT id FROM tbl_project WHERE organization_id = $2))
          RETURNING {GRANT_COLS}"
    ))
    .bind(user_id)
    .bind(org_id)
    .bind(by)
    .bind(now)
    .bind(reason)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows)
}

/// 組織內（組織層 grant）持有某 permission 的人；交易內讀，給「最後一位」防呆用。
pub async fn holders_in_tx(tx: &mut Tx<'_>, org_id: i64, permission: &str) -> ApiResult<Vec<i64>> {
    let ids = sqlx::query_scalar(
        "SELECT DISTINCT g.user_id
           FROM tbl_user_rbac_role g
           JOIN tbl_rbac_role r ON r.id = g.role_id AND r.deleted_at IS NULL
           JOIN mn_rbac_role_permission rp ON rp.role_id = r.id
           JOIN tbl_rbac_permission p ON p.id = rp.permission_id AND p.deleted_at IS NULL
           LEFT JOIN mn_rbac_complex_permission cp ON cp.complex_one_id = p.id AND p.complex
           LEFT JOIN tbl_rbac_permission sp ON sp.id = cp.simple_one_id AND sp.deleted_at IS NULL
           JOIN tbl_user u ON u.id = g.user_id AND u.deleted_at IS NULL AND NOT u.disabled
          WHERE g.deleted_at IS NULL AND g.organization_id = $1 AND g.project_id IS NULL
            AND g.resource_identifier = '' AND (p.unique_name = $2 OR sp.unique_name = $2)
          ORDER BY g.user_id",
    )
    .bind(org_id)
    .bind(permission)
    .fetch_all(&mut **tx)
    .await?;
    Ok(ids)
}

/// 「最後一位」防呆：在變更已寫入（尚未 commit）之後呼叫，若組織內已沒人持有任何一個治理 permission 就拒絕。
pub async fn ensure_still_governed(tx: &mut Tx<'_>, org_id: i64, permissions: &[&str]) -> ApiResult<()> {
    for p in permissions {
        if !holders_in_tx(tx, org_id, p).await?.is_empty() {
            return Ok(());
        }
    }
    Err(ApiError::conflict("last_governor", "this would leave the organization without an administrator")
        .with_details(serde_json::json!({ "permissions": permissions })))
}

/// 平台層的治理 permission（任一）。
pub const PLATFORM_GOVERNANCE: &[&str] = &[PLATFORM_RBAC_MANAGE];

impl Rbac {
    /// 提交交易，之後才讓受影響的使用者快取失效。
    pub async fn commit(&self, tx: Tx<'_>, affected_users: &[i64]) -> ApiResult<()> {
        tx.commit().await?;
        for u in affected_users {
            self.invalidate_user(*u);
        }
        Ok(())
    }
}
