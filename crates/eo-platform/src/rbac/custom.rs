//! 自訂（user-defined）permission 與角色的增刪改。照 az `RbacCustomDefService`：
//! · 寫入一律 type='user-defined'、created_by_plugin=NULL；system／plugin 列不可改不可刪；
//! · uniqueName 必須以 `custom:` 開頭；
//! · complex 只能包同 scope 的 simple；角色只能綁同 scope 的 permission；
//! · 還被角色引用的 permission、還有有效 grant 的角色不能刪。
//! 多組織版多一條：自訂列可以屬於某組織（owning_organization_id），只有該組織看得到、用得到；
//! 全域自訂列（NULL）只有超級使用者能建。自訂的 simple permission 也只有超級使用者能建
//! （產品碼不會檢查一個使用者自己發明的 simple permission，給組織建只會誤導）。

use crate::error::{ApiError, ApiResult};
use chrono::{DateTime, Utc};
use eo_domain::rbac::RbacScope;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Postgres, Transaction};

pub const CUSTOM_PREFIX: &str = "custom:";

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRow {
    pub id: i64,
    pub scope: String,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub row_type: String,
    pub created_by_plugin: Option<String>,
    pub unique_name: String,
    pub name: String,
    pub complex: bool,
    pub owning_organization_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    #[sqlx(default)]
    pub included: Vec<String>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RoleRow {
    pub id: i64,
    pub scope: String,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub row_type: String,
    pub created_by_plugin: Option<String>,
    pub unique_name: String,
    pub name: String,
    pub owning_organization_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    #[sqlx(default)]
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreatePermissionInput {
    pub scope: RbacScope,
    pub unique_name: String,
    pub name: String,
    #[serde(default)]
    pub complex: bool,
    #[serde(default)]
    pub included: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdatePermissionInput {
    pub name: Option<String>,
    pub included: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateRoleInput {
    pub scope: RbacScope,
    pub unique_name: String,
    pub name: String,
    #[serde(default)]
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateRoleInput {
    pub name: Option<String>,
    pub permissions: Option<Vec<String>>,
}

/// 這個組織看得到的列：system／plugin、全域自訂、本組織自訂。
const VISIBLE: &str = "(owning_organization_id IS NULL OR owning_organization_id = $1)";

pub async fn list_permissions(pool: &PgPool, org: Option<i64>) -> ApiResult<Vec<PermissionRow>> {
    let rows = sqlx::query_as(&format!(
        "SELECT p.id, p.scope, p.type, p.created_by_plugin, p.unique_name, p.name, p.complex, p.owning_organization_id, p.created_at,
                COALESCE(ARRAY(SELECT sp.unique_name FROM mn_rbac_complex_permission cp JOIN tbl_rbac_permission sp ON sp.id = cp.simple_one_id
                                WHERE cp.complex_one_id = p.id AND sp.deleted_at IS NULL ORDER BY sp.unique_name), '{{}}') AS included
           FROM tbl_rbac_permission p
          WHERE p.deleted_at IS NULL AND ($1::bigint IS NULL OR {VISIBLE})
          ORDER BY p.scope, p.unique_name"
    ))
    .bind(org)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_roles(pool: &PgPool, org: Option<i64>) -> ApiResult<Vec<RoleRow>> {
    let rows = sqlx::query_as(&format!(
        "SELECT r.id, r.scope, r.type, r.created_by_plugin, r.unique_name, r.name, r.owning_organization_id, r.created_at,
                COALESCE(ARRAY(SELECT p.unique_name FROM mn_rbac_role_permission rp JOIN tbl_rbac_permission p ON p.id = rp.permission_id
                                WHERE rp.role_id = r.id AND p.deleted_at IS NULL ORDER BY p.unique_name), '{{}}') AS permissions
           FROM tbl_rbac_role r
          WHERE r.deleted_at IS NULL AND ($1::bigint IS NULL OR {VISIBLE})
          ORDER BY r.scope, r.unique_name"
    ))
    .bind(org)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_role(pool: &PgPool, role_id: i64) -> ApiResult<RoleRow> {
    list_roles(pool, None)
        .await?
        .into_iter()
        .find(|r| r.id == role_id)
        .ok_or_else(|| ApiError::not_found("role_not_found", "role not found"))
}

pub async fn get_permission(pool: &PgPool, id: i64) -> ApiResult<PermissionRow> {
    list_permissions(pool, None)
        .await?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| ApiError::not_found("permission_not_found", "permission not found"))
}

fn check_unique_name(name: &str) -> ApiResult<()> {
    if !name.starts_with(CUSTOM_PREFIX) || name.len() <= CUSTOM_PREFIX.len() {
        return Err(ApiError::bad_request("invalid_unique_name", format!("uniqueName must start with '{CUSTOM_PREFIX}'")));
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | '_' | '.')) {
        return Err(ApiError::bad_request("invalid_unique_name", "uniqueName may only contain [A-Za-z0-9:-_.]"));
    }
    Ok(())
}

/// 可修改的列：必須是 user-defined，且（有 owner 時）屬於呼叫端組織。
fn ensure_editable(row_type: &str, owner: Option<i64>, caller_org: Option<i64>) -> ApiResult<()> {
    if row_type == "system" || row_type == "plugin" {
        return Err(ApiError::forbidden("protected_row", format!("{row_type}-managed rows cannot be modified")));
    }
    if row_type != "user-defined" {
        return Err(ApiError::bad_request("not_editable", "row is not user-defined"));
    }
    match (owner, caller_org) {
        (None, None) => Ok(()),
        (Some(o), Some(c)) if o == c => Ok(()),
        (None, Some(_)) => Err(ApiError::forbidden("global_row", "global custom rows can only be modified by a super user")),
        _ => Err(ApiError::not_found("not_found", "not found")),
    }
}

/// 取一組同 scope、可見的 permission id；`simple_only` 時拒絕 complex。
async fn resolve_permissions(
    tx: &mut Transaction<'_, Postgres>,
    scope: RbacScope,
    names: &[String],
    org: Option<i64>,
    simple_only: bool,
) -> ApiResult<Vec<i64>> {
    let mut ids = Vec::new();
    for n in names {
        let row: Option<(i64, bool, Option<i64>)> = sqlx::query_as(
            "SELECT id, complex, owning_organization_id FROM tbl_rbac_permission WHERE scope = $1 AND unique_name = $2 AND deleted_at IS NULL",
        )
        .bind(scope.as_str())
        .bind(n)
        .fetch_optional(&mut **tx)
        .await?;
        let Some((id, complex, owner)) = row else {
            return Err(ApiError::bad_request("unknown_permission", format!("{n} is not a {}-scope permission", scope.as_str())));
        };
        if let Some(o) = owner {
            if Some(o) != org {
                return Err(ApiError::bad_request("unknown_permission", format!("{n} is not available here")));
            }
        }
        if simple_only && complex {
            return Err(ApiError::bad_request("complex_in_complex", format!("{n} is complex; complex permissions can only include simple ones")));
        }
        ids.push(id);
    }
    Ok(ids)
}

async fn replace_links(tx: &mut Transaction<'_, Postgres>, table: &str, owner_col: &str, member_col: &str, owner: i64, members: &[i64]) -> ApiResult<()> {
    sqlx::query(&format!("DELETE FROM {table} WHERE {owner_col} = $1")).bind(owner).execute(&mut **tx).await?;
    for m in members {
        sqlx::query(&format!("INSERT INTO {table} ({owner_col}, {member_col}) VALUES ($1, $2) ON CONFLICT DO NOTHING"))
            .bind(owner)
            .bind(m)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// `org`：None＝全域（呼叫端需是超級使用者）；Some＝屬於該組織。
pub async fn create_permission(pool: &PgPool, input: &CreatePermissionInput, org: Option<i64>) -> ApiResult<PermissionRow> {
    check_unique_name(&input.unique_name)?;
    if !input.complex && org.is_some() {
        return Err(ApiError::forbidden("simple_custom_permission", "organizations can only define complex (bundle) permissions"));
    }
    if input.complex && input.included.is_empty() {
        return Err(ApiError::bad_request("empty_complex", "complex permission must include at least one permission"));
    }
    if !input.complex && !input.included.is_empty() {
        return Err(ApiError::bad_request("simple_with_included", "simple permission cannot include others"));
    }
    let mut tx = pool.begin().await?;
    let included = resolve_permissions(&mut tx, input.scope, &input.included, org, true).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO tbl_rbac_permission (scope, type, unique_name, name, complex, owning_organization_id)
         VALUES ($1, 'user-defined', $2, $3, $4, $5) RETURNING id",
    )
    .bind(input.scope.as_str())
    .bind(&input.unique_name)
    .bind(&input.name)
    .bind(input.complex)
    .bind(org)
    .fetch_one(&mut *tx)
    .await?;
    replace_links(&mut tx, "mn_rbac_complex_permission", "complex_one_id", "simple_one_id", id, &included).await?;
    tx.commit().await?;
    get_permission(pool, id).await
}

pub async fn update_permission(pool: &PgPool, id: i64, input: &UpdatePermissionInput, org: Option<i64>) -> ApiResult<PermissionRow> {
    let cur = get_permission(pool, id).await?;
    ensure_editable(&cur.row_type, cur.owning_organization_id, org)?;
    let scope = RbacScope::parse(&cur.scope).unwrap_or(RbacScope::Organization);
    let mut tx = pool.begin().await?;
    if let Some(n) = &input.name {
        sqlx::query("UPDATE tbl_rbac_permission SET name = $2 WHERE id = $1").bind(id).bind(n).execute(&mut *tx).await?;
    }
    if let Some(inc) = &input.included {
        if !cur.complex {
            return Err(ApiError::bad_request("simple_with_included", "simple permission cannot include others"));
        }
        if inc.is_empty() {
            return Err(ApiError::bad_request("empty_complex", "complex permission must include at least one permission"));
        }
        let ids = resolve_permissions(&mut tx, scope, inc, cur.owning_organization_id, true).await?;
        replace_links(&mut tx, "mn_rbac_complex_permission", "complex_one_id", "simple_one_id", id, &ids).await?;
    }
    tx.commit().await?;
    get_permission(pool, id).await
}

pub async fn delete_permission(pool: &PgPool, id: i64, org: Option<i64>, now: DateTime<Utc>) -> ApiResult<()> {
    let cur = get_permission(pool, id).await?;
    ensure_editable(&cur.row_type, cur.owning_organization_id, org)?;
    let mut tx = pool.begin().await?;
    let used_by_role: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mn_rbac_role_permission rp JOIN tbl_rbac_role r ON r.id = rp.role_id AND r.deleted_at IS NULL WHERE rp.permission_id = $1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    let used_by_complex: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mn_rbac_complex_permission cp JOIN tbl_rbac_permission p ON p.id = cp.complex_one_id AND p.deleted_at IS NULL WHERE cp.simple_one_id = $1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if used_by_role > 0 || used_by_complex > 0 {
        return Err(ApiError::conflict("permission_in_use", "permission is still referenced by a role or complex permission"));
    }
    sqlx::query("DELETE FROM mn_rbac_complex_permission WHERE complex_one_id = $1").bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE tbl_rbac_permission SET deleted_at = $2 WHERE id = $1").bind(id).bind(now).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn create_role(pool: &PgPool, input: &CreateRoleInput, org: Option<i64>) -> ApiResult<RoleRow> {
    check_unique_name(&input.unique_name)?;
    let mut tx = pool.begin().await?;
    let perms = resolve_permissions(&mut tx, input.scope, &input.permissions, org, false).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO tbl_rbac_role (scope, type, unique_name, name, owning_organization_id) VALUES ($1, 'user-defined', $2, $3, $4) RETURNING id",
    )
    .bind(input.scope.as_str())
    .bind(&input.unique_name)
    .bind(&input.name)
    .bind(org)
    .fetch_one(&mut *tx)
    .await?;
    replace_links(&mut tx, "mn_rbac_role_permission", "role_id", "permission_id", id, &perms).await?;
    tx.commit().await?;
    get_role(pool, id).await
}

pub async fn update_role(pool: &PgPool, id: i64, input: &UpdateRoleInput, org: Option<i64>) -> ApiResult<RoleRow> {
    let cur = get_role(pool, id).await?;
    ensure_editable(&cur.row_type, cur.owning_organization_id, org)?;
    let scope = RbacScope::parse(&cur.scope).unwrap_or(RbacScope::Organization);
    let mut tx = pool.begin().await?;
    if let Some(n) = &input.name {
        sqlx::query("UPDATE tbl_rbac_role SET name = $2 WHERE id = $1").bind(id).bind(n).execute(&mut *tx).await?;
    }
    if let Some(p) = &input.permissions {
        let ids = resolve_permissions(&mut tx, scope, p, cur.owning_organization_id, false).await?;
        replace_links(&mut tx, "mn_rbac_role_permission", "role_id", "permission_id", id, &ids).await?;
    }
    tx.commit().await?;
    get_role(pool, id).await
}

pub async fn delete_role(pool: &PgPool, id: i64, org: Option<i64>, now: DateTime<Utc>) -> ApiResult<()> {
    let cur = get_role(pool, id).await?;
    ensure_editable(&cur.row_type, cur.owning_organization_id, org)?;
    let mut tx = pool.begin().await?;
    let held: i64 = sqlx::query_scalar("SELECT count(*) FROM tbl_user_rbac_role WHERE role_id = $1 AND deleted_at IS NULL")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if held > 0 {
        return Err(ApiError::conflict("role_in_use", "role is still held by an active grant"));
    }
    sqlx::query("DELETE FROM mn_rbac_role_permission WHERE role_id = $1").bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE tbl_rbac_role SET deleted_at = $2 WHERE id = $1").bind(id).bind(now).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
