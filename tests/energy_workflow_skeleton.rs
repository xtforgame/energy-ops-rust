//! 流程引擎 walking skeleton：兩關流程 seed → 發起 → 認領 → 推進 → 完成，每一步都有稽核。

use crate::support::energy::{code_of, date, excluded, potential, sorted};
use crate::support::{TestApp, TestUser};
use eo_domain::workflow_defs::{minimal_bill_approval, AssignmentRule, NodeDef, NodeType, TransitionDef, WorkflowDefinition};
use eo_energy::workflow::engine::run::{start_execution, StartExecution};
use eo_energy::workflow::engine::tasks::{self, ActionInput};
use serde_json::json;

struct Fx {
    app: TestApp,
    org: i64,
    station: String,
    starter: TestUser,
    operator: TestUser,
    finance: TestUser,
    outsider: TestUser,
}

async fn fixture() -> Fx {
    let app = TestApp::new().await;
    let (owner, org) = app.energy_org("wf-org").await;
    let starter = app.member(org, "starter", Some("energy:operator")).await;
    let operator = app.member(org, "operator", Some("energy:operator")).await;
    let finance = app.member(org, "finance", Some("energy:finance")).await;
    let outsider = app.member(org, "outsider", None).await;
    let def = app.install_def(&owner, org, &minimal_bill_approval(), None).await;
    let station = "00-11-2222-33-4".to_string();
    app.wf_binding(org, Some(&station), None, None, def, "2000-01-01", None).await;
    Fx { app, org, station, starter, operator, finance, outsider }
}

fn start_input(station: &str, key: &str, subject: &str) -> StartExecution {
    StartExecution {
        station_key: station.into(),
        workflow_key: key.into(),
        subject_type: "energyBill".into(),
        subject_id: subject.into(),
        business_date: date("2026-05-31"),
        attributes: json!({ "bill": { "totalAmount": 12345 } }),
        definition: None,
        station_id: None,
    }
}

fn act(key: &str) -> ActionInput {
    ActionInput { action_key: key.into(), ..Default::default() }
}

/// 來源：workflowSkeleton.test.ts「seeds a published two-gate definition with nodes and transitions」
/// 來源：workflowSkeleton.test.ts「assigns version numbers per (org, key) — a second seed becomes v2」
#[actix_web::test]
async fn seeds_versions_per_org_and_key() {
    let f = fixture().await;
    let (status, version, parent): (String, i32, Option<i64>) = sqlx::query_as(
        "SELECT status, version, parent_version_id FROM tbl_energy_workflow_definition WHERE organization_id = $1 AND key = 'bill-approval'",
    )
    .bind(f.org)
    .fetch_one(&f.app.core.pool)
    .await
    .unwrap();
    assert_eq!((status.as_str(), version, parent), ("published", 1, None));
    let nodes: Vec<(String, String, bool)> = sqlx::query_as(
        "SELECT n.node_key, n.node_type, n.allow_self_approval FROM tbl_energy_workflow_node n
           JOIN tbl_energy_workflow_definition d ON d.id = n.definition_id WHERE d.organization_id = $1 ORDER BY n.id",
    )
    .bind(f.org)
    .fetch_all(&f.app.core.pool)
    .await
    .unwrap();
    let keys: Vec<&str> = nodes.iter().map(|n| n.0.as_str()).collect();
    assert_eq!(keys, ["start", "operatorCheck", "financeApproval", "end"]);
    // 預設 false ＝ 發起人不得核自己的單。
    assert!(nodes.iter().filter(|n| n.1 == "humanTask").all(|n| !n.2));

    let mut v2 = minimal_bill_approval();
    v2.name = "電費單核可 v2".into();
    let owner = f.app.member(f.org, "owner2", Some("energy:admin")).await;
    let id = f.app.install_def(&owner, f.org, &v2, None).await;
    let ver: i32 = sqlx::query_scalar("SELECT version FROM tbl_energy_workflow_definition WHERE id = $1").bind(id).fetch_one(&f.app.core.pool).await.unwrap();
    assert_eq!(ver, 2);
}

/// 來源：workflowSkeleton.test.ts「resolves the definition by BUSINESS date, not now()」
/// 來源：workflowSkeleton.test.ts「no binding for that date is an ERROR, not a silent fallback to the latest version」
#[actix_web::test]
async fn resolves_by_business_date_and_errors_without_binding() {
    let f = fixture().await;
    let admin = f.app.member(f.org, "admin2", Some("energy:admin")).await;
    let mut a = minimal_bill_approval();
    a.key = "dated".into();
    a.name = "舊約".into();
    let mut b = a.clone();
    b.name = "新約".into();
    let ida = f.app.install_def(&admin, f.org, &a, None).await;
    let idb = f.app.install_def(&admin, f.org, &b, Some(ida)).await;
    f.app.wf_binding(f.org, Some("ST-D"), None, None, ida, "2026-01-01", Some("2026-06-01")).await;
    f.app.wf_binding(f.org, Some("ST-D"), None, None, idb, "2026-06-01", None).await;
    let ctx = f.app.ctx(&admin, f.org).await;
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let resolve = eo_energy::workflow::definition::resolve_definition;
    assert_eq!(resolve(&mut tx, &ctx.access, "ST-D", "dated", date("2026-05-31")).await.unwrap().name, "舊約");
    assert_eq!(resolve(&mut tx, &ctx.access, "ST-D", "dated", date("2026-07-01")).await.unwrap().name, "新約");
    // 半開區間：生效當天屬於新的那一段。
    assert_eq!(resolve(&mut tx, &ctx.access, "ST-D", "dated", date("2026-06-01")).await.unwrap().name, "新約");
    let e = resolve(&mut tx, &ctx.access, "nowhere", "bill-approval", date("2026-05-01")).await;
    assert_eq!(code_of(e), (404, "no_binding_for_business_date".into()));
}

/// 來源：workflowSkeleton.test.ts「startExecution creates the execution and lands on the first human task」
/// 來源：workflowSkeleton.test.ts「the candidate pool is a snapshot and records who was EXCLUDED」
/// 來源：workflowSkeleton.test.ts「someone outside the candidate pool cannot claim (fail-closed)」
/// 來源：workflowSkeleton.test.ts「the initiator cannot claim his own gate either (allowSelfApproval=false)」
/// 來源：workflowSkeleton.test.ts「claim → reserved, release → ready, claim again」
/// 來源：workflowSkeleton.test.ts「a non-owner cannot complete the task」
/// 來源：workflowSkeleton.test.ts「an actionKey the node does not define is 404, NOT a silent advance」
/// 來源：workflowSkeleton.test.ts「advance gate 1 → the second gate opens with its own candidate pool」
/// 來源：workflowSkeleton.test.ts「advance gate 2 → the execution completes」
/// 來源：workflowSkeleton.test.ts「every step of the run left an audit row, in order」
/// 來源：workflowSkeleton.test.ts「flow.started records WHY that version was bound」
/// 來源：workflowSkeleton.test.ts「the candidates_resolved rows record the pool and who resolved it」
/// 來源：workflowSkeleton.test.ts「task_completed records the next node — the chain is readable from audit alone」
#[actix_web::test]
async fn walking_skeleton_end_to_end_with_audit_chain() {
    let f = fixture().await;
    let app = &f.app;
    let starter = app.ctx(&f.starter, f.org).await;
    let operator = app.ctx(&f.operator, f.org).await;
    let finance = app.ctx(&f.finance, f.org).await;
    let outsider = app.ctx(&f.outsider, f.org).await;

    let exec = in_tx!(app, tx => start_execution(&mut tx, &starter, start_input(&f.station, "bill-approval", "BILL-2026-05-0001")).await).unwrap();
    assert_eq!(exec.status, "running");
    assert_eq!(exec.current_node_key.as_deref(), Some("operatorCheck"));
    assert_eq!(exec.started_by_user_id, Some(f.starter.id));
    assert_eq!(exec.business_date, date("2026-05-31"));
    let ts = app.tasks(exec.id).await;
    assert_eq!(ts.len(), 1);
    assert_eq!((ts[0].node_key.as_str(), ts[0].state.as_str(), ts[0].seq), ("operatorCheck", "ready", 1));
    let gate1 = ts[0].id;

    // 發起人也持 operator，但被排除 —— 排除寫成資料，不是過濾掉就算了。
    let c = app.candidates(gate1).await;
    assert_eq!(potential(&c), vec![f.operator.id]);
    assert_eq!(excluded(&c), vec![f.starter.id]);
    assert_eq!(c.iter().find(|x| x.0 == "potentialOwner").unwrap().2, "role");

    assert_eq!(code_of(in_tx!(app, tx => tasks::claim(&mut tx, &outsider, gate1).await)), (403, "not_a_candidate".into()));
    assert_eq!(code_of(in_tx!(app, tx => tasks::claim(&mut tx, &starter, gate1).await)), (403, "not_a_candidate".into()));

    in_tx!(app, tx => tasks::claim(&mut tx, &operator, gate1).await).unwrap();
    let t = &app.tasks(exec.id).await[0];
    assert_eq!((t.state.as_str(), t.assignee_user_id), ("reserved", Some(f.operator.id)));
    in_tx!(app, tx => tasks::release(&mut tx, &operator, gate1).await).unwrap();
    let t = &app.tasks(exec.id).await[0];
    assert_eq!((t.state.as_str(), t.assignee_user_id), ("ready", None));
    in_tx!(app, tx => tasks::claim(&mut tx, &operator, gate1).await).unwrap();
    assert_eq!(app.tasks(exec.id).await[0].state, "reserved");

    assert_eq!(code_of(in_tx!(app, tx => tasks::perform_action(&mut tx, &finance, gate1, act("submit")).await)), (403, "not_task_owner".into()));
    // 動作是定義的一部分：節點上沒有這顆按鈕就是沒有（靜默當成 advance 會讓「駁回」變成「核准」）。
    for key in ["approve", "sendBack", "markPaid", "madeUpKey"] {
        let r = in_tx!(app, tx => tasks::perform_action(&mut tx, &operator, gate1, act(key)).await);
        assert_eq!(code_of(r), (404, "action_not_found".into()), "{key}");
    }

    let mut submit = act("submit");
    submit.payload = json!({ "checkedAmount": 12345 }).as_object().cloned();
    in_tx!(app, tx => tasks::perform_action(&mut tx, &operator, gate1, submit).await).unwrap();
    let ts = app.tasks(exec.id).await;
    assert_eq!(ts.len(), 2);
    assert_eq!((ts[0].state.as_str(), ts[0].outcome.as_deref()), ("completed", Some("approved")));
    assert_eq!(ts[0].payload, json!({ "checkedAmount": 12345 }));
    assert_eq!((ts[1].node_key.as_str(), ts[1].state.as_str(), ts[1].seq), ("financeApproval", "ready", 2));
    assert_eq!(app.exec_state(exec.id).await, ("running".into(), Some("financeApproval".into())));
    assert_eq!(potential(&app.candidates(ts[1].id).await), vec![f.finance.id]);

    let gate2 = ts[1].id;
    in_tx!(app, tx => tasks::claim(&mut tx, &finance, gate2).await).unwrap();
    in_tx!(app, tx => tasks::perform_action(&mut tx, &finance, gate2, act("approve")).await).unwrap();
    assert_eq!(app.exec_state(exec.id).await, ("completed".into(), Some("end".into())));
    let ended: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT ended_at FROM tbl_energy_workflow_execution WHERE id = $1").bind(exec.id).fetch_one(&app.core.pool).await.unwrap();
    assert!(ended.is_some());
    assert!(app.tasks(exec.id).await.iter().all(|t| t.state == "completed"));

    // 順序：原因在前、結果在後。
    assert_eq!(
        app.audit_types_for_execution(f.org, exec.id).await,
        [
            "flow.started",
            "flow.candidates_resolved",
            "flow.task_claimed",
            "flow.task_released",
            "flow.task_claimed",
            "flow.task_completed",
            "flow.candidates_resolved",
            "flow.task_claimed",
            "flow.task_completed",
            "flow.completed",
        ]
    );
    let started = &app.audit_rows(f.org, "energyWorkflowExecution", exec.id).await[0];
    assert_eq!(started.0, "flow.started");
    assert_eq!(started.1["workflowKey"], "bill-approval");
    assert_eq!(started.1["businessDate"], "2026-05-31");
    assert_eq!(started.1["version"], 1);
    assert!(started.1["definitionId"].as_i64().unwrap() > 0);
    assert_eq!(started.3, Some(f.starter.id), "actor 是發起人");

    let g1 = app.audit_rows(f.org, "energyWorkflowTask", gate1).await;
    let resolved = g1.iter().find(|r| r.0 == "flow.candidates_resolved").unwrap();
    assert_eq!(resolved.1["potentialOwners"], json!([f.operator.id]));
    assert_eq!(resolved.1["excludedOwners"], json!([f.starter.id]));
    assert_eq!(resolved.1["resolvedBy"], "role");
    assert_eq!(resolved.1["empty"], false);
    assert_eq!(g1.iter().find(|r| r.0 == "flow.task_completed").unwrap().1["nextNodeKey"], "financeApproval");
    let g2 = app.audit_rows(f.org, "energyWorkflowTask", gate2).await;
    assert_eq!(g2.iter().find(|r| r.0 == "flow.task_completed").unwrap().1["nextNodeKey"], "end");
}

fn ghost_def(key: &str) -> WorkflowDefinition {
    let mut d = WorkflowDefinition::new(key, "無人可核", "energyBill", "");
    d.nodes = vec![
        NodeDef::new("start", "起點", NodeType::Start),
        NodeDef::new("ghost", "沒人的關卡", NodeType::HumanTask).rule(AssignmentRule::role(&["energy:viewer"])),
        NodeDef::new("end", "終點", NodeType::End),
    ];
    d.transitions = vec![TransitionDef::new("start", "ghost", 0), TransitionDef::new("ghost", "end", 0)];
    d
}

/// 來源：workflowSkeleton.test.ts「an empty candidate pool does NOT auto-advance — the task waits at ready」
/// 來源：workflowSkeleton.test.ts「a definition whose node has no outgoing edge fails loudly」
/// 來源：workflowSkeleton.test.ts「"my tasks" is org-scoped and only lists open tasks」
#[actix_web::test]
async fn empty_pool_waits_and_dead_end_fails_loudly() {
    let f = fixture().await;
    let app = &f.app;
    let admin = app.member(f.org, "admin3", Some("energy:admin")).await;
    let ghost = app.install_def(&admin, f.org, &ghost_def("empty"), None).await;
    app.wf_binding(f.org, Some("ST-E"), None, None, ghost, "2000-01-01", None).await;
    let starter = app.ctx(&f.starter, f.org).await;
    let exec = in_tx!(app, tx => start_execution(&mut tx, &starter, start_input("ST-E", "empty", "BILL-EMPTY")).await).unwrap();
    // 絕不自動跳關 —— 那等於無人核准就放行。
    assert_eq!(exec.status, "running");
    let ts = app.tasks(exec.id).await;
    assert_eq!((ts.len(), ts[0].state.as_str()), (1, "ready"));
    let resolved = app.audit_rows(f.org, "energyWorkflowTask", ts[0].id).await.into_iter().find(|r| r.0 == "flow.candidates_resolved").unwrap();
    assert_eq!(resolved.1["empty"], true);
    assert_eq!(resolved.2, "failed", "解析出 0 人不是成功");

    let mut dead = WorkflowDefinition::new("deadend", "死路", "energyBill", "");
    dead.nodes = vec![NodeDef::new("start", "起點", NodeType::Start)];
    let dead_id = app.install_def(&admin, f.org, &dead, None).await;
    app.wf_binding(f.org, Some("ST-X"), None, None, dead_id, "2000-01-01", None).await;
    let r = in_tx!(app, tx => start_execution(&mut tx, &starter, start_input("ST-X", "deadend", "BILL-DEAD")).await);
    assert_eq!(code_of(r), (409, "no_matching_transition".into()));
    // 整個發起在一個交易內 → 失敗時連 execution 都不該留下。
    let n = app.scalar_i64(&format!("SELECT COUNT(*) FROM tbl_energy_workflow_execution WHERE organization_id = {} AND subject_id = 'BILL-DEAD'", f.org)).await;
    assert_eq!(n, 0);

    // 我的待辦：只列開著的、限本組織。
    let mut tx = app.core.pool.begin().await.unwrap();
    let mine = eo_energy::workflow::views::my_open_tasks(&mut tx, f.org, f.operator.id).await.unwrap();
    assert!(mine.iter().all(|t| t.is_active()));
    let other = eo_energy::workflow::views::my_open_tasks(&mut tx, f.org + 999_999, f.operator.id).await.unwrap();
    assert!(other.is_empty());
    let _ = sorted(vec![]);
}

/// 來源：workflowSkeleton.test.ts「POST /executions → 200 and lands on gate 1」
/// 來源：workflowSkeleton.test.ts「rejects a missing/badly-shaped businessDate (shared zod)」
/// 來源：workflowSkeleton.test.ts「GET /executions/:id shows the task list」
/// 來源：workflowSkeleton.test.ts「cross-tenant: another org cannot read this execution」
/// 來源：workflowSkeleton.test.ts「工作台列得出 gate 1 給承辦，不給發起人（防自核）」
/// 來源：workflowSkeleton.test.ts「claim → action(advance) → gate 2 → completed, all through HTTP」
/// 來源：workflowSkeleton.test.ts「an undefined actionKey returns 404 with the code, not a silent advance」
/// 來源：workflowSkeleton.test.ts「a user with no flow permission gets 403 (fail-closed)」
#[actix_web::test]
async fn flow_endpoints_over_http() {
    let f = fixture().await;
    let app = &f.app;
    let start = |subject: &str| {
        json!({ "stationKey": f.station, "workflowKey": "bill-approval", "subjectType": "energyBill",
                "subjectId": subject, "businessDate": "2026-05-31", "attributes": { "bill": { "totalAmount": 999 } } })
    };
    let v = expect_status!(app.post(&f.starter, &app.e(f.org, "/executions"), start("BILL-EP-1")).await, 200);
    let exec = v["id"].as_i64().unwrap();
    assert_eq!(v["currentNodeKey"], "operatorCheck");

    let mut bad = start("X");
    bad["businessDate"] = json!("2026/05/31");
    expect_status!(app.post(&f.starter, &app.e(f.org, "/executions"), bad).await, 400);

    let v = expect_status!(app.get(&f.operator, &app.e(f.org, &format!("/executions/{exec}"))).await, 200);
    let tasks = v["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 1);
    let gate1 = tasks[0]["id"].as_i64().unwrap();

    // 跨組織：別的組織的人拿自己的組織過閘，也讀不到這一筆。
    let stranger_owner = app.user("stranger owner", None).await;
    let other = app.org(&stranger_owner, "ep-other").await;
    let stranger = app.member(other, "stranger", Some("energy:operator")).await;
    let v = expect_status!(app.get(&stranger, &app.e(other, &format!("/executions/{exec}"))).await, 403);
    assert_eq!(v["code"], "execution_not_in_org");
    // 拿別人的組織過閘：沒有那裡的權限。
    expect_status!(app.get(&stranger, &app.e(f.org, &format!("/executions/{exec}"))).await, 403);

    let ids = |v: &serde_json::Value| -> Vec<i64> {
        v["sections"].as_array().unwrap().iter().flat_map(|s| s["rows"].as_array().unwrap().iter().map(|r| r["id"].as_i64().unwrap())).collect()
    };
    let wb = expect_status!(app.get(&f.operator, &app.e(f.org, "/workbench")).await, 200);
    assert!(ids(&wb).contains(&gate1));
    let wb = expect_status!(app.get(&f.starter, &app.e(f.org, "/workbench")).await, 200);
    assert!(!ids(&wb).contains(&gate1), "發起人被排除，不該出現在待辦");

    expect_status!(app.post(&f.operator, &app.e(f.org, &format!("/tasks/{gate1}/claim")), json!({})).await, 200);
    let v = expect_status!(
        app.post(&f.operator, &app.e(f.org, &format!("/tasks/{gate1}/actions")), json!({ "actionKey": "submit", "payload": { "checked": true } })).await,
        200
    );
    assert_eq!(v["row"]["state"], "completed");
    let v = expect_status!(app.get(&f.finance, &app.e(f.org, &format!("/executions/{exec}"))).await, 200);
    let gate2 = v["tasks"].as_array().unwrap().iter().find(|t| t["nodeKey"] == "financeApproval").unwrap()["id"].as_i64().unwrap();
    expect_status!(app.post(&f.finance, &app.e(f.org, &format!("/tasks/{gate2}/claim")), json!({})).await, 200);
    expect_status!(app.post(&f.finance, &app.e(f.org, &format!("/tasks/{gate2}/actions")), json!({ "actionKey": "approve" })).await, 200);
    let v = expect_status!(app.get(&f.finance, &app.e(f.org, &format!("/executions/{exec}"))).await, 200);
    assert_eq!(v["status"], "completed");

    let v = expect_status!(app.post(&f.starter, &app.e(f.org, "/executions"), start("BILL-EP-2")).await, 200);
    let e2 = v["id"].as_i64().unwrap();
    let v = expect_status!(app.get(&f.operator, &app.e(f.org, &format!("/executions/{e2}"))).await, 200);
    let tid = v["tasks"][0]["id"].as_i64().unwrap();
    expect_status!(app.post(&f.operator, &app.e(f.org, &format!("/tasks/{tid}/claim")), json!({})).await, 200);
    let v = expect_status!(
        app.post(&f.operator, &app.e(f.org, &format!("/tasks/{tid}/actions")), json!({ "actionKey": "sendBack", "reason": "缺附件" })).await,
        404
    );
    assert_eq!(v["code"], "action_not_found");

    let nobody = app.member(f.org, "nobody", None).await;
    expect_status!(app.get(&nobody, &app.e(f.org, "/workbench")).await, 403);
}
