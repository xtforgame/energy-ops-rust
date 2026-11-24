//! 案場↔電號一對多（卡 A）。同一組織、同一正規化電號的有效期不准重疊 —— 回 409 `meter_overlap`。
//! 正規化比對照 `ingest/bills.rs` 的寫法（`eo_station_meter_customer_number_key` trigger 維護 `customer_number_key`）。

use super::station_in_org;
use crate::common::{day_opt, err, err_d, parse_day, Ctx, RequestMeta, Tx};
use crate::gates;
use actix_web::{web, HttpResponse};
use chrono::NaiveDate;
use eo_domain::business_key::normalize_customer_number;
use eo_platform::auth::CurrentUser;
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct MeterRow {
    pub id: i64,
    pub organization_id: i64,
    pub station_id: i64,
    pub customer_number: String,
    pub usage: String,
    pub taipower_contract_no: Option<String>,
    pub feed_in_rate: Option<Decimal>,
    pub green_rate: Option<Decimal>,
    pub valid_from: Option<NaiveDate>,
    pub valid_to: Option<NaiveDate>,
    pub note: Option<String>,
}

const METER_COLS: &str = "id, organization_id, station_id, customer_number, usage, taipower_contract_no, feed_in_rate, green_rate, valid_from, valid_to, note";

fn meter_view(m: &MeterRow) -> Value {
    json!({
        "id": m.id, "stationId": m.station_id, "customerNumber": m.customer_number, "usage": m.usage,
        "taipowerContractNo": m.taipower_contract_no,
        "feedInRate": m.feed_in_rate.map(|v| v.normalize().to_string()),
        "greenRate": m.green_rate.map(|v| v.normalize().to_string()),
        "validFrom": day_opt(m.valid_from), "validTo": day_opt(m.valid_to), "note": m.note,
    })
}

/// 兩段效期是否重疊（任一邊缺日視為開放到無限）。
fn overlaps(a_from: Option<NaiveDate>, a_to: Option<NaiveDate>, b_from: Option<NaiveDate>, b_to: Option<NaiveDate>) -> bool {
    let starts_before_b_ends = match b_to {
        Some(bt) => a_from.is_none_or(|af| af <= bt),
        None => true,
    };
    let ends_after_b_starts = match b_from {
        Some(bf) => a_to.is_none_or(|at| at >= bf),
        None => true,
    };
    starts_before_b_ends && ends_after_b_starts
}

pub async fn list_meters(tx: &mut Tx<'_>, access: &eo_platform::rbac::OrgAccess, station_id: i64) -> ApiResult<Vec<Value>> {
    let st = station_in_org(tx, access, station_id).await?;
    let rows: Vec<MeterRow> = sqlx::query_as(&format!(
        "SELECT {METER_COLS} FROM tbl_energy_station_meter WHERE organization_id = $1 AND station_id = $2 AND deleted_at IS NULL ORDER BY valid_from ASC NULLS FIRST, id ASC"
    ))
    .bind(access.org_id())
    .bind(st.id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows.iter().map(meter_view).collect())
}

fn vstr(b: &Value, k: &str) -> Option<String> {
    b.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

fn vday(b: &Value, k: &str) -> ApiResult<Option<NaiveDate>> {
    match vstr(b, k) {
        None => Ok(None),
        Some(s) => parse_day(&s).map(Some).ok_or_else(|| err_d(400, "invalid_date", json!({ "field": k }))),
    }
}

fn vdecimal(b: &Value, k: &str) -> ApiResult<Option<Decimal>> {
    match b.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let s = match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => return Err(err_d(400, "invalid_number", json!({ "field": k }))),
            };
            if s.trim().is_empty() {
                return Ok(None);
            }
            s.parse().map(Some).map_err(|_| err_d(400, "invalid_number", json!({ "field": k })))
        }
    }
}

/// 同組織、同正規化電號的現有段（排除自己，給 overlap 檢查用）。
async fn existing_segments(tx: &mut Tx<'_>, org_id: i64, key: &str, exclude_id: Option<i64>) -> ApiResult<Vec<(i64, Option<NaiveDate>, Option<NaiveDate>)>> {
    Ok(sqlx::query_as(
        "SELECT id, valid_from, valid_to FROM tbl_energy_station_meter
          WHERE organization_id = $1 AND customer_number_key = $2 AND deleted_at IS NULL AND ($3::bigint IS NULL OR id != $3)",
    )
    .bind(org_id)
    .bind(key)
    .bind(exclude_id)
    .fetch_all(&mut **tx)
    .await?)
}

pub async fn create_meter(tx: &mut Tx<'_>, ctx: &Ctx, station_id: i64, b: &Value) -> ApiResult<i64> {
    let st = station_in_org(tx, &ctx.access, station_id).await?;
    let customer_number = vstr(b, "customerNumber").ok_or_else(|| err(400, "customer_number_required"))?;
    let usage = vstr(b, "usage").unwrap_or_else(|| "wholesale".into());
    if usage != "wholesale" && usage != "consumption" {
        return Err(err(400, "invalid_usage"));
    }
    let valid_from = vday(b, "validFrom")?;
    let valid_to = vday(b, "validTo")?;
    let key = normalize_customer_number(Some(&customer_number)).ok_or_else(|| err(400, "customer_number_required"))?;
    let existing = existing_segments(tx, ctx.org_id(), &key, None).await?;
    if existing.iter().any(|(_, f, t)| overlaps(valid_from, valid_to, *f, *t)) {
        return Err(err(409, "meter_overlap"));
    }
    Ok(sqlx::query_scalar(
        "INSERT INTO tbl_energy_station_meter
           (organization_id, station_id, customer_number, usage, taipower_contract_no, feed_in_rate, green_rate, valid_from, valid_to, note)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING id",
    )
    .bind(ctx.org_id())
    .bind(st.id)
    .bind(&customer_number)
    .bind(&usage)
    .bind(vstr(b, "taipowerContractNo"))
    .bind(vdecimal(b, "feedInRate")?)
    .bind(vdecimal(b, "greenRate")?)
    .bind(valid_from)
    .bind(valid_to)
    .bind(vstr(b, "note"))
    .fetch_one(&mut **tx)
    .await?)
}

/// 陌生電號人工繫結成功之後：把這張單的電號寫進案場電號表（用途 wholesale，valid_from＝這張單的計費起日），
/// 同一正規化電號已有重疊的段就不重寫（照 `create_meter` 的 overlap 判斷）；案場主電號是空的就補上
/// （執行、工作流綁定都靠它當 key）。寫不進去（例如電號是空字串）就靜靜跳過，不擋繫結本身。
pub async fn record_match_meter(tx: &mut Tx<'_>, ctx: &Ctx, station_id: i64, customer_number: &str, valid_from: Option<NaiveDate>) -> ApiResult<()> {
    let customer_number = customer_number.trim();
    if customer_number.is_empty() {
        return Ok(());
    }
    let Some(key) = normalize_customer_number(Some(customer_number)) else { return Ok(()) };
    let existing = existing_segments(tx, ctx.org_id(), &key, None).await?;
    if !existing.iter().any(|(_, f, t)| overlaps(valid_from, None, *f, *t)) {
        sqlx::query(
            "INSERT INTO tbl_energy_station_meter (organization_id, station_id, customer_number, usage, valid_from) VALUES ($1, $2, $3, 'wholesale', $4)",
        )
        .bind(ctx.org_id())
        .bind(station_id)
        .bind(customer_number)
        .bind(valid_from)
        .execute(&mut **tx)
        .await?;
    }
    // 主電號是空的才補；已經有主電號的案場不覆蓋（可能是另一個電號）。
    sqlx::query(
        "UPDATE tbl_energy_station SET customer_number = $2 WHERE id = $1 AND organization_id = $3 AND customer_number IS NULL AND deleted_at IS NULL",
    )
    .bind(station_id)
    .bind(customer_number)
    .bind(ctx.org_id())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn update_meter(tx: &mut Tx<'_>, ctx: &Ctx, meter_id: i64, b: &Value) -> ApiResult<()> {
    let row: Option<MeterRow> = sqlx::query_as(&format!("SELECT {METER_COLS} FROM tbl_energy_station_meter WHERE id = $1 AND deleted_at IS NULL"))
        .bind(meter_id)
        .fetch_optional(&mut **tx)
        .await?;
    let row = row.ok_or_else(|| err(404, "meter_not_found"))?;
    if row.organization_id != ctx.org_id() {
        return Err(err(403, "meter_not_in_org"));
    }
    let customer_number = vstr(b, "customerNumber").unwrap_or(row.customer_number.clone());
    let usage = vstr(b, "usage").unwrap_or(row.usage.clone());
    if usage != "wholesale" && usage != "consumption" {
        return Err(err(400, "invalid_usage"));
    }
    let valid_from = vday(b, "validFrom")?;
    let valid_to = vday(b, "validTo")?;
    let key = normalize_customer_number(Some(&customer_number)).ok_or_else(|| err(400, "customer_number_required"))?;
    let existing = existing_segments(tx, ctx.org_id(), &key, Some(meter_id)).await?;
    if existing.iter().any(|(_, f, t)| overlaps(valid_from, valid_to, *f, *t)) {
        return Err(err(409, "meter_overlap"));
    }
    sqlx::query(
        "UPDATE tbl_energy_station_meter SET
           customer_number = $2, usage = $3, taipower_contract_no = $4, feed_in_rate = $5, green_rate = $6,
           valid_from = $7, valid_to = $8, note = $9
         WHERE id = $1 AND organization_id = $10 AND deleted_at IS NULL",
    )
    .bind(meter_id)
    .bind(&customer_number)
    .bind(&usage)
    .bind(vstr(b, "taipowerContractNo"))
    .bind(vdecimal(b, "feedInRate")?)
    .bind(vdecimal(b, "greenRate")?)
    .bind(valid_from)
    .bind(valid_to)
    .bind(vstr(b, "note"))
    .bind(ctx.org_id())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

// ── handlers ──

type Core = web::Data<CoreRef>;

pub async fn list_handler(core: Core, user: CurrentUser, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, station_id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::FLOW_VIEW).await?;
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(json!({ "items": list_meters(&mut tx, &access, station_id).await? })))
}

pub async fn create_handler(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let (org, station_id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::MASTER_MANAGE).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    create_meter(&mut tx, &ctx, station_id, &body).await?;
    let out = list_meters(&mut tx, &ctx.access, station_id).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "items": out })))
}

pub async fn update_handler(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let (org, meter_id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::MASTER_MANAGE).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    update_meter(&mut tx, &ctx, meter_id, &body).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}
