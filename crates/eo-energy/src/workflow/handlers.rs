//! 流程的 HTTP 端點：閘 → 解析 → 呼叫引擎（同一個交易）→ 回寫。

use super::access::{authorize_execution, has_view_all};
use super::engine::delegation::{self, NewDelegation};
use super::engine::lineage::{self, Direction, NewLineage};
use super::engine::run::{self, StartExecution};
use super::engine::{admin, tasks};
use super::list::{self, ListFilter};
use super::{graph, mode, model, views};
use crate::common::{err, err_d, parse_body, parse_day, Ctx, RequestMeta, Tx};
use crate::gates;
use actix_web::{web, HttpResponse};
use eo_domain::display_id::is_db_id;
use eo_domain::schemas::{
    CreateDelegationInput, DecideAbortInput, LinkLineageInput, PerformActionInput, RelationKind, ReleaseSideEffectInput,
    StartExecutionInput, SubjectIdInput,
};
use eo_platform::auth::CurrentUser;
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use eo_platform::rbac::OrgAccess;
use serde::Deserialize;
use serde_json::{json, Value};

type Core = web::Data<CoreRef>;

async fn gate(core: &Core, user: &CurrentUser, org: i64, perms: &[&str], meta: &RequestMeta) -> ApiResult<Ctx> {
    let access = core.rbac.authorize_org(user, org, perms).await?;
    Ok(Ctx::new(core, access, meta))
}

fn body_or_empty(body: Option<web::Json<Value>>) -> Value {
    body.map(|b| b.into_inner()).unwrap_or_else(|| json!({}))
}

fn str_field(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(String::from)
}

// ── 清單、開流程、詳情 ──

pub async fn list_executions(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, q: web::Query<Value>) -> ApiResult<HttpResponse> {
    let ctx = gate(&core, &user, org.into_inner(), gates::FLOW_VIEW, &meta).await?;
    let q = q.into_inner();
    let scope = str_field(&q, "scope").unwrap_or_else(|| "all".into());
    if !["mine", "all"].contains(&scope.as_str()) {
        return Err(err_d(400, "invalid_scope", json!({ "field": "scope" })));
    }
    let view_all = has_view_all(&core, &ctx.access).await?;
    // scope=all 沒有 view-all 要 403，不可靜默降級成 mine（看到的比實際少而不自知）。
    if scope == "all" && !view_all {
        return Err(err(403, "view_all_required"));
    }
    let status = str_field(&q, "status").filter(|s| !s.is_empty());
    if status.as_deref().is_some_and(|s| !["running", "completed", "all"].contains(&s)) {
        return Err(err_d(400, "invalid_status", json!({ "field": "status" })));
    }
    let num = |k: &str| -> ApiResult<Option<i64>> {
        match q.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()) {
            None => Ok(None),
            Some(s) => s.parse::<i64>().map(Some).map_err(|_| err_d(400, "invalid_number", json!({ "field": k }))),
        }
    };
    let filter = ListFilter {
        status,
        scope: Some(scope),
        execution_id: num("executionId")?,
        workflow_key: str_field(&q, "workflowKey"),
        node_key: str_field(&q, "nodeKey"),
        station_id: num("stationId")?,
        landowner_id: num("landownerId")?,
        waiting_days_min: num("waitingDaysMin")?,
        month: str_field(&q, "month"),
        document_no: str_field(&q, "documentNo"),
        sort: str_field(&q, "sort"),
        page: num("page")?,
        page_size: num("pageSize")?,
    };
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(list::list(&mut tx, &ctx, view_all, &filter).await?))
}

pub async fn start_execution(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let ctx = gate(&core, &user, org.into_inner(), gates::FLOW_ACT, &meta).await?;
    let input: StartExecutionInput = parse_body(&body)?;
    let subject_id = match &input.subject_id {
        SubjectIdInput::Text(s) => s.clone(),
        SubjectIdInput::Number(n) if n.fract() == 0.0 => format!("{}", *n as i64),
        SubjectIdInput::Number(n) => n.to_string(),
    };
    let business_date = parse_day(&input.business_date).ok_or_else(|| err_d(400, "invalid_body", json!({ "field": "businessDate" })))?;
    let mut tx = core.pool.begin().await?;
    let exec = run::start_execution(
        &mut tx,
        &ctx,
        StartExecution {
            station_key: input.station_key,
            workflow_key: input.workflow_key,
            subject_type: input.subject_type,
            subject_id,
            business_date,
            attributes: input.attributes.map(Value::Object).unwrap_or_else(|| json!({})),
            definition: None,
            station_id: None,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "id": exec.id, "status": exec.status, "currentNodeKey": exec.current_node_key })))
}

/// 支援貼單號進網址：純數字當 id，否則當單號（解析走本組織的查詢）。
async fn resolve_execution_id(tx: &mut Tx<'_>, ctx: &Ctx, raw: &str) -> ApiResult<i64> {
    if is_db_id(Some(raw)) {
        return raw.trim().parse().map_err(|_| err(404, "execution_not_found"));
    }
    list::find_execution(tx, ctx, raw).await?.map(|e| e.id).ok_or_else(|| err(404, "execution_not_found"))
}

pub async fn get_execution(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, String)>) -> ApiResult<HttpResponse> {
    let (org, raw) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_VIEW, &meta).await?;
    let mut tx = core.pool.begin().await?;
    let id = resolve_execution_id(&mut tx, &ctx, &raw).await?;
    let ea = authorize_execution(&core, &mut tx, &ctx.access, id).await?;
    let exec = ea.execution().clone();
    let summary = list::decorate(&mut tx, &ctx, std::slice::from_ref(&exec)).await?.into_iter().next();
    let tasks = model::tasks_of(&mut tx, exec.id).await?;
    let names: std::collections::HashMap<String, String> =
        model::nodes(&mut tx, exec.definition_id).await?.into_iter().map(|n| (n.node_key, n.name)).collect();
    let user_ids: Vec<i64> = crate::common::uniq(tasks.iter().filter_map(|t| t.assignee_user_id));
    let users = crate::common::user_names(&mut tx, &user_ids).await?;
    let subject_id = crate::common::numeric_id(&exec.subject_id).unwrap_or(0);
    let mode = mode::mode_for(&mut tx, &ctx, &exec.subject_type, subject_id).await?;
    let mut body = json!({
        "id": exec.id, "status": exec.status, "currentNodeKey": exec.current_node_key,
        "subjectType": exec.subject_type, "subjectId": exec.subject_id, "businessDate": crate::common::day(exec.business_date),
        "workflowKey": summary.as_ref().and_then(|s| s.workflow_key.clone()),
        "workflowName": summary.as_ref().and_then(|s| s.workflow_name.clone()),
        "definitionId": exec.definition_id,
        "version": summary.as_ref().and_then(|s| s.version),
        "nodeName": summary.as_ref().and_then(|s| s.node_name.clone()),
        "waitingDays": summary.as_ref().and_then(|s| s.waiting_days),
        "startedAt": exec.started_at,
        "startedByName": summary.as_ref().and_then(|s| s.started_by_name.clone()),
        "displayId": summary.as_ref().map(|s| s.display_id.clone()),
        "subject": summary.as_ref().map(|s| s.subject.clone()),
        "tasks": tasks.iter().map(|t| json!({
            "id": t.id, "seq": t.seq, "nodeKey": t.node_key, "nodeName": names.get(&t.node_key), "nodeType": t.node_type,
            "state": t.state, "assigneeUserId": t.assignee_user_id,
            "assigneeName": t.assignee_user_id.and_then(|u| users.get(&u).cloned()),
            "outcome": t.outcome, "reason": t.reason, "createdAt": t.created_at, "completedAt": t.completed_at,
            "isOutOfBand": t.is_out_of_band, "branchKey": t.branch_key,
        })).collect::<Vec<_>>(),
    });
    if let (Some(obj), Ok(Value::Object(m))) = (body.as_object_mut(), serde_json::to_value(&mode)) {
        for (k, v) in m {
            obj.insert(k, v);
        }
    }
    Ok(HttpResponse::Ok().json(body))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeKeyQuery {
    node_key: Option<String>,
    direction: Option<String>,
}

async fn exec_view(core: &Core, user: &CurrentUser, meta: &RequestMeta, org: i64, id: i64) -> ApiResult<(Ctx, Tx<'static>, model::ExecutionRow)> {
    let ctx = gate(core, user, org, gates::FLOW_VIEW, meta).await?;
    let mut tx = core.pool.begin().await?;
    let ea = authorize_execution(core, &mut tx, &ctx.access, id).await?;
    let exec = ea.execution().clone();
    Ok((ctx, tx, exec))
}

/// 前端畫按鈕用：回「我」能做的動作與認領狀態（不是定義上的動作清單）。
pub async fn node_actions(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, q: web::Query<NodeKeyQuery>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let (ctx, mut tx, exec) = exec_view(&core, &user, &meta, org, id).await?;
    let node_key = q.node_key.clone().or_else(|| exec.current_node_key.clone()).unwrap_or_default();
    let open = model::tasks_of(&mut tx, exec.id).await?.into_iter().filter(|t| t.node_key == node_key && t.is_active()).max_by_key(|t| t.seq);
    let Some(open) = open else {
        return Ok(HttpResponse::Ok().json(json!({
            "availableActions": [], "actionBlocked": { "code": "notCandidate", "nodeName": node_key },
            "taskId": null, "claimable": false, "claimedByMe": false,
        })));
    };
    let me = user.id;
    let (actions, blocked) = views::actions_for_task(&mut tx, &ctx.access, &exec, &open, me).await?;
    // claimable 是「我認領得了嗎」：被防自核排除的發起人不該拿到一顆按下去 403 的鍵。
    let code = blocked.as_ref().and_then(|b| b.get("code")).and_then(Value::as_str).map(String::from);
    let can_act = code.is_none() || code.as_deref() == Some("revertLocked");
    Ok(HttpResponse::Ok().json(json!({
        "availableActions": actions, "actionBlocked": blocked, "taskId": open.id,
        "claimable": can_act && open.state == "ready", "claimedByMe": open.assignee_user_id == Some(me),
    })))
}

pub async fn execution_graph(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let (_ctx, mut tx, exec) = exec_view(&core, &user, &meta, org, id).await?;
    Ok(HttpResponse::Ok().json(graph::execution_graph(&mut tx, &exec).await?))
}

pub async fn revertable(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, q: web::Query<NodeKeyQuery>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let (_ctx, mut tx, exec) = exec_view(&core, &user, &meta, org, id).await?;
    let node_key = q.node_key.clone().or_else(|| exec.current_node_key.clone()).ok_or_else(|| err(400, "node_key_required"))?;
    Ok(HttpResponse::Ok().json(tasks::can_revert(&mut tx, &exec, &node_key).await?))
}

pub async fn lineage_trace(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, q: web::Query<NodeKeyQuery>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let (ctx, mut tx, exec) = exec_view(&core, &user, &meta, org, id).await?;
    let dir = if q.direction.as_deref() == Some("down") { Direction::Down } else { Direction::Up };
    let items = lineage::trace_lineage(&mut tx, ctx.org_id(), exec.id, dir, 10).await?;
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

// ── 旁路與管理動作 ──

pub async fn request_abort(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: Option<web::Json<Value>>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_ACT, &meta).await?;
    let b = body_or_empty(body);
    let mut tx = core.pool.begin().await?;
    admin::request_abort(&mut tx, &ctx, id, b.get("reason").and_then(Value::as_str)).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub async fn force_advance(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: Option<web::Json<Value>>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_ADMIN, &meta).await?;
    let b = body_or_empty(body);
    let to = str_field(&b, "toNodeKey").filter(|s| !s.is_empty()).ok_or_else(|| err(400, "to_node_key_required"))?;
    let mut tx = core.pool.begin().await?;
    admin::force_advance(&mut tx, &ctx, id, &to, b.get("reason").and_then(Value::as_str)).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub async fn terminate(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: Option<web::Json<Value>>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_ADMIN, &meta).await?;
    let b = body_or_empty(body);
    let mut tx = core.pool.begin().await?;
    admin::terminate(&mut tx, &ctx, id, b.get("reason").and_then(Value::as_str)).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

// ── task ──

pub async fn claim(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, task) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_ACT, &meta).await?;
    let mut tx = core.pool.begin().await?;
    tasks::claim(&mut tx, &ctx, task).await?;
    tx.commit().await?;
    let mut tx = core.pool.begin().await?;
    let row = views::workbench_row_of(&mut tx, &ctx, user.id, task).await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true, "row": row })))
}

pub async fn release(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, task) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_ACT, &meta).await?;
    let mut tx = core.pool.begin().await?;
    tasks::release(&mut tx, &ctx, task).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub async fn perform_action(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let (org, task) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_ACT, &meta).await?;
    let input: PerformActionInput = parse_body(&body)?;
    let mut tx = core.pool.begin().await?;
    tasks::perform_action(
        &mut tx,
        &ctx,
        task,
        tasks::ActionInput {
            action_key: input.action_key,
            reason: input.reason,
            payload: input.payload,
            note: input.note,
            target_node_key: input.target_node_key,
        },
    )
    .await?;
    tx.commit().await?;
    let mut tx = core.pool.begin().await?;
    let row = views::workbench_row_of(&mut tx, &ctx, user.id, task).await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true, "row": row })))
}

pub async fn decide_abort(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let (org, task) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::ABORT_APPROVE, &meta).await?;
    let input: DecideAbortInput = parse_body(&body)?;
    let mut tx = core.pool.begin().await?;
    admin::decide_abort(&mut tx, &ctx, task, input.approved, input.reason.as_deref()).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub async fn reassign(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: Option<web::Json<Value>>) -> ApiResult<HttpResponse> {
    let (org, task) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_ADMIN, &meta).await?;
    let b = body_or_empty(body);
    let to = b.get("toUserId").and_then(Value::as_i64).ok_or_else(|| err(400, "to_user_required"))?;
    let mut tx = core.pool.begin().await?;
    admin::reassign(&mut tx, &ctx, task, to, b.get("reason").and_then(Value::as_str)).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

/// 全系統最危險的按鈕：解除回退鎖。
pub async fn release_side_effect(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_ADMIN, &meta).await?;
    let input: ReleaseSideEffectInput = parse_body(&body)?;
    let mut tx = core.pool.begin().await?;
    tasks::release_side_effect(&mut tx, &ctx, id, &input.reason).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

// ── 代理 ──

#[derive(Deserialize)]
pub struct DelegationQuery {
    all: Option<String>,
}

pub async fn list_delegations(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, q: web::Query<DelegationQuery>) -> ApiResult<HttpResponse> {
    let ctx = gate(&core, &user, org.into_inner(), gates::FLOW_VIEW, &meta).await?;
    let all = matches!(q.all.as_deref(), Some("1") | Some("true")) && ctx.has(gates::DELEGATION_MANAGE[0]).await?;
    let mut tx = core.pool.begin().await?;
    let rows = delegation::list_delegations(&mut tx, ctx.org_id(), Some(user.id), all).await?;
    let ids: Vec<i64> = crate::common::uniq(rows.iter().flat_map(|r| [r.delegator_user_id, r.delegate_user_id]));
    let names = crate::common::user_names(&mut tx, &ids).await?;
    let today = ctx.today();
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            let mut v = serde_json::to_value(r).unwrap_or(Value::Null);
            v["delegatorName"] = json!(names.get(&r.delegator_user_id));
            v["delegateName"] = json!(names.get(&r.delegate_user_id));
            v["active"] = json!(delegation::is_effective(r, today));
            v["mine"] = json!(r.delegator_user_id == user.id);
            v
        })
        .collect();
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

/// 本人可以幫自己設代理（flow:act）；幫別人設才需要代理管理權。
async fn delegation_gate(core: &Core, user: &CurrentUser, org: i64, delegator: i64, meta: &RequestMeta) -> ApiResult<Ctx> {
    if delegator != user.id {
        gate(core, user, org, gates::DELEGATION_MANAGE, meta).await
    } else {
        gate(core, user, org, gates::FLOW_ACT, meta).await
    }
}

pub async fn create_delegation(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let input: CreateDelegationInput = parse_body(&body)?;
    let ctx = delegation_gate(&core, &user, org.into_inner(), input.delegator_user_id, &meta).await?;
    let valid_from = parse_day(&input.valid_from).ok_or_else(|| err_d(400, "invalid_body", json!({ "field": "validFrom" })))?;
    let valid_to = input.valid_to.flatten().as_deref().and_then(parse_day);
    let mut tx = core.pool.begin().await?;
    let row = delegation::create_delegation(
        &mut tx,
        &ctx,
        NewDelegation {
            delegator_user_id: input.delegator_user_id,
            delegate_user_id: input.delegate_user_id,
            valid_from,
            valid_to,
            reason: input.reason,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "id": row.id })))
}

pub async fn revoke_delegation(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    // 先過最寬的閘再看是誰的代理（判權前不讀資料）。
    let probe = gate(&core, &user, org, &[gates::FLOW_ACT[0], gates::DELEGATION_MANAGE[0]], &meta).await?;
    let mut tx = core.pool.begin().await?;
    let row = delegation::delegation_in_org(&mut tx, probe.org_id(), id).await?;
    let ctx = delegation_gate(&core, &user, org, row.delegator_user_id, &meta).await?;
    delegation::revoke_delegation(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

// ── 血緣 ──

pub async fn link_lineage(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let ctx = gate(&core, &user, org.into_inner(), gates::FLOW_ACT, &meta).await?;
    let input: LinkLineageInput = parse_body(&body)?;
    let kind = input.relation_kind.map(|k| match k {
        RelationKind::Derived => "derived",
        RelationKind::Reversal => "reversal",
        RelationKind::Correction => "correction",
        RelationKind::Merged => "merged",
    });
    let mut tx = core.pool.begin().await?;
    let id = lineage::link_lineage(
        &mut tx,
        &ctx,
        NewLineage {
            upstream_execution_id: input.upstream_execution_id,
            downstream_execution_id: input.downstream_execution_id,
            relation_kind: kind.map(String::from),
            note: input.note,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "id": id })))
}

// ── 流程總覽 ──

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewQuery {
    stuck_days: Option<i64>,
    with_load: Option<String>,
}

pub async fn workflows(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, q: web::Query<OverviewQuery>) -> ApiResult<HttpResponse> {
    let ctx = gate(&core, &user, org.into_inner(), gates::FLOW_VIEW, &meta).await?;
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(graph::overview(&mut tx, &ctx, q.stuck_days.unwrap_or(graph::DEFAULT_STUCK_DAYS)).await?))
}

pub async fn workflow_graph(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, q: web::Query<OverviewQuery>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_VIEW, &meta).await?;
    let mut tx = core.pool.begin().await?;
    let with_load = q.with_load.as_deref() != Some("0");
    let g = graph::definition_graph(&mut tx, &ctx, id, with_load, q.stuck_days.unwrap_or(graph::DEFAULT_STUCK_DAYS)).await?;
    Ok(HttpResponse::Ok().json(g))
}

/// 點節點 → 卡在這一關的那幾張單（列與工作台同一份 DTO）。
pub async fn workflow_node_tasks(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64, String)>) -> ApiResult<HttpResponse> {
    let (org, def_id, node_key) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_VIEW, &meta).await?;
    let mut tx = core.pool.begin().await?;
    let cols: String = model::TASK_COLS.split(',').map(|c| format!("t.{}", c.trim())).collect::<Vec<_>>().join(", ");
    let rows: Vec<model::TaskRow> = sqlx::query_as(&format!(
        "SELECT {cols} FROM tbl_energy_workflow_task t
           JOIN tbl_energy_workflow_execution e ON e.id = t.execution_id
          WHERE e.organization_id = $1 AND e.definition_id = $2 AND e.status = 'running' AND e.deleted_at IS NULL
            AND t.node_key = $3 AND t.state = ANY($4) AND t.deleted_at IS NULL ORDER BY t.id"
    ))
    .bind(ctx.org_id())
    .bind(def_id)
    .bind(&node_key)
    .bind(&model::ACTIVE_STATES[..])
    .fetch_all(&mut *tx)
    .await?;
    let items = views::decorate_tasks(&mut tx, &ctx, user.id, &rows).await?;
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

/// 冪等安裝內建流程定義、預設計算策略與定義引用的部門。
pub async fn install_defaults(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>) -> ApiResult<HttpResponse> {
    let ctx = gate(&core, &user, org.into_inner(), gates::WORKFLOW_MANAGE, &meta).await?;
    let mut tx = core.pool.begin().await?;
    let report = super::definition::install_builtin_definitions(&mut tx, &ctx).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(report))
}

/// 給其他模組（收件）用：是否持有 view-all。
pub async fn view_all(core: &CoreRef, access: &OrgAccess) -> ApiResult<bool> {
    has_view_all(core, access).await
}
