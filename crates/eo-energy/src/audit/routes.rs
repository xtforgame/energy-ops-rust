//! 稽核查詢端點（唯讀）。一律以組織為前綴，租戶隔離不是可選的。

use crate::common::{query_day, Tx};
use crate::{gates, path};
use actix_web::{web, HttpResponse};
use chrono::{DateTime, Duration, NaiveTime, Utc};
use eo_domain::audit_events::get_audit_event_def;
use eo_platform::auth::CurrentUser;
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use eo_platform::plugin::{AppPlugin, RouteRegistrar};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, Postgres, QueryBuilder};
use std::collections::HashMap;
use std::sync::Arc;

pub struct AuditPlugin;

crate::energy_plugin!(AuditPlugin, "energy_audit");

impl AppPlugin for AuditPlugin {
    fn configure_routes(&self, cfg: &mut RouteRegistrar<'_>, _self_arc: &Arc<dyn AppPlugin>) {
        cfg.get(&path("/audit-events"), web::get().to(list_events));
        cfg.get(&path("/audit-actors"), web::get().to(list_actors));
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditFilter {
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub actor_user_id: Option<i64>,
    #[serde(rename = "type")]
    pub event_type: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub outcome: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
struct EventRow {
    id: i64,
    #[sqlx(rename = "type")]
    event_type: String,
    actor_user_id: Option<i64>,
    actor_kind: String,
    actor_label: Option<String>,
    on_behalf_of_user_id: Option<i64>,
    target_type: String,
    target_id: String,
    target_label: Option<String>,
    outcome: String,
    summary: String,
    payload: Value,
    caused_by_event_id: Option<i64>,
    trace_id: Option<String>,
    occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEventView {
    pub id: i64,
    #[serde(rename = "type")]
    pub event_type: String,
    pub severity: String,
    pub summary_key: Option<String>,
    pub actor_user_id: Option<i64>,
    pub actor_kind: String,
    pub actor_label: Option<String>,
    pub actor_name: Option<String>,
    pub on_behalf_of_user_id: Option<i64>,
    pub on_behalf_of_name: Option<String>,
    pub target_type: String,
    pub target_id: String,
    pub target_label: Option<String>,
    pub target_execution_id: Option<i64>,
    pub target_bill_id: Option<i64>,
    pub outcome: String,
    pub summary: String,
    pub payload: Value,
    pub caused_by_event_id: Option<i64>,
    pub caused_by: Option<Value>,
    pub trace_id: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

fn severity_of(t: &str) -> String {
    let s = get_audit_event_def(t).map(|d| d.severity);
    serde_json::to_value(s.unwrap_or(eo_domain::audit_events::AuditSeverity::Info))
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "info".into())
}

const COLS: &str = "id, type, actor_user_id, actor_kind, actor_label, on_behalf_of_user_id, target_type, target_id, target_label,
    outcome, summary, payload, caused_by_event_id, trace_id, occurred_at";

/// 查稽核紀錄：新到舊，上限固定在伺服器端（預設 100、最多 500）。
pub async fn list_audit_events(tx: &mut Tx<'_>, org_id: i64, f: &AuditFilter) -> ApiResult<Vec<AuditEventView>> {
    let from = query_day("from", f.from.as_deref())?;
    let to = query_day("to", f.to.as_deref())?;
    let mut q: QueryBuilder<Postgres> = QueryBuilder::new(format!("SELECT {COLS} FROM tbl_energy_audit_event WHERE organization_id = "));
    q.push_bind(org_id);
    if let Some(v) = f.target_type.as_deref().filter(|s| !s.is_empty()) {
        q.push(" AND target_type = ").push_bind(v.to_string());
    }
    if let Some(v) = f.target_id.as_deref().filter(|s| !s.is_empty()) {
        q.push(" AND target_id = ").push_bind(v.to_string());
    }
    if let Some(v) = f.actor_user_id {
        q.push(" AND actor_user_id = ").push_bind(v);
    }
    if let Some(v) = f.event_type.as_deref().filter(|s| !s.is_empty()) {
        q.push(" AND type = ").push_bind(v.to_string());
    }
    if let Some(v) = f.outcome.as_deref().filter(|s| !s.is_empty()) {
        q.push(" AND outcome = ").push_bind(v.to_string());
    }
    // `to` 用「< 隔天 00:00」：occurred_at 帶時間，`<= 當天` 會把當天整天排除。
    if let Some(d) = from {
        q.push(" AND occurred_at >= ").push_bind(d.and_time(NaiveTime::MIN).and_utc());
    }
    if let Some(d) = to {
        q.push(" AND occurred_at < ").push_bind((d + Duration::days(1)).and_time(NaiveTime::MIN).and_utc());
    }
    let limit = f.limit.filter(|n| *n > 0).unwrap_or(100).min(500);
    q.push(" AND deleted_at IS NULL ORDER BY occurred_at DESC, id DESC LIMIT ").push_bind(limit);
    let rows: Vec<EventRow> = q.build_query_as().fetch_all(&mut **tx).await?;
    decorate(tx, org_id, rows).await
}

/// 補上「現在的名字」與前端查不到的關聯（task → execution、應付 → 電費單、因果那一筆的摘要）。
async fn decorate(tx: &mut Tx<'_>, org_id: i64, rows: Vec<EventRow>) -> ApiResult<Vec<AuditEventView>> {
    let ids: Vec<i64> = crate::common::uniq(rows.iter().flat_map(|r| [r.actor_user_id, r.on_behalf_of_user_id]).flatten());
    let names = crate::common::user_names(tx, &ids).await?;

    let ids_of = |t: &str| -> Vec<i64> {
        crate::common::uniq(rows.iter().filter(|r| r.target_type == t).filter_map(|r| crate::common::numeric_id(&r.target_id)))
    };
    let task_ids = ids_of("energyWorkflowTask");
    let exec_by_task: HashMap<i64, i64> = if task_ids.is_empty() {
        HashMap::new()
    } else {
        sqlx::query_as::<_, (i64, i64)>(
            "SELECT t.id, t.execution_id FROM tbl_energy_workflow_task t
               JOIN tbl_energy_workflow_execution e ON e.id = t.execution_id AND e.organization_id = $2 AND e.deleted_at IS NULL
              WHERE t.id = ANY($1) AND t.deleted_at IS NULL",
        )
        .bind(&task_ids)
        .bind(org_id)
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .collect()
    };
    let payout_ids = ids_of("energyPayoutRecord");
    let bill_by_payout: HashMap<i64, i64> = if payout_ids.is_empty() {
        HashMap::new()
    } else {
        sqlx::query_as::<_, (i64, i64)>(
            "SELECT id, bill_id FROM tbl_energy_payout_record WHERE id = ANY($1) AND organization_id = $2 AND deleted_at IS NULL",
        )
        .bind(&payout_ids)
        .bind(org_id)
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .collect()
    };
    let cause_ids: Vec<i64> = crate::common::uniq(rows.iter().filter_map(|r| r.caused_by_event_id));
    let causes: HashMap<i64, Value> = if cause_ids.is_empty() {
        HashMap::new()
    } else {
        let cs: Vec<(i64, String, String, Value)> = sqlx::query_as(
            "SELECT id, type, summary, payload FROM tbl_energy_audit_event
              WHERE id = ANY($1) AND organization_id = $2 AND deleted_at IS NULL ORDER BY id",
        )
        .bind(&cause_ids)
        .bind(org_id)
        .fetch_all(&mut **tx)
        .await?;
        cs.into_iter()
            .map(|(id, t, summary, payload)| {
                let key = get_audit_event_def(&t).map(|d| d.summary_key);
                (id, json!({ "id": id, "type": t, "summary": summary, "summaryKey": key, "payload": payload }))
            })
            .collect()
    };

    Ok(rows
        .into_iter()
        .map(|r| {
            let tid = crate::common::numeric_id(&r.target_id);
            AuditEventView {
                severity: severity_of(&r.event_type),
                summary_key: get_audit_event_def(&r.event_type).map(|d| d.summary_key.to_string()),
                actor_name: r.actor_user_id.and_then(|u| names.get(&u).cloned()),
                on_behalf_of_name: r.on_behalf_of_user_id.and_then(|u| names.get(&u).cloned()),
                target_execution_id: if r.target_type == "energyWorkflowTask" {
                    tid.and_then(|t| exec_by_task.get(&t).copied())
                } else {
                    None
                },
                target_bill_id: if r.target_type == "energyPayoutRecord" {
                    tid.and_then(|t| bill_by_payout.get(&t).copied())
                } else {
                    None
                },
                caused_by: r.caused_by_event_id.and_then(|c| causes.get(&c).cloned()),
                id: r.id,
                event_type: r.event_type,
                actor_user_id: r.actor_user_id,
                actor_kind: r.actor_kind,
                actor_label: r.actor_label,
                on_behalf_of_user_id: r.on_behalf_of_user_id,
                target_type: r.target_type,
                target_id: r.target_id,
                target_label: r.target_label,
                outcome: r.outcome,
                summary: r.summary,
                payload: r.payload,
                caused_by_event_id: r.caused_by_event_id,
                trace_id: r.trace_id,
                occurred_at: r.occurred_at,
            }
        })
        .collect())
}

async fn list_events(
    core: web::Data<CoreRef>,
    user: CurrentUser,
    org: web::Path<i64>,
    q: web::Query<AuditFilter>,
) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::AUDIT_VIEW).await?;
    let mut tx = core.pool.begin().await?;
    let items = list_audit_events(&mut tx, access.org_id(), &q).await?;
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

/// 操作者下拉：曾在這個組織留下稽核的使用者。
async fn list_actors(core: web::Data<CoreRef>, user: CurrentUser, org: web::Path<i64>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::AUDIT_VIEW).await?;
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT u.id, u.name FROM tbl_user u
          WHERE u.deleted_at IS NULL AND u.id IN (
            SELECT DISTINCT actor_user_id FROM tbl_energy_audit_event
             WHERE organization_id = $1 AND deleted_at IS NULL AND actor_user_id IS NOT NULL)
          ORDER BY u.id",
    )
    .bind(access.org_id())
    .fetch_all(&core.pool)
    .await?;
    let items: Vec<Value> = rows.into_iter().map(|(id, name)| json!({ "id": id, "name": name })).collect();
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}
