//! 七節點預設流程：R1（發起人的部門主管＋fallback）、金額門檻分支、計算節點、付款鎖、流程圖、工作台列。

use crate::support::energy::{code_of, date, excluded, potential, sorted, TaskLite};
use crate::support::{TestApp, TestUser};
use eo_domain::workflow_defs::{default_bill_approval, DEPT_ACCOUNTING, DEPT_SALES, EXEC_APPROVAL_THRESHOLD};
use eo_energy::common::Ctx;
use eo_energy::workflow::engine::run::{start_execution, StartExecution};
use eo_energy::workflow::engine::tasks::{self, ActionInput};
use eo_energy::workflow::{graph, model, views};
use serde_json::{json, Value};

struct Fx {
    app: TestApp,
    org: i64,
    station_key: String,
    station: i64,
    clerk: TestUser,
    manager: TestUser,
    cxo: TestUser,
    finance: TestUser,
    accountant: TestUser,
}

async fn fixture() -> Fx {
    let app = TestApp::new().await;
    let owner = app.user("b4 owner", None).await;
    let org = app.org(&owner, "b4-org").await;
    let clerk = app.member(org, "clerk", Some("energy:operator")).await;
    let manager = app.member(org, "sales manager", Some("energy:operator")).await;
    let cxo = app.member(org, "cxo", Some("energy:cxo")).await;
    let finance = app.member(org, "finance", Some("energy:finance")).await;
    let accountant = app.member(org, "accountant", Some("energy:finance")).await;
    let sales = app.department(org, DEPT_SALES, "業務部").await;
    app.dept_member(sales, &clerk, "member").await;
    app.dept_member(sales, &manager, "manager").await;
    let acc = app.department(org, DEPT_ACCOUNTING, "會計部門").await;
    app.dept_member(acc, &accountant, "manager").await;
    let lo = app.landowner(org, "黃地主").await;
    let station_key = "B4-CN-0001".to_string();
    let station = app.station(org, lo, &station_key, "嘉義四號案場").await;
    let def = app.install_def(&cxo, org, &default_bill_approval(), None).await;
    app.wf_binding(org, Some(&station_key), None, Some("electricityBill"), def, "2000-01-01", None).await;
    let s = app.strategy(org, "station-payout", "revenueShare", json!({ "sharePercent": 10 }), "售電分潤 10%").await;
    app.strategy_binding(org, station, s, "2000-01-01", None).await;
    Fx { app, org, station_key, station, clerk, manager, cxo, finance, accountant }
}

impl Fx {
    async fn c(&self, u: &TestUser) -> Ctx {
        self.app.ctx(u, self.org).await
    }

    async fn bill(&self, amount: i64) -> i64 {
        self.app.bill(self.org, Some(self.station), "2026-01-01", "2026-01-31", Some(&amount.to_string()), Some("1000")).await
    }

    async fn start_as(&self, who: &TestUser, bill: i64, amount: i64) -> i64 {
        let ctx = self.c(who).await;
        let input = StartExecution {
            station_key: self.station_key.clone(),
            workflow_key: "bill-approval".into(),
            subject_type: "energyBill".into(),
            subject_id: bill.to_string(),
            business_date: date("2026-01-31"),
            attributes: json!({ "bill": { "totalAmount": amount } }),
            definition: None,
            station_id: Some(self.station),
        };
        in_tx!(self.app, tx => start_execution(&mut tx, &ctx, input).await).unwrap().id
    }

    async fn start(&self, bill: i64, amount: i64) -> i64 {
        self.start_as(&self.clerk, bill, amount).await
    }

    async fn open(&self, exec: i64) -> TaskLite {
        self.app.tasks(exec).await.into_iter().find(|t| (t.state == "ready" || t.state == "reserved") && t.node_type != "abortApproval").unwrap()
    }

    /// 認領並按一顆按鈕。
    async fn step(&self, u: &TestUser, exec: i64, key: &str, reason: Option<&str>) -> i64 {
        let t = self.open(exec).await;
        let ctx = self.c(u).await;
        if t.state == "ready" {
            in_tx!(self.app, tx => tasks::claim(&mut tx, &ctx, t.id).await).unwrap();
        }
        let input = ActionInput { action_key: key.into(), reason: reason.map(String::from), ..Default::default() };
        in_tx!(self.app, tx => tasks::perform_action(&mut tx, &ctx, t.id, input).await).unwrap();
        t.id
    }

    async fn can_revert(&self, exec: i64) -> tasks::RevertVerdict {
        let mut tx = self.app.core.pool.begin().await.unwrap();
        let e = model::execution_in_org(&mut tx, self.org, exec).await.unwrap();
        tasks::can_revert(&mut tx, &e, "manager-approve").await.unwrap()
    }

    async fn graph(&self, exec: i64) -> Value {
        let mut tx = self.app.core.pool.begin().await.unwrap();
        let e = model::execution_in_org(&mut tx, self.org, exec).await.unwrap();
        graph::execution_graph(&mut tx, &e).await.unwrap()
    }
}

/// 來源：defaultFlow.test.ts「seeds all seven nodes with their actions」
/// 來源：defaultFlow.test.ts「自核旗標：只有「承辦確認」開，三個**核可**關卡一律關」
#[actix_web::test]
async fn seeds_seven_nodes_and_self_approval_flags() {
    let f = fixture().await;
    let rows: Vec<(String, bool, Option<String>, bool)> = sqlx::query_as(
        "SELECT n.node_key, n.revertable, n.service_key, n.allow_self_approval FROM tbl_energy_workflow_node n
           JOIN tbl_energy_workflow_definition d ON d.id = n.definition_id WHERE d.organization_id = $1 ORDER BY n.id",
    )
    .bind(f.org)
    .fetch_all(&f.app.core.pool)
    .await
    .unwrap();
    let keys: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(keys, ["start", "clerk-review", "manager-approve", "exec-approve", "calc", "finance-release", "payment", "end"]);
    let by = |k: &str| rows.iter().find(|r| r.0 == k).unwrap().clone();
    assert!(!by("payment").1, "付款這一關不可回退");
    assert_eq!(by("calc").2.as_deref(), Some("rating.computePayout"));
    assert!(by("clerk-review").3, "承辦確認：送件者要能改自己被退回的單");
    for gate in ["manager-approve", "exec-approve", "finance-release", "payment"] {
        assert!(!by(gate).3, "{gate} 是核可關卡，不可以自核");
    }
    let rule: Value = sqlx::query_scalar("SELECT abort_approval_rule FROM tbl_energy_workflow_definition WHERE organization_id = $1")
        .bind(f.org)
        .fetch_one(&f.app.core.pool)
        .await
        .unwrap();
    assert_eq!(rule["params"]["roleUniqueNames"], json!(["energy:admin", "energy:cxo"]));
}

/// 來源：defaultFlow.test.ts「the manager gate lands on the department manager」
/// 來源：defaultFlow.test.ts「when the MANAGER himself starts the bill, R1 falls back to the CXO」
#[actix_web::test]
async fn r1_department_manager_of_initiator_with_fallback() {
    let f = fixture().await;
    let b = f.bill(50000).await;
    let exec = f.start(b, 50000).await;
    let t1 = f.open(exec).await;
    let c1 = f.app.candidates(t1.id).await;
    assert_eq!(sorted(potential(&c1)), sorted(vec![f.manager.id, f.clerk.id]), "承辦確認：發起人也在候選池裡");
    assert!(excluded(&c1).is_empty());
    f.step(&f.manager, exec, "submit", None).await;
    let t2 = f.open(exec).await;
    assert_eq!(t2.node_key, "manager-approve");
    let c2 = f.app.candidates(t2.id).await;
    assert_eq!(potential(&c2), vec![f.manager.id]);
    assert_eq!(c2.iter().find(|c| c.0 == "potentialOwner").unwrap().2, "departmentManagerOfInitiator");
    assert_eq!(excluded(&c2), vec![f.clerk.id], "主管核准：發起人被排除（防自核）");

    let b2 = f.bill(50000).await;
    let e2 = f.start_as(&f.manager, b2, 50000).await;
    f.step(&f.clerk, e2, "submit", None).await;
    let t = f.open(e2).await;
    let c = f.app.candidates(t.id).await;
    assert_eq!(potential(&c), vec![f.cxo.id], "主管不能核自己發起的單 → 升到 CXO");
    assert_eq!(c[0].2, "departmentManagerOfInitiator:fallback");
}

/// 來源：defaultFlow.test.ts「below the threshold (100000) goes straight to calc」
/// 來源：defaultFlow.test.ts「at or above the threshold opens the exec gate first (US-8)」
#[actix_web::test]
async fn amount_threshold_decides_exec_gate() {
    let f = fixture().await;
    for (amount, expect_exec) in [(EXEC_APPROVAL_THRESHOLD - 1, false), (EXEC_APPROVAL_THRESHOLD, true)] {
        let b = f.bill(amount).await;
        let exec = f.start(b, amount).await;
        f.step(&f.manager, exec, "submit", None).await;
        f.step(&f.manager, exec, "approve", None).await;
        let t = f.open(exec).await;
        let keys: Vec<String> = f.app.tasks(exec).await.into_iter().map(|t| t.node_key).collect();
        if expect_exec {
            assert_eq!(t.node_key, "exec-approve");
            assert_eq!(potential(&f.app.candidates(t.id).await), vec![f.cxo.id]);
        } else {
            assert_eq!(t.node_key, "finance-release", "不經過加簽");
            assert!(!keys.contains(&"exec-approve".to_string()));
            assert!(keys.contains(&"calc".to_string()), "計算節點也產生 task 列");
        }
    }
}

/// 來源：defaultFlow.test.ts「end-to-end: seven nodes, a payout record, and a revert lock (US-6/US-7)」
/// 來源：defaultFlow.test.ts「a bill that never got a station cannot even start the calc node」
#[actix_web::test]
async fn end_to_end_payout_and_revert_lock() {
    let f = fixture().await;
    let bill = f.bill(50000).await;
    let exec = f.start(bill, 50000).await;
    f.step(&f.manager, exec, "submit", None).await;
    assert_eq!(f.open(exec).await.node_key, "manager-approve");
    f.step(&f.manager, exec, "approve", None).await;
    let calc = f.app.tasks(exec).await.into_iter().find(|t| t.node_key == "calc").unwrap();
    assert_eq!((calc.state.as_str(), calc.outcome.as_deref()), ("completed", Some("auto")));
    let payout_id = calc.payload["payoutRecordId"].as_i64().expect("計算節點要把應付紀錄 id 寫進 task payload");
    let (amount, params, exec_id, trigger): (String, Value, Option<i64>, String) = sqlx::query_as(
        "SELECT amount::text, params_snapshot, execution_id, trigger FROM tbl_energy_payout_record WHERE id = $1",
    )
    .bind(payout_id)
    .fetch_one(&f.app.core.pool)
    .await
    .unwrap();
    assert_eq!(amount, "5000.00", "50000 × 10%");
    assert_eq!(params["sharePercent"], 10);
    assert_eq!((exec_id, trigger.as_str()), (Some(exec), "workflowNode"));

    assert_eq!(f.open(exec).await.node_key, "finance-release");
    f.step(&f.finance, exec, "release", None).await;
    let pay = f.open(exec).await;
    assert_eq!(pay.node_key, "payment");
    assert!(f.can_revert(exec).await.allowed, "還沒付款前，前面的關卡退得了");
    let acc = f.c(&f.accountant).await;
    in_tx!(f.app, tx => tasks::claim(&mut tx, &acc, pay.id).await).unwrap();
    let mark = ActionInput { action_key: "markPaid".into(), note: Some("匯款單號 B4-0001".into()), ..Default::default() };
    in_tx!(f.app, tx => tasks::perform_action(&mut tx, &acc, pay.id, mark).await).unwrap();
    let v = f.can_revert(exec).await;
    assert_eq!((v.allowed, v.reason), (false, Some("side_effect_lock")), "錢出去了就不能當沒發生");
    assert_eq!(v.blocked_by.unwrap()[0].side_effect_key, "payment.sent");
    let submit = ActionInput { action_key: "submit".into(), ..Default::default() };
    in_tx!(f.app, tx => tasks::perform_action(&mut tx, &acc, pay.id, submit).await).unwrap();
    assert_eq!(f.app.exec_state(exec).await, ("completed".into(), Some("end".into())));
    // 走到 end 那一刻才讓單據成立。
    let state: String = sqlx::query_scalar("SELECT approval_state FROM tbl_energy_bill WHERE id = $1").bind(bill).fetch_one(&f.app.core.pool).await.unwrap();
    assert_eq!(state, "approved");
    let types: Vec<String> = sqlx::query_scalar("SELECT type FROM tbl_energy_audit_event WHERE organization_id = $1 ORDER BY id")
        .bind(f.org)
        .fetch_all(&f.app.core.pool)
        .await
        .unwrap();
    for t in ["flow.started", "payout.calculated", "flow.side_effect_applied", "flow.completed"] {
        assert!(types.contains(&t.to_string()), "{t}");
    }

    // 例外件（沒指認案場）算不了 —— calc 節點不會產生半筆爛帳。
    let orphan = f.app.bill(f.org, None, "2026-01-01", "2026-01-31", Some("100"), None).await;
    let fin = f.c(&f.finance).await;
    let r = in_tx!(f.app, tx => eo_energy::rating::compute_for_bill(&mut tx, &fin, orphan, "workflowNode", None).await);
    assert_eq!(code_of(r).1, "bill_has_no_station");
}

/// 來源：defaultFlow.test.ts「graph: 節點狀態由 task 推導，`current` 是陣列，`sendBack` 的邊標得出來」
/// 來源：defaultFlow.test.ts「graph: 退回後那一關要回到「進行中」，不可停在「已完成」」
#[actix_web::test]
async fn execution_graph_derives_state_and_send_back_edges() {
    let f = fixture().await;
    let b = f.bill(50000).await;
    let exec = f.start(b, 50000).await;
    let g = f.graph(exec).await;
    let nodes = g["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 8);
    let node = |g: &Value, k: &str| g["nodes"].as_array().unwrap().iter().find(|n| n["nodeKey"] == k).unwrap().clone();
    let clerk = node(&g, "clerk-review");
    assert_eq!(clerk["name"], "承辦確認");
    assert_eq!(clerk["state"], "active");
    assert!(g["current"].as_array().unwrap().contains(&json!("clerk-review")));
    assert_eq!(node(&g, "payment")["state"], "pending");
    assert!(clerk["layout"]["x"].is_number());
    let edges = g["edges"].as_array().unwrap();
    let send_backs: Vec<&Value> = edges.iter().filter(|e| e["kind"] == "sendBack").collect();
    assert!(!send_backs.is_empty(), "退回路徑要由 revert 動作推導出來");
    assert!(send_backs.iter().all(|e| e["implied"] == true));
    assert!(send_backs.iter().any(|e| e["from"] == "manager-approve" && e["to"] == "clerk-review"));
    assert_eq!(send_backs[0]["labelKey"], "flow.action.sendBack");
    let conditional: Vec<&Value> = edges.iter().filter(|e| e["hasCondition"] == true).collect();
    assert!(conditional.iter().any(|e| e["label"].is_string()), "分支邊要有 label");

    f.step(&f.manager, exec, "submit", None).await;
    let g1 = f.graph(exec).await;
    assert_eq!(node(&g1, "clerk-review")["state"], "completed");
    assert!(g1["current"].as_array().unwrap().contains(&json!("manager-approve")));
    assert!(!g1["current"].as_array().unwrap().contains(&json!("clerk-review")));

    // 退回 → clerk-review 有兩筆 task，取序號最大的那筆：進行中。
    f.step(&f.manager, exec, "sendBack", Some("金額請再確認")).await;
    let g2 = f.graph(exec).await;
    assert_eq!(node(&g2, "clerk-review")["state"], "active");
    assert!(g2["current"].as_array().unwrap().contains(&json!("clerk-review")));
}

/// 來源：defaultFlow.test.ts「claim／action 之後回得出「更新後的那一列」，且形狀與工作台的列一致」
#[actix_web::test]
async fn row_after_claim_and_action_has_the_workbench_shape() {
    let f = fixture().await;
    let b = f.bill(50000).await;
    let exec = f.start(b, 50000).await;
    let t1 = f.open(exec).await;
    let ctx = f.c(&f.manager).await;
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let task = model::task_by_id(&mut tx, t1.id).await.unwrap().unwrap();
    let bench = views::decorate_tasks(&mut tx, &ctx, f.manager.id, &[task]).await.unwrap().remove(0);
    drop(tx);
    let keys = |v: &Value| {
        let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    };
    in_tx!(f.app, tx => tasks::claim(&mut tx, &ctx, t1.id).await).unwrap();
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let after_claim = views::workbench_row_of(&mut tx, &ctx, f.manager.id, t1.id).await.unwrap().unwrap();
    drop(tx);
    assert_eq!(keys(&after_claim), keys(&bench), "欄位集合必須跟工作台的列一模一樣");
    assert_eq!(after_claim["id"], t1.id);
    assert_eq!(after_claim["claimedByMe"], true);
    let submit = ActionInput { action_key: "submit".into(), ..Default::default() };
    in_tx!(f.app, tx => tasks::perform_action(&mut tx, &ctx, t1.id, submit).await).unwrap();
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let after = views::workbench_row_of(&mut tx, &ctx, f.manager.id, t1.id).await.unwrap().unwrap();
    assert_eq!(keys(&after), keys(&bench));
    assert_eq!(after["state"], "completed");
    assert_eq!(after["availableActions"], json!([]), "已處理的列不該還有動作");
}

/// 來源：defaultFlow.test.ts「稽核的「對象」欄不是 raw key（行為層守門）」
#[actix_web::test]
async fn audit_target_label_is_never_a_raw_key() {
    let f = fixture().await;
    let b = f.bill(50000).await;
    let exec = f.start(b, 50000).await;
    f.step(&f.manager, exec, "submit", None).await;
    f.step(&f.manager, exec, "sendBack", Some("請再確認金額")).await;
    f.step(&f.manager, exec, "submit", None).await;
    f.step(&f.manager, exec, "approve", None).await;
    f.step(&f.finance, exec, "release", None).await;
    let pay = f.open(exec).await;
    let acc = f.c(&f.accountant).await;
    in_tx!(f.app, tx => tasks::claim(&mut tx, &acc, pay.id).await).unwrap();
    let mark = ActionInput { action_key: "markPaid".into(), note: Some("匯款單號 B8-0001".into()), ..Default::default() };
    in_tx!(f.app, tx => tasks::perform_action(&mut tx, &acc, pay.id, mark).await).unwrap();
    let submit = ActionInput { action_key: "submit".into(), ..Default::default() };
    in_tx!(f.app, tx => tasks::perform_action(&mut tx, &acc, pay.id, submit).await).unwrap();

    let mut raw: Vec<String> = default_bill_approval().nodes.into_iter().map(|n| n.node_key).collect();
    raw.push("bill-approval".into());
    raw.push("payment.sent".into());
    let rows: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT type, target_label FROM tbl_energy_audit_event WHERE organization_id = $1 ORDER BY id")
            .bind(f.org)
            .fetch_all(&f.app.core.pool)
            .await
            .unwrap();
    assert!(rows.len() > 10);
    let leaked: Vec<String> =
        rows.iter().filter(|r| r.1.as_ref().is_some_and(|l| raw.contains(l))).map(|r| format!("{} → {:?}", r.0, r.1)).collect();
    assert!(leaked.is_empty(), "{leaked:?}");
    // 反向：不可以整欄都空（不寫 label 是最省事的過關法）。
    let with_label = rows.iter().filter(|r| r.1.as_deref().is_some_and(|l| !l.trim().is_empty())).count();
    assert!(with_label as f64 > rows.len() as f64 * 0.8, "{with_label}/{}", rows.len());
}
