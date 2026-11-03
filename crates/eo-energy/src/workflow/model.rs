//! 流程表的列型別與基本查詢。所有查詢都帶 `deleted_at IS NULL`；順序不明確的一律 id 升序。

use crate::common::{err, err_d, Tx};
use chrono::{DateTime, NaiveDate, Utc};
use eo_domain::conditions::{Condition, ConditionalEdge};
use eo_platform::error::ApiResult;
use serde_json::{json, Value};
use sqlx::FromRow;

/// 活躍中的 task 狀態（會被回退／join／中斷取消的那些）。
pub const ACTIVE_STATES: [&str; 3] = ["ready", "reserved", "inProgress"];

#[derive(Debug, Clone, FromRow)]
pub struct DefinitionRow {
    pub id: i64,
    pub key: String,
    pub version: i32,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub published_at: Option<DateTime<Utc>>,
    pub subject_type: String,
    pub abort_enabled: bool,
    pub abort_approval_rule: Value,
    pub organization_id: i64,
    pub parent_version_id: Option<i64>,
}

pub const DEF_COLS: &str = "id, key, version, name, description, status, published_at, subject_type, abort_enabled,
    abort_approval_rule, organization_id, parent_version_id";

#[derive(Debug, Clone, FromRow)]
pub struct NodeRow {
    pub id: i64,
    pub node_key: String,
    pub name: String,
    pub allows_subject_edit: bool,
    pub node_type: String,
    pub assignment_rule: Value,
    pub allow_self_approval: bool,
    pub revertable: bool,
    pub allow_jump_target: bool,
    pub parallel: bool,
    pub join_policy: Option<String>,
    pub join_node_key: Option<String>,
    pub service_key: Option<String>,
    pub layout: Value,
    pub definition_id: i64,
}

pub const NODE_COLS: &str = "id, node_key, name, allows_subject_edit, node_type, assignment_rule, allow_self_approval, revertable,
    allow_jump_target, parallel, join_policy, join_node_key, service_key, layout, definition_id";

#[derive(Debug, Clone, FromRow)]
pub struct TransitionRow {
    pub id: i64,
    pub from_node_key: String,
    pub to_node_key: String,
    pub kind: String,
    pub label: Option<String>,
    pub condition: Value,
    pub requires_reason: bool,
    pub sort_order: i32,
    #[sqlx(skip)]
    pub parsed: Condition,
}

pub const TRANSITION_COLS: &str = "id, from_node_key, to_node_key, kind, label, condition, requires_reason, sort_order";

impl ConditionalEdge for TransitionRow {
    fn condition(&self) -> Option<&Condition> {
        Some(&self.parsed)
    }
    fn sort_order(&self) -> i64 {
        self.sort_order as i64
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct ActionRow {
    pub id: i64,
    pub action_key: String,
    pub label_key: String,
    pub kind: String,
    pub target_mode: Option<String>,
    pub target_node_key: Option<String>,
    pub who: Value,
    pub requires_reason: bool,
    pub locks_revert: bool,
    pub side_effect_key: Option<String>,
    pub state_key: Option<String>,
    pub sort_order: i32,
    pub node_id: i64,
}

pub const ACTION_COLS: &str = "id, action_key, label_key, kind, target_mode, target_node_key, who, requires_reason, locks_revert,
    side_effect_key, state_key, sort_order, node_id";

#[derive(Debug, Clone, FromRow)]
pub struct ExecutionRow {
    pub id: i64,
    pub workflow_key: Option<String>,
    pub station_key: String,
    pub subject_type: String,
    pub subject_id: String,
    pub business_date: NaiveDate,
    pub status: String,
    pub current_node_key: Option<String>,
    pub abort_reason: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub organization_id: i64,
    pub definition_id: i64,
    pub station_id: Option<i64>,
    pub started_by_user_id: Option<i64>,
    pub data: Value,
}

pub const EXEC_COLS: &str = "id, workflow_key, station_key, subject_type, subject_id, business_date, status, current_node_key,
    abort_reason, started_at, ended_at, organization_id, definition_id, station_id, started_by_user_id, data";

impl ExecutionRow {
    /// 分支條件求值用的屬性快照。
    pub fn attributes(&self) -> Value {
        self.data.get("attributes").cloned().unwrap_or_else(|| json!({}))
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct TaskRow {
    pub id: i64,
    pub node_key: String,
    pub node_type: String,
    pub seq: i32,
    pub branch_key: Option<String>,
    pub is_out_of_band: bool,
    pub state: String,
    pub outcome: Option<String>,
    pub reason: Option<String>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub payload: Value,
    pub execution_id: i64,
    pub assignee_user_id: Option<i64>,
    pub on_behalf_of_user_id: Option<i64>,
    pub created_at: DateTime<Utc>,
}

pub const TASK_COLS: &str = "id, node_key, node_type, seq, branch_key, is_out_of_band, state, outcome, reason, claimed_at,
    completed_at, payload, execution_id, assignee_user_id, on_behalf_of_user_id, created_at";

impl TaskRow {
    pub fn is_active(&self) -> bool {
        ACTIVE_STATES.contains(&self.state.as_str())
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct SideEffectRow {
    pub id: i64,
    pub side_effect_key: String,
    pub locks_revert: bool,
    pub applied_at: DateTime<Utc>,
    pub note: Option<String>,
    pub released_at: Option<DateTime<Utc>>,
    pub release_reason: Option<String>,
    pub execution_id: i64,
    pub task_id: Option<i64>,
    pub applied_by_user_id: Option<i64>,
}

pub const SIDE_EFFECT_COLS: &str =
    "id, side_effect_key, locks_revert, applied_at, note, released_at, release_reason, execution_id, task_id, applied_by_user_id";

pub async fn definition_by_id(tx: &mut Tx<'_>, org_id: i64, id: i64) -> ApiResult<Option<DefinitionRow>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {DEF_COLS} FROM tbl_energy_workflow_definition WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL"
    ))
    .bind(id)
    .bind(org_id)
    .fetch_optional(&mut **tx)
    .await?)
}

pub async fn nodes(tx: &mut Tx<'_>, definition_id: i64) -> ApiResult<Vec<NodeRow>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {NODE_COLS} FROM tbl_energy_workflow_node WHERE definition_id = $1 AND deleted_at IS NULL ORDER BY id"
    ))
    .bind(definition_id)
    .fetch_all(&mut **tx)
    .await?)
}

pub async fn node_by_key_opt(tx: &mut Tx<'_>, definition_id: i64, node_key: &str) -> ApiResult<Option<NodeRow>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {NODE_COLS} FROM tbl_energy_workflow_node
          WHERE definition_id = $1 AND node_key = $2 AND deleted_at IS NULL ORDER BY id LIMIT 1"
    ))
    .bind(definition_id)
    .bind(node_key)
    .fetch_optional(&mut **tx)
    .await?)
}

pub async fn node_by_key(tx: &mut Tx<'_>, definition_id: i64, node_key: &str) -> ApiResult<NodeRow> {
    node_by_key_opt(tx, definition_id, node_key).await?.ok_or_else(|| err_d(500, "node_not_found", json!({ "nodeKey": node_key })))
}

/// 這一關當時叫什麼（稽核的 targetLabel 快照）；查不到才退回 key。
pub async fn node_label(tx: &mut Tx<'_>, definition_id: i64, node_key: &str) -> ApiResult<String> {
    if node_key.is_empty() {
        return Ok(String::new());
    }
    Ok(node_by_key_opt(tx, definition_id, node_key).await?.map(|n| n.name).unwrap_or_else(|| node_key.to_string()))
}

pub async fn transitions(tx: &mut Tx<'_>, definition_id: i64) -> ApiResult<Vec<TransitionRow>> {
    let mut rows: Vec<TransitionRow> = sqlx::query_as(&format!(
        "SELECT {TRANSITION_COLS} FROM tbl_energy_workflow_transition
          WHERE definition_id = $1 AND deleted_at IS NULL ORDER BY sort_order ASC, id ASC"
    ))
    .bind(definition_id)
    .fetch_all(&mut **tx)
    .await?;
    for r in &mut rows {
        r.parsed = Condition::from(r.condition.clone());
    }
    Ok(rows)
}

pub async fn outgoing(tx: &mut Tx<'_>, definition_id: i64, from: &str, kind: &str) -> ApiResult<Vec<TransitionRow>> {
    let mut rows: Vec<TransitionRow> = sqlx::query_as(&format!(
        "SELECT {TRANSITION_COLS} FROM tbl_energy_workflow_transition
          WHERE definition_id = $1 AND from_node_key = $2 AND kind = $3 AND deleted_at IS NULL ORDER BY id"
    ))
    .bind(definition_id)
    .bind(from)
    .bind(kind)
    .fetch_all(&mut **tx)
    .await?;
    for r in &mut rows {
        r.parsed = Condition::from(r.condition.clone());
    }
    Ok(rows)
}

pub async fn actions_of_node(tx: &mut Tx<'_>, node_id: i64) -> ApiResult<Vec<ActionRow>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {ACTION_COLS} FROM tbl_energy_workflow_action WHERE node_id = $1 AND deleted_at IS NULL ORDER BY sort_order ASC, id ASC"
    ))
    .bind(node_id)
    .fetch_all(&mut **tx)
    .await?)
}

pub async fn actions_of_definition(tx: &mut Tx<'_>, definition_id: i64) -> ApiResult<Vec<ActionRow>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {ACTION_COLS} FROM tbl_energy_workflow_action WHERE definition_id = $1 AND deleted_at IS NULL ORDER BY id"
    ))
    .bind(definition_id)
    .fetch_all(&mut **tx)
    .await?)
}

pub async fn action(tx: &mut Tx<'_>, node_id: i64, action_key: &str) -> ApiResult<ActionRow> {
    let row: Option<ActionRow> = sqlx::query_as(&format!(
        "SELECT {ACTION_COLS} FROM tbl_energy_workflow_action WHERE node_id = $1 AND action_key = $2 AND deleted_at IS NULL ORDER BY id LIMIT 1"
    ))
    .bind(node_id)
    .bind(action_key)
    .fetch_optional(&mut **tx)
    .await?;
    row.ok_or_else(|| err_d(404, "action_not_found", json!({ "actionKey": action_key })))
}

/// 本組織的 execution；不存在 404、別的組織 403（org 由 server 從 execution 推導）。
pub async fn execution_in_org(tx: &mut Tx<'_>, org_id: i64, id: i64) -> ApiResult<ExecutionRow> {
    let row: Option<ExecutionRow> =
        sqlx::query_as(&format!("SELECT {EXEC_COLS} FROM tbl_energy_workflow_execution WHERE id = $1 AND deleted_at IS NULL"))
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?;
    let row = row.ok_or_else(|| err(404, "execution_not_found"))?;
    if row.organization_id != org_id {
        return Err(err(403, "execution_not_in_org"));
    }
    Ok(row)
}

/// 鎖住 execution 列（同一筆流程的並發動作排隊）。
pub async fn execution_for_update(tx: &mut Tx<'_>, org_id: i64, id: i64) -> ApiResult<ExecutionRow> {
    let row: Option<ExecutionRow> = sqlx::query_as(&format!(
        "SELECT {EXEC_COLS} FROM tbl_energy_workflow_execution WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL FOR UPDATE"
    ))
    .bind(id)
    .bind(org_id)
    .fetch_optional(&mut **tx)
    .await?;
    row.ok_or_else(|| err(404, "execution_not_found"))
}

pub async fn task_by_id(tx: &mut Tx<'_>, id: i64) -> ApiResult<Option<TaskRow>> {
    Ok(sqlx::query_as(&format!("SELECT {TASK_COLS} FROM tbl_energy_workflow_task WHERE id = $1 AND deleted_at IS NULL"))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?)
}

/// task 的組織一律從它的 execution 推導；回傳 (task, execution) 並鎖住 execution。
pub async fn task_in_org(tx: &mut Tx<'_>, org_id: i64, task_id: i64) -> ApiResult<(TaskRow, ExecutionRow)> {
    let task = task_by_id(tx, task_id).await?.ok_or_else(|| err(404, "task_not_found"))?;
    execution_in_org(tx, org_id, task.execution_id).await?;
    let exec = execution_for_update(tx, org_id, task.execution_id).await?;
    // 鎖到之後重讀 task，避免拿到並發動作之前的狀態。
    let task = task_by_id(tx, task_id).await?.ok_or_else(|| err(404, "task_not_found"))?;
    Ok((task, exec))
}

pub async fn tasks_of(tx: &mut Tx<'_>, execution_id: i64) -> ApiResult<Vec<TaskRow>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {TASK_COLS} FROM tbl_energy_workflow_task WHERE execution_id = $1 AND deleted_at IS NULL ORDER BY seq ASC, id ASC"
    ))
    .bind(execution_id)
    .fetch_all(&mut **tx)
    .await?)
}

pub async fn next_seq(tx: &mut Tx<'_>, execution_id: i64) -> ApiResult<i32> {
    let max: Option<i32> =
        sqlx::query_scalar("SELECT MAX(seq) FROM tbl_energy_workflow_task WHERE execution_id = $1 AND deleted_at IS NULL")
            .bind(execution_id)
            .fetch_one(&mut **tx)
            .await?;
    Ok(max.unwrap_or(0) + 1)
}

/// 這個 task 上某人的候選列 kind（potentialOwner／excludedOwner）。
pub async fn candidate_kinds(tx: &mut Tx<'_>, task_id: i64, user_id: i64) -> ApiResult<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT kind FROM tbl_energy_workflow_task_candidate WHERE task_id = $1 AND user_id = $2 AND deleted_at IS NULL ORDER BY id",
    )
    .bind(task_id)
    .bind(user_id)
    .fetch_all(&mut **tx)
    .await?)
}

pub async fn is_potential_owner(tx: &mut Tx<'_>, task_id: i64, user_id: i64) -> ApiResult<bool> {
    Ok(candidate_kinds(tx, task_id, user_id).await?.iter().any(|k| k == "potentialOwner"))
}

pub async fn insert_candidate(tx: &mut Tx<'_>, task_id: i64, user_id: i64, kind: &str, resolved_by: &str) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO tbl_energy_workflow_task_candidate (task_id, user_id, kind, resolved_by) VALUES ($1, $2, $3, $4)",
    )
    .bind(task_id)
    .bind(user_id)
    .bind(kind)
    .bind(resolved_by)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
