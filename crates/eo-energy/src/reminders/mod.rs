//! 提醒機制（卡 E）：規則參數與假日表的 CRUD、系統內通知、手動／背景檢查器。
//! 規則與假日表的讀用 `FLOW_VIEW`，改用 `MASTER_MANAGE`（照 `master_data` 的慣例）；通知讀寫都是 `FLOW_VIEW`
//! （組織內共用，誰看得到行事曆就看得到通知，不限管理員）。

pub mod check;

use crate::common::{day, err, err_d, parse_day, Ctx, RequestMeta, Tx};
use crate::{gates, path};
use actix_web::{web, HttpResponse};
use chrono::NaiveDate;
use eo_platform::auth::CurrentUser;
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use eo_platform::plugin::{AppPlugin, RouteRegistrar, StartContext, StartError};
use serde_json::{json, Value};
use sqlx::FromRow;
use std::sync::{Arc, Mutex};

pub use check::run_check;

/// 已知的提醒種類（規則表的 `kind`）。固定四種，不讓前端／客戶自己取名。
pub const RULE_KINDS: [&str; 4] = ["installmentDue", "contractExpiry", "contractRenewal", "billExpected"];

pub struct RemindersPlugin {
    core: CoreRef,
    background: Mutex<Option<actix_web::rt::task::JoinHandle<()>>>,
}

impl RemindersPlugin {
    pub fn new(core: CoreRef) -> Self {
        Self { core, background: Mutex::new(None) }
    }
}

crate::energy_plugin!(RemindersPlugin, "energy_reminders");

#[async_trait::async_trait]
impl AppPlugin for RemindersPlugin {
    fn configure_routes(&self, cfg: &mut RouteRegistrar<'_>, _self_arc: &Arc<dyn AppPlugin>) {
        cfg.get(&path("/reminder-rules"), web::get().to(list_rules_handler));
        cfg.put(&path("/reminder-rules"), web::put().to(put_rules_handler));
        cfg.get(&path("/holidays"), web::get().to(list_holidays_handler));
        cfg.post(&path("/holidays"), web::post().to(create_holiday_handler));
        cfg.delete(&path("/holidays/{holidayId}"), web::delete().to(delete_holiday_handler));
        cfg.get(&path("/notifications"), web::get().to(list_notifications_handler));
        cfg.post(&path("/notifications/{notificationId}/read"), web::post().to(mark_read_handler));
        cfg.post(&path("/reminders/run-check"), web::post().to(run_check_handler));
    }

    async fn on_start(&self, _ctx: &StartContext) -> Result<(), StartError> {
        let secs = self.core.config.reminder_check_secs;
        if secs > 0 {
            *self.background.lock().unwrap() = Some(check::spawn_background(Arc::clone(&self.core), secs));
            log::info!("[energy] reminder check every {secs}s");
        }
        Ok(())
    }

    async fn on_destroy(&self) {
        if let Some(h) = self.background.lock().unwrap().take() {
            h.abort();
        }
    }
}

// ── 規則參數 ──

#[derive(Debug, Clone, FromRow)]
struct RuleRow {
    kind: String,
    lead_days: Option<i32>,
    repeat_days: Option<i32>,
    business_days: bool,
    day_of_month: Option<i32>,
    enabled: bool,
}

fn rule_view(r: &RuleRow) -> Value {
    json!({
        "kind": r.kind, "leadDays": r.lead_days, "repeatDays": r.repeat_days,
        "businessDays": r.business_days, "dayOfMonth": r.day_of_month, "enabled": r.enabled,
    })
}

/// 四種固定 kind 都回一列：資料庫沒有的那種回全部空值的「未設定」列（前端不用猜）。
pub async fn list_rules(tx: &mut Tx<'_>, org: i64) -> ApiResult<Vec<Value>> {
    let rows: Vec<RuleRow> = sqlx::query_as(
        "SELECT kind, lead_days, repeat_days, business_days, day_of_month, enabled
           FROM tbl_energy_reminder_rule WHERE organization_id = $1 AND deleted_at IS NULL",
    )
    .bind(org)
    .fetch_all(&mut **tx)
    .await?;
    Ok(RULE_KINDS
        .iter()
        .map(|k| {
            rows.iter().find(|r| r.kind == *k).map(rule_view).unwrap_or_else(|| {
                json!({ "kind": k, "leadDays": null, "repeatDays": null, "businessDays": false, "dayOfMonth": null, "enabled": false })
            })
        })
        .collect())
}

fn vi32_opt(b: &Value, k: &str) -> Option<i32> {
    b.get(k).and_then(Value::as_i64).map(|n| n as i32)
}

/// 整包覆蓋（body: `{ rules: [{kind, leadDays, repeatDays, businessDays, dayOfMonth, enabled}, ...] }`）。
/// 每種 kind upsert 一列；沒出現在 body 裡的 kind 不動。
pub async fn put_rules(tx: &mut Tx<'_>, ctx: &Ctx, b: &Value) -> ApiResult<()> {
    let org = ctx.org_id();
    let items = b.get("rules").and_then(Value::as_array).cloned().unwrap_or_default();
    for item in items {
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or("").to_string();
        if !RULE_KINDS.contains(&kind.as_str()) {
            return Err(err_d(400, "unknown_kind", json!({ "kind": kind })));
        }
        let lead_days = vi32_opt(&item, "leadDays");
        let repeat_days = vi32_opt(&item, "repeatDays");
        let business_days = item.get("businessDays").and_then(Value::as_bool).unwrap_or(false);
        let day_of_month = vi32_opt(&item, "dayOfMonth");
        let enabled = item.get("enabled").and_then(Value::as_bool).unwrap_or(false);
        sqlx::query(
            "INSERT INTO tbl_energy_reminder_rule (organization_id, kind, lead_days, repeat_days, business_days, day_of_month, enabled)
             VALUES ($1,$2,$3,$4,$5,$6,$7)
             ON CONFLICT (organization_id, kind) WHERE deleted_at IS NULL DO UPDATE SET
               lead_days = EXCLUDED.lead_days, repeat_days = EXCLUDED.repeat_days, business_days = EXCLUDED.business_days,
               day_of_month = EXCLUDED.day_of_month, enabled = EXCLUDED.enabled, updated_at = clock_timestamp()",
        )
        .bind(org)
        .bind(&kind)
        .bind(lead_days)
        .bind(repeat_days)
        .bind(business_days)
        .bind(day_of_month)
        .bind(enabled)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

// ── 假日表 ──

#[derive(Debug, Clone, FromRow)]
struct HolidayRow {
    id: i64,
    day: NaiveDate,
    name: Option<String>,
    is_workday: bool,
}

fn holiday_view(r: &HolidayRow) -> Value {
    json!({ "id": r.id, "day": day(r.day), "name": r.name, "isWorkday": r.is_workday })
}

pub async fn list_holidays(tx: &mut Tx<'_>, org: i64) -> ApiResult<Vec<Value>> {
    let rows: Vec<HolidayRow> = sqlx::query_as(
        "SELECT id, day, name, is_workday FROM tbl_energy_holiday WHERE organization_id = $1 AND deleted_at IS NULL ORDER BY day ASC, id ASC",
    )
    .bind(org)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows.iter().map(holiday_view).collect())
}

pub async fn create_holiday(tx: &mut Tx<'_>, ctx: &Ctx, b: &Value) -> ApiResult<i64> {
    let day_str = b.get("day").and_then(Value::as_str).ok_or_else(|| err(400, "day_required"))?;
    let d = parse_day(day_str).ok_or_else(|| err_d(400, "invalid_date", json!({ "field": "day" })))?;
    let name = b.get("name").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let is_workday = b.get("isWorkday").and_then(Value::as_bool).unwrap_or(false);
    let id: Option<i64> = sqlx::query_scalar(
        "INSERT INTO tbl_energy_holiday (organization_id, day, name, is_workday) VALUES ($1,$2,$3,$4)
         ON CONFLICT (organization_id, day) WHERE deleted_at IS NULL DO NOTHING RETURNING id",
    )
    .bind(ctx.org_id())
    .bind(d)
    .bind(&name)
    .bind(is_workday)
    .fetch_optional(&mut **tx)
    .await?;
    id.ok_or_else(|| err(409, "holiday_exists"))
}

pub async fn delete_holiday(tx: &mut Tx<'_>, ctx: &Ctx, id: i64) -> ApiResult<()> {
    let n = sqlx::query("UPDATE tbl_energy_holiday SET deleted_at = clock_timestamp() WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL")
        .bind(id)
        .bind(ctx.org_id())
        .execute(&mut **tx)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(err(404, "holiday_not_found"));
    }
    Ok(())
}

// ── 通知 ──

#[derive(Debug, Clone, FromRow)]
struct NotificationRow {
    id: i64,
    event_id: i64,
    kind: String,
    message: String,
    notify_on: NaiveDate,
    read_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn notification_view(r: &NotificationRow, event_date: Option<NaiveDate>, event_kind: Option<String>) -> Value {
    json!({
        "id": r.id, "eventId": r.event_id, "kind": r.kind, "message": r.message, "notifyOn": day(r.notify_on),
        "readAt": r.read_at, "eventDate": event_date.map(day), "eventKind": event_kind,
    })
}

/// `unreadOnly=true` 只回未讀；一律按 `notifyOn` 新到舊排序。
pub async fn list_notifications(tx: &mut Tx<'_>, org: i64, unread_only: bool) -> ApiResult<Vec<Value>> {
    let rows: Vec<NotificationRow> = sqlx::query_as(
        "SELECT id, event_id, kind, message, notify_on, read_at FROM tbl_energy_notification
          WHERE organization_id = $1 AND deleted_at IS NULL AND ($2::bool = false OR read_at IS NULL)
          ORDER BY notify_on DESC, id DESC LIMIT 200",
    )
    .bind(org)
    .bind(unread_only)
    .fetch_all(&mut **tx)
    .await?;
    let event_ids: Vec<i64> = crate::common::uniq(rows.iter().map(|r| r.event_id));
    let events: Vec<(i64, NaiveDate, String)> = if event_ids.is_empty() {
        vec![]
    } else {
        sqlx::query_as("SELECT id, event_date, kind FROM tbl_energy_calendar_event WHERE id = ANY($1)").bind(&event_ids).fetch_all(&mut **tx).await?
    };
    Ok(rows
        .iter()
        .map(|r| {
            let ev = events.iter().find(|e| e.0 == r.event_id);
            notification_view(r, ev.map(|e| e.1), ev.map(|e| e.2.clone()))
        })
        .collect())
}

pub async fn mark_read(tx: &mut Tx<'_>, ctx: &Ctx, id: i64) -> ApiResult<()> {
    let n = sqlx::query(
        "UPDATE tbl_energy_notification SET read_at = clock_timestamp(), read_by = $3
          WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL AND read_at IS NULL",
    )
    .bind(id)
    .bind(ctx.org_id())
    .bind(ctx.user_id())
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if n == 0 {
        // 已經讀過或查無此筆：對使用者來說都是「已讀」，不報錯（標已讀不是強操作）。
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM tbl_energy_notification WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL)")
                .bind(id)
                .bind(ctx.org_id())
                .fetch_one(&mut **tx)
                .await?;
        if !exists {
            return Err(err(404, "notification_not_found"));
        }
    }
    Ok(())
}

// ── handlers ──

type Core = web::Data<CoreRef>;

async fn list_rules_handler(core: Core, user: CurrentUser, org: web::Path<i64>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::FLOW_VIEW).await?;
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(json!({ "items": list_rules(&mut tx, access.org_id()).await? })))
}

async fn put_rules_handler(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::MASTER_MANAGE).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    put_rules(&mut tx, &ctx, &body).await?;
    let items = list_rules(&mut tx, ctx.org_id()).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

async fn list_holidays_handler(core: Core, user: CurrentUser, org: web::Path<i64>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::FLOW_VIEW).await?;
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(json!({ "items": list_holidays(&mut tx, access.org_id()).await? })))
}

async fn create_holiday_handler(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::MASTER_MANAGE).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    create_holiday(&mut tx, &ctx, &body).await?;
    let items = list_holidays(&mut tx, ctx.org_id()).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

async fn delete_holiday_handler(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::MASTER_MANAGE).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    delete_holiday(&mut tx, &ctx, id).await?;
    let items = list_holidays(&mut tx, ctx.org_id()).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NotificationQuery {
    unread: Option<i32>,
}

async fn list_notifications_handler(core: Core, user: CurrentUser, org: web::Path<i64>, q: web::Query<NotificationQuery>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::FLOW_VIEW).await?;
    let mut tx = core.pool.begin().await?;
    let unread_only = q.unread.unwrap_or(0) == 1;
    Ok(HttpResponse::Ok().json(json!({ "items": list_notifications(&mut tx, access.org_id(), unread_only).await? })))
}

async fn mark_read_handler(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::FLOW_VIEW).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    mark_read(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunCheckBody {
    as_of: Option<String>,
}

/// 手動端點：smoke 與管理員用，不等背景輪詢。`asOf` 不帶就是今天。
async fn run_check_handler(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, body: Option<web::Json<RunCheckBody>>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::MASTER_MANAGE).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let as_of = match body.as_ref().and_then(|b| b.as_of.as_deref()) {
        Some(s) => parse_day(s).ok_or_else(|| err_d(400, "invalid_date", json!({ "field": "asOf" })))?,
        None => ctx.today(),
    };
    let mut tx = core.pool.begin().await?;
    let out = run_check(&mut tx, &ctx, as_of).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(out))
}
