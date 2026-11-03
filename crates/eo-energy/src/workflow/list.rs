//! 流程單清單：以 execution 為主表、subject 多型展開（「查 bills 再 join」會讓保證金那類永遠撈不到）。
//! 可視度走參與者判定（或 view-all）；`scope=mine` 對有 view-all 的人也要過濾。

use super::access::visible_execution_ids;
use super::model::{ExecutionRow, EXEC_COLS};
use crate::common::{days_since, err_d, Ctx, Tx};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use eo_domain::display_id::{get_display_id, is_db_id, DisplayIdSource, IdLike};
use eo_platform::error::ApiResult;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Postgres, QueryBuilder};
use std::collections::HashMap;

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListFilter {
    pub status: Option<String>,
    pub scope: Option<String>,
    pub execution_id: Option<i64>,
    pub workflow_key: Option<String>,
    pub node_key: Option<String>,
    pub station_id: Option<i64>,
    pub landowner_id: Option<i64>,
    pub waiting_days_min: Option<i64>,
    pub month: Option<String>,
    pub document_no: Option<String>,
    pub sort: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionSummary {
    pub id: i64,
    /// 對外指認用的單號（純顯示，路由仍走 id）。
    pub display_id: String,
    pub document_no: Option<String>,
    pub status: String,
    pub station_id: Option<i64>,
    pub workflow_key: Option<String>,
    pub workflow_name: Option<String>,
    pub definition_id: i64,
    pub version: Option<i32>,
    pub node_key: Option<String>,
    pub node_name: Option<String>,
    pub waiting_days: Option<i64>,
    pub started_at: DateTime<Utc>,
    pub started_by_name: Option<String>,
    pub business_date: String,
    pub subject: Value,
}

/// `2026-02` → `[2026-02-01, 2026-03-01)`。
fn month_range(m: &str) -> Option<(NaiveDate, NaiveDate)> {
    let start = NaiveDate::parse_from_str(&format!("{m}-01"), "%Y-%m-%d").ok().filter(|_| m.len() == 7)?;
    let next = if start.month() == 12 {
        NaiveDate::from_ymd_opt(start.year() + 1, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(start.year(), start.month() + 1, 1)?
    };
    Some((start, next))
}

/// 單號 → subject（兩張 subject 表都找；同單號多列時已通過的優先，其餘取最新）。
pub async fn find_by_document_no(tx: &mut Tx<'_>, org_id: i64, document_no: &str) -> ApiResult<Option<(&'static str, i64)>> {
    for (table, kind) in [("tbl_energy_bill", "energyBill"), ("tbl_energy_installment", "energyInstallment")] {
        let id: Option<i64> = sqlx::query_scalar(&format!(
            "SELECT id FROM {table} WHERE organization_id = $1 AND document_no = $2 AND deleted_at IS NULL
              ORDER BY CASE WHEN approval_state = 'approved' THEN 0 ELSE 1 END ASC, id DESC LIMIT 1"
        ))
        .bind(org_id)
        .bind(document_no)
        .fetch_optional(&mut **tx)
        .await?;
        if let Some(id) = id {
            return Ok(Some((kind, id)));
        }
    }
    Ok(None)
}

/// 把 execution 展開成「一列看得懂的東西」。
pub async fn decorate(tx: &mut Tx<'_>, ctx: &Ctx, rows: &[ExecutionRow]) -> ApiResult<Vec<ExecutionSummary>> {
    if rows.is_empty() {
        return Ok(vec![]);
    }
    let def_ids: Vec<i64> = crate::common::uniq(rows.iter().map(|e| e.definition_id));
    let defs: Vec<(i64, String, String, i32)> = sqlx::query_as(
        "SELECT id, key, name, version FROM tbl_energy_workflow_definition WHERE id = ANY($1) AND organization_id = $2 AND deleted_at IS NULL",
    )
    .bind(&def_ids)
    .bind(ctx.org_id())
    .fetch_all(&mut **tx)
    .await?;
    let def_by_id: HashMap<i64, (String, String, i32)> = defs.into_iter().map(|(id, k, n, v)| (id, (k, n, v))).collect();
    let names: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT definition_id, node_key, name FROM tbl_energy_workflow_node WHERE definition_id = ANY($1) AND deleted_at IS NULL ORDER BY id",
    )
    .bind(&def_ids)
    .fetch_all(&mut **tx)
    .await?;
    let node_name: HashMap<(i64, String), String> = names.into_iter().map(|(d, k, n)| ((d, k), n)).collect();
    let refs: Vec<&ExecutionRow> = rows.iter().collect();
    let subjects = super::subjects::load(tx, ctx.org_id(), &refs).await?;
    // 目前這一關開著的 task（算「等了幾天」）；排除 out-of-band —— 中斷核可不在主流程路徑上。
    let ids: Vec<i64> = rows.iter().map(|e| e.id).collect();
    let open: Vec<(i64, i32, DateTime<Utc>)> = sqlx::query_as(
        "SELECT execution_id, seq, created_at FROM tbl_energy_workflow_task
          WHERE execution_id = ANY($1) AND state = ANY($2) AND NOT is_out_of_band AND deleted_at IS NULL ORDER BY id",
    )
    .bind(&ids)
    .bind(&super::model::ACTIVE_STATES[..])
    .fetch_all(&mut **tx)
    .await?;
    let mut latest: HashMap<i64, (i32, DateTime<Utc>)> = HashMap::new();
    for (e, seq, at) in open {
        if latest.get(&e).is_none_or(|p| seq > p.0) {
            latest.insert(e, (seq, at));
        }
    }
    let starters: Vec<i64> = crate::common::uniq(rows.iter().filter_map(|e| e.started_by_user_id));
    let users = crate::common::user_names(tx, &starters).await?;
    let now = ctx.now();
    Ok(rows
        .iter()
        .map(|e| {
            let subject = subjects.list_summary(e);
            let document_no = subject.get("documentNo").and_then(Value::as_str).map(String::from);
            let def = def_by_id.get(&e.definition_id);
            ExecutionSummary {
                id: e.id,
                display_id: get_display_id(Some(&DisplayIdSource { id: Some(IdLike::Number(e.id)), document_no: document_no.clone() })),
                document_no,
                status: e.status.clone(),
                station_id: e.station_id,
                workflow_key: def.map(|d| d.0.clone()),
                workflow_name: def.map(|d| d.1.clone()),
                definition_id: e.definition_id,
                version: def.map(|d| d.2),
                node_key: e.current_node_key.clone(),
                node_name: e.current_node_key.as_ref().and_then(|k| node_name.get(&(e.definition_id, k.clone())).cloned()),
                waiting_days: latest.get(&e.id).map(|(_, at)| days_since(now, *at)),
                started_at: e.started_at,
                started_by_name: e.started_by_user_id.and_then(|u| users.get(&u).cloned()),
                business_date: crate::common::day(e.business_date),
                subject,
            }
        })
        .collect())
}

/// 主查詢。`has_view_all` 由呼叫端先算好。
pub async fn list(tx: &mut Tx<'_>, ctx: &Ctx, has_view_all: bool, f: &ListFilter) -> ApiResult<Value> {
    let scope = if f.scope.as_deref() == Some("mine") { "mine" } else { "all" };
    // 認不得的 status 回到預設值（全組織問「現在什麼還在跑」，我參與的問「我碰過什麼」）。
    let tab = match f.status.as_deref() {
        Some("completed") => "completed",
        Some("running") => "running",
        Some("all") => "all",
        _ if scope == "mine" => "all",
        _ => "running",
    };
    let empty = || json!({ "tab": tab, "scope": scope, "total": 0, "items": [] });
    let mut q: QueryBuilder<Postgres> = QueryBuilder::new(format!("SELECT {EXEC_COLS} FROM tbl_energy_workflow_execution WHERE organization_id = "));
    q.push_bind(ctx.org_id());
    if let Some(id) = f.execution_id {
        // 指名某一筆時不套 status：那張單的流程可能已經跑完。
        q.push(" AND id = ").push_bind(id);
    } else if tab != "all" {
        // status 有五個值：待中斷核可算進行中、中斷／終止算已完成。
        let set: Vec<&str> = if tab == "running" { vec!["running", "abortPending"] } else { vec!["completed", "aborted", "terminated"] };
        q.push(" AND status = ANY(").push_bind(set.into_iter().map(String::from).collect::<Vec<_>>()).push(")");
    }
    if let Some(k) = f.workflow_key.as_deref().filter(|s| !s.is_empty()) {
        // 涵蓋所有版本（只取現行版會漏掉跑在舊版上的單）。
        let defs: Vec<i64> = sqlx::query_scalar(
            "SELECT id FROM tbl_energy_workflow_definition WHERE organization_id = $1 AND key = $2 AND deleted_at IS NULL ORDER BY id",
        )
        .bind(ctx.org_id())
        .bind(k)
        .fetch_all(&mut **tx)
        .await?;
        if defs.is_empty() {
            return Ok(empty());
        }
        q.push(" AND definition_id = ANY(").push_bind(defs).push(")");
    }
    if let Some(k) = f.node_key.as_deref().filter(|s| !s.is_empty()) {
        q.push(" AND current_node_key = ").push_bind(k.to_string());
    }
    if let Some(no) = f.document_no.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let Some((kind, id)) = find_by_document_no(tx, ctx.org_id(), no).await? else { return Ok(empty()) };
        q.push(" AND subject_type = ").push_bind(kind).push(" AND subject_id = ").push_bind(id.to_string());
    }
    if let Some((a, b)) = f.month.as_deref().and_then(month_range) {
        q.push(" AND business_date >= ").push_bind(a).push(" AND business_date < ").push_bind(b);
    }
    q.push(" AND deleted_at IS NULL ORDER BY id DESC LIMIT 2000");
    let rows: Vec<ExecutionRow> = q.build_query_as().fetch_all(&mut **tx).await?;
    if rows.is_empty() {
        return Ok(empty());
    }
    let mut items = decorate(tx, ctx, &rows).await?;
    if scope == "mine" || !has_view_all {
        let uid = ctx.access.require_user()?.id;
        let ids: Vec<i64> = items.iter().map(|r| r.id).collect();
        let ok = visible_execution_ids(tx, &ids, uid, ctx.today()).await?;
        items.retain(|r| ok.contains(&r.id));
    }
    // 案場有兩個來源：execution 自己的 station_id 與 subject 推導出來的，兩個都算。
    if let Some(sid) = f.station_id {
        items.retain(|r| r.station_id == Some(sid) || r.subject.get("stationId").and_then(Value::as_i64) == Some(sid));
    }
    if let Some(lid) = f.landowner_id {
        items.retain(|r| r.subject.get("landownerId").and_then(Value::as_i64) == Some(lid));
    }
    if let Some(min) = f.waiting_days_min {
        items.retain(|r| r.waiting_days.unwrap_or(0) >= min);
    }
    let sort = f.sort.clone().unwrap_or_else(|| "waitingDays:desc".into());
    let (key, dir) = sort.split_once(':').unwrap_or((sort.as_str(), "desc"));
    let asc = dir == "asc";
    let amount = |r: &ExecutionSummary| -> f64 {
        r.subject.get("amount").and_then(|v| v.as_str().and_then(|s| s.parse().ok()).or(v.as_f64())).unwrap_or(0.0)
    };
    items.sort_by(|a, b| {
        let o = match key {
            "startedAt" => a.started_at.cmp(&b.started_at),
            "amount" => amount(a).partial_cmp(&amount(b)).unwrap_or(std::cmp::Ordering::Equal),
            _ => a.waiting_days.unwrap_or(0).cmp(&b.waiting_days.unwrap_or(0)),
        };
        if asc {
            o
        } else {
            o.reverse()
        }
    });
    let total = items.len();
    let page_size = f.page_size.filter(|n| *n > 0).unwrap_or(50).min(200) as usize;
    let page = f.page.filter(|n| *n > 0).unwrap_or(1) as usize;
    let slice: Vec<ExecutionSummary> = items.into_iter().skip((page - 1) * page_size).take(page_size).collect();
    Ok(json!({ "tab": tab, "scope": scope, "total": total, "page": page, "pageSize": page_size, "items": slice }))
}

/// 單筆摘要（id 或單號）；單號在 subject 上 → 先解成 subject 再找它最新的 execution。不做可視度判定。
pub async fn find_execution(tx: &mut Tx<'_>, ctx: &Ctx, id_or_no: &str) -> ApiResult<Option<ExecutionRow>> {
    if is_db_id(Some(id_or_no)) {
        let id: i64 = id_or_no.trim().parse().map_err(|_| err_d(400, "invalid_path", json!({})))?;
        return Ok(sqlx::query_as(&format!(
            "SELECT {EXEC_COLS} FROM tbl_energy_workflow_execution WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL"
        ))
        .bind(id)
        .bind(ctx.org_id())
        .fetch_optional(&mut **tx)
        .await?);
    }
    let Some((kind, sid)) = find_by_document_no(tx, ctx.org_id(), id_or_no.trim()).await? else { return Ok(None) };
    Ok(sqlx::query_as(&format!(
        "SELECT {EXEC_COLS} FROM tbl_energy_workflow_execution
          WHERE organization_id = $1 AND subject_type = $2 AND subject_id = $3 AND deleted_at IS NULL ORDER BY id DESC LIMIT 1"
    ))
    .bind(ctx.org_id())
    .bind(kind)
    .bind(sid.to_string())
    .fetch_optional(&mut **tx)
    .await?)
}
