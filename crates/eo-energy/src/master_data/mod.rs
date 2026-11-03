//! 主檔（地主／案場）的讀取層，以及地址 → 座標（推估不寫入、採用才寫入）。
//! 詳情一次組好整包（「一個地主的所有錢與時間」），不讓前端打七支自己拼。

mod overview;

use crate::audit::{self, AuditInput};
use crate::common::{day, day_opt, err, Ctx, RequestMeta, Tx};
use crate::{gates, path};
use actix_web::{web, HttpResponse};
use chrono::{DateTime, NaiveDate, Utc};
use eo_domain::geocoder::{is_usable_precision, mock_geocoder, GeocodePrecision};
use eo_platform::auth::CurrentUser;
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use eo_platform::plugin::{AppPlugin, RouteRegistrar};
use eo_platform::rbac::OrgAccess;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::FromRow;
use std::collections::HashMap;
use std::sync::Arc;

pub use overview::{landowner_overview, station_overview};

/// 近期資料與清單的筆數上限 —— 參數不是猜測。
pub const RECENT_BILL_LIMIT: i64 = 20;
pub const RECENT_PAYOUT_LIMIT: i64 = 20;
pub const UPCOMING_EVENT_LIMIT: i64 = 20;
pub const LIST_LIMIT: i64 = 200;

pub struct MasterDataPlugin;

crate::energy_plugin!(MasterDataPlugin, "energy_master_data");

impl AppPlugin for MasterDataPlugin {
    fn configure_routes(&self, cfg: &mut RouteRegistrar<'_>, _self_arc: &Arc<dyn AppPlugin>) {
        cfg.get(&path("/stations"), web::get().to(station_search));
        cfg.get(&path("/station-list"), web::get().to(station_list));
        cfg.get(&path("/stations/{stationId}/overview"), web::get().to(station_overview_handler));
        cfg.get(&path("/stations/{stationId}/geocode"), web::get().to(geocode_suggest));
        cfg.post(&path("/stations/{stationId}/geocode"), web::post().to(geocode_apply));
        cfg.get(&path("/landowners"), web::get().to(landowner_list));
        cfg.get(&path("/landowners/{landownerId}"), web::get().to(landowner_get));
        cfg.get(&path("/landowners/{landownerId}/overview"), web::get().to(landowner_overview_handler));
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct StationFull {
    pub id: i64,
    pub name: String,
    pub customer_number: String,
    pub status: String,
    pub address: Option<String>,
    pub latitude: Option<Decimal>,
    pub longitude: Option<Decimal>,
    pub geocode_precision: Option<String>,
    pub geocode_source: Option<String>,
    pub geocoded_at: Option<DateTime<Utc>>,
    pub landowner_id: i64,
    pub organization_id: i64,
}

pub const STATION_COLS: &str = "id, name, customer_number, status, address, latitude, longitude, geocode_precision, geocode_source,
    geocoded_at, landowner_id, organization_id";

/// 座標回字串（不轉 Number，地圖要精度）；去掉 numeric 補的尾零。
fn coord(d: Option<Decimal>) -> Value {
    d.map(|v| json!(v.normalize().to_string())).unwrap_or(Value::Null)
}

pub fn station_view(s: &StationFull, owners: &HashMap<i64, String>) -> Value {
    json!({
        "id": s.id, "name": s.name, "customerNumber": s.customer_number, "status": s.status, "address": s.address,
        "latitude": coord(s.latitude), "longitude": coord(s.longitude),
        // 來源要跟著回：前端才知道這一列是不是 manual（不可被自動推估覆蓋）。
        "geocodePrecision": s.geocode_precision, "geocodeSource": s.geocode_source, "geocodedAt": s.geocoded_at,
        "landownerId": s.landowner_id, "landownerName": owners.get(&s.landowner_id),
    })
}

pub async fn station_in_org(tx: &mut Tx<'_>, access: &OrgAccess, id: i64) -> ApiResult<StationFull> {
    let row: Option<StationFull> =
        sqlx::query_as(&format!("SELECT {STATION_COLS} FROM tbl_energy_station WHERE id = $1 AND deleted_at IS NULL"))
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?;
    let row = row.ok_or_else(|| err(404, "station_not_found"))?;
    // 跨組織一律 403（不回 404 —— 那會讓人用 id 掃出「這個 id 存在」）。
    if row.organization_id != access.org_id() {
        return Err(err(403, "station_not_in_org"));
    }
    Ok(row)
}

async fn owner_names(tx: &mut Tx<'_>, org_id: i64, ids: &[i64]) -> ApiResult<HashMap<i64, String>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, name FROM tbl_energy_landowner WHERE id = ANY($1) AND organization_id = $2 AND deleted_at IS NULL")
            .bind(ids)
            .bind(org_id)
            .fetch_all(&mut **tx)
            .await?;
    Ok(rows.into_iter().collect())
}

fn like(q: &str) -> String {
    format!("%{}%", q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"))
}

fn limit_of(n: Option<i64>) -> i64 {
    n.filter(|n| *n > 0).unwrap_or(LIST_LIMIT).min(LIST_LIMIT)
}

/// 地主清單：案場數、異常案場數、合約數一次撈完自己數，不做 N+1。
pub async fn list_landowners(tx: &mut Tx<'_>, access: &OrgAccess, q: Option<&str>, limit: Option<i64>) -> ApiResult<Vec<Value>> {
    let q = q.map(str::trim).filter(|s| !s.is_empty());
    let rows: Vec<(i64, String, Option<String>, Option<String>, Option<String>, String)> = sqlx::query_as(
        "SELECT id, name, tax_id, contact_name, contact_phone, status FROM tbl_energy_landowner
          WHERE organization_id = $1 AND deleted_at IS NULL
            AND ($2::text IS NULL OR name ILIKE $2 OR tax_id ILIKE $2 OR contact_name ILIKE $2)
          ORDER BY name ASC, id ASC LIMIT $3",
    )
    .bind(access.org_id())
    .bind(q.map(like))
    .bind(limit_of(limit))
    .fetch_all(&mut **tx)
    .await?;
    let ids: Vec<i64> = rows.iter().map(|r| r.0).collect();
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let stations: Vec<(i64, String)> = sqlx::query_as(
        "SELECT landowner_id, status FROM tbl_energy_station WHERE organization_id = $1 AND landowner_id = ANY($2) AND deleted_at IS NULL",
    )
    .bind(access.org_id())
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    let contracts: Vec<(i64,)> = sqlx::query_as(
        "SELECT landowner_id FROM tbl_energy_contract WHERE organization_id = $1 AND landowner_id = ANY($2) AND deleted_at IS NULL",
    )
    .bind(access.org_id())
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, tax_id, contact_name, contact_phone, status)| {
            let mine: Vec<&(i64, String)> = stations.iter().filter(|s| s.0 == id).collect();
            json!({
                "id": id, "name": name, "taxId": tax_id, "contactName": contact_name, "contactPhone": contact_phone, "status": status,
                "stationCount": mine.len(),
                "abnormalStationCount": mine.iter().filter(|s| s.1 == "abnormal").count(),
                "contractCount": contracts.iter().filter(|c| c.0 == id).count(),
            })
        })
        .collect())
}

/// 案場清單（含座標與地主，給清單頁與地圖）。
pub async fn list_stations(tx: &mut Tx<'_>, access: &OrgAccess, q: Option<&str>, landowner_id: Option<i64>, limit: Option<i64>) -> ApiResult<Vec<Value>> {
    let q = q.map(str::trim).filter(|s| !s.is_empty());
    let rows: Vec<StationFull> = sqlx::query_as(&format!(
        "SELECT {STATION_COLS} FROM tbl_energy_station
          WHERE organization_id = $1 AND deleted_at IS NULL AND ($2::bigint IS NULL OR landowner_id = $2)
            AND ($3::text IS NULL OR name ILIKE $3 OR customer_number ILIKE $3)
          ORDER BY name ASC, id ASC LIMIT $4"
    ))
    .bind(access.org_id())
    .bind(landowner_id)
    .bind(q.map(like))
    .bind(limit_of(limit))
    .fetch_all(&mut **tx)
    .await?;
    let owners = owner_names(tx, access.org_id(), &crate::common::uniq(rows.iter().map(|r| r.landowner_id))).await?;
    Ok(rows.iter().map(|r| station_view(r, &owners)).collect())
}

#[derive(Debug, Clone, FromRow)]
struct LandownerRow {
    id: i64,
    name: String,
    tax_id: Option<String>,
    status: String,
    contact_name: Option<String>,
    contact_phone: Option<String>,
    contact_email: Option<String>,
    bank_name: Option<String>,
    bank_branch: Option<String>,
    bank_account: Option<String>,
    bank_account_name: Option<String>,
    note: Option<String>,
    organization_id: i64,
}

fn dec(d: Option<Decimal>) -> Value {
    d.map(|v| json!(v.to_string())).unwrap_or(Value::Null)
}

/// 地主詳情：案場、合約（含期數）、行事曆、近期單據（帶現行應付）。
pub async fn get_landowner(tx: &mut Tx<'_>, access: &OrgAccess, id: i64) -> ApiResult<Value> {
    let row: Option<LandownerRow> = sqlx::query_as(
        "SELECT id, name, tax_id, status, contact_name, contact_phone, contact_email, bank_name, bank_branch, bank_account,
                bank_account_name, note, organization_id
           FROM tbl_energy_landowner WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    let row = row.ok_or_else(|| err(404, "landowner_not_found"))?;
    if row.organization_id != access.org_id() {
        return Err(err(403, "landowner_not_in_org"));
    }
    let org = access.org_id();
    let stations: Vec<StationFull> = sqlx::query_as(&format!(
        "SELECT {STATION_COLS} FROM tbl_energy_station WHERE organization_id = $1 AND landowner_id = $2 AND deleted_at IS NULL ORDER BY name ASC, id ASC"
    ))
    .bind(org)
    .bind(id)
    .fetch_all(&mut **tx)
    .await?;
    let station_ids: Vec<i64> = stations.iter().map(|s| s.id).collect();
    let station_name: HashMap<i64, String> = stations.iter().map(|s| (s.id, s.name.clone())).collect();
    let contracts: Vec<(i64, String, Option<String>, NaiveDate, Option<NaiveDate>, String, Option<i32>)> = sqlx::query_as(
        "SELECT id, contract_no, sign_method, valid_from, valid_to, stage, installment_total FROM tbl_energy_contract
          WHERE organization_id = $1 AND landowner_id = $2 AND deleted_at IS NULL ORDER BY valid_from DESC, id DESC",
    )
    .bind(org)
    .bind(id)
    .fetch_all(&mut **tx)
    .await?;
    let contract_ids: Vec<i64> = contracts.iter().map(|c| c.0).collect();
    // 契約與案場是多對多：顯示第一座涵蓋的案場。
    let coverage: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT contract_id, station_id FROM mn_energy_contract_station
          WHERE contract_id = ANY($1) AND station_id IS NOT NULL AND deleted_at IS NULL ORDER BY id",
    )
    .bind(&contract_ids)
    .fetch_all(&mut **tx)
    .await?;
    let installments: Vec<(i64, i64, i32, NaiveDate, Decimal, String)> = sqlx::query_as(
        "SELECT id, contract_id, seq, due_date, amount, status FROM tbl_energy_installment
          WHERE contract_id = ANY($1) AND organization_id = $2 AND deleted_at IS NULL ORDER BY seq ASC, id ASC",
    )
    .bind(&contract_ids)
    .bind(org)
    .fetch_all(&mut **tx)
    .await?;
    let events: Vec<(i64, NaiveDate, String, String, String, Option<i64>)> = sqlx::query_as(
        "SELECT id, event_date, kind, title, status, station_id FROM tbl_energy_calendar_event
          WHERE organization_id = $1 AND landowner_id = $2 AND deleted_at IS NULL ORDER BY event_date ASC, id ASC LIMIT $3",
    )
    .bind(org)
    .bind(id)
    .bind(UPCOMING_EVENT_LIMIT)
    .fetch_all(&mut **tx)
    .await?;
    let bills: Vec<(i64, Option<i64>, Option<NaiveDate>, Option<NaiveDate>, Option<Decimal>, String)> = sqlx::query_as(
        "SELECT id, station_id, billing_period_start, billing_period_end, total_amount, review_status FROM tbl_energy_bill
          WHERE organization_id = $1 AND station_id = ANY($2) AND deleted_at IS NULL ORDER BY id DESC LIMIT $3",
    )
    .bind(org)
    .bind(&station_ids)
    .bind(RECENT_BILL_LIMIT)
    .fetch_all(&mut **tx)
    .await?;
    let bill_ids: Vec<i64> = bills.iter().map(|b| b.0).collect();
    // 只取現行的應付 —— 被取代的版本屬於稽核歷程，不該混進「這個地主現在該拿多少」。
    let payouts: Vec<(i64, Decimal, String)> = sqlx::query_as(
        "SELECT bill_id, amount, strategy_type FROM tbl_energy_payout_record
          WHERE organization_id = $1 AND bill_id = ANY($2) AND status = 'active' AND deleted_at IS NULL ORDER BY id DESC LIMIT $3",
    )
    .bind(org)
    .bind(&bill_ids)
    .bind(RECENT_PAYOUT_LIMIT)
    .fetch_all(&mut **tx)
    .await?;
    let owners: HashMap<i64, String> = [(row.id, row.name.clone())].into_iter().collect();
    Ok(json!({
        "id": row.id, "name": row.name, "taxId": row.tax_id, "status": row.status,
        "contactName": row.contact_name, "contactPhone": row.contact_phone, "contactEmail": row.contact_email,
        "bankName": row.bank_name, "bankBranch": row.bank_branch, "bankAccount": row.bank_account,
        "bankAccountName": row.bank_account_name, "note": row.note,
        "stations": stations.iter().map(|s| station_view(s, &owners)).collect::<Vec<_>>(),
        "contracts": contracts.iter().map(|c| {
            let mine: Vec<&(i64, i64, i32, NaiveDate, Decimal, String)> = installments.iter().filter(|i| i.1 == c.0).collect();
            let sid = coverage.iter().find(|x| x.0 == c.0).map(|x| x.1);
            json!({
                "id": c.0, "contractNo": c.1, "stationId": sid, "stationName": sid.and_then(|s| station_name.get(&s)),
                "signMethod": c.2, "validFrom": day(c.3), "validTo": day_opt(c.4), "stage": c.5,
                "installmentTotal": c.6, "installmentPaid": mine.iter().filter(|i| i.5 == "paid").count(),
                "installments": mine.iter().map(|i| json!({
                    "id": i.0, "seq": i.2, "dueDate": day(i.3), "amount": i.4.to_string(), "status": i.5,
                })).collect::<Vec<_>>(),
            })
        }).collect::<Vec<_>>(),
        "calendarEvents": events.iter().map(|e| json!({
            "id": e.0, "eventDate": day(e.1), "kind": e.2, "title": e.3, "status": e.4, "stationId": e.5,
        })).collect::<Vec<_>>(),
        "recentBills": bills.iter().map(|b| {
            // 沒有應付就是還沒算（不填 0，那看起來像「不用付」）。
            let p = payouts.iter().find(|p| p.0 == b.0);
            json!({
                "id": b.0, "stationId": b.1, "stationName": b.1.and_then(|s| station_name.get(&s)),
                "billingPeriodStart": day_opt(b.2), "billingPeriodEnd": day_opt(b.3), "totalAmount": dec(b.4),
                "reviewStatus": b.5, "payoutAmount": p.map(|p| p.1.to_string()), "payoutStrategyType": p.map(|p| p.2.clone()),
            })
        }).collect::<Vec<_>>(),
    }))
}

/// 推估座標 —— 不寫入，只回建議值讓人確認（靜默的錯座標比空白糟）。
pub async fn suggest_coordinates(tx: &mut Tx<'_>, access: &OrgAccess, station_id: i64) -> ApiResult<Value> {
    let st = station_in_org(tx, access, station_id).await?;
    let Some(address) = st.address.clone().filter(|a| !a.is_empty()) else {
        return Ok(json!({ "stationId": station_id, "address": null, "result": null, "usable": false, "reason": "no_address" }));
    };
    // 查不到就是查不到 —— 不回退成行政區中心點。
    let Some(result) = mock_geocoder(&address) else {
        return Ok(json!({ "stationId": station_id, "address": address, "result": null, "usable": false, "reason": "not_found" }));
    };
    let usable = is_usable_precision(result.precision);
    let mut out = json!({ "stationId": station_id, "address": address, "result": result, "usable": usable });
    if !usable {
        out["reason"] = json!("too_coarse");
    }
    Ok(out)
}

fn precision_str(p: GeocodePrecision) -> String {
    serde_json::to_value(p).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| "unknown".into())
}

/// 採用推估結果。兩道閘：手動填的座標永遠優先；精度不夠不寫。稽核與寫入同一交易。
pub async fn apply_geocode(tx: &mut Tx<'_>, ctx: &Ctx, station_id: i64) -> ApiResult<Value> {
    let st = station_in_org(tx, &ctx.access, station_id).await?;
    if st.geocode_source.as_deref() == Some("manual") {
        return Err(err(409, "manual_coordinates_protected"));
    }
    let address = st.address.clone().filter(|a| !a.is_empty()).ok_or_else(|| err(400, "station_has_no_address"))?;
    let result = mock_geocoder(&address).ok_or_else(|| err(404, "geocode_not_found"))?;
    if !is_usable_precision(result.precision) {
        return Err(err(422, "geocode_too_coarse"));
    }
    let lat: Decimal = result.latitude.to_string().parse().map_err(|_| err(500, "invalid_coordinate"))?;
    let lng: Decimal = result.longitude.to_string().parse().map_err(|_| err(500, "invalid_coordinate"))?;
    let precision = precision_str(result.precision);
    sqlx::query(
        "UPDATE tbl_energy_station SET latitude = $2, longitude = $3, geocode_precision = $4, geocode_source = $5, geocoded_at = $6
          WHERE id = $1 AND organization_id = $7 AND deleted_at IS NULL",
    )
    .bind(station_id)
    .bind(lat)
    .bind(lng)
    .bind(&precision)
    .bind(&result.source)
    .bind(ctx.now())
    .bind(ctx.org_id())
    .execute(&mut **tx)
    .await?;
    let lat_s = lat.normalize().to_string();
    let lng_s = lng.normalize().to_string();
    let mut payload = json!({
        "changes": {
            "latitude": { "from": coord(st.latitude), "to": lat_s },
            "longitude": { "from": coord(st.longitude), "to": lng_s },
        },
        "address": address, "precision": precision, "source": result.source,
    });
    if let Some(m) = &result.matched_address {
        payload["matchedAddress"] = json!(m);
    }
    audit::record(
        tx,
        ctx,
        AuditInput::new(
            "station.geocoded",
            station_id,
            format!("由地址推估座標：「{address}」→ {}, {}（{precision} / {}）", result.latitude, result.longitude, result.source),
        )
        .label(st.name.clone())
        .payload(payload),
    )
    .await?;
    Ok(json!({ "id": station_id, "latitude": lat_s, "longitude": lng_s, "geocodePrecision": precision, "geocodeSource": result.source }))
}

// ── handlers ──

type Core = web::Data<CoreRef>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListQuery {
    q: Option<String>,
    landowner_id: Option<i64>,
    limit: Option<i64>,
}

/// 指認對話框的輕量搜尋（電號或名稱）。
async fn station_search(core: Core, user: CurrentUser, org: web::Path<i64>, q: web::Query<ListQuery>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::FLOW_VIEW).await?;
    let term = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let rows: Vec<(i64, String, String, String)> = sqlx::query_as(
        "SELECT id, name, customer_number, status FROM tbl_energy_station
          WHERE organization_id = $1 AND deleted_at IS NULL AND ($2::text IS NULL OR customer_number ILIKE $2 OR name ILIKE $2)
          ORDER BY name ASC, id ASC LIMIT 50",
    )
    .bind(access.org_id())
    .bind(term.map(like))
    .fetch_all(&core.pool)
    .await?;
    let items: Vec<Value> =
        rows.into_iter().map(|(id, name, cn, status)| json!({ "id": id, "name": name, "customerNumber": cn, "status": status })).collect();
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

async fn station_list(core: Core, user: CurrentUser, org: web::Path<i64>, q: web::Query<ListQuery>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::FLOW_VIEW).await?;
    let mut tx = core.pool.begin().await?;
    let items = list_stations(&mut tx, &access, q.q.as_deref(), q.landowner_id, q.limit).await?;
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

async fn landowner_list(core: Core, user: CurrentUser, org: web::Path<i64>, q: web::Query<ListQuery>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::FLOW_VIEW).await?;
    let mut tx = core.pool.begin().await?;
    let items = list_landowners(&mut tx, &access, q.q.as_deref(), q.limit).await?;
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

async fn landowner_get(core: Core, user: CurrentUser, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::FLOW_VIEW).await?;
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(get_landowner(&mut tx, &access, id).await?))
}

async fn landowner_overview_handler(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::FLOW_VIEW).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(landowner_overview(&mut tx, &ctx, id, crate::workflow::graph::DEFAULT_STUCK_DAYS).await?))
}

async fn station_overview_handler(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::FLOW_VIEW).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(station_overview(&mut tx, &ctx, id).await?))
}

async fn geocode_suggest(core: Core, user: CurrentUser, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::FLOW_VIEW).await?;
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(suggest_coordinates(&mut tx, &access, id).await?))
}

/// 會改資料 → 走 act 的閘。
async fn geocode_apply(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::FLOW_ACT).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    let out = apply_geocode(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(out))
}
