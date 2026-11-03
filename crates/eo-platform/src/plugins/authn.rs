//! 登入、登出、目前 session、註冊（只能透過邀請，或開了 EO_ALLOW_SIGNUP）。

use crate::auth::{self, CurrentUser};
use crate::azldi::Plugin;
use crate::core::CoreRef;
use crate::error::{ApiError, ApiResult};
use crate::plugin::{AppPlugin, RouteRegistrar, StartContext, StartError};
use crate::tenancy::{self, UserRow, BASIC_PROVIDER, USER_COLS};
use actix_web::{web, HttpRequest, HttpResponse};
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;

pub struct AuthnPlugin {
    core: CoreRef,
}

impl AuthnPlugin {
    pub fn new(core: CoreRef) -> Self {
        Self { core }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LoginBody {
    username: String,
    password: String,
}

#[derive(sqlx::FromRow)]
struct LoginRow {
    account_link_id: i64,
    password_hash: Option<String>,
    user_id: i64,
    disabled: bool,
}

fn client_ip(req: &HttpRequest) -> Option<String> {
    req.connection_info().realip_remote_addr().map(String::from)
}

fn user_agent(req: &HttpRequest) -> Option<String> {
    req.headers().get("user-agent").and_then(|v| v.to_str().ok()).map(String::from)
}

async fn session_response(core: &CoreRef, req: &HttpRequest, user_id: i64, account_link_id: Option<i64>) -> ApiResult<HttpResponse> {
    let s = auth::create_session(core, user_id, account_link_id, user_agent(req).as_deref(), client_ip(req).as_deref()).await?;
    let user: UserRow = sqlx::query_as(&format!("SELECT {USER_COLS} FROM tbl_user WHERE id = $1"))
        .bind(user_id)
        .fetch_one(&core.pool)
        .await?;
    let max_age = s.absolute_expires_at - core.clock.now();
    Ok(HttpResponse::Ok()
        .cookie(auth::session_cookie(&s.token, max_age, core.config.cookie_secure))
        .json(serde_json::json!({ "user": user, "token": s.token, "expiresAt": s.absolute_expires_at })))
}

/// POST /api/v1/auth/login。帳號錯與密碼錯回同一個錯誤，不洩漏帳號是否存在。
async fn login(core: web::Data<CoreRef>, req: HttpRequest, body: web::Json<LoginBody>) -> ApiResult<HttpResponse> {
    let row: Option<LoginRow> = sqlx::query_as(
        "SELECT a.id AS account_link_id, a.provider_user_access_info->>'password' AS password_hash, u.id AS user_id, u.disabled
           FROM tbl_account_link a JOIN tbl_user u ON u.id = a.user_id AND u.deleted_at IS NULL
          WHERE a.provider_id = $1 AND lower(a.provider_user_id) = lower($2) AND a.deleted_at IS NULL",
    )
    .bind(BASIC_PROVIDER)
    .bind(body.username.trim())
    .fetch_optional(&core.pool)
    .await?;
    let wrong = || ApiError::unauthorized().with_details(serde_json::json!({ "reason": "wrong_credentials" }));
    let Some(row) = row else {
        // 沒這個帳號也跑一次 bcrypt，讓回應時間不洩漏帳號是否存在。
        auth::verify_against_dummy(&body.password);
        return Err(wrong());
    };
    if !row.password_hash.as_deref().map(|h| auth::verify_password(&body.password, h)).unwrap_or(false) {
        return Err(wrong());
    }
    if row.disabled {
        return Err(ApiError::forbidden("account_disabled", "this account has been disabled"));
    }
    session_response(core.get_ref(), &req, row.user_id, Some(row.account_link_id)).await
}

async fn logout(core: web::Data<CoreRef>, user: CurrentUser) -> ApiResult<HttpResponse> {
    auth::revoke_session(core.get_ref(), user.session_id).await?;
    Ok(HttpResponse::Ok().cookie(auth::clear_session_cookie(core.config.cookie_secure)).json(serde_json::json!({ "ok": true })))
}

async fn session(core: web::Data<CoreRef>, user: CurrentUser) -> ApiResult<HttpResponse> {
    let row: UserRow = sqlx::query_as(&format!("SELECT {USER_COLS} FROM tbl_user WHERE id = $1"))
        .bind(user.id)
        .fetch_one(&core.pool)
        .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "user": row, "superUser": user.is_super() })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RegisterBody {
    name: String,
    email: String,
    password: String,
    invitation_token: Option<String>,
}

/// 註冊。必須帶有效的邀請 token（email 要和邀請相同），除非開了 EO_ALLOW_SIGNUP。
/// 註冊不等於接受邀請：前端接著呼叫 accept。
async fn register(core: web::Data<CoreRef>, req: HttpRequest, body: web::Json<RegisterBody>) -> ApiResult<HttpResponse> {
    let allow_signup = std::env::var("EO_ALLOW_SIGNUP").map(|v| v == "1" || v == "true").unwrap_or(false);
    let mut tx = core.pool.begin().await?;
    if !allow_signup {
        let token = body
            .invitation_token
            .as_deref()
            .ok_or_else(|| ApiError::forbidden("signup_closed", "registration requires an invitation"))?;
        let email: Option<String> = sqlx::query_scalar(
            "SELECT invitee_email FROM tbl_invitation WHERE token = $1 AND status = 'pending' AND expires_at > $2 AND deleted_at IS NULL",
        )
        .bind(token)
        .bind(core.clock.now())
        .fetch_optional(&mut *tx)
        .await?;
        let email = email.ok_or_else(|| ApiError::forbidden("invalid_invitation", "invitation is invalid or expired"))?;
        if tenancy::normalize_email(&email) != tenancy::normalize_email(&body.email) {
            return Err(ApiError::forbidden("invitation_email_mismatch", "email does not match the invitation"));
        }
    }
    let user = tenancy::create_user(&mut tx, &body.name, &body.email, &body.password, None).await?;
    let link: i64 = sqlx::query_scalar("SELECT id FROM tbl_account_link WHERE user_id = $1 AND provider_id = $2")
        .bind(user.id)
        .bind(BASIC_PROVIDER)
        .fetch_one(&mut *tx)
        .await?;
    core.rbac.commit(tx, &[user.id]).await?;
    session_response(core.get_ref(), &req, user.id, Some(link)).await
}

impl Plugin for AuthnPlugin {
    fn name(&self) -> &str {
        "authn"
    }
    crate::plugin_boilerplate!();

    fn deps(&self, method: &str) -> Vec<&str> {
        match method {
            "start" => vec!["rbac"],
            _ => vec![],
        }
    }
}

#[async_trait]
impl AppPlugin for AuthnPlugin {
    fn configure_routes(&self, cfg: &mut RouteRegistrar<'_>, _self_arc: &Arc<dyn AppPlugin>) {
        cfg.post("/api/v1/auth/login", web::post().to(login));
        cfg.post("/api/v1/auth/logout", web::post().to(logout));
        cfg.get("/api/v1/auth/session", web::get().to(session));
        cfg.post("/api/v1/auth/register", web::post().to(register));
    }

    /// 設了 EO_BOOTSTRAP_ADMIN_USER／PASSWORD 而且還沒有這個帳號時，建一個超級使用者。
    async fn on_start(&self, _ctx: &StartContext) -> Result<(), StartError> {
        let Some((email, password)) = self.core.config.bootstrap_admin.clone() else { return Ok(()) };
        let err = |e: ApiError| StartError::new(format!("bootstrap admin: {e}"));
        let mut tx = self.core.pool.begin().await.map_err(|e| err(e.into()))?;
        if tenancy::user_by_email(&mut tx, &email).await.map_err(err)?.is_some() {
            return Ok(());
        }
        let u = tenancy::create_user(&mut tx, "Administrator", &email, &password, Some("admin")).await.map_err(err)?;
        tx.commit().await.map_err(|e| err(e.into()))?;
        log::info!("[authn] bootstrap admin created: {} (#{})", email, u.id);
        Ok(())
    }
}
