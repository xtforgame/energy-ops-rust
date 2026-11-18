//! 儀表板、行事曆、登入落點、角色工作台。
//! 分區、排序、等待天數、退回理由全部在後端算 —— 只有後端知道呼叫者的角色與部門身分。

use crate::common::{err, err_d, has_permission, parse_day, query_day, Ctx, RequestMeta, Tx};
use crate::workflow::views::{decorate_tasks, my_open_tasks};
use crate::{gates, path};
use actix_web::{web, HttpResponse};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use eo_platform::auth::CurrentUser;
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use eo_platform::plugin::{AppPlugin, RouteRegistrar};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

/// 「已放行超過 N 天未付」的門檻 —— 常數不是前端 hardcode。
pub const PAYOUT_OVERDUE_DAYS: i64 = 5;

/// 「已放行未付」的關卡（兩個流程的付款關 key 不一樣）。儀表板與工作台必須用同一組。
pub const PAYMENT_NODE_KEYS: [&str; 2] = ["payment", "pay"];

pub struct WorkbenchPlugin;

crate::energy_plugin!(WorkbenchPlugin, "energy_workbench");

impl AppPlugin for WorkbenchPlugin {
    fn configure_routes(&self, cfg: &mut RouteRegistrar<'_>, _self_arc: &Arc<dyn AppPlugin>) {
        cfg.get(&path("/dashboard"), web::get().to(dashboard_handler));
        cfg.get(&path("/calendar-events"), web::get().to(calendar_handler));
        cfg.post(&path("/calendar-events"), web::post().to(create_calendar_event_handler));
        cfg.patch(&path("/calendar-events/{eventId}"), web::patch().to(update_calendar_event_handler));
        cfg.get(&path("/me/landing"), web::get().to(landing_handler));
        cfg.get(&path("/workbench"), web::get().to(workbench_handler));
    }
}

fn month_bounds(month: &str) -> Option<(NaiveDate, NaiveDate)> {
    let start = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d").ok().filter(|_| month.len() == 7)?;
    let next = if start.month() == 12 {
        NaiveDate::from_ymd_opt(start.year() + 1, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(start.year(), start.month() + 1, 1)?
    };
    Some((start, next))
}

/// 儀表板：本月應付（只算現行的）、幾件卡關、已放行未付、例外件、案場狀態分布。
pub async fn dashboard(tx: &mut Tx<'_>, ctx: &Ctx, month: Option<&str>, stuck_days: i64) -> ApiResult<Value> {
    let today = ctx.today();
    let month = month
        .filter(|m| month_bounds(m).is_some())
        .map(String::from)
        .unwrap_or_else(|| format!("{:04}-{:02}", today.year(), today.month()));
    let (start, end) = month_bounds(&month).ok_or_else(|| err(400, "invalid_month"))?;
    let org = ctx.org_id();
    // 金額用定點加總（不用浮點：這是錢）。
    let payouts: Vec<Decimal> = sqlx::query_scalar(
        "SELECT amount FROM tbl_energy_payout_record
          WHERE organization_id = $1 AND status = 'active' AND business_date >= $2 AND business_date < $3 AND deleted_at IS NULL ORDER BY id",
    )
    .bind(org)
    .bind(start)
    .bind(end)
    .fetch_all(&mut **tx)
    .await?;
    let total: Decimal = payouts.iter().copied().sum();
    let stations: Vec<String> =
        sqlx::query_scalar("SELECT status FROM tbl_energy_station WHERE organization_id = $1 AND deleted_at IS NULL ORDER BY id")
            .bind(org)
            .fetch_all(&mut **tx)
            .await?;
    let mut by_status: BTreeMap<String, i64> = BTreeMap::new();
    for s in &stations {
        *by_status.entry(s.clone()).or_default() += 1;
    }
    let running: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tbl_energy_workflow_execution WHERE organization_id = $1 AND status = 'running' AND deleted_at IS NULL",
    )
    .bind(org)
    .fetch_one(&mut **tx)
    .await?;
    let unmatched: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tbl_energy_bill WHERE organization_id = $1 AND review_status = 'pendingMatch' AND deleted_at IS NULL",
    )
    .bind(org)
    .fetch_one(&mut **tx)
    .await?;
    // S2：找不到生效合約跟找不到案場是兩種各自的待辦，各自一格。
    let no_contract: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tbl_energy_bill WHERE organization_id = $1 AND review_status = 'pendingContract' AND deleted_at IS NULL",
    )
    .bind(org)
    .fetch_one(&mut **tx)
    .await?;
    // 卡關：running 的執行裡，有未完成的 task 已經等超過門檻天數。
    let open: Vec<(i64, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT t.execution_id, t.node_key, t.created_at FROM tbl_energy_workflow_task t
           JOIN tbl_energy_workflow_execution e ON e.id = t.execution_id
          WHERE e.organization_id = $1 AND e.status = 'running' AND e.deleted_at IS NULL AND t.state = ANY($2) AND t.deleted_at IS NULL",
    )
    .bind(org)
    .bind(&crate::workflow::model::ACTIVE_STATES[..])
    .fetch_all(&mut **tx)
    .await?;
    let now = ctx.now();
    let mut stuck = HashSet::new();
    let mut released_unpaid = HashSet::new();
    for (e, k, at) in &open {
        if PAYMENT_NODE_KEYS.contains(&k.as_str()) {
            released_unpaid.insert(*e);
        }
        if now - *at > chrono::Duration::days(stuck_days) {
            stuck.insert(*e);
        }
    }
    Ok(json!({
        "month": month,
        "payableThisMonth": format!("{:.2}", total),
        "payoutCount": payouts.len(),
        "runningCount": running,
        "stuckDays": stuck_days,
        "stuckCount": stuck.len(),
        "releasedUnpaidCount": released_unpaid.len(),
        "unmatchedCount": unmatched,
        "noContractCount": no_contract,
        "stationTotal": stations.len(),
        "stationByStatus": by_status,
    }))
}

/// 行事曆（eventDate 是日期，兩端都用 ≤／≥）。卡 E：多回案場（id／名）、實際發生日。
pub async fn calendar_events(tx: &mut Tx<'_>, ctx: &Ctx, from: Option<NaiveDate>, to: Option<NaiveDate>) -> ApiResult<Vec<Value>> {
    let rows: Vec<(i64, NaiveDate, String, String, String, Option<i64>, Option<String>, Option<i64>, Option<String>, Option<NaiveDate>)> = sqlx::query_as(
        "SELECT e.id, e.event_date, e.kind, e.title, e.status, e.landowner_id, l.name, e.station_id, s.name, e.occurred_on
           FROM tbl_energy_calendar_event e
           LEFT JOIN tbl_energy_landowner l ON l.id = e.landowner_id AND l.organization_id = e.organization_id AND l.deleted_at IS NULL
           LEFT JOIN tbl_energy_station s ON s.id = e.station_id AND s.organization_id = e.organization_id AND s.deleted_at IS NULL
          WHERE e.organization_id = $1 AND e.deleted_at IS NULL
            AND ($2::date IS NULL OR e.event_date >= $2) AND ($3::date IS NULL OR e.event_date <= $3)
          ORDER BY e.event_date ASC, e.id ASC LIMIT 500",
    )
    .bind(ctx.org_id())
    .bind(from)
    .bind(to)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, d, kind, title, status, owner, owner_name, station_id, station_name, occurred_on)| {
            json!({ "id": id, "eventDate": crate::common::day(d), "kind": kind, "title": title, "status": status,
                    "landownerId": owner, "landownerName": owner_name, "stationId": station_id, "stationName": station_name,
                    "occurredOn": crate::common::day_opt(occurred_on) })
        })
        .collect())
}

/// 手動新增預定事件（卡 E S1）：事前提醒、逾期都靠這一列的 `eventDate`／`kind`。
pub async fn create_calendar_event(tx: &mut Tx<'_>, ctx: &Ctx, b: &Value) -> ApiResult<i64> {
    let title = b.get("title").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| err(400, "title_required"))?;
    let kind = b.get("kind").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| err(400, "kind_required"))?;
    let event_date_str = b.get("eventDate").and_then(Value::as_str).ok_or_else(|| err(400, "event_date_required"))?;
    let event_date = parse_day(event_date_str).ok_or_else(|| err_d(400, "invalid_date", json!({ "field": "eventDate" })))?;
    let station_id = b.get("stationId").and_then(Value::as_i64);
    let landowner_id = b.get("landownerId").and_then(Value::as_i64);
    Ok(sqlx::query_scalar(
        "INSERT INTO tbl_energy_calendar_event (organization_id, station_id, landowner_id, event_date, kind, title, status)
         VALUES ($1,$2,$3,$4,$5,$6,'pending') RETURNING id",
    )
    .bind(ctx.org_id())
    .bind(station_id)
    .bind(landowner_id)
    .bind(event_date)
    .bind(&kind)
    .bind(title)
    .fetch_one(&mut **tx)
    .await?)
}

/// 記實際發生日／改狀態（卡 E S1）：填了 `occurredOn` 就自動轉 `done`（不用使用者另外切狀態）。
pub async fn update_calendar_event(tx: &mut Tx<'_>, ctx: &Ctx, id: i64, b: &Value) -> ApiResult<()> {
    let org_of: Option<i64> = sqlx::query_scalar("SELECT organization_id FROM tbl_energy_calendar_event WHERE id = $1 AND deleted_at IS NULL")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
    let org_of = org_of.ok_or_else(|| err(404, "event_not_found"))?;
    if org_of != ctx.org_id() {
        return Err(err(403, "event_not_in_org"));
    }
    let occurred_on = match b.get("occurredOn") {
        None => None,
        Some(Value::Null) => None,
        Some(Value::String(s)) if s.is_empty() => None,
        Some(Value::String(s)) => Some(parse_day(s).ok_or_else(|| err_d(400, "invalid_date", json!({ "field": "occurredOn" })))?),
        Some(_) => return Err(err_d(400, "invalid_date", json!({ "field": "occurredOn" }))),
    };
    // 填了實際發生日 → 一律自動轉「已完成」，不管畫面上的狀態下拉選了什麼
    // （對話框預設帶著目前狀態，使用者不會特地把它切回 pending 再存）。
    // 只有在沒有實際發生日時，才照 body 給的 status 或保留原狀態。
    let status = if occurred_on.is_some() {
        "done".to_string()
    } else {
        match b.get("status").and_then(Value::as_str) {
            Some(s) => s.to_string(),
            None => {
                let current: String =
                    sqlx::query_scalar("SELECT status FROM tbl_energy_calendar_event WHERE id = $1").bind(id).fetch_one(&mut **tx).await?;
                current
            }
        }
    };
    sqlx::query("UPDATE tbl_energy_calendar_event SET occurred_on = $2, status = $3 WHERE id = $1 AND deleted_at IS NULL")
        .bind(id)
        .bind(occurred_on)
        .bind(&status)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// 呼叫者在這個組織實際持有的能源權限（逐一問，分區判斷用）。
pub async fn energy_permissions_of(ctx: &Ctx) -> ApiResult<Vec<String>> {
    const CANDIDATES: [&str; 10] = [
        "energy:operator",
        "energy:flow:act",
        "energy:flow:view",
        "energy:flow:view-all",
        "energy:payout:view",
        "energy:audit:view",
        "energy:workflow:manage",
        "energy:flow:abort-approve",
        "energy:member:manage",
        eo_platform::rbac::PLATFORM_RBAC_MANAGE,
    ];
    let mut held = Vec::new();
    for p in CANDIDATES {
        if has_permission(&ctx.core, &ctx.access, p).await? {
            held.push(p.to_string());
        }
    }
    Ok(held)
}

/// 「做事身分」—— 部門主管是組織事實不是 RBAC 角色，要從部門成員表推導。
async fn who_am_i(tx: &mut Tx<'_>, ctx: &Ctx, user_id: i64, permissions: &[String]) -> ApiResult<Value> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT d.key, m.role FROM mn_energy_department_member m
           JOIN tbl_energy_department d ON d.id = m.department_id AND d.organization_id = $1 AND d.deleted_at IS NULL
          WHERE m.user_id = $2 AND m.deleted_at IS NULL ORDER BY m.id",
    )
    .bind(ctx.org_id())
    .bind(user_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(json!({
        "roles": permissions,
        "isDepartmentManager": rows.iter().any(|r| r.1 == "manager"),
        "departmentKeys": rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>(),
    }))
}

/// 登入落點。順序＝優先級，從「最專一的訊號」開始判（大家都有的 flow:act 不能當身分訊號）。
pub async fn landing(tx: &mut Tx<'_>, ctx: &Ctx, user_id: i64) -> ApiResult<Value> {
    let perms = energy_permissions_of(ctx).await?;
    let identity = who_am_i(tx, ctx, user_id, &perms).await?;
    let has = |p: &str| perms.iter().any(|x| x == p);
    let (route, reason) = if has("energy:workflow:manage") {
        ("energyWorkflowConsole", "admin")
    } else if has("energy:member:manage") {
        // 高階主管必須排在財務前面：CXO 也綁了 finance:act。
        ("energyDashboardConsole", "cxo")
    } else if has("energy:payout:view") {
        ("energyMyWorkConsole", "finance")
    } else if identity["isDepartmentManager"].as_bool() == Some(true) {
        ("energyMyWorkConsole", "departmentManager")
    } else if has("energy:operator") {
        ("energyIngestConsole", "operator")
    } else if has("energy:flow:view-all") {
        ("energyDashboardConsole", "cxo")
    } else if has("energy:audit:view") {
        ("energyAuditConsole", "viewer")
    } else if has("energy:flow:act") {
        ("energyIngestConsole", "operator")
    } else {
        // 落不到任何身分時回總覽 —— 不要回空白。
        ("energyDashboardConsole", "default")
    };
    Ok(json!({ "route": route, "reason": reason }))
}

/// 被退回要我補：退回理由與退回的人直接印在列上。資料在稽核（flow.task_sent_back），不在 task 上。
async fn sent_back_rows(tx: &mut Tx<'_>, ctx: &Ctx, tasks: &[crate::workflow::model::TaskRow], rows: &[Value]) -> ApiResult<Vec<Value>> {
    if tasks.is_empty() {
        return Ok(vec![]);
    }
    let exec_ids: Vec<i64> = crate::common::uniq(tasks.iter().map(|t| t.execution_id));
    // 退回事件的 target 是被退回的那個 task；以它所屬的 execution 對回我現在的待辦。
    let events: Vec<(i64, Option<i64>, Value, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT t.execution_id, a.actor_user_id, a.payload, a.summary, a.occurred_at FROM tbl_energy_audit_event a
           JOIN tbl_energy_workflow_task t ON t.id::text = a.target_id AND t.deleted_at IS NULL
          WHERE a.organization_id = $1 AND a.type = 'flow.task_sent_back' AND a.target_type = 'energyWorkflowTask'
            AND t.execution_id = ANY($2) AND a.deleted_at IS NULL
          ORDER BY a.occurred_at DESC, a.id DESC",
    )
    .bind(ctx.org_id())
    .bind(&exec_ids)
    .fetch_all(&mut **tx)
    .await?;
    let mut latest: HashMap<i64, &(i64, Option<i64>, Value, String, DateTime<Utc>)> = HashMap::new();
    for e in &events {
        latest.entry(e.0).or_insert(e);
    }
    if latest.is_empty() {
        return Ok(vec![]);
    }
    let actor_ids: Vec<i64> = crate::common::uniq(latest.values().filter_map(|e| e.1));
    let names = crate::common::user_names(tx, &actor_ids).await?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            let e = latest.get(&r["executionId"].as_i64()?)?;
            let mut row = r.clone();
            row["rejectReason"] = e.2.get("reason").cloned().unwrap_or_else(|| json!(e.3));
            row["rejectedBy"] = json!(e.1.map(|u| names.get(&u).cloned().unwrap_or_else(|| format!("#{u}"))));
            row["rejectedAt"] = json!(e.4);
            Some(row)
        })
        .collect())
}

struct Section {
    key: &'static str,
    title_key: &'static str,
    sort: &'static str,
    overdue_days: Option<i64>,
    rows: Vec<Value>,
    tier: u8,
}

fn waiting(r: &Value) -> i64 {
    r["waitingDays"].as_i64().unwrap_or(0)
}

/// 工作台：依角色變形的分區（聯集，該出現的都出現；順序由優先序決定）。空的區不顯示。
pub async fn workbench(tx: &mut Tx<'_>, ctx: &Ctx, user_id: i64) -> ApiResult<Value> {
    let perms = energy_permissions_of(ctx).await?;
    let identity = who_am_i(tx, ctx, user_id, &perms).await?;
    let tasks = my_open_tasks(tx, ctx.org_id(), user_id).await?;
    let rows = decorate_tasks(tx, ctx, user_id, &tasks).await?;
    let at = |keys: &[&str]| -> Vec<Value> {
        rows.iter().filter(|r| r["nodeKey"].as_str().is_some_and(|k| keys.contains(&k))).cloned().collect()
    };
    let mut sections: Vec<Section> = Vec::new();
    let push = |sections: &mut Vec<Section>, key, title_key, mut rs: Vec<Value>, sort, overdue_days, tier| {
        if rs.is_empty() {
            return;
        }
        rs.sort_by_key(|r| std::cmp::Reverse(waiting(r)));
        sections.push(Section { key, title_key, sort, overdue_days, rows: rs, tier });
    };
    // 層 2：有人在等我動作。
    push(&mut sections, "sentBack", "energy.workbench.sentBack", sent_back_rows(tx, ctx, &tasks, &rows).await?, "waitingDaysDesc", None, 2);
    push(&mut sections, "toApprove", "energy.workbench.toApprove", at(&["clerk-review", "manager-approve", "exec-approve"]), "waitingDaysDesc", None, 2);
    push(&mut sections, "toRelease", "energy.workbench.toRelease", at(&["finance-release"]), "waitingDaysDesc", None, 2);
    // 這一區的「幾天」是從放行到現在（task 是放行後才建的）。
    push(&mut sections, "toPay", "energy.workbench.toPay", at(&PAYMENT_NODE_KEYS), "releasedDaysDesc", Some(PAYOUT_OVERDUE_DAYS), 2);
    // 層 3：我在等別人。
    let claimed: HashSet<i64> = sections.iter().flat_map(|s| s.rows.iter().filter_map(|r| r["id"].as_i64())).collect();
    let others: Vec<Value> = rows.iter().filter(|r| r["id"].as_i64().is_some_and(|id| !claimed.contains(&id))).cloned().collect();
    push(&mut sections, "others", "energy.workbench.others", others, "waitingDaysDesc", None, 3);
    // 層 1：超時的區升上來（不另開一區 —— 同一張單不出現兩次）。
    for s in &mut sections {
        let threshold = s.overdue_days.unwrap_or(PAYOUT_OVERDUE_DAYS);
        if s.tier == 2 && s.rows.iter().any(|r| waiting(r) >= threshold) {
            s.tier = 1;
        }
    }
    let max_wait = |s: &Section| s.rows.iter().map(waiting).max().unwrap_or(0).max(0);
    sections.sort_by(|a, b| a.tier.cmp(&b.tier).then(max_wait(b).cmp(&max_wait(a))));
    let out: Vec<Value> = sections
        .into_iter()
        .map(|s| {
            let mut v = json!({ "key": s.key, "titleKey": s.title_key, "sort": s.sort, "rows": s.rows });
            if let Some(d) = s.overdue_days {
                v["overdueDays"] = json!(d);
            }
            v
        })
        .collect();
    Ok(json!({ "identity": identity, "payoutOverdueDays": PAYOUT_OVERDUE_DAYS, "sections": out }))
}

// ── handlers ──

type Core = web::Data<CoreRef>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DashQuery {
    month: Option<String>,
    stuck_days: Option<i64>,
    from: Option<String>,
    to: Option<String>,
}

async fn view_ctx(core: &Core, user: &CurrentUser, org: i64, meta: &RequestMeta) -> ApiResult<Ctx> {
    let access = core.rbac.authorize_org(user, org, gates::FLOW_VIEW).await?;
    Ok(Ctx::new(core, access, meta))
}

async fn dashboard_handler(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, q: web::Query<DashQuery>) -> ApiResult<HttpResponse> {
    let ctx = view_ctx(&core, &user, org.into_inner(), &meta).await?;
    let mut tx = core.pool.begin().await?;
    let stuck = q.stuck_days.unwrap_or(crate::workflow::graph::DEFAULT_STUCK_DAYS);
    Ok(HttpResponse::Ok().json(dashboard(&mut tx, &ctx, q.month.as_deref(), stuck).await?))
}

async fn calendar_handler(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, q: web::Query<DashQuery>) -> ApiResult<HttpResponse> {
    let ctx = view_ctx(&core, &user, org.into_inner(), &meta).await?;
    // 壞掉的日期參數必須是 400 不是 500。
    let from = query_day("from", q.from.as_deref())?;
    let to = query_day("to", q.to.as_deref())?;
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(json!({ "items": calendar_events(&mut tx, &ctx, from, to).await? })))
}

async fn create_calendar_event_handler(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::MASTER_MANAGE).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    let id = create_calendar_event(&mut tx, &ctx, &body).await?;
    let items = calendar_events(&mut tx, &ctx, None, None).await?;
    tx.commit().await?;
    let created = items.into_iter().find(|v| v["id"] == id).unwrap_or(json!({ "id": id }));
    Ok(HttpResponse::Ok().json(created))
}

async fn update_calendar_event_handler(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::MASTER_MANAGE).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    update_calendar_event(&mut tx, &ctx, id, &body).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

async fn landing_handler(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>) -> ApiResult<HttpResponse> {
    let ctx = view_ctx(&core, &user, org.into_inner(), &meta).await?;
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(landing(&mut tx, &ctx, user.id).await?))
}

async fn workbench_handler(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>) -> ApiResult<HttpResponse> {
    let ctx = view_ctx(&core, &user, org.into_inner(), &meta).await?;
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(workbench(&mut tx, &ctx, user.id).await?))
}
