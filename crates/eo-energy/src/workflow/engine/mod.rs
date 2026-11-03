//! 流程引擎（WS-HumanTask 狀態機）。
//!
//! 三條硬規則：
//! 1. 每個 mutation 都在呼叫端給的交易內，稽核寫在同一個交易。
//! 2. task 狀態轉移一律走狀態機；非法轉移 → `invalid_task_transition`。
//! 3. 推進之後由引擎決定下一步；找不到可走的邊 → `no_matching_transition`（不是靜默結束）。
//!
//! 三條最容易寫錯的：
//! 1. 回退判定 ＝ 節點 `revertable` ∧ execution 上沒有鎖回退的副作用（副作用是動作造成的，不是位置）。
//! 2. 血緣是來歷紀錄不是控制通道：寫進去不觸發任何行為，撤回不傳播。
//! 3. 代理（任務仍記在本人名下）≠ 轉移（擁有權移給新人）≠ 放回（回到池子）。

pub mod admin;
pub mod candidates;
pub mod delegation;
pub mod lineage;
pub mod run;
pub mod tasks;

use super::model::{ExecutionRow, TaskRow, TASK_COLS};
use crate::audit::{self, AuditInput};
use crate::common::{err_d, Ctx, Tx};
use eo_platform::error::ApiResult;
use serde_json::json;

/// WS-HumanTask 1.1 的合法轉移。
fn allowed_next(from: &str) -> &'static [&'static str] {
    match from {
        "ready" => &["reserved", "completed", "exited", "failed"],
        "reserved" => &["ready", "inProgress", "completed", "exited", "failed"],
        "inProgress" => &["completed", "failed", "exited"],
        _ => &[],
    }
}

pub fn assert_transition(from: &str, to: &str) -> ApiResult<()> {
    if allowed_next(from).contains(&to) {
        Ok(())
    } else {
        Err(err_d(400, "invalid_task_transition", json!({ "from": from, "to": to })))
    }
}

/// 取消的原因 → task 的 outcome。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelCause {
    Revert,
    Join,
    Abort,
    Terminate,
}

impl CancelCause {
    fn as_str(self) -> &'static str {
        match self {
            CancelCause::Revert => "revert",
            CancelCause::Join => "join",
            CancelCause::Abort => "abort",
            CancelCause::Terminate => "terminate",
        }
    }
    fn outcome(self) -> &'static str {
        match self {
            CancelCause::Revert => "cancelledByRevert",
            CancelCause::Join => "cancelledByJoin",
            CancelCause::Abort | CancelCause::Terminate => "cancelledByAbort",
        }
    }
}

/// 這筆流程在處理的是哪一張單（`電費單 #123`）—— 寫入時的快照。
pub fn subject_label(e: &ExecutionRow) -> String {
    eo_domain::side_effects::subject_label(Some(&e.subject_type), Some(&e.subject_id))
}

/// 取消所有仍活著的 task，每一條各留一筆稽核（不可只記一筆總結）。已完成的一律不動。
pub async fn cancel_active_tasks(
    tx: &mut Tx<'_>,
    ctx: &Ctx,
    exec: &ExecutionRow,
    cause: CancelCause,
    except_task_id: Option<i64>,
) -> ApiResult<u32> {
    let rows: Vec<TaskRow> = sqlx::query_as(&format!(
        "SELECT {TASK_COLS} FROM tbl_energy_workflow_task
          WHERE execution_id = $1 AND state = ANY($2) AND deleted_at IS NULL ORDER BY id"
    ))
    .bind(exec.id)
    .bind(&super::model::ACTIVE_STATES[..])
    .fetch_all(&mut **tx)
    .await?;
    let mut n = 0;
    for t in rows {
        if Some(t.id) == except_task_id {
            continue;
        }
        // 中斷核可的 task 不在主流程路徑上；join／revert 不動它。
        if t.is_out_of_band && !matches!(cause, CancelCause::Abort | CancelCause::Terminate) {
            continue;
        }
        sqlx::query("UPDATE tbl_energy_workflow_task SET state = 'exited', outcome = $2, completed_at = $3 WHERE id = $1")
            .bind(t.id)
            .bind(cause.outcome())
            .bind(ctx.now())
            .execute(&mut **tx)
            .await?;
        let name = super::model::node_label(tx, exec.definition_id, &t.node_key).await?;
        audit::record(
            tx,
            ctx,
            AuditInput::new("flow.branch_cancelled", t.id, format!("取消「{name}」（{}）", cause.as_str()))
                .label(name)
                .payload(json!({
                    "nodeKey": t.node_key, "branchKey": t.branch_key, "stateWhenCancelled": t.state, "cause": cause.as_str(),
                })),
        )
        .await?;
        n += 1;
    }
    Ok(n)
}

/// 理由必填的動作：空白也算沒給。
pub fn require_reason(reason: Option<&str>) -> ApiResult<String> {
    match reason.map(str::trim).filter(|s| !s.is_empty()) {
        Some(_) => Ok(reason.unwrap_or_default().to_string()),
        None => Err(crate::common::err(400, "reason_required")),
    }
}
