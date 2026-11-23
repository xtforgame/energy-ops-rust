//! 執行側：開一次執行、推進到下一關、建 task、走到 end 時讓單據成立。

use super::super::definition::resolve_definition;
use super::super::model::{self, DefinitionRow, ExecutionRow, NodeRow, TaskRow, EXEC_COLS, TASK_COLS};
use super::candidates::resolve_candidates;
use super::subject_label;
use crate::audit::{self, AuditInput};
use crate::common::{err, err_d, Ctx, Tx};
use chrono::NaiveDate;
use eo_domain::conditions::select_transitions;
use eo_platform::error::{ApiError, ApiResult};
use futures::future::BoxFuture;
use serde_json::{json, Value};

/// 開流程的輸入。`business_date` 是業務日期（計費期間），不是今天。
#[derive(Debug, Clone)]
pub struct StartExecution {
    pub station_key: String,
    pub workflow_key: String,
    pub subject_type: String,
    pub subject_id: String,
    pub business_date: NaiveDate,
    /// 分支條件求值用的單據屬性快照。
    pub attributes: Value,
    /// 呼叫端已經解好的定義版本（送審那條路會給）。給了就不再解第二次。
    pub definition: Option<DefinitionRow>,
    pub station_id: Option<i64>,
}

/// 唯一索引「同一張單同一條流程只能一筆 running」撞到時，回業務錯誤碼。
fn map_unique(e: ApiError) -> ApiError {
    let constraint = e.details.as_ref().and_then(|d| d.get("constraint")).and_then(Value::as_str).unwrap_or("");
    match constraint {
        "energy_workflow_execution_one_running" => err(409, "already_in_review"),
        "energy_bill_org_business_key" | "energy_bill_org_document_no" | "energy_installment_org_business_key"
        | "energy_installment_org_document_no" => err(409, "duplicate_document"),
        _ => e,
    }
}

/// 開一次執行：取版 → 建 execution → 從 start 推進到第一個真正的關卡。全部在呼叫端的交易內。
pub async fn start_execution(tx: &mut Tx<'_>, ctx: &Ctx, input: StartExecution) -> ApiResult<ExecutionRow> {
    let def = match input.definition.clone() {
        Some(d) => d,
        None => resolve_definition(tx, &ctx.access, &input.station_key, &input.workflow_key, input.business_date).await?,
    };
    let nodes = model::nodes(tx, def.id).await?;
    let start = nodes.into_iter().find(|n| n.node_type == "start").ok_or_else(|| err(500, "definition_has_no_start_node"))?;
    let now = ctx.now();
    let attributes = if input.attributes.is_object() { input.attributes.clone() } else { json!({}) };
    let exec: ExecutionRow = sqlx::query_as(&format!(
        "INSERT INTO tbl_energy_workflow_execution
           (organization_id, definition_id, workflow_key, station_key, station_id, subject_type, subject_id, business_date,
            status, current_node_key, started_by_user_id, started_at, data, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'running', $9, $10, $11, $12, $11) RETURNING {EXEC_COLS}"
    ))
    .bind(ctx.org_id())
    .bind(def.id)
    // 不分版本的那條線取自出生時那一版的 key：真相在定義上。
    .bind(&def.key)
    .bind(&input.station_key)
    .bind(input.station_id)
    .bind(&input.subject_type)
    .bind(&input.subject_id)
    .bind(input.business_date)
    .bind(&start.node_key)
    .bind(ctx.user_id())
    .bind(now)
    .bind(json!({ "attributes": attributes }))
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| map_unique(e.into()))?;

    let subject = eo_domain::side_effects::subject_label(Some(&input.subject_type), Some(&input.subject_id));
    audit::record(
        tx,
        ctx,
        AuditInput::new(
            "flow.started",
            exec.id,
            format!("發起流程「{}」v{}（{} {}）", def.name, def.version, input.subject_type, input.subject_id),
        )
        .label(format!("{} · {subject}", def.name))
        .payload(json!({
            "workflowKey": input.workflow_key, "definitionId": def.id, "version": def.version,
            "businessDate": crate::common::day(input.business_date), "stationKey": input.station_key,
            "subjectType": input.subject_type, "subjectId": input.subject_id,
        })),
    )
    .await?;

    // start 節點不產生 task（沒有人要做），直接推進。
    advance_from(tx, ctx, &exec, &start).await?;
    let fresh: ExecutionRow = sqlx::query_as(&format!("SELECT {EXEC_COLS} FROM tbl_energy_workflow_execution WHERE id = $1"))
        .bind(exec.id)
        .fetch_one(&mut **tx)
        .await?;
    Ok(fresh)
}

/// 單據種類 → 它的表名。目前只有這兩種單走流程；其餘回 None（`check_step_requirements` 也共用這份對照）。
pub(crate) fn subject_table_name(subject_type: &str) -> Option<&'static str> {
    match subject_type {
        "energyBill" => Some("tbl_energy_bill"),
        "energyInstallment" => Some("tbl_energy_installment"),
        _ => None,
    }
}

/// 走到 end 的那一刻才讓單據成立（審批過的才是唯一檔案）。與流程完成同生共死。
async fn mark_subject_approved(tx: &mut Tx<'_>, ctx: &Ctx, exec: &ExecutionRow) -> ApiResult<()> {
    let Some(table) = subject_table_name(&exec.subject_type) else { return Ok(()) };
    let Some(id) = crate::common::numeric_id(&exec.subject_id) else { return Ok(()) };
    sqlx::query(&format!(
        "UPDATE {table} SET approval_state = 'approved', approved_at = $3 WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL"
    ))
    .bind(id)
    .bind(exec.organization_id)
    .bind(ctx.now())
    .execute(&mut **tx)
    .await
    .map_err(|e| map_unique(e.into()))?;
    if exec.subject_type == "energyBill" && exec.workflow_key.as_deref() == Some("bill-data-review") {
        sqlx::query("UPDATE tbl_energy_bill SET review_status='reviewed' WHERE id=$1 AND organization_id=$2")
            .bind(id).bind(exec.organization_id).execute(&mut **tx).await?;
    }
    Ok(())
}

/// 從 `from` 該走哪幾條 forward 邊（純解析不寫入）。並行節點全開，否則取第一個成立者。
pub async fn resolve_next_nodes(tx: &mut Tx<'_>, exec: &ExecutionRow, from: &NodeRow) -> ApiResult<Vec<NodeRow>> {
    let edges = model::outgoing(tx, exec.definition_id, &from.node_key, "forward").await?;
    let chosen = select_transitions(&edges, &exec.attributes(), from.parallel);
    if chosen.is_empty() {
        // 不是靜默結束 —— 定義畫錯了要讓人知道。
        return Err(err_d(409, "no_matching_transition", json!({ "fromNodeKey": from.node_key, "candidates": edges.len() })));
    }
    let keys: Vec<String> = chosen.iter().map(|t| t.to_node_key.clone()).collect();
    let mut out = Vec::new();
    for k in keys {
        out.push(model::node_by_key(tx, exec.definition_id, &k).await?);
    }
    Ok(out)
}

/// 推進到 `next`：end 就收尾，否則更新目前節點再建 task。
pub fn apply_next<'a, 'c>(
    tx: &'a mut Tx<'c>,
    ctx: &'a Ctx,
    exec: &'a ExecutionRow,
    next: &'a NodeRow,
    branch_key: Option<String>,
) -> BoxFuture<'a, ApiResult<()>>
where
    'c: 'a,
{
    Box::pin(async move {
        if next.node_type == "end" {
            sqlx::query(
                "UPDATE tbl_energy_workflow_execution SET status = 'completed', current_node_key = $2, ended_at = $3 WHERE id = $1",
            )
            .bind(exec.id)
            .bind(&next.node_key)
            .bind(ctx.now())
            .execute(&mut **tx)
            .await?;
            audit::record(
                tx,
                ctx,
                AuditInput::new("flow.completed", exec.id, format!("流程完成（{} {}）", exec.subject_type, exec.subject_id))
                    .label(subject_label(exec))
                    .payload(json!({ "endNodeKey": next.node_key })),
            )
            .await?;
            return mark_subject_approved(tx, ctx, exec).await;
        }
        // 先更新目前節點再建 task —— 計算節點會再往下推，順序反了會被較早那一步蓋回去。
        sqlx::query("UPDATE tbl_energy_workflow_execution SET current_node_key = $2 WHERE id = $1")
            .bind(exec.id)
            .bind(&next.node_key)
            .execute(&mut **tx)
            .await?;
        create_task(tx, ctx, exec, next, branch_key).await?;
        Ok(())
    })
}

/// 套用多個下一關。並行時每條分支帶 branchKey，currentNodeKey 設成 NULL（唯一真相是 task 列）。
pub async fn apply_next_many(tx: &mut Tx<'_>, ctx: &Ctx, exec: &ExecutionRow, from: &NodeRow, next: &[NodeRow]) -> ApiResult<()> {
    let parallel = from.parallel && next.len() > 1;
    for n in next {
        let branch = parallel.then(|| format!("{}:{}", from.node_key, n.node_key));
        apply_next(tx, ctx, exec, n, branch).await?;
    }
    if parallel {
        sqlx::query("UPDATE tbl_energy_workflow_execution SET current_node_key = NULL WHERE id = $1")
            .bind(exec.id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

pub async fn advance_from(tx: &mut Tx<'_>, ctx: &Ctx, exec: &ExecutionRow, from: &NodeRow) -> ApiResult<()> {
    let next = resolve_next_nodes(tx, exec, from).await?;
    apply_next_many(tx, ctx, exec, from, &next).await
}

/// 建一個 task 並解析候選池。計算節點直接 ready → completed 再往下走。
async fn create_task(tx: &mut Tx<'_>, ctx: &Ctx, exec: &ExecutionRow, node: &NodeRow, branch_key: Option<String>) -> ApiResult<TaskRow> {
    let seq = model::next_seq(tx, exec.id).await?;
    let task: TaskRow = sqlx::query_as(&format!(
        "INSERT INTO tbl_energy_workflow_task (execution_id, node_key, node_type, seq, branch_key, state, payload, created_at)
         VALUES ($1, $2, $3, $4, $5, 'ready', '{{}}', $6) RETURNING {TASK_COLS}"
    ))
    .bind(exec.id)
    .bind(&node.node_key)
    .bind(&node.node_type)
    .bind(seq)
    .bind(branch_key.as_deref())
    .bind(ctx.now())
    .fetch_one(&mut **tx)
    .await?;

    match node.node_type.as_str() {
        "humanTask" => {
            let r = resolve_candidates(tx, ctx, exec, node, task.id).await?;
            let n = r.potential_owners.len();
            let summary = if n > 0 {
                format!("「{}」解析出 {n} 位候選人", node.name)
            } else {
                format!("「{}」候選池是空的，需要管理員處理", node.name)
            };
            audit::record(
                tx,
                ctx,
                AuditInput::new("flow.candidates_resolved", task.id, summary)
                    .label(node.name.clone())
                    // 解析出 0 人是「要有人處理」，不是成功。
                    .outcome(if n > 0 { "success" } else { "failed" })
                    .payload(json!({
                        "nodeKey": node.node_key, "potentialOwners": r.potential_owners, "excludedOwners": r.excluded_owners,
                        "resolvedBy": r.resolved_by, "empty": n == 0,
                    })),
            )
            .await?;
            Ok(task)
        }
        "calculation" => {
            // 流程呼叫計算、不內含計算；算不出來就整個交易失敗，絕不寫金額 0 的紀錄。
            let mut payload = json!({});
            match node.service_key.as_deref() {
                Some("rating.computePayout") => {
                    let bill_id = crate::common::numeric_id(&exec.subject_id)
                        .ok_or_else(|| err_d(404, "bill_not_found", json!({ "subjectId": exec.subject_id })))?;
                    let record = crate::rating::compute_for_bill(tx, ctx, bill_id, "workflowNode", Some(exec.id)).await?;
                    payload = json!({ "payoutRecordId": record.id, "amount": record.amount_str() });
                }
                Some(other) => return Err(err_d(501, "unknown_service_key", json!({ "serviceKey": other }))),
                None => {}
            }
            sqlx::query(
                "UPDATE tbl_energy_workflow_task SET state = 'completed', outcome = 'auto', completed_at = $2, payload = $3 WHERE id = $1",
            )
            .bind(task.id)
            .bind(ctx.now())
            .bind(&payload)
            .execute(&mut **tx)
            .await?;
            let fresh = model::task_by_id(tx, task.id).await?.ok_or_else(|| err(404, "task_not_found"))?;
            advance_from(tx, ctx, exec, node).await?;
            Ok(fresh)
        }
        _ => Ok(task),
    }
}
