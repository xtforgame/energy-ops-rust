//! 流程圖（定義圖與執行圖共用同一份邊的推導）與流程總覽（定義＋即時負載）。

use super::model::{self, ActionRow, DefinitionRow, ExecutionRow, NodeRow, TransitionRow, DEF_COLS};
use crate::common::{err, Ctx, Tx};
use chrono::{DateTime, NaiveDate, Utc};
use eo_platform::error::ApiResult;
use serde_json::{json, Value};
use std::collections::HashMap;

/// 卡關門檻的預設值 —— 參數，不是寫死在 schema 裡的東西。
pub const DEFAULT_STUCK_DAYS: i64 = 3;

fn layout_of(n: &NodeRow) -> Value {
    match (n.layout.get("x").and_then(Value::as_f64), n.layout.get("y").and_then(Value::as_f64)) {
        (Some(_), Some(_)) => json!({ "x": n.layout["x"], "y": n.layout["y"] }),
        // 沒有 layout 就回 null，讓前端自動排版；後端硬排前端就蓋不掉了。
        _ => Value::Null,
    }
}

/// 圖上的所有邊。退回路徑是推導出來的（revert 動作），不只是 transition 表裡的 sendBack 列 ——
/// 只讀 transition 表的話圖上完全看不到退回路徑，而且不會有任何錯誤。
pub fn derive_edges(nodes: &[NodeRow], edges: &[TransitionRow], actions: &[ActionRow]) -> Vec<Value> {
    let key_of: HashMap<i64, &str> = nodes.iter().map(|n| (n.id, n.node_key.as_str())).collect();
    let mut predecessors: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in edges.iter().filter(|e| e.kind != "sendBack") {
        predecessors.entry(e.to_node_key.as_str()).or_default().push(e.from_node_key.as_str());
    }
    let mut out: Vec<Value> = edges
        .iter()
        .map(|e| {
            let has_condition = e.condition.as_object().is_some_and(|m| !m.is_empty());
            json!({ "from": e.from_node_key, "to": e.to_node_key, "kind": e.kind, "label": e.label,
                    "labelKey": null, "hasCondition": has_condition, "implied": false })
        })
        .collect();
    for a in actions.iter().filter(|a| a.kind == "revert") {
        let Some(from) = key_of.get(&a.node_id).copied() else { continue };
        let targets: Vec<&str> = if a.target_mode.as_deref() == Some("previousNode") {
            predecessors.get(from).cloned().unwrap_or_default()
        } else {
            a.target_node_key.as_deref().into_iter().collect()
        };
        for to in targets {
            out.push(json!({ "from": from, "to": to, "kind": "sendBack", "labelKey": a.label_key, "label": null,
                             "hasCondition": false, "implied": true }));
        }
    }
    out
}

/// 定義本身的流程圖（沒有 execution 也看得到）；`with_load` 時每個節點帶「積了幾張／幾張卡關」。
pub async fn definition_graph(tx: &mut Tx<'_>, ctx: &Ctx, definition_id: i64, with_load: bool, stuck_days: i64) -> ApiResult<Value> {
    let def: Option<DefinitionRow> =
        sqlx::query_as(&format!("SELECT {DEF_COLS} FROM tbl_energy_workflow_definition WHERE id = $1 AND deleted_at IS NULL"))
            .bind(definition_id)
            .fetch_optional(&mut **tx)
            .await?;
    let def = def.ok_or_else(|| err(404, "definition_not_found"))?;
    // 跨組織要擋 403 不是 404 —— 404 會洩漏「這個 id 存在」與否的差別。
    if def.organization_id != ctx.org_id() {
        return Err(err(403, "definition_not_in_org"));
    }
    let nodes = model::nodes(tx, definition_id).await?;
    let edges = model::transitions(tx, definition_id).await?;
    let actions = model::actions_of_definition(tx, definition_id).await?;
    let load = if with_load { node_load(tx, ctx, &[definition_id], stuck_days).await? } else { HashMap::new() };
    Ok(json!({
        "definitionId": def.id, "key": def.key, "name": def.name, "version": def.version, "status": def.status,
        "stuckDays": stuck_days,
        "nodes": nodes.iter().map(|n| {
            let (waiting, stuck) = load.get(&(def.id, n.node_key.clone())).copied().unwrap_or((0, 0));
            json!({ "nodeKey": n.node_key, "name": n.name, "nodeType": n.node_type, "layout": layout_of(n),
                    "serviceKey": n.service_key, "state": "definition", "waiting": waiting, "stuck": stuck })
        }).collect::<Vec<_>>(),
        "edges": derive_edges(&nodes, &edges, &actions),
    }))
}

/// (定義, 節點) → (等待中, 其中卡關)。只算 running 的執行。
async fn node_load(tx: &mut Tx<'_>, ctx: &Ctx, def_ids: &[i64], stuck_days: i64) -> ApiResult<HashMap<(i64, String), (i64, i64)>> {
    let rows: Vec<(i64, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT e.definition_id, t.node_key, t.created_at FROM tbl_energy_workflow_task t
           JOIN tbl_energy_workflow_execution e ON e.id = t.execution_id
          WHERE e.organization_id = $1 AND e.definition_id = ANY($2) AND e.status = 'running' AND e.deleted_at IS NULL
            AND t.state = ANY($3) AND t.deleted_at IS NULL ORDER BY t.id",
    )
    .bind(ctx.org_id())
    .bind(def_ids)
    .bind(&model::ACTIVE_STATES[..])
    .fetch_all(&mut **tx)
    .await?;
    let now = ctx.now();
    let mut out: HashMap<(i64, String), (i64, i64)> = HashMap::new();
    for (d, k, at) in rows {
        let e = out.entry((d, k)).or_default();
        e.0 += 1;
        if now - at > chrono::Duration::days(stuck_days) {
            e.1 += 1;
        }
    }
    Ok(out)
}

/// 執行圖：節點狀態由 task 推導（同一關走過多次取序號最大那筆），`current` 是陣列（並行時多個）。
pub async fn execution_graph(tx: &mut Tx<'_>, exec: &ExecutionRow) -> ApiResult<Value> {
    let nodes = model::nodes(tx, exec.definition_id).await?;
    let edges = model::transitions(tx, exec.definition_id).await?;
    let actions = model::actions_of_definition(tx, exec.definition_id).await?;
    let tasks = model::tasks_of(tx, exec.id).await?;
    let mut current = Vec::new();
    let nodes_out: Vec<Value> = nodes
        .iter()
        .map(|n| {
            let latest = tasks.iter().filter(|t| t.node_key == n.node_key).max_by_key(|t| t.seq);
            let state = match latest {
                None => "pending",
                Some(t) if t.state == "completed" => "completed",
                Some(t) if t.is_active() => {
                    current.push(n.node_key.clone());
                    "active"
                }
                // exited（輸掉 join、中斷、強制推進）與 failed 都不是走完。
                Some(_) => "cancelled",
            };
            json!({ "nodeKey": n.node_key, "name": n.name, "nodeType": n.node_type, "layout": layout_of(n),
                    "state": state, "serviceKey": n.service_key })
        })
        .collect();
    Ok(json!({
        "executionId": exec.id, "status": exec.status, "nodes": nodes_out,
        "edges": derive_edges(&nodes, &edges, &actions), "current": current,
    }))
}

/// 流程總覽：每份定義的版本譜系、綁定、執行中件數，以及每個節點的負載。
pub async fn overview(tx: &mut Tx<'_>, ctx: &Ctx, stuck_days: i64) -> ApiResult<Value> {
    let defs: Vec<DefinitionRow> = sqlx::query_as(&format!(
        "SELECT {DEF_COLS} FROM tbl_energy_workflow_definition WHERE organization_id = $1 AND deleted_at IS NULL ORDER BY key ASC, version DESC"
    ))
    .bind(ctx.org_id())
    .fetch_all(&mut **tx)
    .await?;
    if defs.is_empty() {
        return Ok(json!({ "definitions": [], "stuckDays": stuck_days }));
    }
    let ids: Vec<i64> = defs.iter().map(|d| d.id).collect();
    let nodes: Vec<NodeRow> = sqlx::query_as(&format!(
        "SELECT {} FROM tbl_energy_workflow_node WHERE definition_id = ANY($1) AND deleted_at IS NULL ORDER BY id",
        model::NODE_COLS
    ))
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    let running: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT definition_id, COUNT(*) FROM tbl_energy_workflow_execution
          WHERE organization_id = $1 AND definition_id = ANY($2) AND status = 'running' AND deleted_at IS NULL GROUP BY definition_id",
    )
    .bind(ctx.org_id())
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    let running: HashMap<i64, i64> = running.into_iter().collect();
    let bindings: Vec<(i64, i64, Option<String>, Option<i64>, Option<i64>, Option<String>, Option<String>, NaiveDate, Option<NaiveDate>)> =
        sqlx::query_as(
            "SELECT b.id, b.definition_id, b.station_key, b.station_id, b.contract_id, c.contract_no, b.doc_type, b.valid_from, b.valid_to
               FROM tbl_energy_workflow_binding b
               LEFT JOIN tbl_energy_contract c ON c.id = b.contract_id AND c.deleted_at IS NULL
              WHERE b.organization_id = $1 AND b.definition_id = ANY($2) AND b.deleted_at IS NULL ORDER BY b.id",
        )
        .bind(ctx.org_id())
        .bind(&ids)
        .fetch_all(&mut **tx)
        .await?;
    let load = node_load(tx, ctx, &ids, stuck_days).await?;
    let version_of: HashMap<i64, i32> = defs.iter().map(|d| (d.id, d.version)).collect();
    let out: Vec<Value> = defs
        .iter()
        .map(|d| {
            json!({
                "id": d.id, "key": d.key, "name": d.name, "version": d.version,
                // 譜系回「版號」不是 definition id。
                "parentVersion": d.parent_version_id.and_then(|p| version_of.get(&p).copied()),
                "status": d.status, "subjectType": d.subject_type,
                "runningCount": running.get(&d.id).copied().unwrap_or(0),
                "bindings": bindings.iter().filter(|b| b.1 == d.id).map(|b| json!({
                    "id": b.0, "stationKey": b.2, "stationId": b.3, "contractId": b.4, "contractNo": b.5, "docType": b.6,
                    "validFrom": crate::common::day(b.7), "validTo": crate::common::day_opt(b.8),
                })).collect::<Vec<_>>(),
                "nodes": nodes.iter().filter(|n| n.definition_id == d.id).map(|n| {
                    let (waiting, stuck) = load.get(&(d.id, n.node_key.clone())).copied().unwrap_or((0, 0));
                    json!({ "nodeKey": n.node_key, "name": n.name, "nodeType": n.node_type, "waiting": waiting, "stuck": stuck })
                }).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({ "stuckDays": stuck_days, "definitions": out }))
}
