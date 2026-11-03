//! 個人資料、改密碼；超級使用者的帳號管理。

use crate::auth::{self, CurrentUser};
use crate::azldi::Plugin;
use crate::core::CoreRef;
use crate::error::{ApiError, ApiResult};
use crate::plugin::{AppPlugin, RouteRegistrar};
use crate::tenancy::{self, UserRow, USER_COLS};
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use std::sync::Arc;

pub struct UserPlugin;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProfileBody {
    name: Option<String>,
    picture: Option<String>,
}

async fn patch_profile(core: web::Data<CoreRef>, user: CurrentUser, body: web::Json<ProfileBody>) -> ApiResult<HttpResponse> {
    if let Some(n) = &body.name {
        if n.trim().is_empty() {
            return Err(ApiError::bad_request("invalid_name", "name is required"));
        }
    }
    let row: UserRow = sqlx::query_as(&format!(
        "UPDATE tbl_user SET name = COALESCE($2, name), picture = COALESCE($3, picture) WHERE id = $1 RETURNING {USER_COLS}"
    ))
    .bind(user.id)
    .bind(body.name.as_deref().map(str::trim))
    .bind(&body.picture)
    .fetch_one(&core.pool)
    .await?;
    Ok(HttpResponse::Ok().json(row))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PasswordBody {
    current_password: String,
    new_password: String,
}

/// 改密碼：要驗舊密碼；改完撤掉其他 session（本次保留）。
async fn change_password(core: web::Data<CoreRef>, user: CurrentUser, body: web::Json<PasswordBody>) -> ApiResult<HttpResponse> {
    let hash: Option<String> = sqlx::query_scalar(
        "SELECT provider_user_access_info->>'password' FROM tbl_account_link WHERE user_id = $1 AND provider_id = $2 AND deleted_at IS NULL",
    )
    .bind(user.id)
    .bind(tenancy::BASIC_PROVIDER)
    .fetch_optional(&core.pool)
    .await?
    .flatten();
    if !hash.as_deref().map(|h| auth::verify_password(&body.current_password, h)).unwrap_or(false) {
        return Err(ApiError::forbidden("wrong_password", "current password is incorrect"));
    }
    let mut tx = core.pool.begin().await?;
    tenancy::set_password(&mut tx, user.id, &body.new_password).await?;
    sqlx::query("UPDATE tbl_user_session SET revoked_at = $3 WHERE user_id = $1 AND id <> $2 AND revoked_at IS NULL")
        .bind(user.id)
        .bind(user.session_id)
        .bind(core.clock.now())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

async fn require_super(core: &CoreRef, user: &CurrentUser) -> ApiResult<()> {
    if core.rbac.is_super_user(user.id).await? {
        Ok(())
    } else {
        Err(ApiError::forbidden("super_user_required", "super user only"))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListQuery {
    q: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

async fn admin_list(core: web::Data<CoreRef>, user: CurrentUser, q: web::Query<ListQuery>) -> ApiResult<HttpResponse> {
    require_super(&core, &user).await?;
    let page_size = q.page_size.unwrap_or(50).clamp(1, 200);
    let offset = (q.page.unwrap_or(1).max(1) - 1) * page_size;
    let pattern = q.q.as_deref().map(|s| format!("%{}%", s.trim()));
    let items: Vec<UserRow> = sqlx::query_as(&format!(
        "SELECT {USER_COLS} FROM tbl_user WHERE deleted_at IS NULL AND ($1::text IS NULL OR name ILIKE $1 OR email ILIKE $1)
          ORDER BY id LIMIT $2 OFFSET $3"
    ))
    .bind(&pattern)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&core.pool)
    .await?;
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM tbl_user WHERE deleted_at IS NULL AND ($1::text IS NULL OR name ILIKE $1 OR email ILIKE $1)",
    )
    .bind(&pattern)
    .fetch_one(&core.pool)
    .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": items, "total": total })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AdminCreateBody {
    name: String,
    email: String,
    password: String,
    privilege: Option<String>,
}

fn check_privilege(p: Option<&str>) -> ApiResult<()> {
    match p {
        None | Some("admin") | Some("manager") | Some("user") => Ok(()),
        Some(_) => Err(ApiError::bad_request("invalid_privilege", "privilege must be admin, manager or user")),
    }
}

async fn admin_create(core: web::Data<CoreRef>, user: CurrentUser, body: web::Json<AdminCreateBody>) -> ApiResult<HttpResponse> {
    require_super(&core, &user).await?;
    check_privilege(body.privilege.as_deref())?;
    let mut tx = core.pool.begin().await?;
    let u = tenancy::create_user(&mut tx, &body.name, &body.email, &body.password, body.privilege.as_deref()).await?;
    core.rbac.commit(tx, &[u.id]).await?;
    Ok(HttpResponse::Created().json(u))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AdminPatchBody {
    name: Option<String>,
    privilege: Option<Option<String>>,
    disabled: Option<bool>,
    password: Option<String>,
}

async fn admin_patch(
    core: web::Data<CoreRef>,
    user: CurrentUser,
    path: web::Path<i64>,
    body: web::Json<AdminPatchBody>,
) -> ApiResult<HttpResponse> {
    require_super(&core, &user).await?;
    let id = path.into_inner();
    if id == user.id && (body.disabled == Some(true) || matches!(body.privilege, Some(ref p) if !auth::is_super_privilege(p.as_deref()))) {
        return Err(ApiError::bad_request("cannot_demote_self", "cannot disable or demote yourself"));
    }
    if let Some(p) = &body.privilege {
        check_privilege(p.as_deref())?;
    }
    let mut tx = core.pool.begin().await?;
    tenancy::user_by_id(&mut tx, id).await?;
    if let Some(n) = &body.name {
        sqlx::query("UPDATE tbl_user SET name = $2 WHERE id = $1").bind(id).bind(n.trim()).execute(&mut *tx).await?;
    }
    if let Some(p) = &body.privilege {
        sqlx::query("UPDATE tbl_user SET privilege = $2 WHERE id = $1").bind(id).bind(p).execute(&mut *tx).await?;
    }
    if let Some(d) = body.disabled {
        sqlx::query("UPDATE tbl_user SET disabled = $2 WHERE id = $1").bind(id).bind(d).execute(&mut *tx).await?;
    }
    if let Some(pw) = &body.password {
        tenancy::set_password(&mut tx, id, pw).await?;
    }
    let row = tenancy::user_by_id(&mut tx, id).await?;
    tx.commit().await?;
    if body.disabled == Some(true) || body.password.is_some() {
        auth::revoke_all_sessions(core.get_ref(), id).await?;
    }
    core.rbac.invalidate_user(id);
    Ok(HttpResponse::Ok().json(row))
}

impl Plugin for UserPlugin {
    fn name(&self) -> &str {
        "user"
    }
    crate::plugin_boilerplate!();
}

impl AppPlugin for UserPlugin {
    fn configure_routes(&self, cfg: &mut RouteRegistrar<'_>, _self_arc: &Arc<dyn AppPlugin>) {
        cfg.patch("/api/v1/me/profile", web::patch().to(patch_profile));
        cfg.post("/api/v1/me/password", web::post().to(change_password));
        cfg.get("/api/v1/admin/users", web::get().to(admin_list));
        cfg.post("/api/v1/admin/users", web::post().to(admin_create));
        cfg.patch("/api/v1/admin/users/{userId}", web::patch().to(admin_patch));
    }
}
