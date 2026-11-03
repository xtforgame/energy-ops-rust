//! 「這一關有哪些動作」≠「我能做什麼」：動作清單一律在後端依可執行性過濾，
//! 不回按不動的按鈕；做不了就回 `actionBlocked` 說明原因。工作台的列 DTO 也在這裡組。

use super::engine::tasks::can_revert;
use super::model::{self, ExecutionRow, NodeRow, TaskRow, EXEC_COLS, TASK_COLS};
use crate::common::{days_since, Ctx, Tx};
use eo_domain::energy_rbac::get_energy_role_def;
use eo_domain::workflow_defs::AssignmentRule;
use eo_platform::error::ApiResult;
use eo_platform::rbac::OrgAccess;
use serde_json::{json, Value};
use std::collections::HashMap;

/// 這一關由誰處理的一句話（中文的部門／角色說法，不回 uniqueName）。部門依 key 查，限本組織。
pub async fn describe_assignee(tx: &mut Tx<'_>, access: &OrgAccess, node: Option<&NodeRow>) -> ApiResult<Option<String>> {
    let Some(node) = node else { return Ok(None) };
    Ok(match AssignmentRule::from(node.assignment_rule.clone()) {
        AssignmentRule::Department { department_key: Some(k), .. } => {
            crate::org::departments::by_key(tx, access, &k).await?.map(|d| d.name)
        }
        AssignmentRule::DepartmentManagerOfInitiator { .. } => Some("發起人的部門主管".into()),
        AssignmentRule::Role { role_unique_names: Some(names) } => {
            let labels: Vec<&str> = names.iter().filter_map(|u| get_energy_role_def(u).map(|d| d.name)).collect();
            (!labels.is_empty()).then(|| labels.join("、"))
        }
        _ => None,
    })
}

/// 只有部門規則才有的提示（`modeReason` 的說明句用）。部門依 key 查，限本組織。
pub async fn department_hint(tx: &mut Tx<'_>, access: &OrgAccess, node: &NodeRow) -> ApiResult<Option<String>> {
    match AssignmentRule::from(node.assignment_rule.clone()) {
        AssignmentRule::Department { department_key: Some(k), .. } => {
            Ok(crate::org::departments::by_key(tx, access, &k).await?.map(|d| d.name))
        }
        _ => Ok(None),
    }
}

/// 擋住的原因：None ＝ 可以做事。
pub async fn resolve_action_blocked(
    tx: &mut Tx<'_>,
    task: &TaskRow,
    user_id: i64,
    node_name: Option<&str>,
    assignee_hint: Option<&str>,
) -> ApiResult<Option<Value>> {
    let base = |code: &str| json!({ "code": code, "nodeName": node_name, "assigneeHint": assignee_hint });
    // 別人在處理（不是權限不足）。
    if task.assignee_user_id.is_some_and(|a| a != user_id) {
        return Ok(Some(base("claimedByOther")));
    }
    if !task.is_active() {
        return Ok(Some(base("notCandidate")));
    }
    let kinds = model::candidate_kinds(tx, task.id, user_id).await?;
    // 發起人被排除 —— 這一句同時是防自核的使用者說明。
    if kinds.iter().any(|k| k == "excludedOwner") {
        return Ok(Some(base("excludedOwner")));
    }
    if !kinds.iter().any(|k| k == "potentialOwner") {
        return Ok(Some(base("notCandidate")));
    }
    Ok(None)
}

/// 過濾後的動作清單＋擋住的原因。回退類另過一道副作用鎖（只擋回退，其餘照樣可做）。
pub async fn actions_for_task(
    tx: &mut Tx<'_>,
    access: &OrgAccess,
    exec: &ExecutionRow,
    task: &TaskRow,
    user_id: i64,
) -> ApiResult<(Vec<Value>, Option<Value>)> {
    let node = model::node_by_key_opt(tx, exec.definition_id, &task.node_key).await?;
    let node_name = node.as_ref().map(|n| n.name.clone());
    let hint = describe_assignee(tx, access, node.as_ref()).await?;
    if let Some(b) = resolve_action_blocked(tx, task, user_id, node_name.as_deref(), hint.as_deref()).await? {
        return Ok((vec![], Some(b)));
    }
    let Some(node) = node else { return Ok((vec![], None)) };
    let actions: Vec<Value> = model::actions_of_node(tx, node.id)
        .await?
        .into_iter()
        .map(|a| {
            json!({ "actionKey": a.action_key, "labelKey": a.label_key, "kind": a.kind,
                    "requiresReason": a.requires_reason, "sortOrder": a.sort_order })
        })
        .collect();
    if actions.iter().any(|a| a["kind"] == "revert") {
        let v = can_revert(tx, exec, &task.node_key).await?;
        if !v.allowed && v.reason == Some("side_effect_lock") {
            return Ok((
                actions.into_iter().filter(|a| a["kind"] != "revert").collect(),
                Some(json!({ "code": "revertLocked", "nodeName": node.name })),
            ));
        }
    }
    Ok((actions, None))
}

/// 工作台的列（我的待辦、流程總覽的節點清單、動作後回傳的那一列共用這一份）。
pub async fn decorate_tasks(tx: &mut Tx<'_>, ctx: &Ctx, user_id: i64, tasks: &[TaskRow]) -> ApiResult<Vec<Value>> {
    if tasks.is_empty() {
        return Ok(vec![]);
    }
    let exec_ids: Vec<i64> = crate::common::uniq(tasks.iter().map(|t| t.execution_id));
    let execs: Vec<ExecutionRow> = sqlx::query_as(&format!(
        "SELECT {EXEC_COLS} FROM tbl_energy_workflow_execution WHERE id = ANY($1) AND organization_id = $2 AND deleted_at IS NULL ORDER BY id"
    ))
    .bind(&exec_ids)
    .bind(ctx.org_id())
    .fetch_all(&mut **tx)
    .await?;
    let by_id: HashMap<i64, &ExecutionRow> = execs.iter().map(|e| (e.id, e)).collect();
    let def_ids: Vec<i64> = crate::common::uniq(execs.iter().map(|e| e.definition_id));
    let names: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT definition_id, node_key, name FROM tbl_energy_workflow_node WHERE definition_id = ANY($1) AND deleted_at IS NULL ORDER BY id",
    )
    .bind(&def_ids)
    .fetch_all(&mut **tx)
    .await?;
    let node_name: HashMap<(i64, String), String> = names.into_iter().map(|(d, k, n)| ((d, k), n)).collect();
    let refs: Vec<&ExecutionRow> = execs.iter().collect();
    let subjects = super::subjects::load(tx, ctx.org_id(), &refs).await?;
    let now = ctx.now();
    let mut out = Vec::new();
    for t in tasks {
        // 租戶隔離：不是本組織的 execution 就不出現。
        let Some(exec) = by_id.get(&t.execution_id).copied() else { continue };
        let (actions, blocked) = actions_for_task(tx, &ctx.access, exec, t, user_id).await?;
        out.push(json!({
            "id": t.id,
            "executionId": t.execution_id,
            "nodeKey": t.node_key,
            // 查不到節點名就回 null，不 fallback 成 nodeKey。
            "nodeName": node_name.get(&(exec.definition_id, t.node_key.clone())),
            "state": t.state,
            "claimedByMe": t.assignee_user_id == Some(user_id),
            "claimable": t.state == "ready",
            "waitingDays": days_since(now, t.created_at),
            "subject": subjects.task_summary(exec),
            "availableActions": actions,
            "actionBlocked": blocked,
        }));
    }
    Ok(out)
}

/// 動作做完之後「更新後的那一列」（前端就地替換，不重抓整頁）。
pub async fn workbench_row_of(tx: &mut Tx<'_>, ctx: &Ctx, user_id: i64, task_id: i64) -> ApiResult<Option<Value>> {
    let Some(task) = model::task_by_id(tx, task_id).await? else { return Ok(None) };
    Ok(decorate_tasks(tx, ctx, user_id, &[task]).await?.into_iter().next())
}

/// 我是候選人、還沒完成的 task（候選表沒有組織欄位，一定要回頭用 execution 過濾）。
pub async fn my_open_tasks(tx: &mut Tx<'_>, org_id: i64, user_id: i64) -> ApiResult<Vec<TaskRow>> {
    let cols: String = TASK_COLS.split(',').map(|c| format!("t.{}", c.trim())).collect::<Vec<_>>().join(", ");
    Ok(sqlx::query_as(&format!(
        "SELECT {cols} FROM tbl_energy_workflow_task t
           JOIN tbl_energy_workflow_execution e ON e.id = t.execution_id AND e.organization_id = $1 AND e.deleted_at IS NULL
          WHERE t.deleted_at IS NULL AND t.state = ANY($3) AND t.id IN (
            SELECT task_id FROM tbl_energy_workflow_task_candidate
             WHERE user_id = $2 AND kind = 'potentialOwner' AND deleted_at IS NULL)
          ORDER BY t.id"
    ))
    .bind(org_id)
    .bind(user_id)
    .bind(&model::ACTIVE_STATES[..])
    .fetch_all(&mut **tx)
    .await?)
}
