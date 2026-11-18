//! 效期綁定管理 —— 一套機制、兩種 kind（workflow／rating），三個具名動作（綁定、換版、解除），不做 CRUD。
//!
//! · 效期不重疊的範圍是「一條線」＝(單別, 目標 key)，不是整個案場（一份文件可以同時開多條流程）。
//! · 已結束的段唯讀；沒有「刪除」只有「解除」（刪掉會讓已完成的 execution 指向不存在的效期）。
//! · 兩種 kind 的案場比對鍵不一樣：workflow 比電號（station_key）、rating 比 station_id。
//! · 權限：workflow → `energy:workflow:manage`；rating → `energy:rating:manage`（只有 admin，CXO 也不行）。

use crate::audit::{self, AuditInput};
use crate::common::{day, day_opt, err, err_d, has_permission, parse_day, Ctx, RequestMeta, Tx};
use crate::workflow::definition::in_effect;
use crate::{gates, path};
use actix_web::{web, HttpResponse};
use chrono::NaiveDate;
use eo_platform::auth::CurrentUser;
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use eo_platform::plugin::{AppPlugin, RouteRegistrar};
use eo_platform::rbac::OrgAccess;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::FromRow;
use std::sync::Arc;

pub struct BindingsPlugin;

crate::energy_plugin!(BindingsPlugin, "energy_bindings");

impl AppPlugin for BindingsPlugin {
    fn configure_routes(&self, cfg: &mut RouteRegistrar<'_>, _self_arc: &Arc<dyn AppPlugin>) {
        cfg.get(&path("/stations/{stationId}/bindings"), web::get().to(list_handler));
        cfg.get(&path("/stations/{stationId}/bindings/preview"), web::get().to(preview_handler));
        cfg.post(&path("/stations/{stationId}/bindings"), web::post().to(create_handler));
        cfg.post(&path("/stations/{stationId}/bindings/switch"), web::post().to(switch_handler));
        cfg.post(&path("/stations/{stationId}/bindings/end"), web::post().to(end_handler));
        cfg.get(&path("/binding-options"), web::get().to(options_handler));
        cfg.get(&path("/unbound-stations"), web::get().to(unbound_handler));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Workflow,
    Rating,
}

/// 兩種 kind 的差異全部集中在這裡。
struct KindDef {
    binding_table: &'static str,
    version_table: &'static str,
    key_col: &'static str,
    fk_col: &'static str,
    supports_doc_type: bool,
    audit_created: &'static str,
    audit_switched: &'static str,
    audit_ended: &'static str,
    permission: &'static str,
}

impl Kind {
    pub fn parse(v: Option<&str>) -> ApiResult<Kind> {
        match v {
            Some("workflow") => Ok(Kind::Workflow),
            Some("rating") => Ok(Kind::Rating),
            _ => Err(err(400, "invalid_kind")),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Workflow => "workflow",
            Kind::Rating => "rating",
        }
    }
    fn def(self) -> KindDef {
        match self {
            Kind::Workflow => KindDef {
                binding_table: "tbl_energy_workflow_binding",
                version_table: "tbl_energy_workflow_definition",
                key_col: "workflow_key",
                fk_col: "definition_id",
                supports_doc_type: true,
                audit_created: "flow.binding_created",
                audit_switched: "flow.binding_switched",
                audit_ended: "flow.binding_ended",
                permission: gates::WORKFLOW_MANAGE[0],
            },
            Kind::Rating => KindDef {
                binding_table: "tbl_energy_rating_strategy_binding",
                version_table: "tbl_energy_rating_strategy",
                key_col: "strategy_key",
                fk_col: "strategy_id",
                // 計價策略不分單別：一座案場的錢怎麼算，與收到哪一種單無關。
                supports_doc_type: false,
                audit_created: "strategy.binding_created",
                audit_switched: "strategy.binding_switched",
                audit_ended: "strategy.binding_ended",
                permission: gates::RATING_MANAGE[0],
            },
        }
    }
    pub fn permission(self) -> &'static str {
        self.def().permission
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct StationRow {
    pub id: i64,
    pub name: String,
    pub customer_number: String,
    pub organization_id: i64,
    pub status: String,
}

/// 案場的組織由 server 從案場列推導：別的組織 → 403，不存在 → 404。
pub async fn station_in_org(tx: &mut Tx<'_>, access: &OrgAccess, id: i64) -> ApiResult<StationRow> {
    let row: Option<StationRow> = sqlx::query_as(
        "SELECT id, name, customer_number, organization_id, status FROM tbl_energy_station WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    let row = row.ok_or_else(|| err(404, "station_not_found"))?;
    if row.organization_id != access.org_id() {
        return Err(err(403, "station_not_in_org"));
    }
    Ok(row)
}

#[derive(Debug, Clone, FromRow)]
struct Segment {
    id: i64,
    doc_type: Option<String>,
    target_key: String,
    version_id: i64,
    valid_from: NaiveDate,
    valid_to: Option<NaiveDate>,
}

#[derive(Debug, Clone, FromRow)]
struct VersionRow {
    id: i64,
    key: String,
    version: i32,
    name: String,
    status: String,
    organization_id: i64,
    vtype: Option<String>,
}

fn lane_of(s: &Segment) -> String {
    format!("{}|{}", s.doc_type.clone().unwrap_or_default(), s.target_key)
}

/// 這個案場、這個 kind 的所有段（舊 → 新）。給 lane 就只取那一條線。
async fn segments(
    tx: &mut Tx<'_>,
    kind: Kind,
    org_id: i64,
    st: &StationRow,
    lane: Option<(&Option<String>, Option<&str>)>,
) -> ApiResult<Vec<Segment>> {
    let k = kind.def();
    let doc_col = if k.supports_doc_type { "doc_type" } else { "NULL::varchar" };
    let (station_where, bind_station): (&str, Value) = match kind {
        Kind::Workflow => ("station_key = $2", json!(st.customer_number)),
        Kind::Rating => ("station_id = $2::bigint", json!(st.id)),
    };
    let sql = format!(
        "SELECT id, {doc_col} AS doc_type, {} AS target_key, {} AS version_id, valid_from, valid_to FROM {}
          WHERE organization_id = $1 AND {station_where} AND deleted_at IS NULL ORDER BY valid_from ASC, id ASC",
        k.key_col, k.fk_col, k.binding_table
    );
    let q = sqlx::query_as::<_, Segment>(&sql).bind(org_id);
    let q = match bind_station {
        Value::String(s) => q.bind(s),
        other => q.bind(other.as_i64().unwrap_or(0)),
    };
    let rows = q.fetch_all(&mut **tx).await?;
    Ok(match lane {
        None => rows,
        Some((doc, key)) => rows.into_iter().filter(|r| &r.doc_type == doc && key.is_none_or(|k| r.target_key == k)).collect(),
    })
}

fn active_at(rows: &[Segment], d: NaiveDate) -> Option<&Segment> {
    rows.iter().find(|r| in_effect(r.valid_from, r.valid_to, d))
}

fn open_segment(rows: &[Segment]) -> Option<&Segment> {
    rows.iter().find(|r| r.valid_to.is_none())
}

async fn versions_by_id(tx: &mut Tx<'_>, kind: Kind, org_id: i64, ids: &[i64]) -> ApiResult<Vec<VersionRow>> {
    let k = kind.def();
    let t = if kind == Kind::Rating { "type" } else { "NULL::varchar" };
    Ok(sqlx::query_as(&format!(
        "SELECT id, key, version, name, status, organization_id, {t} AS vtype FROM {} WHERE id = ANY($1) AND organization_id = $2 AND deleted_at IS NULL ORDER BY id",
        k.version_table
    ))
    .bind(ids)
    .bind(org_id)
    .fetch_all(&mut **tx)
    .await?)
}

/// 兩條軸一起回；每一段帶 `editable`（已結束的段唯讀）與 `isCurrent`（每條線各算一次）。
pub async fn list(tx: &mut Tx<'_>, ctx: &Ctx, station_id: i64) -> ApiResult<Value> {
    let st = station_in_org(tx, &ctx.access, station_id).await?;
    let today = ctx.today();
    let mut out = json!({ "stationId": st.id, "stationName": st.name, "customerNumber": st.customer_number });
    for kind in [Kind::Workflow, Kind::Rating] {
        let rows = segments(tx, kind, ctx.org_id(), &st, None).await?;
        let ids: Vec<i64> = crate::common::uniq(rows.iter().map(|r| r.version_id));
        let versions = versions_by_id(tx, kind, ctx.org_id(), &ids).await?;
        let mut active_ids = Vec::new();
        for lane in crate::common::uniq(rows.iter().map(lane_of)) {
            let mine: Vec<Segment> = rows.iter().filter(|r| lane_of(r) == lane).cloned().collect();
            if let Some(hit) = active_at(&mine, today) {
                active_ids.push(hit.id);
            }
        }
        let items: Vec<Value> = rows
            .iter()
            .map(|r| {
                let v = versions.iter().find(|v| v.id == r.version_id);
                json!({
                    "id": r.id, "kind": kind.as_str(), "targetKey": r.target_key, "docType": r.doc_type,
                    "versionId": r.version_id, "name": v.map(|v| v.name.clone()), "version": v.map(|v| v.version),
                    "type": v.and_then(|v| v.vtype.clone()),
                    "validFrom": day(r.valid_from), "validTo": day_opt(r.valid_to),
                    "isCurrent": active_ids.contains(&r.id),
                    "editable": r.valid_to.is_none_or(|t| today < t),
                })
            })
            .collect();
        out[kind.as_str()] = json!(items);
    }
    Ok(out)
}

/// 這個 kind 有哪些版本可以綁（只列已發佈的）。
pub async fn version_options(tx: &mut Tx<'_>, ctx: &Ctx, kind: Kind) -> ApiResult<Vec<Value>> {
    let k = kind.def();
    let t = if kind == Kind::Rating { "type" } else { "NULL::varchar" };
    let rows: Vec<VersionRow> = sqlx::query_as(&format!(
        "SELECT id, key, version, name, status, organization_id, {t} AS vtype FROM {}
          WHERE organization_id = $1 AND status = 'published' AND deleted_at IS NULL ORDER BY key ASC, version DESC",
        k.version_table
    ))
    .bind(ctx.org_id())
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| json!({ "id": r.id, "key": r.key, "name": r.name, "version": r.version, "type": r.vtype }))
        .collect())
}

/// 「按下去會怎樣」—— 獨立一支，後果必須在按下之前看得到。兩個數字都給（含不受影響的那個）。
pub async fn preview(tx: &mut Tx<'_>, ctx: &Ctx, station_id: i64, kind: Kind, effective_from: NaiveDate) -> ApiResult<Value> {
    let st = station_in_org(tx, &ctx.access, station_id).await?;
    // 業務日期用 billing_period_end（送審時取版用的就是它）。
    let affected: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tbl_energy_bill
          WHERE organization_id = $1 AND station_id = $2 AND review_status = ANY($3) AND billing_period_end >= $4 AND deleted_at IS NULL",
    )
    .bind(ctx.org_id())
    .bind(st.id)
    .bind(vec!["pendingReview", "pendingMatch", "pendingContract", "reviewed"])
    .bind(effective_from)
    .fetch_one(&mut **tx)
    .await?;
    // 進行中的執行不受影響（鎖在自己那一版），但要講出來。
    let running: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tbl_energy_workflow_execution e
          WHERE e.organization_id = $1 AND e.status = 'running' AND e.subject_type = 'energyBill' AND e.deleted_at IS NULL
            AND e.subject_id IN (SELECT id::text FROM tbl_energy_bill WHERE organization_id = $1 AND station_id = $2 AND deleted_at IS NULL)",
    )
    .bind(ctx.org_id())
    .bind(st.id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(json!({
        "kind": kind.as_str(), "effectiveFrom": day(effective_from), "isBackdated": effective_from < ctx.today(),
        "affectedDraftBills": affected, "runningExecutions": running,
    }))
}

async fn version(tx: &mut Tx<'_>, kind: Kind, ctx: &Ctx, id: i64) -> ApiResult<VersionRow> {
    let k = kind.def();
    let t = if kind == Kind::Rating { "type" } else { "NULL::varchar" };
    let v: Option<VersionRow> = sqlx::query_as(&format!(
        "SELECT id, key, version, name, status, organization_id, {t} AS vtype FROM {} WHERE id = $1 AND deleted_at IS NULL",
        k.version_table
    ))
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    let v = v.ok_or_else(|| err(404, "version_not_found"))?;
    if v.organization_id != ctx.org_id() {
        return Err(err(403, "version_not_in_org"));
    }
    if v.status != "published" {
        return Err(err(409, "version_not_published"));
    }
    Ok(v)
}

async fn create_segment(
    tx: &mut Tx<'_>,
    kind: Kind,
    ctx: &Ctx,
    st: &StationRow,
    v: &VersionRow,
    doc_type: &Option<String>,
    valid_from: NaiveDate,
) -> ApiResult<i64> {
    Ok(match kind {
        // workflow 也一併填 station_id（補充欄），但取版永遠比電號。
        Kind::Workflow => sqlx::query_scalar(
            "INSERT INTO tbl_energy_workflow_binding (organization_id, station_key, station_id, doc_type, workflow_key, definition_id, valid_from)
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
        )
        .bind(ctx.org_id())
        .bind(&st.customer_number)
        .bind(st.id)
        .bind(doc_type.as_deref())
        .bind(&v.key)
        .bind(v.id)
        .bind(valid_from)
        .fetch_one(&mut **tx)
        .await?,
        Kind::Rating => sqlx::query_scalar(
            "INSERT INTO tbl_energy_rating_strategy_binding (organization_id, station_id, strategy_key, strategy_id, valid_from)
             VALUES ($1, $2, $3, $4, $5) RETURNING id",
        )
        .bind(ctx.org_id())
        .bind(st.id)
        .bind(&v.key)
        .bind(v.id)
        .bind(valid_from)
        .fetch_one(&mut **tx)
        .await?,
    })
}

async fn record(tx: &mut Tx<'_>, ctx: &Ctx, kind: Kind, event: &'static str, st: &StationRow, id: i64, summary: String, extra: Value) -> ApiResult<()> {
    let mut payload = json!({ "kind": kind.as_str(), "stationId": st.id, "stationKey": st.customer_number, "stationName": st.name });
    if let (Some(p), Value::Object(e)) = (payload.as_object_mut(), extra) {
        p.extend(e);
    }
    audit::record(tx, ctx, AuditInput::new(event, id, summary).label(st.name.clone()).payload(payload)).await?;
    Ok(())
}

fn doc_type_of(kind: Kind, raw: Option<&str>) -> ApiResult<Option<String>> {
    if !kind.def().supports_doc_type {
        return Ok(None);
    }
    match raw.filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) if eo_domain::document_ingest::EnergyDocType::parse(s).is_some() => Ok(Some(s.to_string())),
        Some(_) => Err(err(400, "invalid_doc_type")),
    }
}

/// 動作一：綁定（這條線目前沒有開著的段）。
pub async fn create(tx: &mut Tx<'_>, ctx: &Ctx, station_id: i64, kind: Kind, doc_type: Option<&str>, version_id: i64, valid_from: NaiveDate) -> ApiResult<()> {
    let st = station_in_org(tx, &ctx.access, station_id).await?;
    let v = version(tx, kind, ctx, version_id).await?;
    let doc = doc_type_of(kind, doc_type)?;
    let rows = segments(tx, kind, ctx.org_id(), &st, Some((&doc, Some(&v.key)))).await?;
    if open_segment(&rows).is_some() {
        return Err(err(409, "binding_already_open"));
    }
    if active_at(&rows, valid_from).is_some() {
        return Err(err(409, "binding_overlap"));
    }
    let id = create_segment(tx, kind, ctx, &st, &v, &doc, valid_from).await?;
    record(
        tx,
        ctx,
        kind,
        kind.def().audit_created,
        &st,
        id,
        format!("綁定「{} v{}」到案場「{}」，{} 起生效", v.name, v.version, st.name, day(valid_from)),
        json!({ "targetKey": v.key, "toVersion": v.version, "validFrom": day(valid_from), "validTo": null }),
    )
    .await
}

/// 動作二：換版 —— 截斷舊段＋開新段＋稽核同一個交易（分開寫會有「兩段都生效」的瞬間）。
pub async fn switch_version(tx: &mut Tx<'_>, ctx: &Ctx, station_id: i64, kind: Kind, doc_type: Option<&str>, to_version_id: i64, effective_from: NaiveDate) -> ApiResult<()> {
    let st = station_in_org(tx, &ctx.access, station_id).await?;
    let v = version(tx, kind, ctx, to_version_id).await?;
    let doc = doc_type_of(kind, doc_type)?;
    let rows = segments(tx, kind, ctx.org_id(), &st, Some((&doc, Some(&v.key)))).await?;
    let open = open_segment(&rows).cloned().ok_or_else(|| err(409, "no_open_binding"))?;
    // 生效日不可早於（或等於）現行段的起日 —— 那會產生一段負長度的效期。
    if effective_from <= open.valid_from {
        return Err(err_d(409, "effective_from_before_current", json!({ "currentValidFrom": day(open.valid_from) })));
    }
    let from_version = versions_by_id(tx, kind, ctx.org_id(), &[open.version_id]).await?.into_iter().next();
    // 按下當時的後果一起存進稽核 —— 事後重算會得到不同答案。
    let consequence = preview(tx, ctx, station_id, kind, effective_from).await?;
    let k = kind.def();
    sqlx::query(&format!("UPDATE {} SET valid_to = $2 WHERE id = $1 AND deleted_at IS NULL", k.binding_table))
        .bind(open.id)
        .bind(effective_from)
        .execute(&mut **tx)
        .await?;
    let id = create_segment(tx, kind, ctx, &st, &v, &doc, effective_from).await?;
    let backdated = consequence["isBackdated"].as_bool().unwrap_or(false);
    let suffix = if backdated {
        format!("（回溯生效，影響 {} 張未送審的單）", consequence["affectedDraftBills"])
    } else {
        String::new()
    };
    record(
        tx,
        ctx,
        kind,
        k.audit_switched,
        &st,
        id,
        format!(
            "案場「{}」換版：v{} → 「{} v{}」，{} 起生效{suffix}",
            st.name,
            from_version.as_ref().map(|f| f.version.to_string()).unwrap_or_else(|| "?".into()),
            v.name,
            v.version,
            day(effective_from)
        ),
        json!({
            "targetKey": v.key, "toVersion": v.version, "fromVersion": from_version.map(|f| f.version),
            "validFrom": day(effective_from), "validTo": null, "isBackdated": backdated,
            "affectedDraftBills": consequence["affectedDraftBills"], "runningExecutions": consequence["runningExecutions"],
        }),
    )
    .await
}

/// 動作三：解除（critical，理由必填）。之後這個案場收不了那種單／算不出錢。不刪列，只設 valid_to。
pub async fn end(
    tx: &mut Tx<'_>,
    ctx: &Ctx,
    station_id: i64,
    kind: Kind,
    doc_type: Option<&str>,
    target_key: Option<&str>,
    valid_to: NaiveDate,
    reason: &str,
) -> ApiResult<()> {
    if reason.trim().is_empty() {
        return Err(err(400, "reason_required"));
    }
    let st = station_in_org(tx, &ctx.access, station_id).await?;
    let doc = doc_type_of(kind, doc_type)?;
    let target_key = target_key.filter(|s| !s.is_empty());
    let rows = match target_key {
        Some(k) => segments(tx, kind, ctx.org_id(), &st, Some((&doc, Some(k)))).await?,
        None => segments(tx, kind, ctx.org_id(), &st, None).await?,
    };
    // 沒指定線、而開著的不只一條：猜一條解除比解不掉糟得多。
    let opens: Vec<&Segment> = rows.iter().filter(|r| r.valid_to.is_none()).collect();
    if opens.len() > 1 {
        let lanes: Vec<Value> = opens.iter().map(|r| json!({ "docType": r.doc_type, "targetKey": r.target_key })).collect();
        return Err(err_d(409, "binding_lane_required", json!({ "lanes": lanes })));
    }
    let open = open_segment(&rows).cloned().ok_or_else(|| err(409, "no_open_binding"))?;
    if valid_to <= open.valid_from {
        return Err(err_d(409, "valid_to_before_current", json!({ "currentValidFrom": day(open.valid_from) })));
    }
    let v = versions_by_id(tx, kind, ctx.org_id(), &[open.version_id]).await?.into_iter().next();
    let k = kind.def();
    sqlx::query(&format!("UPDATE {} SET valid_to = $2 WHERE id = $1 AND deleted_at IS NULL", k.binding_table))
        .bind(open.id)
        .bind(valid_to)
        .execute(&mut **tx)
        .await?;
    let consequence = match kind {
        Kind::Workflow => "之後這個案場收不了單",
        Kind::Rating => "之後這個案場算不出應付",
    };
    let what = match kind {
        Kind::Workflow => "流程",
        Kind::Rating => "策略",
    };
    record(
        tx,
        ctx,
        kind,
        k.audit_ended,
        &st,
        open.id,
        format!("解除案場「{}」的{what}綁定，{} 起失效 —— {consequence}。理由：{reason}", st.name, day(valid_to)),
        json!({
            "targetKey": open.target_key, "fromVersion": v.map(|v| v.version),
            "validFrom": day(open.valid_from), "validTo": day(valid_to), "reason": reason,
        }),
    )
    .await
}

/// 今天沒有生效綁定的案場（漏綁不會報錯，幾週後才變成一張退回的單）。兩種 kind 分開報。
pub async fn unbound_stations(tx: &mut Tx<'_>, ctx: &Ctx) -> ApiResult<Vec<Value>> {
    let today = ctx.today();
    let stations: Vec<StationRow> = sqlx::query_as(
        "SELECT id, name, customer_number, organization_id, status FROM tbl_energy_station
          WHERE organization_id = $1 AND deleted_at IS NULL ORDER BY id ASC",
    )
    .bind(ctx.org_id())
    .fetch_all(&mut **tx)
    .await?;
    if stations.is_empty() {
        return Ok(vec![]);
    }
    let flows: Vec<(Option<String>, NaiveDate, Option<NaiveDate>)> = sqlx::query_as(
        "SELECT station_key, valid_from, valid_to FROM tbl_energy_workflow_binding WHERE organization_id = $1 AND deleted_at IS NULL ORDER BY id",
    )
    .bind(ctx.org_id())
    .fetch_all(&mut **tx)
    .await?;
    let rates: Vec<(i64, NaiveDate, Option<NaiveDate>)> = sqlx::query_as(
        "SELECT station_id, valid_from, valid_to FROM tbl_energy_rating_strategy_binding WHERE organization_id = $1 AND deleted_at IS NULL ORDER BY id",
    )
    .bind(ctx.org_id())
    .fetch_all(&mut **tx)
    .await?;
    Ok(stations
        .into_iter()
        .filter_map(|s| {
            let missing_workflow =
                !flows.iter().any(|f| f.0.as_deref() == Some(s.customer_number.as_str()) && in_effect(f.1, f.2, today));
            let missing_rating = !rates.iter().any(|r| r.0 == s.id && in_effect(r.1, r.2, today));
            (missing_workflow || missing_rating).then(|| {
                json!({ "id": s.id, "name": s.name, "customerNumber": s.customer_number, "status": s.status,
                        "missingWorkflow": missing_workflow, "missingRating": missing_rating })
            })
        })
        .collect())
}

// ── handlers ──

type Core = web::Data<CoreRef>;

async fn kind_gate(core: &Core, user: &CurrentUser, org: i64, kind: Kind, meta: &RequestMeta) -> ApiResult<Ctx> {
    let access = core.rbac.authorize_org(user, org, &[kind.permission()]).await?;
    Ok(Ctx::new(core, access, meta))
}

async fn list_handler(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, station) = path.into_inner();
    // 看綁定不需要管理權；動它才需要（由後端說「這個人能不能動」）。
    let access = core.rbac.authorize_org(&user, org, gates::FLOW_VIEW).await?;
    let can_workflow = has_permission(&core, &access, Kind::Workflow.permission()).await?;
    let can_rating = has_permission(&core, &access, Kind::Rating.permission()).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    let mut body = list(&mut tx, &ctx, station).await?;
    body["canManage"] = json!({ "workflow": can_workflow, "rating": can_rating });
    Ok(HttpResponse::Ok().json(body))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KindQuery {
    kind: Option<String>,
    effective_from: Option<String>,
}

async fn options_handler(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, q: web::Query<KindQuery>) -> ApiResult<HttpResponse> {
    let kind = Kind::parse(q.kind.as_deref())?;
    let ctx = kind_gate(&core, &user, org.into_inner(), kind, &meta).await?;
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(json!({ "items": version_options(&mut tx, &ctx, kind).await? })))
}

async fn preview_handler(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, q: web::Query<KindQuery>) -> ApiResult<HttpResponse> {
    let (org, station) = path.into_inner();
    let kind = Kind::parse(q.kind.as_deref())?;
    let ctx = kind_gate(&core, &user, org, kind, &meta).await?;
    let d = q.effective_from.as_deref().and_then(parse_day).ok_or_else(|| err(400, "invalid_date"))?;
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(preview(&mut tx, &ctx, station, kind, d).await?))
}

fn body_str<'a>(b: &'a Value, k: &str) -> Option<&'a str> {
    b.get(k).and_then(Value::as_str)
}

fn body_day(b: &Value, k: &str) -> ApiResult<NaiveDate> {
    body_str(b, k).and_then(parse_day).ok_or_else(|| err_d(400, "invalid_date", json!({ "field": k })))
}

async fn write<F>(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>, f: F) -> ApiResult<HttpResponse>
where
    F: for<'a> FnOnce(&'a mut Tx<'static>, &'a Ctx, i64, Kind, &'a Value) -> futures::future::BoxFuture<'a, ApiResult<()>>,
{
    let (org, station) = path.into_inner();
    let kind = Kind::parse(body_str(&body, "kind"))?;
    let ctx = kind_gate(&core, &user, org, kind, &meta).await?;
    let mut tx = core.pool.begin().await?;
    f(&mut tx, &ctx, station, kind, &body).await?;
    let out = list(&mut tx, &ctx, station).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(out))
}

async fn create_handler(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    write(core, user, meta, path, body, |tx, ctx, station, kind, b| {
        Box::pin(async move {
            let version_id = b.get("versionId").and_then(Value::as_i64).ok_or_else(|| err(400, "version_id_required"))?;
            create(tx, ctx, station, kind, body_str(b, "docType"), version_id, body_day(b, "validFrom")?).await
        })
    })
    .await
}

async fn switch_handler(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    write(core, user, meta, path, body, |tx, ctx, station, kind, b| {
        Box::pin(async move {
            let to = b.get("toVersionId").and_then(Value::as_i64).ok_or_else(|| err(400, "version_id_required"))?;
            switch_version(tx, ctx, station, kind, body_str(b, "docType"), to, body_day(b, "effectiveFrom")?).await
        })
    })
    .await
}

async fn end_handler(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    write(core, user, meta, path, body, |tx, ctx, station, kind, b| {
        Box::pin(async move {
            let reason = body_str(b, "reason").unwrap_or("");
            if reason.trim().is_empty() {
                return Err(err(400, "reason_required"));
            }
            end(tx, ctx, station, kind, body_str(b, "docType"), body_str(b, "targetKey"), body_day(b, "validTo")?, reason).await
        })
    })
    .await
}

async fn unbound_handler(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::FLOW_VIEW).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(json!({ "items": unbound_stations(&mut tx, &ctx).await? })))
}
