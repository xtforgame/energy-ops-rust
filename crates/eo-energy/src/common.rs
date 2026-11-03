//! 能源 plugin 共用的小工具：錯誤、請求資訊、操作情境、日期與 JSON 的轉換。

use crate::audit::Actor;
use actix_web::dev::Payload;
use actix_web::http::StatusCode;
use actix_web::{FromRequest, HttpRequest};
use chrono::{DateTime, NaiveDate, Utc};
use eo_platform::core::CoreRef;
use eo_platform::error::{ApiError, ApiResult};
use eo_platform::rbac::{OrgAccess, ScopeRef, PLATFORM_RBAC_MANAGE};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;
use std::future::{ready, Ready};

pub use eo_platform::rbac::grant::Tx;

/// 帶 HTTP 狀態與穩定錯誤碼的錯誤（照 App 5 的 `{code, details}`）。
pub fn err(status: u16, code: &str) -> ApiError {
    ApiError::new(StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), code, code)
}

pub fn err_d(status: u16, code: &str, details: Value) -> ApiError {
    err(status, code).with_details(details)
}

/// 請求的來源資訊（稽核要記）。trace id 優先沿用上游給的 `x-request-id`，沒有就自己配一個。
#[derive(Debug, Clone, Default)]
pub struct RequestMeta {
    pub ip: Option<String>,
    pub trace_id: Option<String>,
}

impl FromRequest for RequestMeta {
    type Error = ApiError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        let ip = req.connection_info().realip_remote_addr().map(String::from);
        let trace_id = req
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.chars().take(64).collect::<String>())
            .filter(|s| !s.is_empty())
            .or_else(|| Some(uuid::Uuid::new_v4().simple().to_string()));
        ready(Ok(RequestMeta { ip, trace_id }))
    }
}

/// 一次操作的情境：誰（已判權的組織存取）、用哪套基礎設施。
/// 操作者一律從 `OrgAccess` 推導，不收 body 給的身分。
#[derive(Clone)]
pub struct Ctx {
    pub core: CoreRef,
    pub access: OrgAccess,
    pub actor: Actor,
}

impl Ctx {
    pub fn new(core: &CoreRef, access: OrgAccess, meta: &RequestMeta) -> Self {
        let actor = Actor::from_access(&access, meta);
        Self { core: core.clone(), access, actor }
    }

    /// 背景任務或其他模組以系統身分操作。
    pub fn system(core: &CoreRef, org_id: i64) -> Self {
        let access = core.rbac.system_access(org_id);
        Self { core: core.clone(), actor: Actor::system(), access }
    }

    pub fn org_id(&self) -> i64 {
        self.access.org_id()
    }

    pub fn user_id(&self) -> Option<i64> {
        self.access.user_id()
    }

    pub fn now(&self) -> DateTime<Utc> {
        self.core.clock.now()
    }

    pub fn today(&self) -> NaiveDate {
        self.core.clock.today()
    }

    /// 呼叫者在這個組織有沒有某個 permission（不擋，只用來分支）。
    /// `platform:rbac:manage` 視同持有所有能源 permission，與閘的語意一致。
    pub async fn has(&self, permission: &str) -> ApiResult<bool> {
        has_permission(&self.core, &self.access, permission).await
    }
}

pub async fn has_permission(core: &CoreRef, access: &OrgAccess, permission: &str) -> ApiResult<bool> {
    if access.is_super_user() {
        return Ok(true);
    }
    let Some(uid) = access.user_id() else { return Ok(true) };
    core.rbac.has(uid, &ScopeRef::org(access.org_id()), &[permission, PLATFORM_RBAC_MANAGE]).await
}

/// body 反序列化＋共用 schema 驗證；失敗一律 400。
pub fn parse_body<T: DeserializeOwned + eo_domain::schemas::Validate>(body: &Value) -> ApiResult<T> {
    eo_domain::schemas::parse_body(body).map_err(|e| match e {
        eo_domain::schemas::SchemaError::Shape(m) => err_d(400, "invalid_body", serde_json::json!({ "message": m })),
        eo_domain::schemas::SchemaError::Invalid(issues) => err_d(400, "invalid_body", serde_json::json!({ "issues": issues })),
    })
}

/// `YYYY-MM-DD`。
pub fn day(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

pub fn day_opt(d: Option<NaiveDate>) -> Option<String> {
    d.map(day)
}

/// 查詢參數裡的日期：空的當沒給，格式不對 400。
pub fn parse_day(v: &str) -> Option<NaiveDate> {
    let s: String = v.chars().take(10).collect();
    NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok().filter(|_| s.len() == 10)
}

pub fn query_day(field: &str, v: Option<&str>) -> ApiResult<Option<NaiveDate>> {
    match v.filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => parse_day(s)
            .map(Some)
            .ok_or_else(|| err_d(400, "invalid_date", serde_json::json!({ "field": field }))),
    }
}

/// 等了幾天（向下取整）。
pub fn days_since(now: DateTime<Utc>, t: DateTime<Utc>) -> i64 {
    (now - t).num_milliseconds().div_euclid(86_400_000)
}

/// 使用者 id → 名字（查不到的不放）。
pub async fn user_names(tx: &mut Tx<'_>, ids: &[i64]) -> ApiResult<HashMap<i64, String>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, name FROM tbl_user WHERE id = ANY($1) AND deleted_at IS NULL ORDER BY id")
            .bind(ids)
            .fetch_all(&mut **tx)
            .await?;
    Ok(rows.into_iter().collect())
}

pub async fn user_name(tx: &mut Tx<'_>, id: i64) -> ApiResult<Option<String>> {
    Ok(user_names(tx, &[id]).await?.remove(&id))
}

/// 去重但保留第一次出現的順序。
pub fn uniq<T: PartialEq + Clone>(v: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut out: Vec<T> = Vec::new();
    for x in v {
        if !out.contains(&x) {
            out.push(x);
        }
    }
    out
}

/// 某些 id 欄位可能是字串型（例如 `subject_id`）：只認正整數。
pub fn numeric_id(s: &str) -> Option<i64> {
    s.parse::<i64>().ok().filter(|n| *n > 0)
}
