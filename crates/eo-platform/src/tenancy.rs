//! 使用者與組織的共用操作（都在呼叫端交易內）。plugin 與能源模組都用這裡。

use crate::auth::hash_password;
use crate::error::{ApiError, ApiResult};
use crate::rbac::grant::{self, GrantInput, Tx};
use crate::rbac::{PLATFORM_ORG_ADMIN_ROLE, PLATFORM_PROJECT_ADMIN_ROLE};
use chrono::{DateTime, Utc};
use eo_domain::rbac::RbacScope;
use serde::Serialize;
use sqlx::FromRow;

pub const BASIC_PROVIDER: &str = "basic";

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UserRow {
    pub id: i64,
    pub name: String,
    pub email: Option<String>,
    pub privilege: Option<String>,
    pub disabled: bool,
    pub picture: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub const USER_COLS: &str = "id, name, email, privilege, disabled, picture, created_at";

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OrgRow {
    pub id: i64,
    pub unique_name: String,
    pub name: String,
    pub kind: String,
    pub personal_user_id: Option<i64>,
    pub created_at: DateTime<Utc>,
}

pub const ORG_COLS: &str = "id, unique_name, name, kind, personal_user_id, created_at";

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRow {
    pub id: i64,
    pub organization_id: i64,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub project_type: String,
    pub is_default: bool,
    pub unique_name: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

pub const PROJECT_COLS: &str = "id, organization_id, type, is_default, unique_name, name, created_at";

pub fn normalize_email(e: &str) -> String {
    e.trim().to_lowercase()
}

pub fn valid_email(e: &str) -> bool {
    let e = e.trim();
    let Some((local, domain)) = e.split_once('@') else { return false };
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.') && !e.contains(' ')
}

pub fn valid_password(p: &str) -> ApiResult<()> {
    if p.chars().count() < 8 {
        return Err(ApiError::bad_request("weak_password", "password must be at least 8 characters"));
    }
    Ok(())
}

pub async fn user_by_id(tx: &mut Tx<'_>, id: i64) -> ApiResult<UserRow> {
    sqlx::query_as(&format!("SELECT {USER_COLS} FROM tbl_user WHERE id = $1 AND deleted_at IS NULL"))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ApiError::not_found("user_not_found", "user not found"))
}

pub async fn user_by_email(tx: &mut Tx<'_>, email: &str) -> ApiResult<Option<UserRow>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {USER_COLS} FROM tbl_user WHERE lower(email) = lower($1) AND deleted_at IS NULL"
    ))
    .bind(email.trim())
    .fetch_optional(&mut **tx)
    .await?)
}

/// 建使用者＋basic 帳號（帳號名＝email）＋個人組織。
pub async fn create_user(
    tx: &mut Tx<'_>,
    name: &str,
    email: &str,
    password: &str,
    privilege: Option<&str>,
) -> ApiResult<UserRow> {
    let email = normalize_email(email);
    if !valid_email(&email) {
        return Err(ApiError::bad_request("invalid_email", "invalid email"));
    }
    if name.trim().is_empty() {
        return Err(ApiError::bad_request("invalid_name", "name is required"));
    }
    valid_password(password)?;
    if user_by_email(tx, &email).await?.is_some() {
        return Err(ApiError::conflict("email_taken", "an account with this email already exists"));
    }
    let user: UserRow = sqlx::query_as(&format!(
        "INSERT INTO tbl_user (name, email, privilege) VALUES ($1, $2, $3) RETURNING {USER_COLS}"
    ))
    .bind(name.trim())
    .bind(&email)
    .bind(privilege)
    .fetch_one(&mut **tx)
    .await?;
    let hash = hash_password(password)?;
    sqlx::query(
        "INSERT INTO tbl_account_link (provider_id, provider_user_id, provider_user_access_info, identifiers, user_id)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(BASIC_PROVIDER)
    .bind(&email)
    .bind(serde_json::json!({ "password": hash }))
    .bind(serde_json::json!({ "username": email, "email": email }))
    .bind(user.id)
    .execute(&mut **tx)
    .await?;
    create_personal_org(tx, &user).await?;
    Ok(user)
}

pub async fn set_password(tx: &mut Tx<'_>, user_id: i64, password: &str) -> ApiResult<()> {
    valid_password(password)?;
    let hash = hash_password(password)?;
    let n = sqlx::query(
        "UPDATE tbl_account_link SET provider_user_access_info = jsonb_set(provider_user_access_info, '{password}', to_jsonb($2::text))
          WHERE user_id = $1 AND provider_id = $3 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .bind(hash)
    .bind(BASIC_PROVIDER)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(ApiError::not_found("account_not_found", "user has no password login"));
    }
    Ok(())
}

async fn org_admin_role_id(tx: &mut Tx<'_>) -> ApiResult<i64> {
    Ok(grant::role_by_unique_name(tx, RbacScope::Organization, PLATFORM_ORG_ADMIN_ROLE).await?.id)
}

pub async fn project_admin_role_id(tx: &mut Tx<'_>) -> ApiResult<i64> {
    Ok(grant::role_by_unique_name(tx, RbacScope::Project, PLATFORM_PROJECT_ADMIN_ROLE).await?.id)
}

/// 個人組織：`personal-user-<id>`，本人是 owner 與組織管理員。
async fn create_personal_org(tx: &mut Tx<'_>, user: &UserRow) -> ApiResult<OrgRow> {
    let org: OrgRow = sqlx::query_as(&format!(
        "INSERT INTO tbl_organization (unique_name, name, kind, personal_user_id) VALUES ($1, $2, 'personal', $3) RETURNING {ORG_COLS}"
    ))
    .bind(format!("personal-user-{}", user.id))
    .bind(format!("{} 的個人空間", user.name))
    .bind(user.id)
    .fetch_one(&mut **tx)
    .await?;
    init_org(tx, &org, user.id).await?;
    Ok(org)
}

/// 新組織的固定配備：lobby 專案、建立者是 owner 成員並持有組織管理員角色。
async fn init_org(tx: &mut Tx<'_>, org: &OrgRow, owner_id: i64) -> ApiResult<()> {
    sqlx::query("INSERT INTO tbl_project (organization_id, type, is_default, unique_name, name) VALUES ($1, 'lobby', true, 'lobby', 'Lobby')")
        .bind(org.id)
        .execute(&mut **tx)
        .await?;
    add_org_member(tx, owner_id, org.id, "owner").await?;
    let role_id = org_admin_role_id(tx).await?;
    grant::grant(
        tx,
        &GrantInput { user_id: owner_id, role_id, organization_id: Some(org.id), project_id: None, resource_identifier: None },
        Some(owner_id),
    )
    .await?;
    Ok(())
}

pub fn valid_unique_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.starts_with('-')
        && !s.starts_with("personal-")
}

pub async fn create_org(tx: &mut Tx<'_>, unique_name: &str, name: &str, owner_id: i64) -> ApiResult<OrgRow> {
    if !valid_unique_name(unique_name) {
        return Err(ApiError::bad_request("invalid_unique_name", "uniqueName: lowercase letters, digits and '-', not starting with 'personal-'"));
    }
    if name.trim().is_empty() {
        return Err(ApiError::bad_request("invalid_name", "name is required"));
    }
    let org: OrgRow = sqlx::query_as(&format!(
        "INSERT INTO tbl_organization (unique_name, name) VALUES ($1, $2) RETURNING {ORG_COLS}"
    ))
    .bind(unique_name)
    .bind(name.trim())
    .fetch_one(&mut **tx)
    .await?;
    init_org(tx, &org, owner_id).await?;
    Ok(org)
}

pub async fn org_by_id(tx: &mut Tx<'_>, id: i64) -> ApiResult<OrgRow> {
    sqlx::query_as(&format!("SELECT {ORG_COLS} FROM tbl_organization WHERE id = $1 AND deleted_at IS NULL"))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ApiError::not_found("organization_not_found", "organization not found"))
}

pub async fn lobby_of(tx: &mut Tx<'_>, org_id: i64) -> ApiResult<i64> {
    sqlx::query_scalar("SELECT id FROM tbl_project WHERE organization_id = $1 AND is_default AND deleted_at IS NULL")
        .bind(org_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ApiError::internal("organization has no lobby"))
}

pub async fn is_org_member(tx: &mut Tx<'_>, user_id: i64, org_id: i64) -> ApiResult<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM mn_user_organization WHERE user_id = $1 AND organization_id = $2 AND deleted_at IS NULL)",
    )
    .bind(user_id)
    .bind(org_id)
    .fetch_one(&mut **tx)
    .await?)
}

/// 加入組織（冪等）並一併加入 lobby。
pub async fn add_org_member(tx: &mut Tx<'_>, user_id: i64, org_id: i64, role: &str) -> ApiResult<()> {
    if !is_org_member(tx, user_id, org_id).await? {
        sqlx::query("INSERT INTO mn_user_organization (user_id, organization_id, role) VALUES ($1, $2, $3)")
            .bind(user_id)
            .bind(org_id)
            .bind(role)
            .execute(&mut **tx)
            .await?;
    }
    let lobby = lobby_of(tx, org_id).await?;
    add_project_member(tx, user_id, lobby, "member").await
}

pub async fn add_project_member(tx: &mut Tx<'_>, user_id: i64, project_id: i64, role: &str) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO mn_user_project (user_id, project_id, role) SELECT $1, $2, $3
          WHERE NOT EXISTS (SELECT 1 FROM mn_user_project WHERE user_id = $1 AND project_id = $2 AND deleted_at IS NULL)",
    )
    .bind(user_id)
    .bind(project_id)
    .bind(role)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// 移出組織：成員關係（含所有專案）軟刪除。grant 由呼叫端另外撤。
pub async fn remove_org_member(tx: &mut Tx<'_>, user_id: i64, org_id: i64, now: DateTime<Utc>) -> ApiResult<()> {
    let n = sqlx::query("UPDATE mn_user_organization SET deleted_at = $3 WHERE user_id = $1 AND organization_id = $2 AND deleted_at IS NULL")
        .bind(user_id)
        .bind(org_id)
        .bind(now)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(ApiError::not_found("member_not_found", "user is not a member of this organization"));
    }
    sqlx::query(
        "UPDATE mn_user_project SET deleted_at = $3 WHERE user_id = $1 AND deleted_at IS NULL
           AND project_id IN (SELECT id FROM tbl_project WHERE organization_id = $2)",
    )
    .bind(user_id)
    .bind(org_id)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
