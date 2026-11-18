//! task 動作：認領、放回、單一入口 performAction（五種 kind）、回退判定與副作用鎖。

use super::super::model::{self, ActionRow, ExecutionRow, NodeRow, SideEffectRow, TaskRow, SIDE_EFFECT_COLS};
use super::candidates::resolve_rule;
use super::delegation::delegators_for;
use super::run::{apply_next, apply_next_many, resolve_next_nodes, subject_table_name};
use super::{assert_transition, cancel_active_tasks, require_reason, subject_label, CancelCause};
use crate::audit::{self, AuditInput};
use crate::common::{err, err_d, Ctx, Tx};
use chrono::{DateTime, Utc};
use eo_domain::side_effects::side_effect_label;
use eo_domain::workflow_defs::StepRequirements;
use eo_platform::error::ApiResult;
use serde::Serialize;
use serde_json::{json, Map, Value};

/// camelCase → snake_case（`totalAmount` → `total_amount`）：`requirements.fields[].path` 存的是
/// 前端／DTO 用的 camelCase，單據表的 `to_jsonb` 欄位名是 snake_case。
fn to_snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// 這一關要求的狀態：`requirements` 本體，以及每個附件類別／欄位現在滿足了嗎。
pub struct RequirementsStatus {
    pub requirements: StepRequirements,
    /// 附件類別 → 滿足了嗎。
    pub attachments_met: Vec<(String, bool)>,
    /// 欄位 path → 滿足了嗎。
    pub fields_met: Vec<(String, bool)>,
}

impl RequirementsStatus {
    pub fn is_met(&self) -> bool {
        self.attachments_met.iter().all(|(_, met)| *met) && self.fields_met.iter().all(|(_, met)| *met)
    }

    pub fn missing_attachments(&self) -> Vec<String> {
        self.attachments_met.iter().filter(|(_, met)| !met).map(|(c, _)| c.clone()).collect()
    }

    pub fn missing_fields(&self) -> Vec<String> {
        self.fields_met.iter().filter(|(_, met)| !met).map(|(f, _)| f.clone()).collect()
    }
}

/// 算這一關的要求現在滿足了嗎（給 422 擋下用，也給 `node_actions` 的 DTO 用，同一份邏輯只寫一次）。
pub async fn requirements_status(tx: &mut Tx<'_>, exec: &ExecutionRow, node: &NodeRow) -> ApiResult<RequirementsStatus> {
    let requirements: StepRequirements = serde_json::from_value(node.requirements.clone()).unwrap_or_default();
    let mut attachments_met = Vec::new();
    for req in &requirements.attachments {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tbl_energy_attachment
              WHERE execution_id = $1 AND category = $2 AND status = 'active' AND deleted_at IS NULL",
        )
        .bind(exec.id)
        .bind(&req.category)
        .fetch_one(&mut **tx)
        .await?;
        attachments_met.push((req.category.clone(), count >= req.min));
    }
    let mut fields_met = Vec::new();
    if !requirements.fields.is_empty() {
        let row: Value = match subject_table_name(&exec.subject_type).zip(crate::common::numeric_id(&exec.subject_id)) {
            Some((table, id)) => sqlx::query_scalar(&format!(
                "SELECT to_jsonb(t) FROM {table} t WHERE t.id = $1 AND t.organization_id = $2 AND t.deleted_at IS NULL"
            ))
            .bind(id)
            .bind(exec.organization_id)
            .fetch_optional(&mut **tx)
            .await?
            .unwrap_or_else(|| json!({})),
            None => json!({}),
        };
        for f in &requirements.fields {
            let col = to_snake_case(&f.path);
            let has_value = row.get(&col).is_some_and(|v| match v {
                Value::Null => false,
                Value::String(s) => !s.trim().is_empty(),
                _ => true,
            });
            fields_met.push((f.path.clone(), has_value));
        }
    }
    Ok(RequirementsStatus { requirements, attachments_met, fields_met })
}

/// `ready → reserved`。只有候選池裡的 potentialOwner（或他的有效代理人）能認領。
pub async fn claim(tx: &mut Tx<'_>, ctx: &Ctx, task_id: i64) -> ApiResult<()> {
    let me = ctx.access.require_user()?.id;
    let (task, exec) = model::task_in_org(tx, ctx.org_id(), task_id).await?;
    assert_transition(&task.state, "reserved")?;
    let mut on_behalf_of: Option<i64> = None;
    if !model::is_potential_owner(tx, task_id, me).await? {
        // 代理：本人在候選池、我是他的代理人 → 我可以認領，但任務仍記在本人名下。
        let delegators = delegators_for(tx, exec.organization_id, me, ctx.today()).await?;
        if !delegators.is_empty() {
            on_behalf_of = sqlx::query_scalar(
                "SELECT user_id FROM tbl_energy_workflow_task_candidate
                  WHERE task_id = $1 AND user_id = ANY($2) AND kind = 'potentialOwner' AND deleted_at IS NULL ORDER BY id LIMIT 1",
            )
            .bind(task_id)
            .bind(&delegators)
            .fetch_optional(&mut **tx)
            .await?;
        }
        // 權限大不等於是這一關的主責者。
        if on_behalf_of.is_none() {
            return Err(err_d(403, "not_a_candidate", json!({ "taskId": task_id })));
        }
    }
    sqlx::query(
        "UPDATE tbl_energy_workflow_task SET state = 'reserved', assignee_user_id = $2, on_behalf_of_user_id = $3, claimed_at = $4 WHERE id = $1",
    )
    .bind(task_id)
    .bind(me)
    .bind(on_behalf_of)
    .bind(ctx.now())
    .execute(&mut **tx)
    .await?;
    let name = model::node_label(tx, exec.definition_id, &task.node_key).await?;
    audit::record(
        tx,
        ctx,
        AuditInput::new("flow.task_claimed", task_id, format!("認領任務「{name}」"))
            .label(name)
            .payload(json!({ "nodeKey": task.node_key })),
    )
    .await?;
    Ok(())
}

/// `reserved → ready`（放回池子）。只有目前的 assignee 能放回。
pub async fn release(tx: &mut Tx<'_>, ctx: &Ctx, task_id: i64) -> ApiResult<()> {
    let me = ctx.access.require_user()?.id;
    let (task, exec) = model::task_in_org(tx, ctx.org_id(), task_id).await?;
    assert_transition(&task.state, "ready")?;
    if task.assignee_user_id != Some(me) {
        return Err(err_d(403, "not_task_owner", json!({ "taskId": task_id })));
    }
    sqlx::query(
        "UPDATE tbl_energy_workflow_task SET state = 'ready', assignee_user_id = NULL, on_behalf_of_user_id = NULL, claimed_at = NULL WHERE id = $1",
    )
    .bind(task_id)
    .execute(&mut **tx)
    .await?;
    let name = model::node_label(tx, exec.definition_id, &task.node_key).await?;
    audit::record(
        tx,
        ctx,
        AuditInput::new("flow.task_released", task_id, format!("放回任務「{name}」"))
            .label(name)
            .payload(json!({ "nodeKey": task.node_key })),
    )
    .await?;
    Ok(())
}

/// performAction 的輸入（送的是 actionKey ＝ 定義側 action 表的一列，不是 kind）。
#[derive(Debug, Clone, Default)]
pub struct ActionInput {
    pub action_key: String,
    pub reason: Option<String>,
    pub payload: Option<Map<String, Value>>,
    pub note: Option<String>,
    pub target_node_key: Option<String>,
}

/// 誰能按這顆按鈕：`who = {}` → 這一關的 assignee；否則依規則解析（可以與主責者不同）。
async fn assert_can_press(tx: &mut Tx<'_>, action: &ActionRow, task: &TaskRow, exec: &ExecutionRow, me: i64) -> ApiResult<()> {
    let has_rule = action.who.get("type").and_then(Value::as_str).is_some_and(|s| !s.is_empty());
    if !has_rule {
        if task.assignee_user_id != Some(me) {
            return Err(err_d(403, "not_task_owner", json!({ "taskId": task.id })));
        }
        return Ok(());
    }
    let allowed = resolve_rule(tx, exec.organization_id, &action.who).await?;
    if !allowed.contains(&me) {
        return Err(err_d(403, "not_allowed_to_press", json!({ "actionKey": action.action_key })));
    }
    Ok(())
}

/// 所有按鈕都走這裡；引擎依 action.kind 分派。節點上沒有這顆按鈕就是 404。
pub async fn perform_action(tx: &mut Tx<'_>, ctx: &Ctx, task_id: i64, input: ActionInput) -> ApiResult<()> {
    let me = ctx.access.require_user()?.id;
    let (task, exec) = model::task_in_org(tx, ctx.org_id(), task_id).await?;
    if exec.status != "running" {
        return Err(err_d(409, "execution_not_running", json!({ "status": exec.status })));
    }
    let node = model::node_by_key(tx, exec.definition_id, &task.node_key).await?;
    let action = model::action(tx, node.id, &input.action_key).await?;
    assert_can_press(tx, &action, &task, &exec, me).await?;
    // revert／abort 的理由由安裝時強制必填；這裡是後端的最後一道。
    if action.requires_reason && input.reason.as_deref().map(str::trim).unwrap_or("").is_empty() {
        return Err(err_d(400, "reason_required", json!({ "actionKey": input.action_key })));
    }
    match action.kind.as_str() {
        "advance" => do_advance(tx, ctx, &task, &exec, &node, &input, me).await,
        "revert" => do_revert(tx, ctx, &task, &exec, &action, &input).await,
        "setState" => do_set_state(tx, ctx, &task, &exec, &action, &input).await,
        "sideEffect" => do_side_effect(tx, ctx, &task, &exec, &action, &input).await,
        "abort" => super::admin::request_abort_within(tx, ctx, &exec, input.reason.as_deref().unwrap_or("")).await,
        other => Err(err_d(501, "unsupported_action_kind", json!({ "kind": other }))),
    }
}

async fn do_advance(
    tx: &mut Tx<'_>,
    ctx: &Ctx,
    task: &TaskRow,
    exec: &ExecutionRow,
    node: &NodeRow,
    input: &ActionInput,
    me: i64,
) -> ApiResult<()> {
    assert_transition(&task.state, "completed")?;
    // `who` 允許別人按，但完成人工關卡仍要求是 assignee —— 否則「誰做的」沒有答案。
    if task.node_type == "humanTask" && task.assignee_user_id != Some(me) {
        return Err(err_d(403, "not_task_owner", json!({ "taskId": task.id })));
    }
    // 送出前的最後一道：這一關的「必傳附件類別」「必填欄位」沒滿足就不准推進。
    let status = requirements_status(tx, exec, node).await?;
    if !status.is_met() {
        return Err(err_d(
            422,
            "step_requirements_unmet",
            json!({ "missingAttachments": status.missing_attachments(), "missingFields": status.missing_fields() }),
        ));
    }
    let payload = input.payload.clone().map(Value::Object).unwrap_or_else(|| task.payload.clone());
    sqlx::query(
        "UPDATE tbl_energy_workflow_task SET state = 'completed', outcome = 'approved', reason = $2, completed_at = $3, payload = $4 WHERE id = $1",
    )
    .bind(task.id)
    .bind(input.reason.as_deref())
    .bind(ctx.now())
    .bind(&payload)
    .execute(&mut **tx)
    .await?;
    // 先算、寫稽核、再套用（稽核順序：原因在前、結果在後）。
    let next = resolve_next_nodes(tx, exec, node).await?;
    let name = model::node_label(tx, exec.definition_id, &task.node_key).await?;
    let next_keys: Vec<&str> = next.iter().map(|n| n.node_key.as_str()).collect();
    audit::record(
        tx,
        ctx,
        AuditInput::new("flow.task_completed", task.id, format!("完成任務「{name}」"))
            .label(name)
            // 代理核准：actor 是代理人、onBehalfOf 是本人，兩個都記。
            .on_behalf_of(task.on_behalf_of_user_id)
            .payload(json!({
                "nodeKey": task.node_key, "outcome": "approved", "nextNodeKey": next_keys.join(","), "reason": input.reason,
            })),
    )
    .await?;
    // 分支間先搶先贏：這條分支完成 → 其餘仍活著的分支一律 exited。
    if task.branch_key.is_some() {
        cancel_active_tasks(tx, ctx, exec, CancelCause::Join, Some(task.id)).await?;
    }
    apply_next_many(tx, ctx, exec, node, &next).await
}

/// 上一個已完成的人工關卡（previousNode 回退用），忽略 out-of-band 的列。
async fn previous_human_task(tx: &mut Tx<'_>, exec: &ExecutionRow, task: &TaskRow) -> ApiResult<Option<TaskRow>> {
    let rows = model::tasks_of(tx, exec.id).await?;
    Ok(rows.into_iter().rev().find(|t| t.seq < task.seq && !t.is_out_of_band && t.node_type == "humanTask"))
}

/// 回退：先過判定式，不通過就整個不執行（不是部分回退）。
async fn do_revert(tx: &mut Tx<'_>, ctx: &Ctx, task: &TaskRow, exec: &ExecutionRow, action: &ActionRow, input: &ActionInput) -> ApiResult<()> {
    let verdict = can_revert(tx, exec, &task.node_key).await?;
    if !verdict.allowed {
        return Err(err_d(403, "revert_locked", json!({ "reason": verdict.reason, "blockedBy": verdict.blocked_by })));
    }
    let reason = input.reason.clone().unwrap_or_default();
    let target_mode = action.target_mode.clone().unwrap_or_else(|| "previousNode".into());
    if target_mode == "cancelExecution" {
        cancel_active_tasks(tx, ctx, exec, CancelCause::Revert, None).await?;
        sqlx::query("UPDATE tbl_energy_workflow_execution SET status = 'terminated', ended_at = $2 WHERE id = $1")
            .bind(exec.id)
            .bind(ctx.now())
            .execute(&mut **tx)
            .await?;
        audit::record(
            tx,
            ctx,
            AuditInput::new("flow.terminated", exec.id, format!("整個撤銷：{reason}"))
                .label(subject_label(exec))
                .payload(json!({ "reason": reason })),
        )
        .await?;
        return Ok(());
    }
    let target = if target_mode == "explicitNode" {
        let key = action.target_node_key.clone().or_else(|| input.target_node_key.clone()).ok_or_else(|| err(400, "target_node_required"))?;
        let t = model::node_by_key(tx, exec.definition_id, &key).await?;
        // 跳躍預設關閉，節點層級 opt-in。
        if !t.allow_jump_target {
            return Err(err_d(403, "jump_target_not_allowed", json!({ "targetNodeKey": key })));
        }
        t
    } else {
        // 沿 sendBack 邊；沒有時退回上一個已完成的人工關卡。
        let back = model::outgoing(tx, exec.definition_id, &task.node_key, "sendBack").await?;
        if let Some(e) = back.first() {
            model::node_by_key(tx, exec.definition_id, &e.to_node_key).await?
        } else {
            let prev = previous_human_task(tx, exec, task)
                .await?
                .ok_or_else(|| err_d(409, "no_previous_node", json!({ "nodeKey": task.node_key })))?;
            model::node_by_key(tx, exec.definition_id, &prev.node_key).await?
        }
    };
    // 舊 task 留著（append-only）：重走的關卡要重新核准，舊核准紀錄保留。
    sqlx::query(
        "UPDATE tbl_energy_workflow_task SET state = 'completed', outcome = 'sentBack', reason = $2, completed_at = $3 WHERE id = $1",
    )
    .bind(task.id)
    .bind(&reason)
    .bind(ctx.now())
    .execute(&mut **tx)
    .await?;
    let name = model::node_label(tx, exec.definition_id, &task.node_key).await?;
    audit::record(
        tx,
        ctx,
        AuditInput::new("flow.task_sent_back", task.id, format!("退回「{name}」→「{}」：{reason}", target.name))
            .label(name)
            .payload(json!({
                "nodeKey": task.node_key, "targetNodeKey": target.node_key, "targetMode": target_mode,
                "actionKey": action.action_key, "reason": reason,
            })),
    )
    .await?;
    // 並行 × 回退：一併取消所有仍活著的分支，各留一筆稽核。
    cancel_active_tasks(tx, ctx, exec, CancelCause::Revert, Some(task.id)).await?;
    apply_next(tx, ctx, exec, &target, None).await
}

/// setState：改狀態旗標，不移動節點。
async fn do_set_state(tx: &mut Tx<'_>, ctx: &Ctx, task: &TaskRow, exec: &ExecutionRow, action: &ActionRow, input: &ActionInput) -> ApiResult<()> {
    let key = action
        .state_key
        .clone()
        .ok_or_else(|| err_d(500, "state_key_required", json!({ "actionKey": action.action_key })))?;
    let mut payload = if task.payload.is_object() { task.payload.clone() } else { json!({}) };
    let value = input.payload.as_ref().and_then(|p| p.get("value")).cloned().filter(|v| !v.is_null()).unwrap_or(json!(true));
    let mut flags = payload.get("stateFlags").cloned().filter(Value::is_object).unwrap_or_else(|| json!({}));
    flags[&key] = value.clone();
    payload["stateFlags"] = flags;
    sqlx::query("UPDATE tbl_energy_workflow_task SET payload = $2 WHERE id = $1")
        .bind(task.id)
        .bind(&payload)
        .execute(&mut **tx)
        .await?;
    let name = model::node_label(tx, exec.definition_id, &task.node_key).await?;
    audit::record(
        tx,
        ctx,
        AuditInput::new("flow.state_set", task.id, format!("標記「{name}」的 {key}"))
            .label(name)
            .payload(json!({ "nodeKey": task.node_key, "actionKey": action.action_key, "stateKey": key, "value": value })),
    )
    .await?;
    Ok(())
}

/// sideEffect：標記副作用已發生（例「已付款」），不移動節點；鎖回退的旗標快照自 action。
async fn do_side_effect(tx: &mut Tx<'_>, ctx: &Ctx, task: &TaskRow, exec: &ExecutionRow, action: &ActionRow, input: &ActionInput) -> ApiResult<()> {
    let key = action
        .side_effect_key
        .clone()
        .ok_or_else(|| err_d(500, "side_effect_key_required", json!({ "actionKey": action.action_key })))?;
    // 同一張單上已有未解除的同一種副作用就不再記一次（連按兩下「已付款」不可變成兩筆付款）。
    // App 5 沒擋。解除（release）之後才能再標一次。
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM tbl_energy_workflow_side_effect
          WHERE execution_id = $1 AND side_effect_key = $2 AND released_at IS NULL AND deleted_at IS NULL
          FOR UPDATE",
    )
    .bind(exec.id)
    .bind(&key)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(prev) = existing {
        return Err(err_d(409, "side_effect_already_applied", json!({ "sideEffectKey": key, "sideEffectId": prev })));
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO tbl_energy_workflow_side_effect
           (execution_id, task_id, side_effect_key, locks_revert, applied_by_user_id, applied_at, note)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(exec.id)
    .bind(task.id)
    .bind(&key)
    .bind(action.locks_revert)
    .bind(ctx.user_id())
    .bind(ctx.now())
    .bind(input.note.as_deref())
    .fetch_one(&mut **tx)
    .await?;
    let label = side_effect_label(Some(&key));
    let note = input.note.as_deref().map(|n| format!("（{n}）")).unwrap_or_default();
    audit::record(
        tx,
        ctx,
        AuditInput::new("flow.side_effect_applied", id, format!("標記副作用「{label}」已發生{note}"))
            .label(label)
            .payload(json!({
                "sideEffectKey": key, "executionId": exec.id, "nodeKey": task.node_key,
                "locksRevert": action.locks_revert, "note": input.note,
            })),
    )
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockingSideEffect {
    pub id: i64,
    pub side_effect_key: String,
    pub applied_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevertVerdict {
    pub allowed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<Vec<BlockingSideEffect>>,
}

/// 可以回退 ⇔ 節點 revertable ∧ execution 上沒有未解除、鎖回退的副作用。不通過時說得出是哪幾筆擋住。
pub async fn can_revert(tx: &mut Tx<'_>, exec: &ExecutionRow, node_key: &str) -> ApiResult<RevertVerdict> {
    let node = model::node_by_key(tx, exec.definition_id, node_key).await?;
    if !node.revertable {
        return Ok(RevertVerdict { allowed: false, reason: Some("node_not_revertable"), blocked_by: None });
    }
    let locks: Vec<SideEffectRow> = sqlx::query_as(&format!(
        "SELECT {SIDE_EFFECT_COLS} FROM tbl_energy_workflow_side_effect
          WHERE execution_id = $1 AND locks_revert AND released_at IS NULL AND deleted_at IS NULL ORDER BY id"
    ))
    .bind(exec.id)
    .fetch_all(&mut **tx)
    .await?;
    if !locks.is_empty() {
        return Ok(RevertVerdict {
            allowed: false,
            reason: Some("side_effect_lock"),
            blocked_by: Some(
                locks
                    .into_iter()
                    .map(|l| BlockingSideEffect { id: l.id, side_effect_key: l.side_effect_key, applied_at: l.applied_at })
                    .collect(),
            ),
        });
    }
    Ok(RevertVerdict { allowed: true, reason: None, blocked_by: None })
}

/// 解除副作用鎖 —— 刻意保留的逃生門（沒有銀行串接可以自動查證「其實沒匯出去」）。不刪原列。
pub async fn release_side_effect(tx: &mut Tx<'_>, ctx: &Ctx, side_effect_id: i64, reason: &str) -> ApiResult<()> {
    let reason = require_reason(Some(reason))?;
    let row: Option<SideEffectRow> = sqlx::query_as(&format!(
        "SELECT {SIDE_EFFECT_COLS} FROM tbl_energy_workflow_side_effect WHERE id = $1 AND deleted_at IS NULL"
    ))
    .bind(side_effect_id)
    .fetch_optional(&mut **tx)
    .await?;
    let row = row.ok_or_else(|| err(404, "side_effect_not_found"))?;
    // 副作用的組織一律從它的 execution 推導。
    let exec = model::execution_in_org(tx, ctx.org_id(), row.execution_id).await?;
    if row.released_at.is_some() {
        return Err(err(409, "side_effect_already_released"));
    }
    sqlx::query(
        "UPDATE tbl_energy_workflow_side_effect SET released_by_user_id = $2, released_at = $3, release_reason = $4 WHERE id = $1",
    )
    .bind(side_effect_id)
    .bind(ctx.user_id())
    .bind(ctx.now())
    .bind(&reason)
    .execute(&mut **tx)
    .await?;
    let label = side_effect_label(Some(&row.side_effect_key));
    audit::record(
        tx,
        ctx,
        AuditInput::new("flow.side_effect_released", side_effect_id, format!("解除回退鎖「{label}」：{reason}"))
            .label(label)
            .payload(json!({ "sideEffectKey": row.side_effect_key, "executionId": exec.id, "reason": reason })),
    )
    .await?;
    Ok(())
}
