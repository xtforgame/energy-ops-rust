//! 組織、專案、成員、邀請。
//!
//! · 每個組織恰有一個 lobby 專案，組織成員一律加入、不能退出。
//! · 每個使用者自動擁有個人組織（不能邀請別人進來）。
//! · 邀請：組織或專案入口，選一個 preset（決定接受時授予哪些角色）；接受時同一交易內
//!   建成員關係、加入 lobby（專案邀請再加入該專案）、授權、改狀態。
//! · 移除成員連帶撤銷他在該組織的所有授權；不能移除自己，也不能讓組織沒有管理者。

use crate::auth::{CurrentUser, MaybeUser};
use crate::azldi::Plugin;
use crate::core::CoreRef;
use crate::error::{ApiError, ApiResult};
use crate::plugin::{AppPlugin, RouteRegistrar};
use crate::rbac::grant::{self, GrantInput, Tx};
use crate::rbac::{
    OrgAccess, PLATFORM_MEMBER_MANAGE, PLATFORM_ORG_ADMIN_ROLE, PLATFORM_ORG_MANAGE, PLATFORM_PROJECT_ADMIN_ROLE,
    PLATFORM_PROJECT_MANAGE, PLATFORM_RBAC_MANAGE,
};
use crate::tenancy::{self, OrgRow, ProjectRow, UserRow, ORG_COLS, PROJECT_COLS};
use actix_web::{web, HttpResponse};
use chrono::{DateTime, Duration, Utc};
use eo_domain::rbac::RbacScope;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const INVITE_EXPIRY_DAYS: i64 = 14;

pub struct MultitenancyPlugin;

/// 邀請 preset：接受後在該入口授予哪些角色。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvitePreset {
    pub key: &'static str,
    pub scope: &'static str,
    pub label_key: &'static str,
    pub grant_role_unique_names: &'static [&'static str],
}

pub const INVITE_PRESETS: &[InvitePreset] = &[
    InvitePreset { key: "org_admin", scope: "organization", label_key: "inviteRole.orgAdmin", grant_role_unique_names: &[PLATFORM_ORG_ADMIN_ROLE] },
    InvitePreset { key: "org_member", scope: "organization", label_key: "inviteRole.orgMember", grant_role_unique_names: &[] },
    InvitePreset { key: "project_admin", scope: "project", label_key: "inviteRole.projectAdmin", grant_role_unique_names: &[PLATFORM_PROJECT_ADMIN_ROLE] },
    InvitePreset { key: "project_member", scope: "project", label_key: "inviteRole.projectMember", grant_role_unique_names: &[] },
];

pub fn preset(key: &str) -> Option<&'static InvitePreset> {
    INVITE_PRESETS.iter().find(|p| p.key == key)
}

// ── 我的組織 ──

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct MyOrgRow {
    id: i64,
    unique_name: String,
    name: String,
    kind: String,
    membership_role: String,
    joined_at: DateTime<Utc>,
}

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct MyProjectRow {
    id: i64,
    organization_id: i64,
    name: String,
    unique_name: String,
    is_default: bool,
    membership_role: String,
}

/// 我所屬的組織與（各組織內）我所屬的專案。個人組織排最後。
async fn my_organizations(core: web::Data<CoreRef>, user: CurrentUser) -> ApiResult<HttpResponse> {
    let orgs: Vec<MyOrgRow> = sqlx::query_as(
        "SELECT o.id, o.unique_name, o.name, o.kind, m.role AS membership_role, m.created_at AS joined_at
           FROM mn_user_organization m JOIN tbl_organization o ON o.id = m.organization_id AND o.deleted_at IS NULL
          WHERE m.user_id = $1 AND m.deleted_at IS NULL
          ORDER BY (o.kind = 'personal'), m.created_at, o.id",
    )
    .bind(user.id)
    .fetch_all(&core.pool)
    .await?;
    let projects: Vec<MyProjectRow> = sqlx::query_as(
        "SELECT p.id, p.organization_id, p.name, p.unique_name, p.is_default, m.role AS membership_role
           FROM mn_user_project m JOIN tbl_project p ON p.id = m.project_id AND p.deleted_at IS NULL
          WHERE m.user_id = $1 AND m.deleted_at IS NULL
          ORDER BY p.is_default DESC, p.id",
    )
    .bind(user.id)
    .fetch_all(&core.pool)
    .await?;
    let items: Vec<serde_json::Value> = orgs
        .into_iter()
        .map(|o| {
            let ps: Vec<&MyProjectRow> = projects.iter().filter(|p| p.organization_id == o.id).collect();
            let mut v = serde_json::to_value(&o).unwrap();
            v["projects"] = serde_json::to_value(ps).unwrap();
            v
        })
        .collect();
    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": items })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateOrgBody {
    unique_name: String,
    name: String,
}

/// 建組織：超級使用者，或開了 EO_ALLOW_ORG_CREATE。
async fn create_org(core: web::Data<CoreRef>, user: CurrentUser, body: web::Json<CreateOrgBody>) -> ApiResult<HttpResponse> {
    let allow = std::env::var("EO_ALLOW_ORG_CREATE").map(|v| v == "1" || v == "true").unwrap_or(false);
    if !allow && !core.rbac.is_super_user(user.id).await? {
        return Err(ApiError::forbidden("org_create_forbidden", "only super users can create organizations"));
    }
    let mut tx = core.pool.begin().await?;
    let org = tenancy::create_org(&mut tx, &body.unique_name, &body.name, user.id).await?;
    core.rbac.commit(tx, &[user.id]).await?;
    Ok(HttpResponse::Created().json(org))
}

async fn get_org(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>) -> ApiResult<HttpResponse> {
    let org = path.into_inner();
    core.rbac.authorize_member(&user, org).await?;
    let mut tx = core.pool.begin().await?;
    let o = tenancy::org_by_id(&mut tx, org).await?;
    Ok(HttpResponse::Ok().json(o))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PatchOrgBody {
    name: Option<String>,
}

async fn patch_org(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>, body: web::Json<PatchOrgBody>) -> ApiResult<HttpResponse> {
    let org = path.into_inner();
    core.rbac.authorize_org(&user, org, &[PLATFORM_ORG_MANAGE]).await?;
    if let Some(n) = &body.name {
        if n.trim().is_empty() {
            return Err(ApiError::bad_request("invalid_name", "name is required"));
        }
    }
    let o: OrgRow = sqlx::query_as(&format!(
        "UPDATE tbl_organization SET name = COALESCE($2, name) WHERE id = $1 AND deleted_at IS NULL RETURNING {ORG_COLS}"
    ))
    .bind(org)
    .bind(body.name.as_deref().map(str::trim))
    .fetch_optional(&core.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("organization_not_found", "organization not found"))?;
    Ok(HttpResponse::Ok().json(o))
}

// ── 組織成員 ──

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MemberRow {
    pub user_id: i64,
    pub name: String,
    pub email: Option<String>,
    pub disabled: bool,
    pub membership_role: String,
    pub joined_at: DateTime<Utc>,
    pub roles: Vec<String>,
}

pub async fn list_org_members(pool: &sqlx::PgPool, org: i64) -> ApiResult<Vec<MemberRow>> {
    Ok(sqlx::query_as(
        "SELECT u.id AS user_id, u.name, u.email, u.disabled, m.role AS membership_role, m.created_at AS joined_at,
                COALESCE(ARRAY(SELECT r.unique_name FROM tbl_user_rbac_role g JOIN tbl_rbac_role r ON r.id = g.role_id
                                WHERE g.user_id = u.id AND g.organization_id = $1 AND g.project_id IS NULL
                                  AND g.resource_identifier = '' AND g.deleted_at IS NULL ORDER BY r.unique_name), '{}') AS roles
           FROM mn_user_organization m JOIN tbl_user u ON u.id = m.user_id AND u.deleted_at IS NULL
          WHERE m.organization_id = $1 AND m.deleted_at IS NULL
          ORDER BY m.created_at, u.id",
    )
    .bind(org)
    .fetch_all(pool)
    .await?)
}

async fn org_members(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>) -> ApiResult<HttpResponse> {
    let org = path.into_inner();
    core.rbac.authorize_org(&user, org, &[PLATFORM_MEMBER_MANAGE, "energy:member:manage"]).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": list_org_members(&core.pool, org).await? })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AddMemberBody {
    /// 把既有使用者加進來。
    user_id: Option<i64>,
    /// 或直接建帳號（管理者設初始密碼）。
    email: Option<String>,
    name: Option<String>,
    password: Option<String>,
}

/// 直接加成員（不經邀請）：給既有使用者 id，或 email＋name＋password 直接建帳號。
async fn add_org_member(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>, body: web::Json<AddMemberBody>) -> ApiResult<HttpResponse> {
    let org = path.into_inner();
    core.rbac.authorize_org(&user, org, &[PLATFORM_MEMBER_MANAGE, "energy:member:manage"]).await?;
    let mut tx = core.pool.begin().await?;
    let o = tenancy::org_by_id(&mut tx, org).await?;
    if o.kind == "personal" {
        return Err(ApiError::bad_request("personal_org", "cannot add members to a personal organization"));
    }
    let b = body.into_inner();
    let target: UserRow = match (b.user_id, b.email, b.name, b.password) {
        (Some(id), None, None, None) => tenancy::user_by_id(&mut tx, id).await?,
        (None, Some(email), Some(name), Some(password)) => tenancy::create_user(&mut tx, &name, &email, &password, None).await?,
        _ => return Err(ApiError::bad_request("invalid_body", "give either userId, or email + name + password")),
    };
    if tenancy::is_org_member(&mut tx, target.id, org).await? {
        return Err(ApiError::conflict("already_member", "user is already a member"));
    }
    tenancy::add_org_member(&mut tx, target.id, org, "member").await?;
    core.rbac.commit(tx, &[target.id]).await?;
    Ok(HttpResponse::Created().json(target))
}

/// 移除成員（交易內）：撤掉他在組織的所有授權，確認組織還有管理者。能源模組的成員頁也用它。
pub async fn remove_member_in_tx(
    tx: &mut Tx<'_>,
    access: &OrgAccess,
    member_id: i64,
    governance: &[&str],
    now: DateTime<Utc>,
) -> ApiResult<Vec<grant::GrantRow>> {
    if access.user_id() == Some(member_id) {
        return Err(ApiError::bad_request("cannot_remove_self", "cannot remove yourself"));
    }
    let revoked = grant::revoke_all_in_org(tx, member_id, access.org_id(), access.user_id(), "member removed", now).await?;
    tenancy::remove_org_member(tx, member_id, access.org_id(), now).await?;
    grant::ensure_still_governed(tx, access.org_id(), governance).await?;
    Ok(revoked)
}

async fn remove_org_member(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, member) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, &[PLATFORM_MEMBER_MANAGE]).await?;
    let mut tx = core.pool.begin().await?;
    remove_member_in_tx(&mut tx, &access, member, grant::PLATFORM_GOVERNANCE, core.clock.now()).await?;
    core.rbac.commit(tx, &[member]).await?;
    Ok(HttpResponse::NoContent().finish())
}

// ── 專案 ──

async fn org_projects(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>) -> ApiResult<HttpResponse> {
    let org = path.into_inner();
    let access = core.rbac.authorize_member(&user, org).await?;
    let can_see_all = access.is_super_user()
        || core.rbac.has(user.id, &crate::rbac::ScopeRef::org(org), &[PLATFORM_PROJECT_MANAGE, PLATFORM_RBAC_MANAGE]).await?;
    let rows: Vec<ProjectRow> = sqlx::query_as(&format!(
        "SELECT {PROJECT_COLS} FROM tbl_project p WHERE p.organization_id = $1 AND p.deleted_at IS NULL
           AND ($2 OR EXISTS (SELECT 1 FROM mn_user_project m WHERE m.project_id = p.id AND m.user_id = $3 AND m.deleted_at IS NULL))
         ORDER BY p.is_default DESC, p.id"
    ))
    .bind(org)
    .bind(can_see_all)
    .bind(user.id)
    .fetch_all(&core.pool)
    .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": rows })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateProjectBody {
    unique_name: String,
    name: String,
    #[serde(rename = "type")]
    project_type: Option<String>,
}

async fn create_project(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>, body: web::Json<CreateProjectBody>) -> ApiResult<HttpResponse> {
    let org = path.into_inner();
    core.rbac.authorize_org(&user, org, &[PLATFORM_PROJECT_MANAGE]).await?;
    if !tenancy::valid_unique_name(&body.unique_name) || body.unique_name == "lobby" {
        return Err(ApiError::bad_request("invalid_unique_name", "uniqueName: lowercase letters, digits and '-'"));
    }
    let mut tx = core.pool.begin().await?;
    let p: ProjectRow = sqlx::query_as(&format!(
        "INSERT INTO tbl_project (organization_id, type, unique_name, name) VALUES ($1, $2, $3, $4) RETURNING {PROJECT_COLS}"
    ))
    .bind(org)
    .bind(body.project_type.as_deref().unwrap_or("general"))
    .bind(&body.unique_name)
    .bind(body.name.trim())
    .fetch_one(&mut *tx)
    .await?;
    tenancy::add_project_member(&mut tx, user.id, p.id, "owner").await?;
    let role_id = tenancy::project_admin_role_id(&mut tx).await?;
    grant::grant(
        &mut tx,
        &GrantInput { user_id: user.id, role_id, organization_id: Some(org), project_id: Some(p.id), resource_identifier: None },
        Some(user.id),
    )
    .await?;
    core.rbac.commit(tx, &[user.id]).await?;
    Ok(HttpResponse::Created().json(p))
}

async fn get_project(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>) -> ApiResult<HttpResponse> {
    let pid = path.into_inner();
    let p: ProjectRow = sqlx::query_as(&format!("SELECT {PROJECT_COLS} FROM tbl_project WHERE id = $1 AND deleted_at IS NULL"))
        .bind(pid)
        .fetch_optional(&core.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("project_not_found", "project not found"))?;
    core.rbac.authorize_member(&user, p.organization_id).await?;
    Ok(HttpResponse::Ok().json(p))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PatchProjectBody {
    name: Option<String>,
}

async fn patch_project(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>, body: web::Json<PatchProjectBody>) -> ApiResult<HttpResponse> {
    let pid = path.into_inner();
    core.rbac.authorize_project(&user, pid, &[PLATFORM_PROJECT_MANAGE]).await?;
    let p: ProjectRow = sqlx::query_as(&format!(
        "UPDATE tbl_project SET name = COALESCE($2, name) WHERE id = $1 RETURNING {PROJECT_COLS}"
    ))
    .bind(pid)
    .bind(body.name.as_deref().map(str::trim))
    .fetch_one(&core.pool)
    .await?;
    Ok(HttpResponse::Ok().json(p))
}

/// 刪專案（lobby 不能刪）：連帶撤銷專案層的授權、清掉成員關係。
async fn delete_project(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>) -> ApiResult<HttpResponse> {
    let pid = path.into_inner();
    let access = core.rbac.authorize_project(&user, pid, &[PLATFORM_PROJECT_MANAGE]).await?;
    let mut tx = core.pool.begin().await?;
    let is_default: bool = sqlx::query_scalar("SELECT is_default FROM tbl_project WHERE id = $1").bind(pid).fetch_one(&mut *tx).await?;
    if is_default {
        return Err(ApiError::bad_request("lobby_project", "the lobby project cannot be deleted"));
    }
    let now = core.clock.now();
    let users: Vec<i64> = sqlx::query_scalar(
        "UPDATE tbl_user_rbac_role SET revoked_by_user_id = $2, revoked_at = $3, revoked_reason = 'project deleted', deleted_at = $3
          WHERE project_id = $1 AND deleted_at IS NULL RETURNING user_id",
    )
    .bind(pid)
    .bind(access.org().user_id())
    .bind(now)
    .fetch_all(&mut *tx)
    .await?;
    sqlx::query("UPDATE mn_user_project SET deleted_at = $2 WHERE project_id = $1 AND deleted_at IS NULL").bind(pid).bind(now).execute(&mut *tx).await?;
    sqlx::query("UPDATE tbl_project SET deleted_at = $2 WHERE id = $1").bind(pid).bind(now).execute(&mut *tx).await?;
    core.rbac.commit(tx, &users).await?;
    Ok(HttpResponse::NoContent().finish())
}

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct ProjectMemberRow {
    user_id: i64,
    name: String,
    email: Option<String>,
    membership_role: String,
    joined_at: DateTime<Utc>,
    roles: Vec<String>,
}

async fn project_members(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>) -> ApiResult<HttpResponse> {
    let pid = path.into_inner();
    core.rbac.authorize_project(&user, pid, &[PLATFORM_MEMBER_MANAGE]).await?;
    let rows: Vec<ProjectMemberRow> = sqlx::query_as(
        "SELECT u.id AS user_id, u.name, u.email, m.role AS membership_role, m.created_at AS joined_at,
                COALESCE(ARRAY(SELECT r.unique_name FROM tbl_user_rbac_role g JOIN tbl_rbac_role r ON r.id = g.role_id
                                WHERE g.user_id = u.id AND g.project_id = $1 AND g.resource_identifier = '' AND g.deleted_at IS NULL
                                ORDER BY r.unique_name), '{}') AS roles
           FROM mn_user_project m JOIN tbl_user u ON u.id = m.user_id AND u.deleted_at IS NULL
          WHERE m.project_id = $1 AND m.deleted_at IS NULL ORDER BY m.created_at, u.id",
    )
    .bind(pid)
    .fetch_all(&core.pool)
    .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": rows })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AddProjectMemberBody {
    user_id: i64,
}

async fn add_project_member(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>, body: web::Json<AddProjectMemberBody>) -> ApiResult<HttpResponse> {
    let pid = path.into_inner();
    let access = core.rbac.authorize_project(&user, pid, &[PLATFORM_MEMBER_MANAGE]).await?;
    let mut tx = core.pool.begin().await?;
    if !tenancy::is_org_member(&mut tx, body.user_id, access.org_id()).await? {
        return Err(ApiError::bad_request("not_a_member", "user must be a member of the organization first"));
    }
    tenancy::add_project_member(&mut tx, body.user_id, pid, "member").await?;
    core.rbac.commit(tx, &[body.user_id]).await?;
    Ok(HttpResponse::Created().json(serde_json::json!({ "ok": true })))
}

async fn remove_project_member(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (pid, member) = path.into_inner();
    let access = core.rbac.authorize_project(&user, pid, &[PLATFORM_MEMBER_MANAGE]).await?;
    let mut tx = core.pool.begin().await?;
    let is_default: bool = sqlx::query_scalar("SELECT is_default FROM tbl_project WHERE id = $1").bind(pid).fetch_one(&mut *tx).await?;
    if is_default {
        return Err(ApiError::bad_request("lobby_project", "members cannot leave the lobby; remove them from the organization instead"));
    }
    let now = core.clock.now();
    sqlx::query(
        "UPDATE tbl_user_rbac_role SET revoked_by_user_id = $3, revoked_at = $4, revoked_reason = 'removed from project', deleted_at = $4
          WHERE user_id = $1 AND project_id = $2 AND deleted_at IS NULL",
    )
    .bind(member)
    .bind(pid)
    .bind(access.org().user_id())
    .bind(now)
    .execute(&mut *tx)
    .await?;
    let n = sqlx::query("UPDATE mn_user_project SET deleted_at = $3 WHERE user_id = $1 AND project_id = $2 AND deleted_at IS NULL")
        .bind(member)
        .bind(pid)
        .bind(now)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(ApiError::not_found("member_not_found", "user is not a member of this project"));
    }
    core.rbac.commit(tx, &[member]).await?;
    Ok(HttpResponse::NoContent().finish())
}

// ── 邀請 ──

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct InvitationRow {
    pub id: i64,
    pub scope: String,
    pub organization_id: i64,
    pub organization_name: String,
    pub project_id: Option<i64>,
    pub project_name: Option<String>,
    pub inviter_id: i64,
    pub inviter_name: String,
    pub invitee_id: Option<i64>,
    pub invitee_email: String,
    pub invite_role_key: String,
    pub status: String,
    #[serde(skip_serializing)]
    pub token: String,
    pub responded_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

const INV_SELECT: &str = "SELECT i.id, i.scope, i.organization_id, o.name AS organization_name, i.project_id, p.name AS project_name,
       i.inviter_id, u.name AS inviter_name, i.invitee_id, i.invitee_email, i.invite_role_key,
       CASE WHEN i.status = 'pending' AND i.expires_at <= now() THEN 'expired' ELSE i.status END AS status,
       i.token, i.responded_at, i.expires_at, i.created_at
  FROM tbl_invitation i
  JOIN tbl_organization o ON o.id = i.organization_id
  LEFT JOIN tbl_project p ON p.id = i.project_id
  JOIN tbl_user u ON u.id = i.inviter_id";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InviteBody {
    email: String,
    invite_role_key: String,
}

/// 建邀請（交易內）；同一個人同一入口已有 pending 就沿用那筆。
async fn create_invitation(
    core: &CoreRef,
    inviter: &CurrentUser,
    org: i64,
    project: Option<i64>,
    body: &InviteBody,
) -> ApiResult<serde_json::Value> {
    let scope = if project.is_some() { "project" } else { "organization" };
    let p = preset(&body.invite_role_key)
        .filter(|p| p.scope == scope)
        .ok_or_else(|| ApiError::bad_request("invalid_invite_role", format!("unknown {scope} invite preset")))?;
    let email = tenancy::normalize_email(&body.email);
    if !tenancy::valid_email(&email) {
        return Err(ApiError::bad_request("invalid_email", "invalid email"));
    }
    // 組織入口會授予管理角色的 preset：邀請者自己要能管 RBAC。專案入口由專案管理者決定，過了閘就夠。
    if project.is_none()
        && !p.grant_role_unique_names.is_empty()
        && !core.rbac.has(inviter.id, &crate::rbac::ScopeRef::org(org), &[PLATFORM_RBAC_MANAGE]).await?
    {
        return Err(ApiError::permission_denied(&[PLATFORM_RBAC_MANAGE]));
    }
    let mut tx = core.pool.begin().await?;
    let o = tenancy::org_by_id(&mut tx, org).await?;
    if o.kind == "personal" {
        return Err(ApiError::bad_request("personal_org", "cannot invite into a personal organization"));
    }
    let invitee = tenancy::user_by_email(&mut tx, &email).await?;
    if let Some(u) = &invitee {
        let already = match project {
            None => tenancy::is_org_member(&mut tx, u.id, org).await?,
            Some(pid) => sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM mn_user_project WHERE user_id = $1 AND project_id = $2 AND deleted_at IS NULL)",
            )
            .bind(u.id)
            .bind(pid)
            .fetch_one(&mut *tx)
            .await?,
        };
        if already {
            return Err(ApiError::conflict("already_member", "user is already a member"));
        }
    }
    let now = core.clock.now();
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM tbl_invitation WHERE scope = $1 AND organization_id = $2 AND COALESCE(project_id, 0) = COALESCE($3, 0)
            AND lower(invitee_email) = $4 AND status = 'pending' AND deleted_at IS NULL",
    )
    .bind(scope)
    .bind(org)
    .bind(project)
    .bind(&email)
    .fetch_optional(&mut *tx)
    .await?;
    let id = match existing {
        Some(id) => {
            // 過期的沿用那筆、延長效期並更新 preset。
            sqlx::query("UPDATE tbl_invitation SET invite_role_key = $2, expires_at = $3, inviter_id = $4 WHERE id = $1")
                .bind(id)
                .bind(p.key)
                .bind(now + Duration::days(INVITE_EXPIRY_DAYS))
                .bind(inviter.id)
                .execute(&mut *tx)
                .await?;
            id
        }
        None => sqlx::query_scalar(
            "INSERT INTO tbl_invitation (scope, organization_id, project_id, inviter_id, invitee_id, invitee_email, invite_role_key, token, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id",
        )
        .bind(scope)
        .bind(org)
        .bind(project)
        .bind(inviter.id)
        .bind(invitee.as_ref().map(|u| u.id))
        .bind(&email)
        .bind(p.key)
        .bind(crate::auth::new_token())
        .bind(now + Duration::days(INVITE_EXPIRY_DAYS))
        .fetch_one(&mut *tx)
        .await?,
    };
    tx.commit().await?;
    let row: InvitationRow = sqlx::query_as(&format!("{INV_SELECT} WHERE i.id = $1")).bind(id).fetch_one(&core.pool).await?;
    let mut v = serde_json::to_value(&row).unwrap();
    // token 只在建立時回一次（寄信或複製連結用）。
    v["token"] = serde_json::Value::String(row.token.clone());
    Ok(v)
}

async fn org_invitations(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>) -> ApiResult<HttpResponse> {
    let org = path.into_inner();
    core.rbac.authorize_org(&user, org, &[PLATFORM_MEMBER_MANAGE]).await?;
    let rows: Vec<InvitationRow> = sqlx::query_as(&format!("{INV_SELECT} WHERE i.organization_id = $1 AND i.deleted_at IS NULL ORDER BY i.id DESC"))
        .bind(org)
        .fetch_all(&core.pool)
        .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": rows })))
}

async fn org_invite(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>, body: web::Json<InviteBody>) -> ApiResult<HttpResponse> {
    let org = path.into_inner();
    core.rbac.authorize_org(&user, org, &[PLATFORM_MEMBER_MANAGE]).await?;
    Ok(HttpResponse::Created().json(create_invitation(&core, &user, org, None, &body).await?))
}

async fn project_invitations(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>) -> ApiResult<HttpResponse> {
    let pid = path.into_inner();
    core.rbac.authorize_project(&user, pid, &[PLATFORM_MEMBER_MANAGE]).await?;
    let rows: Vec<InvitationRow> = sqlx::query_as(&format!("{INV_SELECT} WHERE i.project_id = $1 AND i.deleted_at IS NULL ORDER BY i.id DESC"))
        .bind(pid)
        .fetch_all(&core.pool)
        .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": rows })))
}

async fn project_invite(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>, body: web::Json<InviteBody>) -> ApiResult<HttpResponse> {
    let pid = path.into_inner();
    let access = core.rbac.authorize_project(&user, pid, &[PLATFORM_MEMBER_MANAGE]).await?;
    Ok(HttpResponse::Created().json(create_invitation(&core, &user, access.org_id(), Some(pid), &body).await?))
}

async fn revoke_invitation(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let inv: InvitationRow = sqlx::query_as(&format!("{INV_SELECT} WHERE i.id = $1 AND i.deleted_at IS NULL"))
        .bind(id)
        .fetch_optional(&core.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("invitation_not_found", "invitation not found"))?;
    match inv.project_id {
        Some(p) => {
            core.rbac.authorize_project(&user, p, &[PLATFORM_MEMBER_MANAGE]).await?;
        }
        None => {
            core.rbac.authorize_org(&user, inv.organization_id, &[PLATFORM_MEMBER_MANAGE]).await?;
        }
    }
    let n = sqlx::query("UPDATE tbl_invitation SET status = 'revoked', responded_at = $2 WHERE id = $1 AND status = 'pending'")
        .bind(id)
        .bind(core.clock.now())
        .execute(&core.pool)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(ApiError::conflict("invitation_not_pending", "invitation is no longer pending"));
    }
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

/// 寄給我（email 相同）的 pending 邀請。
async fn my_invitations(core: web::Data<CoreRef>, user: CurrentUser) -> ApiResult<HttpResponse> {
    let rows: Vec<InvitationRow> = sqlx::query_as(&format!(
        "{INV_SELECT} WHERE i.deleted_at IS NULL AND i.status = 'pending' AND i.expires_at > now()
            AND (i.invitee_id = $1 OR lower(i.invitee_email) = lower($2)) ORDER BY i.id DESC"
    ))
    .bind(user.id)
    .bind(user.email.clone().unwrap_or_default())
    .fetch_all(&core.pool)
    .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": rows })))
}

/// 接受：同一交易內建成員關係、加入 lobby／專案、依 preset 授權、改狀態。
async fn accept_invitation(core: &CoreRef, user: &CurrentUser, inv_id: i64) -> ApiResult<InvitationRow> {
    let mut tx = core.pool.begin().await?;
    let inv: InvitationRow = sqlx::query_as(&format!("{INV_SELECT} WHERE i.id = $1 AND i.deleted_at IS NULL FOR UPDATE OF i"))
        .bind(inv_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::not_found("invitation_not_found", "invitation not found"))?;
    if inv.status != "pending" {
        return Err(ApiError::conflict("invitation_not_pending", format!("invitation is {}", inv.status)));
    }
    let mine = inv.invitee_id == Some(user.id)
        || user.email.as_deref().map(tenancy::normalize_email) == Some(tenancy::normalize_email(&inv.invitee_email));
    if !mine {
        return Err(ApiError::forbidden("invitation_not_for_you", "this invitation was sent to someone else"));
    }
    tenancy::add_org_member(&mut tx, user.id, inv.organization_id, "member").await?;
    if let Some(pid) = inv.project_id {
        tenancy::add_project_member(&mut tx, user.id, pid, "member").await?;
    }
    let p = preset(&inv.invite_role_key).ok_or_else(|| ApiError::internal("invitation preset no longer exists"))?;
    let scope = if inv.project_id.is_some() { RbacScope::Project } else { RbacScope::Organization };
    for role in p.grant_role_unique_names {
        let r = grant::role_by_unique_name(&mut tx, scope, role).await?;
        grant::grant(
            &mut tx,
            &GrantInput {
                user_id: user.id,
                role_id: r.id,
                organization_id: Some(inv.organization_id),
                project_id: inv.project_id,
                resource_identifier: None,
            },
            Some(inv.inviter_id),
        )
        .await?;
    }
    sqlx::query("UPDATE tbl_invitation SET status = 'accepted', responded_at = $2, invitee_id = $3 WHERE id = $1")
        .bind(inv_id)
        .bind(core.clock.now())
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    core.rbac.commit(tx, &[user.id]).await?;
    let row = sqlx::query_as(&format!("{INV_SELECT} WHERE i.id = $1")).bind(inv_id).fetch_one(&core.pool).await?;
    Ok(row)
}

async fn accept_mine(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>) -> ApiResult<HttpResponse> {
    Ok(HttpResponse::Ok().json(accept_invitation(&core, &user, path.into_inner()).await?))
}

async fn decline_mine(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<i64>) -> ApiResult<HttpResponse> {
    let id = path.into_inner();
    let n = sqlx::query(
        "UPDATE tbl_invitation SET status = 'declined', responded_at = $3
          WHERE id = $1 AND status = 'pending' AND deleted_at IS NULL AND (invitee_id = $2 OR lower(invitee_email) = lower($4))",
    )
    .bind(id)
    .bind(user.id)
    .bind(core.clock.now())
    .bind(user.email.clone().unwrap_or_default())
    .execute(&core.pool)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(ApiError::not_found("invitation_not_found", "no pending invitation for you"));
    }
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

/// 用 token 看邀請摘要（不需要登入；給「接受邀請」頁顯示，未註冊的人據此去註冊）。
async fn by_token(core: web::Data<CoreRef>, _maybe: MaybeUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let token = path.into_inner();
    let inv: InvitationRow = sqlx::query_as(&format!("{INV_SELECT} WHERE i.token = $1 AND i.deleted_at IS NULL"))
        .bind(&token)
        .fetch_optional(&core.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("invitation_not_found", "invitation not found"))?;
    let mut tx = core.pool.begin().await?;
    let registered = tenancy::user_by_email(&mut tx, &inv.invitee_email).await?.is_some();
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "id": inv.id,
        "scope": inv.scope,
        "organizationName": inv.organization_name,
        "projectName": inv.project_name,
        "inviterName": inv.inviter_name,
        "inviteeEmail": inv.invitee_email,
        "inviteRoleKey": inv.invite_role_key,
        "status": inv.status,
        "expiresAt": inv.expires_at,
        "inviteeRegistered": registered,
    })))
}

async fn accept_by_token(core: web::Data<CoreRef>, user: CurrentUser, path: web::Path<String>) -> ApiResult<HttpResponse> {
    let token = path.into_inner();
    let id: i64 = sqlx::query_scalar("SELECT id FROM tbl_invitation WHERE token = $1 AND deleted_at IS NULL")
        .bind(&token)
        .fetch_optional(&core.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("invitation_not_found", "invitation not found"))?;
    Ok(HttpResponse::Ok().json(accept_invitation(&core, &user, id).await?))
}

async fn presets() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({ "items": INVITE_PRESETS }))
}

impl Plugin for MultitenancyPlugin {
    fn name(&self) -> &str {
        "multitenancy"
    }
    crate::plugin_boilerplate!();
}

impl AppPlugin for MultitenancyPlugin {
    fn configure_routes(&self, cfg: &mut RouteRegistrar<'_>, _self_arc: &Arc<dyn AppPlugin>) {
        cfg.get("/api/v1/me/organizations", web::get().to(my_organizations));
        cfg.get("/api/v1/me/invitations", web::get().to(my_invitations));
        cfg.post("/api/v1/me/invitations/{invitationId}/accept", web::post().to(accept_mine));
        cfg.post("/api/v1/me/invitations/{invitationId}/decline", web::post().to(decline_mine));
        cfg.get("/api/v1/invite-presets", web::get().to(presets));
        cfg.get("/api/v1/invitations/by-token/{token}", web::get().to(by_token));
        cfg.post("/api/v1/invitations/by-token/{token}/accept", web::post().to(accept_by_token));
        cfg.post("/api/v1/invitations/{invitationId}/revoke", web::post().to(revoke_invitation));

        cfg.post("/api/v1/orgs", web::post().to(create_org));
        cfg.get("/api/v1/orgs/{orgId}", web::get().to(get_org));
        cfg.patch("/api/v1/orgs/{orgId}", web::patch().to(patch_org));
        cfg.get("/api/v1/orgs/{orgId}/members", web::get().to(org_members));
        cfg.post("/api/v1/orgs/{orgId}/members", web::post().to(add_org_member));
        cfg.delete("/api/v1/orgs/{orgId}/members/{userId}", web::delete().to(remove_org_member));
        cfg.get("/api/v1/orgs/{orgId}/projects", web::get().to(org_projects));
        cfg.post("/api/v1/orgs/{orgId}/projects", web::post().to(create_project));
        cfg.get("/api/v1/orgs/{orgId}/invitations", web::get().to(org_invitations));
        cfg.post("/api/v1/orgs/{orgId}/invitations", web::post().to(org_invite));

        cfg.get("/api/v1/projects/{projectId}", web::get().to(get_project));
        cfg.patch("/api/v1/projects/{projectId}", web::patch().to(patch_project));
        cfg.delete("/api/v1/projects/{projectId}", web::delete().to(delete_project));
        cfg.get("/api/v1/projects/{projectId}/members", web::get().to(project_members));
        cfg.post("/api/v1/projects/{projectId}/members", web::post().to(add_project_member));
        cfg.delete("/api/v1/projects/{projectId}/members/{userId}", web::delete().to(remove_project_member));
        cfg.get("/api/v1/projects/{projectId}/invitations", web::get().to(project_invitations));
        cfg.post("/api/v1/projects/{projectId}/invitations", web::post().to(project_invite));
    }
}
