//! 模式判定的唯一入口（詳情頁與單據 PATCH 的防線都走這一支，兩份判定遲早漂移）。

use super::access::{has_view_all, visible_execution_ids};
use super::model::{self, ExecutionRow, TaskRow, EXEC_COLS, TASK_COLS};
use super::views::{actions_for_task, department_hint};
use crate::common::{Ctx, Tx};
use eo_domain::resolve_mode::{resolve_mode, ActiveTaskState, BillMode, EndedAt, ExecutionState, ModeVerdict, ResolveModeInput};
use eo_platform::error::ApiResult;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeOut {
    #[serde(flatten)]
    pub verdict: ModeVerdict,
    pub available_actions: Vec<Value>,
    pub execution_id: Option<i64>,
}

impl ModeOut {
    pub fn mode(&self) -> BillMode {
        self.verdict.mode
    }
}

/// 這張單（subject）對呼叫者是什麼模式。還沒送審時 execution 為 None。
pub async fn mode_for(tx: &mut Tx<'_>, ctx: &Ctx, subject_type: &str, subject_id: i64) -> ApiResult<ModeOut> {
    let me = ctx.access.require_user()?.id;
    let exec: Option<ExecutionRow> = sqlx::query_as(&format!(
        "SELECT {EXEC_COLS} FROM tbl_energy_workflow_execution
          WHERE organization_id = $1 AND subject_type = $2 AND subject_id = $3 AND deleted_at IS NULL ORDER BY id DESC LIMIT 1"
    ))
    .bind(ctx.org_id())
    .bind(subject_type)
    .bind(subject_id.to_string())
    .fetch_optional(&mut **tx)
    .await?;

    let Some(exec) = exec else {
        // 還沒送審：我是送件者，或我有 flow:act（只做後者的話，承辦連自己剛上傳的單都改不了）。
        let mut can_intake = ctx.has("energy:flow:act").await?;
        if !can_intake && subject_type == "energyBill" {
            let submitter: Option<Option<i64>> = sqlx::query_scalar(
                "SELECT b.submitted_by_user_id FROM tbl_energy_ingest_item i
                   JOIN tbl_energy_ingest_batch b ON b.id = i.batch_id AND b.deleted_at IS NULL
                  WHERE i.organization_id = $1 AND i.bill_id = $2 AND i.deleted_at IS NULL ORDER BY i.id LIMIT 1",
            )
            .bind(ctx.org_id())
            .bind(subject_id)
            .fetch_optional(&mut **tx)
            .await?;
            can_intake = submitter.flatten() == Some(me);
        }
        let verdict = resolve_mode(&ResolveModeInput { has_access: true, can_intake, me, ..Default::default() });
        return Ok(ModeOut { verdict, available_actions: vec![], execution_id: None });
    };

    let visible = has_view_all(&ctx.core, &ctx.access).await?
        || visible_execution_ids(tx, &[exec.id], me, ctx.today()).await?.contains(&exec.id);
    if !visible {
        let verdict = resolve_mode(&ResolveModeInput { has_access: false, me, ..Default::default() });
        return Ok(ModeOut { verdict, available_actions: vec![], execution_id: Some(exec.id) });
    }

    let task: Option<TaskRow> = sqlx::query_as(&format!(
        "SELECT {TASK_COLS} FROM tbl_energy_workflow_task
          WHERE execution_id = $1 AND state = ANY($2) AND NOT is_out_of_band AND deleted_at IS NULL ORDER BY seq DESC LIMIT 1"
    ))
    .bind(exec.id)
    .bind(&model::ACTIVE_STATES[..])
    .fetch_optional(&mut **tx)
    .await?;

    let mut input = ResolveModeInput {
        has_access: true,
        execution: Some(ExecutionState { status: exec.status.clone(), ended_at: exec.ended_at.map(EndedAt::Time) }),
        me,
        ..Default::default()
    };
    if let Some(t) = &task {
        let node = model::node_by_key_opt(tx, exec.definition_id, &t.node_key).await?;
        input.active_task = Some(ActiveTaskState {
            state: t.state.clone(),
            assignee_user_id: t.assignee_user_id,
            node_name: node.as_ref().map(|n| n.name.clone()),
            // 節點旗標，不是硬編節點名。
            allows_subject_edit: node.as_ref().is_some_and(|n| n.allows_subject_edit),
        });
        let kinds = model::candidate_kinds(tx, t.id, me).await?;
        input.is_candidate = kinds.iter().any(|k| k == "potentialOwner");
        input.is_excluded = kinds.iter().any(|k| k == "excludedOwner");
        if let Some(n) = &node {
            input.assignee_hint = department_hint(tx, &ctx.access, n).await?;
        }
    }
    // 「能不能動手」只影響 readonly 的說法（viewOnly vs notMyTurn），不影響權限。
    input.can_act = Some(ctx.has("energy:flow:act").await?);
    let verdict = resolve_mode(&input);
    let mut available = vec![];
    if let Some(t) = &task {
        if matches!(verdict.mode, BillMode::Act | BillMode::Revise) {
            available = actions_for_task(tx, &ctx.access, &exec, t, me).await?.0;
        }
    }
    Ok(ModeOut { verdict, available_actions: available, execution_id: Some(exec.id) })
}
