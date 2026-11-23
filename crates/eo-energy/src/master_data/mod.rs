//! 主檔（地主／案場）的讀取層，以及地址 → 座標（推估不寫入、採用才寫入）。
//! 詳情一次組好整包（「一個地主的所有錢與時間」），不讓前端打七支自己拼。
//!
//! 卡 A（20261118-0427）起，本檔也管案場／受款對象的寫入端點與專案編號算法；
//! 電號（一對多）、地主分配各自獨立成 `meters`／`allocations` 子模組。

mod allocations;
mod contracts;
mod rent_cycle;
mod meters;
mod overview;
mod station_import;

use crate::audit::{self, AuditInput};
use crate::common::{day, day_opt, err, err_d, parse_day, Ctx, RequestMeta, Tx};
use crate::{gates, path};
use actix_web::{web, HttpResponse};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
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

pub use allocations::{allocation_history, create_allocation, end_allocation, list_allocations, update_allocation};
pub use contracts::contract_in_org;
pub use meters::{create_meter, list_meters, update_meter};
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
        cfg.post(&path("/stations"), web::post().to(create_station_handler));
        cfg.get(&path("/station-list"), web::get().to(station_list));
        cfg.patch(&path("/stations/{stationId}"), web::patch().to(update_station_handler));
        cfg.get(&path("/stations/{stationId}/overview"), web::get().to(station_overview_handler));
        cfg.get(&path("/stations/{stationId}/geocode"), web::get().to(geocode_suggest));
        cfg.post(&path("/stations/{stationId}/geocode"), web::post().to(geocode_apply));
        cfg.get(&path("/stations/{stationId}/meters"), web::get().to(meters::list_handler));
        cfg.post(&path("/stations/{stationId}/meters"), web::post().to(meters::create_handler));
        cfg.patch(&path("/meters/{meterId}"), web::patch().to(meters::update_handler));
        cfg.get(&path("/stations/{stationId}/allocations"), web::get().to(allocations::list_handler));
        cfg.post(&path("/stations/{stationId}/allocations"), web::post().to(allocations::create_handler));
        cfg.patch(&path("/allocations/{allocationId}"), web::patch().to(allocations::update_handler));
        cfg.get(&path("/stations/{stationId}/allocation-history"), web::get().to(allocations::history_handler));
        cfg.get(&path("/landowners"), web::get().to(landowner_list));
        cfg.post(&path("/landowners"), web::post().to(create_landowner_handler));
        cfg.patch(&path("/landowners/{landownerId}"), web::patch().to(update_landowner_handler));
        cfg.get(&path("/landowners/{landownerId}"), web::get().to(landowner_get));
        cfg.get(&path("/landowners/{landownerId}/overview"), web::get().to(landowner_overview_handler));
        cfg.get(&path("/contracts"), web::get().to(contracts::list_handler));
        cfg.post(&path("/contracts"), web::post().to(contracts::create_handler));
        cfg.get(&path("/contracts/{contractId}"), web::get().to(contracts::get_handler));
        cfg.patch(&path("/contracts/{contractId}"), web::patch().to(contracts::update_handler));
        cfg.get(&path("/contracts/{contractId}/rent-cycle"), web::get().to(rent_cycle::get));
        cfg.post(&path("/contracts/{contractId}/rent-cycle"), web::post().to(rent_cycle::save));
        cfg.get(&path("/contracts/{contractId}/rent-cycle/bills"), web::get().to(rent_cycle::bills));
        cfg.post(&path("/contracts/{contractId}/rent-cycle/preview"), web::post().to(rent_cycle::preview));
        cfg.post(&path("/imports/stations"), web::post().to(station_import::import_handler));
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
    // ── 卡 A 擴欄 ──
    pub project_code: Option<String>,
    pub short_name: Option<String>,
    pub region: Option<String>,
    pub category: Option<String>,
    pub site_type: Option<String>,
    pub spv_payee_id: Option<i64>,
    pub service_district: Option<String>,
    pub capacity_kw: Option<Decimal>,
    pub grid_date: Option<NaiveDate>,
    pub serial_no: Option<i32>,
    pub enabled: bool,
}

pub const STATION_COLS: &str = "id, name, customer_number, status, address, latitude, longitude, geocode_precision, geocode_source,
    geocoded_at, landowner_id, organization_id, project_code, short_name, region, category, site_type, spv_payee_id,
    service_district, capacity_kw, grid_date, serial_no, enabled";

/// 座標回字串（不轉 Number，地圖要精度）；去掉 numeric 補的尾零。
fn coord(d: Option<Decimal>) -> Value {
    d.map(|v| json!(v.normalize().to_string())).unwrap_or(Value::Null)
}

/// 型別 → 專案編號的型別碼（資產管理清單 C 欄公式）。
fn site_type_code(site_type: &str) -> Option<char> {
    match site_type {
        "fishery" => Some('B'),
        "water" => Some('W'),
        "rooftop" => Some('R'),
        "other" => Some('L'),
        _ => None,
    }
}

/// `(併網年−1911) & 型別碼 & SPV 代號 & 序號補 4 碼`。四樣缺一就回 None（缺哪樣由 `project_code_missing` 另外答）。
pub fn project_code(grid_date: Option<NaiveDate>, site_type: Option<&str>, spv_code: Option<&str>, serial_no: Option<i32>) -> Option<String> {
    let year = grid_date?.year() - 1911;
    let type_c = site_type.and_then(site_type_code)?;
    let spv = spv_code.filter(|s| !s.is_empty())?;
    let serial = serial_no?;
    if year <= 0 || serial < 0 {
        return None;
    }
    Some(format!("{year}{type_c}{spv}{serial:04}"))
}

/// 算不出專案編號時，缺哪幾樣（畫面顯示「缺：…」）。
pub fn project_code_missing(grid_date: Option<NaiveDate>, site_type: Option<&str>, spv_code: Option<&str>, serial_no: Option<i32>) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if grid_date.is_none() {
        missing.push("gridDate");
    }
    if site_type.and_then(site_type_code).is_none() {
        missing.push("siteType");
    }
    if spv_code.filter(|s| !s.is_empty()).is_none() {
        missing.push("spvCode");
    }
    if serial_no.is_none() {
        missing.push("serialNo");
    }
    missing
}

pub fn station_view(s: &StationFull, owners: &HashMap<i64, String>) -> Value {
    json!({
        "id": s.id, "name": s.name, "customerNumber": s.customer_number, "status": s.status, "address": s.address,
        "latitude": coord(s.latitude), "longitude": coord(s.longitude),
        // 來源要跟著回：前端才知道這一列是不是 manual（不可被自動推估覆蓋）。
        "geocodePrecision": s.geocode_precision, "geocodeSource": s.geocode_source, "geocodedAt": s.geocoded_at,
        "landownerId": s.landowner_id, "landownerName": owners.get(&s.landowner_id),
        "projectCode": s.project_code, "shortName": s.short_name, "region": s.region, "category": s.category,
        "siteType": s.site_type, "spvPayeeId": s.spv_payee_id, "serviceDistrict": s.service_district,
        "capacityKw": s.capacity_kw.map(|v| v.normalize().to_string()),
        "gridDate": day_opt(s.grid_date), "serialNo": s.serial_no, "enabled": s.enabled,
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

#[derive(Debug, Clone, FromRow)]
struct LandownerListRow {
    id: i64,
    name: String,
    tax_id: Option<String>,
    contact_name: Option<String>,
    contact_phone: Option<String>,
    status: String,
    party_type: String,
    code: Option<String>,
    address: Option<String>,
}

/// 地主（受款對象）清單：案場數、異常案場數、合約數一次撈完自己數，不做 N+1。
/// `party_type` 給了就只回那個類型（子公司／台電區處／機關／公司／個人地主）。
pub async fn list_landowners(tx: &mut Tx<'_>, access: &OrgAccess, q: Option<&str>, party_type: Option<&str>, limit: Option<i64>) -> ApiResult<Vec<Value>> {
    let q = q.map(str::trim).filter(|s| !s.is_empty());
    let party_type = party_type.map(str::trim).filter(|s| !s.is_empty());
    let rows: Vec<LandownerListRow> = sqlx::query_as(
        "SELECT id, name, tax_id, contact_name, contact_phone, status, party_type, code, address FROM tbl_energy_landowner
          WHERE organization_id = $1 AND deleted_at IS NULL
            AND ($2::text IS NULL OR name ILIKE $2 OR tax_id ILIKE $2 OR contact_name ILIKE $2)
            AND ($4::text IS NULL OR party_type = $4)
          ORDER BY name ASC, id ASC LIMIT $3",
    )
    .bind(access.org_id())
    .bind(q.map(like))
    .bind(limit_of(limit))
    .bind(party_type)
    .fetch_all(&mut **tx)
    .await?;
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
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
        .map(|r| {
            let mine: Vec<&(i64, String)> = stations.iter().filter(|s| s.0 == r.id).collect();
            json!({
                "id": r.id, "name": r.name, "taxId": r.tax_id, "contactName": r.contact_name, "contactPhone": r.contact_phone,
                "status": r.status, "partyType": r.party_type, "code": r.code, "address": r.address,
                "stationCount": mine.len(),
                "abnormalStationCount": mine.iter().filter(|s| s.1 == "abnormal").count(),
                "contractCount": contracts.iter().filter(|c| c.0 == r.id).count(),
            })
        })
        .collect())
}

/// 案場清單（含座標與地主，給清單頁與地圖）。`site_type`／`enabled` 給了就篩。
#[allow(clippy::too_many_arguments)]
pub async fn list_stations(
    tx: &mut Tx<'_>,
    access: &OrgAccess,
    q: Option<&str>,
    landowner_id: Option<i64>,
    site_type: Option<&str>,
    enabled: Option<bool>,
    limit: Option<i64>,
) -> ApiResult<Vec<Value>> {
    let q = q.map(str::trim).filter(|s| !s.is_empty());
    let site_type = site_type.map(str::trim).filter(|s| !s.is_empty());
    let rows: Vec<StationFull> = sqlx::query_as(&format!(
        "SELECT {STATION_COLS} FROM tbl_energy_station
          WHERE organization_id = $1 AND deleted_at IS NULL AND ($2::bigint IS NULL OR landowner_id = $2)
            AND ($3::text IS NULL OR name ILIKE $3 OR customer_number ILIKE $3)
            AND ($5::text IS NULL OR site_type = $5) AND ($6::boolean IS NULL OR enabled = $6)
          ORDER BY name ASC, id ASC LIMIT $4"
    ))
    .bind(access.org_id())
    .bind(landowner_id)
    .bind(q.map(like))
    .bind(limit_of(limit))
    .bind(site_type)
    .bind(enabled)
    .fetch_all(&mut **tx)
    .await?;
    let owners = owner_names(tx, access.org_id(), &crate::common::uniq(rows.iter().map(|r| r.landowner_id))).await?;
    let spv_ids: Vec<i64> = crate::common::uniq(rows.iter().filter_map(|r| r.spv_payee_id));
    let spv_codes: Vec<(i64, Option<String>)> = if spv_ids.is_empty() {
        vec![]
    } else {
        sqlx::query_as("SELECT id, code FROM tbl_energy_landowner WHERE id = ANY($1) AND organization_id = $2 AND deleted_at IS NULL")
            .bind(&spv_ids)
            .bind(access.org_id())
            .fetch_all(&mut **tx)
            .await?
    };
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let meter_counts: Vec<(i64, i64)> = if ids.is_empty() {
        vec![]
    } else {
        sqlx::query_as(
            "SELECT station_id, COUNT(*) FROM tbl_energy_station_meter WHERE organization_id = $1 AND station_id = ANY($2) AND deleted_at IS NULL GROUP BY station_id",
        )
        .bind(access.org_id())
        .bind(&ids)
        .fetch_all(&mut **tx)
        .await?
    };
    Ok(rows
        .iter()
        .map(|r| {
            let mut v = station_view(r, &owners);
            v["meterCount"] = json!(meter_counts.iter().find(|m| m.0 == r.id).map(|m| m.1).unwrap_or(0));
            let spv_code = r.spv_payee_id.and_then(|id| spv_codes.iter().find(|c| c.0 == id).and_then(|c| c.1.clone()));
            v["projectCodeMissing"] = json!(project_code_missing(r.grid_date, r.site_type.as_deref(), spv_code.as_deref(), r.serial_no));
            v
        })
        .collect())
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
    let contracts: Vec<(i64, String, Option<String>, NaiveDate, Option<NaiveDate>, String, Option<i32>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT id, contract_no, sign_method, valid_from, valid_to, stage, installment_total, contract_type, payment_frequency
           FROM tbl_energy_contract
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
                "installmentTotal": c.6, "contractType": c.7, "paymentFrequency": c.8,
                "installmentPaid": mine.iter().filter(|i| i.5 == "paid").count(),
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

// ── 案場／受款對象寫入（卡 A） ──

fn vstr(b: &Value, k: &str) -> Option<String> {
    b.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from)
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

fn vi64(b: &Value, k: &str) -> Option<i64> {
    b.get(k).and_then(Value::as_i64)
}

fn vi32(b: &Value, k: &str) -> Option<i32> {
    b.get(k).and_then(Value::as_i64).map(|n| n as i32)
}

fn vbool(b: &Value, k: &str, default: bool) -> bool {
    b.get(k).and_then(Value::as_bool).unwrap_or(default)
}

fn vday(b: &Value, k: &str) -> ApiResult<Option<NaiveDate>> {
    match vstr(b, k) {
        None => Ok(None),
        Some(s) => parse_day(&s).map(Some).ok_or_else(|| err_d(400, "invalid_date", json!({ "field": k }))),
    }
}

/// 驗 spv 受款對象存在、同組織、類型是 `subsidiary`；回它的 `code`（可能還沒填）。
async fn spv_code_of(tx: &mut Tx<'_>, org_id: i64, spv_payee_id: i64) -> ApiResult<Option<String>> {
    let row: Option<(String, Option<String>, i64)> =
        sqlx::query_as("SELECT party_type, code, organization_id FROM tbl_energy_landowner WHERE id = $1 AND deleted_at IS NULL")
            .bind(spv_payee_id)
            .fetch_optional(&mut **tx)
            .await?;
    let (party_type, code, owner_org) = row.ok_or_else(|| err(404, "spv_payee_not_found"))?;
    if owner_org != org_id {
        return Err(err(403, "spv_payee_not_in_org"));
    }
    if party_type != "subsidiary" {
        return Err(err_d(400, "spv_payee_must_be_subsidiary", json!({ "partyType": party_type })));
    }
    Ok(code)
}

/// 新增案場：同一交易建主電號（meter）與主要受款對象 100% 分配。
pub async fn create_station(tx: &mut Tx<'_>, ctx: &Ctx, b: &Value) -> ApiResult<i64> {
    let name = vstr(b, "name").ok_or_else(|| err(400, "name_required"))?;
    let customer_number = vstr(b, "customerNumber").ok_or_else(|| err(400, "customer_number_required"))?;
    let landowner_id = vi64(b, "landownerId").ok_or_else(|| err(400, "landowner_id_required"))?;
    let status = vstr(b, "status").unwrap_or_else(|| "operating".into());
    let site_type = vstr(b, "siteType");
    let spv_payee_id = vi64(b, "spvPayeeId");
    let capacity_kw = vdecimal(b, "capacityKw")?;
    let grid_date = vday(b, "gridDate")?;
    let serial_no = vi32(b, "serialNo");
    let enabled = vbool(b, "enabled", true);
    let org = ctx.org_id();

    let owner: Option<i64> =
        sqlx::query_scalar("SELECT id FROM tbl_energy_landowner WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL")
            .bind(landowner_id)
            .bind(org)
            .fetch_optional(&mut **tx)
            .await?;
    owner.ok_or_else(|| err(400, "landowner_not_found"))?;
    let spv_code = match spv_payee_id {
        Some(id) => spv_code_of(tx, org, id).await?,
        None => None,
    };
    let project_code = project_code(grid_date, site_type.as_deref(), spv_code.as_deref(), serial_no);

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO tbl_energy_station
           (organization_id, landowner_id, customer_number, name, status, address, short_name, region, category,
            site_type, spv_payee_id, service_district, capacity_kw, grid_date, serial_no, enabled, project_code)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17) RETURNING id",
    )
    .bind(org)
    .bind(landowner_id)
    .bind(&customer_number)
    .bind(&name)
    .bind(&status)
    .bind(vstr(b, "address"))
    .bind(vstr(b, "shortName"))
    .bind(vstr(b, "region"))
    .bind(vstr(b, "category"))
    .bind(&site_type)
    .bind(spv_payee_id)
    .bind(vstr(b, "serviceDistrict"))
    .bind(capacity_kw)
    .bind(grid_date)
    .bind(serial_no)
    .bind(enabled)
    .bind(&project_code)
    .fetch_one(&mut **tx)
    .await?;

    // 主電號（躉購）—— 收件比對靠這一列，與主檔的 customer_number 永遠同步建立。
    sqlx::query("INSERT INTO tbl_energy_station_meter (organization_id, station_id, customer_number, usage) VALUES ($1, $2, $3, 'wholesale')")
        .bind(org)
        .bind(id)
        .bind(&customer_number)
        .execute(&mut **tx)
        .await?;
    // 主要受款對象 100% 分配 —— 只存，不算錢。
    sqlx::query("INSERT INTO mn_energy_landowner_station (organization_id, landowner_id, station_id, method, ratio) VALUES ($1, $2, $3, 'ratio', 100)")
        .bind(org)
        .bind(landowner_id)
        .bind(id)
        .execute(&mut **tx)
        .await?;

    audit::record(
        tx,
        ctx,
        AuditInput::new("station.geocoded", id, format!("建立案場「{name}」（{customer_number}）"))
            .label(name.clone())
            .payload(json!({ "changes": {} })),
    )
    .await
    .ok(); // 沿用既有已註冊的 type 當輕量留痕；稽核寫不進去不擋建案場（best-effort 不中斷主流程）。
    Ok(id)
}

/// 編輯案場：表單整包送上來（缺的欄位視同清空），重新算專案編號。
pub async fn update_station(tx: &mut Tx<'_>, ctx: &Ctx, station_id: i64, b: &Value) -> ApiResult<()> {
    let st = station_in_org(tx, &ctx.access, station_id).await?;
    let org = ctx.org_id();
    let name = vstr(b, "name").unwrap_or(st.name.clone());
    let status = vstr(b, "status").unwrap_or(st.status.clone());
    let site_type = vstr(b, "siteType");
    let spv_payee_id = vi64(b, "spvPayeeId");
    let capacity_kw = vdecimal(b, "capacityKw")?;
    let grid_date = vday(b, "gridDate")?;
    let serial_no = vi32(b, "serialNo");
    let enabled = vbool(b, "enabled", st.enabled);
    let landowner_id = vi64(b, "landownerId").unwrap_or(st.landowner_id);

    let spv_code = match spv_payee_id {
        Some(id) => spv_code_of(tx, org, id).await?,
        None => None,
    };
    let project_code = project_code(grid_date, site_type.as_deref(), spv_code.as_deref(), serial_no);

    sqlx::query(
        "UPDATE tbl_energy_station SET
           name = $2, landowner_id = $3, status = $4, address = $5, short_name = $6, region = $7, category = $8,
           site_type = $9, spv_payee_id = $10, service_district = $11, capacity_kw = $12, grid_date = $13,
           serial_no = $14, enabled = $15, project_code = $16
         WHERE id = $1 AND organization_id = $17 AND deleted_at IS NULL",
    )
    .bind(station_id)
    .bind(&name)
    .bind(landowner_id)
    .bind(&status)
    .bind(vstr(b, "address"))
    .bind(vstr(b, "shortName"))
    .bind(vstr(b, "region"))
    .bind(vstr(b, "category"))
    .bind(&site_type)
    .bind(spv_payee_id)
    .bind(vstr(b, "serviceDistrict"))
    .bind(capacity_kw)
    .bind(grid_date)
    .bind(serial_no)
    .bind(enabled)
    .bind(&project_code)
    .bind(org)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// 新增受款對象。`partyType` 預設 `individual`；`code` 只有子公司才有意義，但不強制。
pub async fn create_landowner(tx: &mut Tx<'_>, ctx: &Ctx, b: &Value) -> ApiResult<i64> {
    let name = vstr(b, "name").ok_or_else(|| err(400, "name_required"))?;
    let party_type = vstr(b, "partyType").unwrap_or_else(|| "individual".into());
    Ok(sqlx::query_scalar(
        "INSERT INTO tbl_energy_landowner
           (organization_id, name, tax_id, contact_name, contact_phone, contact_email, bank_name, bank_branch,
            bank_account, bank_account_name, note, party_type, code, address, status)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, 'active') RETURNING id",
    )
    .bind(ctx.org_id())
    .bind(&name)
    .bind(vstr(b, "taxId"))
    .bind(vstr(b, "contactName"))
    .bind(vstr(b, "contactPhone"))
    .bind(vstr(b, "contactEmail"))
    .bind(vstr(b, "bankName"))
    .bind(vstr(b, "bankBranch"))
    .bind(vstr(b, "bankAccount"))
    .bind(vstr(b, "bankAccountName"))
    .bind(vstr(b, "note"))
    .bind(&party_type)
    .bind(vstr(b, "code"))
    .bind(vstr(b, "address"))
    .fetch_one(&mut **tx)
    .await?)
}

/// 編輯受款對象（表單整包送上來）。
pub async fn update_landowner(tx: &mut Tx<'_>, ctx: &Ctx, id: i64, b: &Value) -> ApiResult<()> {
    let owner_org: Option<i64> =
        sqlx::query_scalar("SELECT organization_id FROM tbl_energy_landowner WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?;
    let owner_org = owner_org.ok_or_else(|| err(404, "landowner_not_found"))?;
    if owner_org != ctx.org_id() {
        return Err(err(403, "landowner_not_in_org"));
    }
    let name = vstr(b, "name").ok_or_else(|| err(400, "name_required"))?;
    let party_type = vstr(b, "partyType").unwrap_or_else(|| "individual".into());
    sqlx::query(
        "UPDATE tbl_energy_landowner SET
           name = $2, tax_id = $3, contact_name = $4, contact_phone = $5, contact_email = $6, bank_name = $7,
           bank_branch = $8, bank_account = $9, bank_account_name = $10, note = $11, party_type = $12, code = $13, address = $14
         WHERE id = $1 AND organization_id = $15 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(&name)
    .bind(vstr(b, "taxId"))
    .bind(vstr(b, "contactName"))
    .bind(vstr(b, "contactPhone"))
    .bind(vstr(b, "contactEmail"))
    .bind(vstr(b, "bankName"))
    .bind(vstr(b, "bankBranch"))
    .bind(vstr(b, "bankAccount"))
    .bind(vstr(b, "bankAccountName"))
    .bind(vstr(b, "note"))
    .bind(&party_type)
    .bind(vstr(b, "code"))
    .bind(vstr(b, "address"))
    .bind(ctx.org_id())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

// ── handlers ──

type Core = web::Data<CoreRef>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListQuery {
    q: Option<String>,
    landowner_id: Option<i64>,
    limit: Option<i64>,
    site_type: Option<String>,
    enabled: Option<bool>,
    party_type: Option<String>,
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
    let items = list_stations(&mut tx, &access, q.q.as_deref(), q.landowner_id, q.site_type.as_deref(), q.enabled, q.limit).await?;
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

async fn create_station_handler(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::MASTER_MANAGE).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    let id = create_station(&mut tx, &ctx, &body).await?;
    let out = station_overview(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(out))
}

async fn update_station_handler(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let (org, station_id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::MASTER_MANAGE).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    update_station(&mut tx, &ctx, station_id, &body).await?;
    let out = station_overview(&mut tx, &ctx, station_id).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(out))
}

async fn landowner_list(core: Core, user: CurrentUser, org: web::Path<i64>, q: web::Query<ListQuery>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::FLOW_VIEW).await?;
    let mut tx = core.pool.begin().await?;
    let items = list_landowners(&mut tx, &access, q.q.as_deref(), q.party_type.as_deref(), q.limit).await?;
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

async fn create_landowner_handler(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::MASTER_MANAGE).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    let id = create_landowner(&mut tx, &ctx, &body).await?;
    tx.commit().await?;
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(get_landowner(&mut tx, &ctx.access, id).await?))
}

async fn update_landowner_handler(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::MASTER_MANAGE).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    update_landowner(&mut tx, &ctx, id, &body).await?;
    tx.commit().await?;
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(get_landowner(&mut tx, &ctx.access, id).await?))
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
