//! 旁路與管理動作：中斷（申請／裁定）、轉移、強制推進、終止。

use super::super::model::{self, ExecutionRow, TaskRow, TASK_COLS};
use super::candidates::resolve_rule;
use super::run::apply_next;
use super::{cancel_active_tasks, require_reason, subject_label, CancelCause};
use crate::audit::{self, AuditInput};
use crate::common::{err, err_d, Ctx, Tx};
use eo_platform::error::ApiResult;
use serde_json::{json, Value};

/// 申請中斷：任何節點、任何狀態都能觸發。產生一列 out-of-band 的 abortApproval task；
/// 主流程的 task 不動，所以駁回時「回到原狀態」天然成立。
pub async fn request_abort(tx: &mut Tx<'_>, ctx: &Ctx, execution_id: i64, reason: Option<&str>) -> ApiResult<()> {
    let reason = require_reason(reason)?;
    model::execution_in_org(tx, ctx.org_id(), execution_id).await?;
    let exec = model::execution_for_update(tx, ctx.org_id(), execution_id).await?;
    request_abort_within(tx, ctx, &exec, &reason).await
}

pub async fn request_abort_within(tx: &mut Tx<'_>, ctx: &Ctx, exec: &ExecutionRow, reason: &str) -> ApiResult<()> {
    if exec.status != "running" {
        return Err(err_d(409, "execution_not_running", json!({ "status": exec.status })));
    }
    let def = model::definition_by_id(tx, exec.organization_id, exec.definition_id).await?.ok_or_else(|| err(500, "definition_not_found"))?;
    if !def.abort_enabled {
        return Err(err(409, "abort_not_enabled"));
    }
    let seq = model::next_seq(tx, exec.id).await?;
    let task: TaskRow = sqlx::query_as(&format!(
        "INSERT INTO tbl_energy_workflow_task (execution_id, node_key, node_type, seq, is_out_of_band, state, payload, created_at)
         VALUES ($1, '__abortApproval__', 'abortApproval', $2, true, 'ready', $3, $4) RETURNING {TASK_COLS}"
    ))
    .bind(exec.id)
    .bind(seq)
    .bind(json!({ "requestReason": reason }))
    .bind(ctx.now())
    .fetch_one(&mut **tx)
    .await?;
    // 核可者＝最高負責人（admin ∪ CXO），定義可覆寫。
    let has_rule = def.abort_approval_rule.get("type").and_then(Value::as_str).is_some_and(|s| !s.is_empty());
    let rule = if has_rule {
        def.abort_approval_rule.clone()
    } else {
        json!({ "type": "role", "params": { "roleUniqueNames": ["energy:admin", "energy:cxo"] } })
    };
    for u in resolve_rule(tx, exec.organization_id, &rule).await? {
        model::insert_candidate(tx, task.id, u, "potentialOwner", "abortApprovalRule").await?;
    }
    sqlx::query(
        "UPDATE tbl_energy_workflow_execution SET status = 'abortPending', abort_requested_by_user_id = $2, abort_reason = $3 WHERE id = $1",
    )
    .bind(exec.id)
    .bind(ctx.user_id())
    .bind(reason)
    .execute(&mut **tx)
    .await?;
    audit::record(
        tx,
        ctx,
        AuditInput::new("flow.abort_requested", exec.id, format!("申請中斷流程：{reason}"))
            .label(subject_label(exec))
            .payload(json!({ "reason": reason, "approvalTaskId": task.id })),
    )
    .await?;
    Ok(())
}

/// 裁定中斷。核可 → aborted（不解除副作用，記下仍開著的）；駁回 → 回到 running。
pub async fn decide_abort(tx: &mut Tx<'_>, ctx: &Ctx, task_id: i64, approved: bool, reason: Option<&str>) -> ApiResult<()> {
    let me = ctx.access.require_user()?.id;
    let (task, exec) = model::task_in_org(tx, ctx.org_id(), task_id).await?;
    if task.node_type != "abortApproval" {
        return Err(err(400, "not_an_abort_approval_task"));
    }
    if task.state == "completed" {
        return Err(err(409, "abort_already_decided"));
    }
    if !model::is_potential_owner(tx, task_id, me).await? {
        return Err(err_d(403, "not_a_candidate", json!({ "taskId": task_id })));
    }
    let request_reason = task.payload.get("requestReason").and_then(Value::as_str).unwrap_or("").to_string();
    let now = ctx.now();
    sqlx::query(
        "UPDATE tbl_energy_workflow_task SET state = 'completed', outcome = $2, assignee_user_id = $3, reason = $4, completed_at = $5 WHERE id = $1",
    )
    .bind(task_id)
    .bind(if approved { "approved" } else { "rejected" })
    .bind(me)
    .bind(reason)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    if approved {
        let open: Vec<String> = sqlx::query_scalar(
            "SELECT side_effect_key FROM tbl_energy_workflow_side_effect
              WHERE execution_id = $1 AND released_at IS NULL AND deleted_at IS NULL ORDER BY id",
        )
        .bind(exec.id)
        .fetch_all(&mut **tx)
        .await?;
        cancel_active_tasks(tx, ctx, &exec, CancelCause::Abort, Some(task_id)).await?;
        sqlx::query("UPDATE tbl_energy_workflow_execution SET status = 'aborted', ended_at = $2 WHERE id = $1")
            .bind(exec.id)
            .bind(now)
            .execute(&mut **tx)
            .await?;
        audit::record(
            tx,
            ctx,
            AuditInput::new("flow.abort_approved", exec.id, format!("核可中斷（原申請理由：{request_reason}）"))
                .label(subject_label(&exec))
                .payload(json!({ "reason": reason, "requestReason": request_reason, "openSideEffects": open })),
        )
        .await?;
    } else {
        sqlx::query(
            "UPDATE tbl_energy_workflow_execution SET status = 'running', abort_requested_by_user_id = NULL, abort_reason = NULL WHERE id = $1",
        )
        .bind(exec.id)
        .execute(&mut **tx)
        .await?;
        audit::record(
            tx,
            ctx,
            AuditInput::new("flow.abort_rejected", exec.id, format!("駁回中斷申請（原申請理由：{request_reason}）"))
                .label(subject_label(&exec))
                .payload(json!({ "reason": reason, "requestReason": request_reason })),
        )
        .await?;
    }
    Ok(())
}

/// 轉移：移轉擁有權給新人（與代理不同：代理仍記在本人名下）。
pub async fn reassign(tx: &mut Tx<'_>, ctx: &Ctx, task_id: i64, to_user: i64, reason: Option<&str>) -> ApiResult<()> {
    let reason = require_reason(reason)?;
    let (task, exec) = model::task_in_org(tx, ctx.org_id(), task_id).await?;
    if !task.is_active() {
        return Err(err_d(400, "invalid_task_transition", json!({ "from": task.state, "to": "reserved" })));
    }
    if crate::common::user_name(tx, to_user).await?.is_none() {
        return Err(err(404, "user_not_found"));
    }
    sqlx::query(
        "UPDATE tbl_energy_workflow_task SET state = 'reserved', assignee_user_id = $2, on_behalf_of_user_id = NULL, claimed_at = $3 WHERE id = $1",
    )
    .bind(task_id)
    .bind(to_user)
    .bind(ctx.now())
    .execute(&mut **tx)
    .await?;
    // 候選池補一列 —— 否則新 assignee 放回之後就再也認領不了。
    if !model::is_potential_owner(tx, task_id, to_user).await? {
        model::insert_candidate(tx, task_id, to_user, "potentialOwner", "reassign").await?;
    }
    let name = model::node_label(tx, exec.definition_id, &task.node_key).await?;
    audit::record(
        tx,
        ctx,
        AuditInput::new("flow.task_reassigned", task_id, format!("轉移任務「{name}」→ #{to_user}：{reason}"))
            .label(name)
            .payload(json!({ "nodeKey": task.node_key, "fromUserId": task.assignee_user_id, "toUserId": to_user, "reason": reason })),
    )
    .await?;
    Ok(())
}

/// 強制推進（business administrator）：critical 稽核，取消所有活著的 task，從目標節點重新開始。
pub async fn force_advance(tx: &mut Tx<'_>, ctx: &Ctx, execution_id: i64, to_node_key: &str, reason: Option<&str>) -> ApiResult<()> {
    let reason = require_reason(reason)?;
    model::execution_in_org(tx, ctx.org_id(), execution_id).await?;
    let exec = model::execution_for_update(tx, ctx.org_id(), execution_id).await?;
    if exec.status != "running" {
        return Err(err_d(409, "execution_not_running", json!({ "status": exec.status })));
    }
    let target = model::node_by_key_opt(tx, exec.definition_id, to_node_key)
        .await?
        .ok_or_else(|| err_d(400, "node_not_found", json!({ "nodeKey": to_node_key })))?;
    let from = exec.current_node_key.clone();
    let from_label = from.clone().unwrap_or_else(|| "(並行)".into());
    audit::record(
        tx,
        ctx,
        AuditInput::new("flow.force_advanced", execution_id, format!("強制推進 {from_label} → {to_node_key}：{reason}"))
            .label(subject_label(&exec))
            .payload(json!({ "fromNodeKey": from, "toNodeKey": to_node_key, "reason": reason })),
    )
    .await?;
    cancel_active_tasks(tx, ctx, &exec, CancelCause::Revert, None).await?;
    apply_next(tx, ctx, &exec, &target, None).await
}

/// 終止流程。已結束的不可再終止（否則一張完成的單會被改寫成 terminated）。
pub async fn terminate(tx: &mut Tx<'_>, ctx: &Ctx, execution_id: i64, reason: Option<&str>) -> ApiResult<()> {
    let reason = require_reason(reason)?;
    model::execution_in_org(tx, ctx.org_id(), execution_id).await?;
    let exec = model::execution_for_update(tx, ctx.org_id(), execution_id).await?;
    if !matches!(exec.status.as_str(), "running" | "abortPending") {
        return Err(err_d(409, "execution_not_running", json!({ "status": exec.status })));
    }
    cancel_active_tasks(tx, ctx, &exec, CancelCause::Terminate, None).await?;
    sqlx::query("UPDATE tbl_energy_workflow_execution SET status = 'terminated', ended_at = $2 WHERE id = $1")
        .bind(execution_id)
        .bind(ctx.now())
        .execute(&mut **tx)
        .await?;
    audit::record(
        tx,
        ctx,
        AuditInput::new("flow.terminated", execution_id, format!("終止流程：{reason}"))
            .label(subject_label(&exec))
            .payload(json!({ "reason": reason })),
    )
    .await?;
    Ok(())
}
