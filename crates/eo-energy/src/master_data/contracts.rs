//! 合約 CRUD＋清單（卡 20261118-0810-a2 S1）。`contractNo`／`landownerId`／`validFrom` 必填
//! （表上 NOT NULL）；`stationIds` 多對多靠 `mn_energy_contract_station`（軟刪＋補列，不硬刪，
//! 照卡 A `allocations.rs` 的「只增不刪」精神）。編輯＝整包送上來（照 `update_landowner`）。

use super::{limit_of, like, vday, vi32, vi64, vstr};
use crate::audit::{self, AuditInput};
use crate::common::{day, day_opt, err, err_d, uniq, Ctx, RequestMeta, Tx};
use crate::gates;
use actix_web::{web, HttpResponse};
use chrono::NaiveDate;
use eo_platform::auth::CurrentUser;
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use eo_platform::rbac::OrgAccess;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::FromRow;

const PAYMENT_FREQUENCIES: &[&str] = &["monthly", "bimonthly", "quarterly", "semiannual", "annual", "other"];
const BILL_CODES: &[&str] = &["E-A", "E-B", "E-C", "E-D"];
const RENT_CALC_CODES: &[&str] = &["R-0", "R-A", "R-B", "R-C", "R-D"];
const RENT_PAY_CODES: &[&str] = &["RP-1", "RP-2", "RP-3", "RP-4"];

#[derive(Debug, Clone, FromRow)]
pub struct ContractRow {
    pub id: i64,
    pub organization_id: i64,
    /// 「第一位地主」—— 給舊碼讀（提醒、租金週期稽核）；完整名單在 `mn_energy_contract_landowner`。
    pub landowner_id: Option<i64>,
    pub contract_no: String,
    pub sign_method: Option<String>,
    pub valid_from: NaiveDate,
    pub valid_to: Option<NaiveDate>,
    pub stage: String,
    pub installment_total: Option<i32>,
    pub note: Option<String>,
    pub contract_type: Option<String>,
    pub signed_on: Option<NaiveDate>,
    pub renewal_remind_on: Option<NaiveDate>,
    pub terms_version: Option<String>,
    pub payment_frequency: Option<String>,
    pub payment_terms: Option<String>,
    pub bill_codes: Vec<String>,
    pub rent_calc_code: Option<String>,
    pub rent_pay_code: Option<String>,
}

pub const CONTRACT_COLS: &str = "id, organization_id, landowner_id, contract_no, sign_method, valid_from, valid_to, stage,
    installment_total, note, contract_type, signed_on, renewal_remind_on, terms_version, payment_frequency, payment_terms,
    bill_codes, rent_calc_code, rent_pay_code";

/// 本組織的某份合約（查無 404、別家 403）。
pub async fn contract_in_org(tx: &mut Tx<'_>, org_id: i64, id: i64) -> ApiResult<ContractRow> {
    let row: Option<ContractRow> =
        sqlx::query_as(&format!("SELECT {CONTRACT_COLS} FROM tbl_energy_contract WHERE id = $1 AND deleted_at IS NULL"))
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?;
    let row = row.ok_or_else(|| err(404, "contract_not_found"))?;
    if row.organization_id != org_id {
        return Err(err(403, "contract_not_in_org"));
    }
    Ok(row)
}

fn vi64_array(b: &Value, k: &str) -> Vec<i64> {
    b.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default()
}

/// 把送上來的 `stationIds` 跟目前有效列對齊：不在新清單的標 `deleted_at`，新的補一列（不硬刪）。
pub async fn sync_contract_stations(tx: &mut Tx<'_>, ctx: &Ctx, contract_id: i64, station_ids: &[i64]) -> ApiResult<()> {
    let org = ctx.org_id();
    for &sid in station_ids {
        let ok: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM tbl_energy_station WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL)",
        )
        .bind(sid)
        .bind(org)
        .fetch_one(&mut **tx)
        .await?;
        if !ok {
            return Err(err_d(404, "station_not_found", json!({ "stationId": sid })));
        }
    }
    let existing: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT id, station_id FROM mn_energy_contract_station WHERE contract_id = $1 AND station_id IS NOT NULL AND deleted_at IS NULL",
    )
    .bind(contract_id)
    .fetch_all(&mut **tx)
    .await?;
    for (row_id, sid) in &existing {
        if !station_ids.contains(sid) {
            sqlx::query("UPDATE mn_energy_contract_station SET deleted_at = $2 WHERE id = $1 AND deleted_at IS NULL")
                .bind(row_id)
                .bind(ctx.now())
                .execute(&mut **tx)
                .await?;
        }
    }
    for &sid in station_ids {
        if !existing.iter().any(|(_, s)| *s == sid) {
            sqlx::query("INSERT INTO mn_energy_contract_station (station_id, contract_id) VALUES ($1, $2)")
                .bind(sid)
                .bind(contract_id)
                .execute(&mut **tx)
                .await?;
        }
    }
    Ok(())
}

/// 把送上來的 `landownerIds` 跟目前有效列對齊（照 `sync_contract_stations` 的只增不刪精神）。
pub async fn sync_contract_landowners(tx: &mut Tx<'_>, ctx: &Ctx, contract_id: i64, landowner_ids: &[i64]) -> ApiResult<()> {
    let org = ctx.org_id();
    for &lid in landowner_ids {
        landowner_in_org(tx, org, lid).await?;
    }
    let existing: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT id, landowner_id FROM mn_energy_contract_landowner WHERE contract_id = $1 AND deleted_at IS NULL",
    )
    .bind(contract_id)
    .fetch_all(&mut **tx)
    .await?;
    for (row_id, lid) in &existing {
        if !landowner_ids.contains(lid) {
            sqlx::query("UPDATE mn_energy_contract_landowner SET deleted_at = $2 WHERE id = $1 AND deleted_at IS NULL")
                .bind(row_id)
                .bind(ctx.now())
                .execute(&mut **tx)
                .await?;
        }
    }
    for &lid in landowner_ids {
        if !existing.iter().any(|(_, l)| *l == lid) {
            sqlx::query("INSERT INTO mn_energy_contract_landowner (contract_id, landowner_id) VALUES ($1, $2)")
                .bind(contract_id)
                .bind(lid)
                .execute(&mut **tx)
                .await?;
        }
    }
    Ok(())
}

/// `landownerIds`（新）優先；沒帶就退回相容舊的單一 `landownerId`。可以先不帶，之後再補（資源順序：合約→案場→地主）。
fn landowner_ids_of(b: &Value) -> ApiResult<Vec<i64>> {
    let arr = vi64_array(b, "landownerIds");
    let ids = if !arr.is_empty() { uniq(arr.into_iter()) } else { vi64(b, "landownerId").into_iter().collect() };
    Ok(ids)
}

fn check_payment_frequency(v: &Option<String>) -> ApiResult<()> {
    if let Some(pf) = v {
        if !PAYMENT_FREQUENCIES.contains(&pf.as_str()) {
            return Err(err_d(400, "bad_payment_frequency", json!({ "paymentFrequency": pf, "allowed": PAYMENT_FREQUENCIES })));
        }
    }
    Ok(())
}

fn vstr_array(b: &Value, k: &str) -> Vec<String> {
    b.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
}

/// 可以全空：還在建置、沒有收費條款的合約可以先不綁代號。
fn check_bill_codes(v: &[String]) -> ApiResult<()> {
    for c in v {
        if !BILL_CODES.contains(&c.as_str()) {
            return Err(err_d(400, "bad_bill_code", json!({ "billCode": c, "allowed": BILL_CODES })));
        }
    }
    Ok(())
}

fn check_rent_calc_code(v: &Option<String>) -> ApiResult<()> {
    if let Some(c) = v {
        if !RENT_CALC_CODES.contains(&c.as_str()) {
            return Err(err_d(400, "bad_rent_calc_code", json!({ "rentCalcCode": c, "allowed": RENT_CALC_CODES })));
        }
    }
    Ok(())
}

fn check_rent_pay_code(v: &Option<String>) -> ApiResult<()> {
    if let Some(c) = v {
        if !RENT_PAY_CODES.contains(&c.as_str()) {
            return Err(err_d(400, "bad_rent_pay_code", json!({ "rentPayCode": c, "allowed": RENT_PAY_CODES })));
        }
    }
    Ok(())
}

async fn landowner_in_org(tx: &mut Tx<'_>, org: i64, id: i64) -> ApiResult<()> {
    let owner: Option<i64> =
        sqlx::query_scalar("SELECT id FROM tbl_energy_landowner WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL")
            .bind(id)
            .bind(org)
            .fetch_optional(&mut **tx)
            .await?;
    owner.ok_or_else(|| err(404, "landowner_not_found")).map(|_| ())
}

/// 新增合約：同一交易同步 `stationIds`、寫稽核 `contract.created`。
pub async fn create_contract(tx: &mut Tx<'_>, ctx: &Ctx, b: &Value) -> ApiResult<i64> {
    let org = ctx.org_id();
    let contract_no = vstr(b, "contractNo").ok_or_else(|| err(400, "contract_no_required"))?;
    let landowner_ids = landowner_ids_of(b)?;
    let valid_from = vday(b, "validFrom")?.ok_or_else(|| err(400, "valid_from_required"))?;
    let valid_to = vday(b, "validTo")?;
    if valid_to.is_some_and(|vt| vt < valid_from) {
        return Err(err(400, "bad_date_range"));
    }
    let payment_frequency = vstr(b, "paymentFrequency");
    check_payment_frequency(&payment_frequency)?;
    let bill_codes = uniq(vstr_array(b, "billCodes").into_iter());
    check_bill_codes(&bill_codes)?;
    let rent_calc_code = vstr(b, "rentCalcCode");
    check_rent_calc_code(&rent_calc_code)?;
    let rent_pay_code = vstr(b, "rentPayCode");
    check_rent_pay_code(&rent_pay_code)?;
    for &lid in &landowner_ids {
        landowner_in_org(tx, org, lid).await?;
    }
    let primary_landowner_id = landowner_ids.first().copied();

    let sign_method = vstr(b, "signMethod");
    let stage = vstr(b, "stage").unwrap_or_else(|| "operating".into());
    let installment_total = vi32(b, "installmentTotal");
    let note = vstr(b, "note");
    let contract_type = vstr(b, "contractType");
    let signed_on = vday(b, "signedOn")?;
    let renewal_remind_on = vday(b, "renewalRemindOn")?;
    let terms_version = vstr(b, "termsVersion");
    let payment_terms = vstr(b, "paymentTerms");

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO tbl_energy_contract
           (organization_id, landowner_id, contract_no, sign_method, valid_from, valid_to, stage, installment_total, note,
            contract_type, signed_on, renewal_remind_on, terms_version, payment_frequency, payment_terms,
            bill_codes, rent_calc_code, rent_pay_code)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18) RETURNING id",
    )
    .bind(org)
    .bind(primary_landowner_id)
    .bind(&contract_no)
    .bind(&sign_method)
    .bind(valid_from)
    .bind(valid_to)
    .bind(&stage)
    .bind(installment_total)
    .bind(&note)
    .bind(&contract_type)
    .bind(signed_on)
    .bind(renewal_remind_on)
    .bind(&terms_version)
    .bind(&payment_frequency)
    .bind(&payment_terms)
    .bind(&bill_codes)
    .bind(&rent_calc_code)
    .bind(&rent_pay_code)
    .fetch_one(&mut **tx)
    .await?;

    sync_contract_stations(tx, ctx, id, &vi64_array(b, "stationIds")).await?;
    sync_contract_landowners(tx, ctx, id, &landowner_ids).await?;

    let landowner_name: Option<String> =
        sqlx::query_scalar("SELECT name FROM tbl_energy_landowner WHERE id = $1").bind(primary_landowner_id).fetch_optional(&mut **tx).await?;
    audit::record(
        tx,
        ctx,
        AuditInput::new("contract.created", id, format!("建立合約「{contract_no}」"))
            .label(contract_no.clone())
            .payload(json!({
                "contractNo": contract_no, "landownerId": primary_landowner_id, "landownerIds": landowner_ids, "landownerName": landowner_name,
                "billCodes": bill_codes, "rentCalcCode": rent_calc_code, "rentPayCode": rent_pay_code,
            })),
    )
    .await?;
    Ok(id)
}

/// 編輯合約：表單整包送上來（照 `update_landowner`），同步 `stationIds`，寫稽核 `contract.updated`。
pub async fn update_contract(tx: &mut Tx<'_>, ctx: &Ctx, id: i64, b: &Value) -> ApiResult<()> {
    let org = ctx.org_id();
    let existing = contract_in_org(tx, org, id).await?;
    let contract_no = vstr(b, "contractNo").ok_or_else(|| err(400, "contract_no_required"))?;
    let landowner_ids = landowner_ids_of(b)?;
    let valid_from = vday(b, "validFrom")?.ok_or_else(|| err(400, "valid_from_required"))?;
    let valid_to = vday(b, "validTo")?;
    if valid_to.is_some_and(|vt| vt < valid_from) {
        return Err(err(400, "bad_date_range"));
    }
    let payment_frequency = vstr(b, "paymentFrequency");
    check_payment_frequency(&payment_frequency)?;
    let bill_codes = uniq(vstr_array(b, "billCodes").into_iter());
    check_bill_codes(&bill_codes)?;
    let rent_calc_code = vstr(b, "rentCalcCode");
    check_rent_calc_code(&rent_calc_code)?;
    let rent_pay_code = vstr(b, "rentPayCode");
    check_rent_pay_code(&rent_pay_code)?;
    for &lid in &landowner_ids {
        landowner_in_org(tx, org, lid).await?;
    }
    let primary_landowner_id = landowner_ids.first().copied();

    let sign_method = vstr(b, "signMethod");
    let stage = vstr(b, "stage").unwrap_or_else(|| existing.stage.clone());
    let installment_total = vi32(b, "installmentTotal");
    let note = vstr(b, "note");
    let contract_type = vstr(b, "contractType");
    let signed_on = vday(b, "signedOn")?;
    let renewal_remind_on = vday(b, "renewalRemindOn")?;
    let terms_version = vstr(b, "termsVersion");
    let payment_terms = vstr(b, "paymentTerms");

    sqlx::query(
        "UPDATE tbl_energy_contract SET
           landowner_id = $2, contract_no = $3, sign_method = $4, valid_from = $5, valid_to = $6, stage = $7,
           installment_total = $8, note = $9, contract_type = $10, signed_on = $11, renewal_remind_on = $12,
           terms_version = $13, payment_frequency = $14, payment_terms = $15,
           bill_codes = $16, rent_calc_code = $17, rent_pay_code = $18
         WHERE id = $1 AND organization_id = $19 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(primary_landowner_id)
    .bind(&contract_no)
    .bind(&sign_method)
    .bind(valid_from)
    .bind(valid_to)
    .bind(&stage)
    .bind(installment_total)
    .bind(&note)
    .bind(&contract_type)
    .bind(signed_on)
    .bind(renewal_remind_on)
    .bind(&terms_version)
    .bind(&payment_frequency)
    .bind(&payment_terms)
    .bind(&bill_codes)
    .bind(&rent_calc_code)
    .bind(&rent_pay_code)
    .bind(org)
    .execute(&mut **tx)
    .await?;

    sync_contract_stations(tx, ctx, id, &vi64_array(b, "stationIds")).await?;
    sync_contract_landowners(tx, ctx, id, &landowner_ids).await?;

    let landowner_name: Option<String> =
        sqlx::query_scalar("SELECT name FROM tbl_energy_landowner WHERE id = $1").bind(primary_landowner_id).fetch_optional(&mut **tx).await?;
    audit::record(
        tx,
        ctx,
        AuditInput::new("contract.updated", id, format!("修改合約「{contract_no}」"))
            .label(contract_no.clone())
            .payload(json!({
                "contractNo": contract_no, "landownerId": primary_landowner_id, "landownerIds": landowner_ids, "landownerName": landowner_name,
                "billCodes": bill_codes, "rentCalcCode": rent_calc_code, "rentPayCode": rent_pay_code,
                "changes": { "validTo": { "from": day_opt(existing.valid_to), "to": day_opt(valid_to) } },
            })),
    )
    .await?;
    Ok(())
}

/// 批次組清單用的視圖（受款對象名、案場、附件數一次撈完，不做 N+1）。
async fn contract_views(tx: &mut Tx<'_>, org: i64, rows: Vec<ContractRow>) -> ApiResult<Vec<Value>> {
    if rows.is_empty() {
        return Ok(vec![]);
    }
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let owner_links: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT contract_id, landowner_id FROM mn_energy_contract_landowner WHERE contract_id = ANY($1) AND deleted_at IS NULL ORDER BY id ASC",
    )
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    let owner_ids = uniq(owner_links.iter().map(|l| l.1).chain(rows.iter().filter_map(|r| r.landowner_id)));
    let owners: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, name FROM tbl_energy_landowner WHERE id = ANY($1) AND organization_id = $2 AND deleted_at IS NULL")
            .bind(&owner_ids)
            .bind(org)
            .fetch_all(&mut **tx)
            .await?;
    let station_links: Vec<(i64, i64, String)> = sqlx::query_as(
        "SELECT cs.contract_id, s.id, s.name FROM mn_energy_contract_station cs JOIN tbl_energy_station s ON s.id = cs.station_id
          WHERE cs.contract_id = ANY($1) AND cs.deleted_at IS NULL AND s.deleted_at IS NULL ORDER BY s.name ASC, s.id ASC",
    )
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    let att_counts: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT contract_id, COUNT(*) FROM tbl_energy_attachment WHERE contract_id = ANY($1) AND deleted_at IS NULL GROUP BY contract_id",
    )
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let landowners: Vec<Value> = owner_links
                .iter()
                .filter(|l| l.0 == r.id)
                .map(|l| json!({ "id": l.1, "name": owners.iter().find(|o| o.0 == l.1).map(|o| o.1.clone()) }))
                .collect();
            let landowner_name = r.landowner_id.and_then(|lid| owners.iter().find(|o| o.0 == lid)).map(|o| o.1.clone());
            let stations: Vec<Value> =
                station_links.iter().filter(|l| l.0 == r.id).map(|l| json!({ "id": l.1, "name": l.2 })).collect();
            let attachment_count = att_counts.iter().find(|a| a.0 == r.id).map(|a| a.1).unwrap_or(0);
            json!({
                "id": r.id, "contractNo": r.contract_no, "contractType": r.contract_type,
                "landownerId": r.landowner_id, "landownerName": landowner_name, "landowners": landowners, "stations": stations,
                "signedOn": day_opt(r.signed_on), "validFrom": day(r.valid_from), "validTo": day_opt(r.valid_to),
                "renewalRemindOn": day_opt(r.renewal_remind_on), "termsVersion": r.terms_version,
                "paymentFrequency": r.payment_frequency, "paymentTerms": r.payment_terms, "stage": r.stage,
                "billCodes": r.bill_codes, "rentCalcCode": r.rent_calc_code, "rentPayCode": r.rent_pay_code,
                "attachmentCount": attachment_count,
            })
        })
        .collect())
}

pub async fn list_contracts(
    tx: &mut Tx<'_>,
    access: &OrgAccess,
    q: Option<&str>,
    landowner_id: Option<i64>,
    station_id: Option<i64>,
    limit: Option<i64>,
) -> ApiResult<Vec<Value>> {
    let org = access.org_id();
    let q = q.map(str::trim).filter(|s| !s.is_empty());
    let rows: Vec<ContractRow> = sqlx::query_as(&format!(
        "SELECT {CONTRACT_COLS} FROM tbl_energy_contract c
          WHERE c.organization_id = $1 AND c.deleted_at IS NULL
            AND ($2::text IS NULL OR c.contract_no ILIKE $2
                 OR EXISTS (SELECT 1 FROM mn_energy_contract_landowner cl JOIN tbl_energy_landowner lo ON lo.id = cl.landowner_id
                             WHERE cl.contract_id = c.id AND cl.deleted_at IS NULL AND lo.name ILIKE $2))
            AND ($3::bigint IS NULL OR EXISTS (
                  SELECT 1 FROM mn_energy_contract_landowner cl WHERE cl.contract_id = c.id AND cl.landowner_id = $3 AND cl.deleted_at IS NULL))
            AND ($4::bigint IS NULL OR EXISTS (
                  SELECT 1 FROM mn_energy_contract_station cs WHERE cs.contract_id = c.id AND cs.station_id = $4 AND cs.deleted_at IS NULL))
          ORDER BY c.valid_from DESC, c.id DESC LIMIT $5"
    ))
    .bind(org)
    .bind(q.map(like))
    .bind(landowner_id)
    .bind(station_id)
    .bind(limit_of(limit))
    .fetch_all(&mut **tx)
    .await?;
    contract_views(tx, org, rows).await
}

// ── handlers ──

type Core = web::Data<CoreRef>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListQuery {
    q: Option<String>,
    landowner_id: Option<i64>,
    station_id: Option<i64>,
    limit: Option<i64>,
}

pub async fn list_handler(core: Core, user: CurrentUser, org: web::Path<i64>, q: web::Query<ListQuery>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::FLOW_VIEW).await?;
    let mut tx = core.pool.begin().await?;
    let items = list_contracts(&mut tx, &access, q.q.as_deref(), q.landowner_id, q.station_id, q.limit).await?;
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

pub async fn get_handler(core: Core, user: CurrentUser, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::FLOW_VIEW).await?;
    let mut tx = core.pool.begin().await?;
    let row = contract_in_org(&mut tx, access.org_id(), id).await?;
    let views = contract_views(&mut tx, access.org_id(), vec![row]).await?;
    Ok(HttpResponse::Ok().json(views.into_iter().next().unwrap()))
}

pub async fn create_handler(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::MASTER_MANAGE).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    let id = create_contract(&mut tx, &ctx, &body).await?;
    let workflow_bindings = crate::bindings::bind_by_contract_codes(&mut tx, &ctx, id).await?;
    let row = contract_in_org(&mut tx, ctx.org_id(), id).await?;
    let views = contract_views(&mut tx, ctx.org_id(), vec![row]).await?;
    tx.commit().await?;
    let mut view = views.into_iter().next().unwrap();
    view["workflowBindings"] = json!(workflow_bindings);
    Ok(HttpResponse::Ok().json(view))
}

pub async fn update_handler(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::MASTER_MANAGE).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    update_contract(&mut tx, &ctx, id, &body).await?;
    let workflow_bindings = crate::bindings::bind_by_contract_codes(&mut tx, &ctx, id).await?;
    let row = contract_in_org(&mut tx, ctx.org_id(), id).await?;
    let views = contract_views(&mut tx, ctx.org_id(), vec![row]).await?;
    tx.commit().await?;
    let mut view = views.into_iter().next().unwrap();
    view["workflowBindings"] = json!(workflow_bindings);
    Ok(HttpResponse::Ok().json(view))
}
