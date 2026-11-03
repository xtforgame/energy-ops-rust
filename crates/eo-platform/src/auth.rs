//! 身分：「這個請求是誰」的唯一縫。
//!
//! token 是隨機 32 bytes（base64url），DB 只存 sha256。放在 cookie `eo_session`（httpOnly）
//! 或 `Authorization: Bearer`。閒置期限滑動（14 天），絕對期限從建立起算（183 天）。

use crate::core::CoreRef;
use crate::error::{ApiError, ApiResult};
use actix_web::dev::Payload;
use actix_web::{web, FromRequest, HttpRequest};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::future::Future;
use std::pin::Pin;

pub const SESSION_COOKIE: &str = "eo_session";
pub const IDLE_TIMEOUT_DAYS: i64 = 14;
pub const ABSOLUTE_TIMEOUT_DAYS: i64 = 183;
/// last_seen_at 至少隔這麼久才寫一次，避免每個請求都寫 DB。
const TOUCH_INTERVAL_MINUTES: i64 = 10;

const SUPER_USER_PRIVILEGES: [&str; 2] = ["admin", "manager"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentUser {
    pub id: i64,
    pub name: String,
    pub email: Option<String>,
    pub privilege: Option<String>,
    #[serde(skip)]
    pub session_id: i64,
}

impl CurrentUser {
    /// 以 session 讀到的 privilege 判斷；RBAC 的超級使用者判定另外讀 DB。
    pub fn is_super(&self) -> bool {
        self.privilege.as_deref().map(|p| SUPER_USER_PRIVILEGES.contains(&p)).unwrap_or(false)
    }
}

pub fn is_super_privilege(p: Option<&str>) -> bool {
    p.map(|p| SUPER_USER_PRIVILEGES.contains(&p)).unwrap_or(false)
}

pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub fn new_token() -> String {
    let mut buf = [0u8; 32];
    rand::rng().fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

pub struct NewSession {
    pub token: String,
    pub absolute_expires_at: DateTime<Utc>,
}

/// 建一筆 session，回傳明文 token（只此一次）。
pub async fn create_session(
    core: &CoreRef,
    user_id: i64,
    account_link_id: Option<i64>,
    user_agent: Option<&str>,
    ip: Option<&str>,
) -> ApiResult<NewSession> {
    let token = new_token();
    let now = core.clock.now();
    let absolute = now + Duration::days(ABSOLUTE_TIMEOUT_DAYS);
    let idle = now + Duration::days(IDLE_TIMEOUT_DAYS);
    sqlx::query(
        "INSERT INTO tbl_user_session (token_hash, user_id, account_link_id, created_at, last_seen_at, expires_at, absolute_expires_at, user_agent, ip)
         VALUES ($1, $2, $3, $4, $4, $5, $6, $7, $8)",
    )
    .bind(hash_token(&token))
    .bind(user_id)
    .bind(account_link_id)
    .bind(now)
    .bind(idle)
    .bind(absolute)
    .bind(user_agent)
    .bind(ip)
    .execute(&core.pool)
    .await?;
    Ok(NewSession { token, absolute_expires_at: absolute })
}

pub async fn revoke_session(core: &CoreRef, session_id: i64) -> ApiResult<()> {
    sqlx::query("UPDATE tbl_user_session SET revoked_at = $2 WHERE id = $1 AND revoked_at IS NULL")
        .bind(session_id)
        .bind(core.clock.now())
        .execute(&core.pool)
        .await?;
    Ok(())
}

/// 使用者所有 session 一起失效（改密碼、停用）。
pub async fn revoke_all_sessions(core: &CoreRef, user_id: i64) -> ApiResult<()> {
    sqlx::query("UPDATE tbl_user_session SET revoked_at = $2 WHERE user_id = $1 AND revoked_at IS NULL")
        .bind(user_id)
        .bind(core.clock.now())
        .execute(&core.pool)
        .await?;
    Ok(())
}

fn token_from_request(req: &HttpRequest) -> Option<String> {
    if let Some(h) = req.headers().get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(t) = h.strip_prefix("Bearer ") {
            let t = t.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    req.cookie(SESSION_COOKIE).map(|c| c.value().to_string()).filter(|v| !v.is_empty())
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    session_id: i64,
    last_seen_at: DateTime<Utc>,
    absolute_expires_at: DateTime<Utc>,
    user_id: i64,
    name: String,
    email: Option<String>,
    privilege: Option<String>,
}

/// 依 token 找出使用者；過期、撤銷、停用一律視為沒有身分。
pub async fn resolve_session(core: &CoreRef, token: &str) -> ApiResult<Option<CurrentUser>> {
    let now = core.clock.now();
    let row: Option<SessionRow> = sqlx::query_as(
        "SELECT s.id AS session_id, s.last_seen_at, s.absolute_expires_at,
                u.id AS user_id, u.name, u.email, u.privilege
           FROM tbl_user_session s JOIN tbl_user u ON u.id = s.user_id
          WHERE s.token_hash = $1 AND s.revoked_at IS NULL
            AND s.expires_at > $2 AND s.absolute_expires_at > $2
            AND u.deleted_at IS NULL AND NOT u.disabled",
    )
    .bind(hash_token(token))
    .bind(now)
    .fetch_optional(&core.pool)
    .await?;
    let Some(r) = row else { return Ok(None) };
    if now - r.last_seen_at > Duration::minutes(TOUCH_INTERVAL_MINUTES) {
        let idle = (now + Duration::days(IDLE_TIMEOUT_DAYS)).min(r.absolute_expires_at);
        sqlx::query("UPDATE tbl_user_session SET last_seen_at = $2, expires_at = $3 WHERE id = $1")
            .bind(r.session_id)
            .bind(now)
            .bind(idle)
            .execute(&core.pool)
            .await?;
    }
    Ok(Some(CurrentUser {
        id: r.user_id,
        name: r.name,
        email: r.email,
        privilege: r.privilege,
        session_id: r.session_id,
    }))
}

/// handler 參數寫 `user: CurrentUser` 就等於「必須登入」。
impl FromRequest for CurrentUser {
    type Error = ApiError;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        let core = req.app_data::<web::Data<CoreRef>>().cloned();
        let token = token_from_request(req);
        Box::pin(async move {
            let core = core.ok_or_else(|| ApiError::internal("core not configured"))?;
            let token = token.ok_or_else(ApiError::unauthorized)?;
            resolve_session(core.get_ref(), &token).await?.ok_or_else(ApiError::unauthorized)
        })
    }
}

/// 可有可無的身分（例如「用 token 查邀請摘要」不需要登入）。
pub struct MaybeUser(pub Option<CurrentUser>);

impl FromRequest for MaybeUser {
    type Error = ApiError;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        let core = req.app_data::<web::Data<CoreRef>>().cloned();
        let token = token_from_request(req);
        Box::pin(async move {
            let (Some(core), Some(token)) = (core, token) else { return Ok(MaybeUser(None)) };
            Ok(MaybeUser(resolve_session(core.get_ref(), &token).await?))
        })
    }
}

pub fn session_cookie(token: &str, max_age: Duration, secure: bool) -> actix_web::cookie::Cookie<'static> {
    actix_web::cookie::Cookie::build(SESSION_COOKIE, token.to_string())
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(actix_web::cookie::SameSite::Lax)
        .max_age(actix_web::cookie::time::Duration::seconds(max_age.num_seconds()))
        .finish()
}

pub fn clear_session_cookie(secure: bool) -> actix_web::cookie::Cookie<'static> {
    session_cookie("", Duration::seconds(0), secure)
}

pub fn hash_password(plain: &str) -> ApiResult<String> {
    bcrypt::hash(plain, 10).map_err(|e| ApiError::internal(format!("bcrypt: {e}")))
}

pub fn verify_password(plain: &str, hash: &str) -> bool {
    bcrypt::verify(plain, hash).unwrap_or(false)
}

/// 帳號不存在時也跑一次同成本的 bcrypt，回應時間不洩漏帳號是否存在。
pub fn verify_against_dummy(plain: &str) {
    static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let h = DUMMY.get_or_init(|| bcrypt::hash("dummy-password", 10).unwrap_or_default());
    let _ = bcrypt::verify(plain, h);
}
