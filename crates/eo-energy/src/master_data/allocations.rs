//! 地主↔案場分配（卡 A）：只存，不算錢。每次新增／修改／結束都寫稽核
//! （`allocation.created`／`allocation.updated`／`allocation.ended`，見 `eo_domain::audit_events`）。

use super::station_in_org;
use crate::audit::{self, AuditInput};
use crate::common::{day, day_opt, err, err_d, parse_day, Ctx, RequestMeta, Tx};
use crate::gates;
use actix_web::{web, HttpResponse};
use chrono::NaiveDate;
use eo_platform::auth::CurrentUser;
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct AllocationRow {
    pub id: i64,
    pub organization_id: i64,
    pub landowner_id: i64,
    pub station_id: i64,
    pub method: String,
    pub ratio: Option<Decimal>,
    pub fixed_amount: Option<Decimal>,
    pub land_area: Option<Decimal>,
    pub valid_from: Option<NaiveDate>,
    pub valid_to: Option<NaiveDate>,
    pub change_reason: Option<String>,
    pub note: Option<String>,
}

const ALLOC_COLS: &str =
    "id, organization_id, landowner_id, station_id, method, ratio, fixed_amount, land_area, valid_from, valid_to, change_reason, note";

fn dec(d: Option<Decimal>) -> Value {
    d.map(|v| json!(v.normalize().to_string())).unwrap_or(Value::Null)
}

/// 分配方式的中文名（稽核摘要給人看，不印英文鍵）。
fn method_label(method: &str) -> &'static str {
    match method {
        "ratio" => "比例",
        "fixed" => "固定金額",
        "land_area" => "依土地面積",
        _ => "?",
    }
}

/// 只印跟目前方式相關的那個值（比例帶 %，固定金額／面積不帶，也不混進別種方式的舊值）。
fn value_label(a: &AllocationRow) -> String {
    match a.method.as_str() {
        "ratio" => a.ratio.map(|v| format!("{}%", v.normalize())).unwrap_or_default(),
        "fixed" => a.fixed_amount.map(|v| v.normalize().to_string()).unwrap_or_default(),
        "land_area" => a.land_area.map(|v| v.normalize().to_string()).unwrap_or_default(),
        _ => String::new(),
    }
}

async fn landowner_name(tx: &mut Tx<'_>, org_id: i64, id: i64) -> ApiResult<Option<String>> {
    Ok(sqlx::query_scalar("SELECT name FROM tbl_energy_landowner WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL")
        .bind(id)
        .bind(org_id)
        .fetch_optional(&mut **tx)
        .await?)
}

fn alloc_view(a: &AllocationRow) -> Value {
    json!({
        "id": a.id, "landownerId": a.landowner_id, "stationId": a.station_id, "method": a.method,
        "ratio": dec(a.ratio), "fixedAmount": dec(a.fixed_amount), "landArea": dec(a.land_area),
        "validFrom": day_opt(a.valid_from), "validTo": day_opt(a.valid_to),
        "changeReason": a.change_reason, "note": a.note,
        "active": a.valid_to.is_none(),
    })
}

pub async fn list_allocations(tx: &mut Tx<'_>, access: &eo_platform::rbac::OrgAccess, station_id: i64) -> ApiResult<Vec<Value>> {
    let st = station_in_org(tx, access, station_id).await?;
    let rows: Vec<AllocationRow> = sqlx::query_as(&format!(
        "SELECT {ALLOC_COLS} FROM mn_energy_landowner_station WHERE organization_id = $1 AND station_id = $2 AND deleted_at IS NULL
          ORDER BY valid_to IS NOT NULL, valid_from ASC NULLS FIRST, id ASC"
    ))
    .bind(access.org_id())
    .bind(st.id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows.iter().map(alloc_view).collect())
}

/// 異動紀錄：讀稽核表裡的三種分配事件，新到舊。
pub async fn allocation_history(tx: &mut Tx<'_>, access: &eo_platform::rbac::OrgAccess, station_id: i64) -> ApiResult<Vec<Value>> {
    let st = station_in_org(tx, access, station_id).await?;
    let rows: Vec<(i64, String, String, chrono::DateTime<chrono::Utc>, Option<String>)> = sqlx::query_as(
        "SELECT id, type, summary, occurred_at, actor_label FROM tbl_energy_audit_event
          WHERE organization_id = $1 AND type IN ('allocation.created', 'allocation.updated', 'allocation.ended')
            AND payload->>'stationId' = $2 AND deleted_at IS NULL
          ORDER BY occurred_at DESC, id DESC LIMIT 100",
    )
    .bind(access.org_id())
    .bind(st.id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, event_type, summary, at, actor)| json!({
            "id": id, "type": event_type, "summary": summary, "occurredAt": at, "actorLabel": actor,
        }))
        .collect())
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

fn payload_of(landowner_id: i64, landowner_name: Option<&str>, station_id: i64, station_name: &str, a: &AllocationRow, changes: Value) -> Value {
    json!({
        "landownerId": landowner_id, "landownerName": landowner_name, "stationId": station_id, "stationName": station_name,
        "method": a.method, "ratio": a.ratio.map(|v| v.normalize().to_string()),
        "fixedAmount": a.fixed_amount.map(|v| v.normalize().to_string()), "landArea": a.land_area.map(|v| v.normalize().to_string()),
        "validFrom": day_opt(a.valid_from), "validTo": day_opt(a.valid_to), "changeReason": a.change_reason, "changes": changes,
    })
}

pub async fn create_allocation(tx: &mut Tx<'_>, ctx: &Ctx, station_id: i64, b: &Value) -> ApiResult<i64> {
    let st = station_in_org(tx, &ctx.access, station_id).await?;
    let landowner_id = b.get("landownerId").and_then(Value::as_i64).ok_or_else(|| err(400, "landowner_id_required"))?;
    let owner: Option<i64> =
        sqlx::query_scalar("SELECT id FROM tbl_energy_landowner WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL")
            .bind(landowner_id)
            .bind(ctx.org_id())
            .fetch_optional(&mut **tx)
            .await?;
    owner.ok_or_else(|| err(400, "landowner_not_found"))?;
    let method = vstr(b, "method").unwrap_or_else(|| "ratio".into());
    if !["ratio", "fixed", "land_area"].contains(&method.as_str()) {
        return Err(err(400, "invalid_method"));
    }
    let ratio = vdecimal(b, "ratio")?;
    let fixed_amount = vdecimal(b, "fixedAmount")?;
    let land_area = vdecimal(b, "landArea")?;
    let valid_from = vday(b, "validFrom")?;
    let valid_to = vday(b, "validTo")?;
    let change_reason = vstr(b, "changeReason");
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO mn_energy_landowner_station
           (organization_id, landowner_id, station_id, method, ratio, fixed_amount, land_area, valid_from, valid_to, change_reason, note)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) RETURNING id",
    )
    .bind(ctx.org_id())
    .bind(landowner_id)
    .bind(st.id)
    .bind(&method)
    .bind(ratio)
    .bind(fixed_amount)
    .bind(land_area)
    .bind(valid_from)
    .bind(valid_to)
    .bind(&change_reason)
    .bind(vstr(b, "note"))
    .fetch_one(&mut **tx)
    .await?;
    let row = AllocationRow {
        id,
        organization_id: ctx.org_id(),
        landowner_id,
        station_id: st.id,
        method,
        ratio,
        fixed_amount,
        land_area,
        valid_from,
        valid_to,
        change_reason,
        note: None,
    };
    let owner_name = landowner_name(tx, ctx.org_id(), landowner_id).await?;
    audit::record(
        tx,
        ctx,
        AuditInput::new(
            "allocation.created",
            id,
            format!(
                "案場「{}」新增分配：{} {} {}{}",
                st.name, owner_name.clone().unwrap_or_default(), method_label(&row.method), value_label(&row),
                day_opt(valid_from).map(|d| format!("，{d} 起")).unwrap_or_default(),
            ),
        )
        .label(st.name.clone())
        .payload(payload_of(landowner_id, owner_name.as_deref(), st.id, &st.name, &row, json!({}))),
    )
    .await?;
    Ok(id)
}

async fn alloc_in_org(tx: &mut Tx<'_>, org_id: i64, id: i64) -> ApiResult<AllocationRow> {
    let row: Option<AllocationRow> =
        sqlx::query_as(&format!("SELECT {ALLOC_COLS} FROM mn_energy_landowner_station WHERE id = $1 AND deleted_at IS NULL"))
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?;
    let row = row.ok_or_else(|| err(404, "allocation_not_found"))?;
    if row.organization_id != org_id {
        return Err(err(403, "allocation_not_in_org"));
    }
    Ok(row)
}

/// PATCH：一般編輯（換成固定金額、改比例…）或填 `validTo`＋`changeReason` 結束它——兩者都在這裡，
/// 用「body 有沒有帶 validTo」分流，`end_allocation` 是給 handler 用的語意化入口。
pub async fn update_allocation(tx: &mut Tx<'_>, ctx: &Ctx, id: i64, b: &Value) -> ApiResult<()> {
    let before = alloc_in_org(tx, ctx.org_id(), id).await?;
    let st = station_in_org(tx, &ctx.access, before.station_id).await?;
    let method = vstr(b, "method").unwrap_or(before.method.clone());
    if !["ratio", "fixed", "land_area"].contains(&method.as_str()) {
        return Err(err(400, "invalid_method"));
    }
    // 換方式時把不相干的舊值清掉（例如從「比例」改成「固定金額」，舊的比例不該留著跟著跑）。
    let method_changed = method != before.method;
    let ratio = if b.get("ratio").is_some() {
        vdecimal(b, "ratio")?
    } else if method_changed {
        (method == "ratio").then_some(before.ratio).flatten()
    } else {
        before.ratio
    };
    let fixed_amount = if b.get("fixedAmount").is_some() {
        vdecimal(b, "fixedAmount")?
    } else if method_changed {
        (method == "fixed").then_some(before.fixed_amount).flatten()
    } else {
        before.fixed_amount
    };
    let land_area = if b.get("landArea").is_some() {
        vdecimal(b, "landArea")?
    } else if method_changed {
        (method == "land_area").then_some(before.land_area).flatten()
    } else {
        before.land_area
    };
    let valid_to = if b.get("validTo").is_some() { vday(b, "validTo")? } else { before.valid_to };
    let change_reason = vstr(b, "changeReason").or_else(|| before.change_reason.clone());
    sqlx::query(
        "UPDATE mn_energy_landowner_station SET method = $2, ratio = $3, fixed_amount = $4, land_area = $5, valid_to = $6, change_reason = $7
          WHERE id = $1 AND organization_id = $8 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(&method)
    .bind(ratio)
    .bind(fixed_amount)
    .bind(land_area)
    .bind(valid_to)
    .bind(&change_reason)
    .bind(ctx.org_id())
    .execute(&mut **tx)
    .await?;
    let after = AllocationRow { method, ratio, fixed_amount, land_area, valid_to, change_reason, ..before.clone() };
    let owner_name = landowner_name(tx, ctx.org_id(), before.landowner_id).await?;
    let event_type = if after.valid_to.is_some() && before.valid_to.is_none() { "allocation.ended" } else { "allocation.updated" };
    let changes = json!({
        "method": { "from": before.method, "to": after.method },
        "ratio": { "from": dec(before.ratio), "to": dec(after.ratio) },
        "validTo": { "from": day_opt(before.valid_to), "to": day_opt(after.valid_to) },
    });
    let summary = if event_type == "allocation.ended" {
        format!(
            "案場「{}」結束分配：{}，{} 起失效。理由：{}",
            st.name, owner_name.clone().unwrap_or_default(),
            day_opt(after.valid_to).unwrap_or_default(),
            after.change_reason.clone().unwrap_or_default(),
        )
    } else {
        format!(
            "案場「{}」修改分配：{} → {} {}",
            st.name, owner_name.clone().unwrap_or_default(), method_label(&after.method), value_label(&after),
        )
    };
    audit::record(
        tx,
        ctx,
        AuditInput::new(event_type, id, summary)
            .label(st.name.clone())
            .payload(payload_of(before.landowner_id, owner_name.as_deref(), st.id, &st.name, &after, changes)),
    )
    .await?;
    Ok(())
}

/// 語意化入口：結束分配（填 `validTo` 與 `changeReason`，理由必填）。
pub async fn end_allocation(tx: &mut Tx<'_>, ctx: &Ctx, id: i64, valid_to: NaiveDate, reason: &str) -> ApiResult<()> {
    if reason.trim().is_empty() {
        return Err(err(400, "reason_required"));
    }
    update_allocation(tx, ctx, id, &json!({ "validTo": day(valid_to), "changeReason": reason })).await
}

// ── handlers ──

type Core = web::Data<CoreRef>;

pub async fn list_handler(core: Core, user: CurrentUser, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, station_id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::FLOW_VIEW).await?;
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(json!({ "items": list_allocations(&mut tx, &access, station_id).await? })))
}

pub async fn history_handler(core: Core, user: CurrentUser, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, station_id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::FLOW_VIEW).await?;
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(json!({ "items": allocation_history(&mut tx, &access, station_id).await? })))
}

pub async fn create_handler(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let (org, station_id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::MASTER_MANAGE).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    create_allocation(&mut tx, &ctx, station_id, &body).await?;
    let out = list_allocations(&mut tx, &ctx.access, station_id).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "items": out })))
}

pub async fn update_handler(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let (org, allocation_id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::MASTER_MANAGE).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    // body 有 `validTo`＋`changeReason` 且原本沒結束 → 這是「結束」，理由必填；否則一般編輯。
    let has_end = body.get("validTo").and_then(Value::as_str).is_some_and(|s| !s.is_empty());
    if has_end {
        let reason = body.get("changeReason").and_then(Value::as_str).unwrap_or("");
        let valid_to = parse_day(body.get("validTo").and_then(Value::as_str).unwrap_or("")).ok_or_else(|| err(400, "invalid_date"))?;
        end_allocation(&mut tx, &ctx, allocation_id, valid_to, reason).await?;
    } else {
        update_allocation(&mut tx, &ctx, allocation_id, &body).await?;
    }
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}
