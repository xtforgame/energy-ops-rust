//! HTTP 層：每個閘對錯的角色回 403、跨組織 403／404、未登入 401；以及內建定義安裝、工作台、代理、旁路動作的端到端。

use crate::support::{TestApp, TestUser};
use reqwest::Method;
use serde_json::{json, Value};

struct Fx {
    app: TestApp,
    org: i64,
    admin: TestUser,
    clerk: TestUser,
    manager: TestUser,
    finance: TestUser,
    accountant: TestUser,
    viewer: TestUser,
    nobody: TestUser,
    station: i64,
}

/// 用安裝端點裝內建定義，再用綁定端點把案場綁上 —— 全程走 API。
async fn fixture() -> Fx {
    let app = TestApp::new().await;
    let (admin, org) = app.energy_org("http-org").await;
    let clerk = app.member(org, "clerk", Some("energy:operator")).await;
    let manager = app.member(org, "manager", Some("energy:operator")).await;
    let finance = app.member(org, "finance", Some("energy:finance")).await;
    let accountant = app.member(org, "accountant", Some("energy:finance")).await;
    let viewer = app.member(org, "viewer", Some("energy:viewer")).await;
    let nobody = app.member(org, "nobody", None).await;
    let v = expect_status!(app.post(&admin, &app.e(org, "/setup/install-defaults"), json!({})).await, 200);
    assert_eq!(v["definitions"].as_array().unwrap().len(), 4);
    assert_eq!(v["strategies"].as_array().unwrap().len(), 1);
    assert_eq!(v["departments"].as_array().unwrap().len(), 3);
    // 冪等：第二次什麼都不裝。
    let v = expect_status!(app.post(&admin, &app.e(org, "/setup/install-defaults"), json!({})).await, 200);
    assert!(v["definitions"].as_array().unwrap().is_empty() && v["strategies"].as_array().unwrap().is_empty());

    let depts = expect_status!(app.get(&admin, &app.e(org, "/departments")).await, 200);
    let dept = |k: &str| depts["items"].as_array().unwrap().iter().find(|d| d["key"] == k).unwrap()["id"].as_i64().unwrap();
    for (d, u, r) in [("sales", &clerk, "member"), ("sales", &manager, "manager"), ("accounting", &accountant, "manager"), ("finance", &finance, "manager")] {
        expect_status!(
            TestApp::send(app.req(Some(&admin), Method::PUT, &app.e(org, &format!("/departments/{}/members/{}", dept(d), u.id))).json(&json!({ "role": r }))).await,
            200
        );
    }
    let lo = app.landowner(org, "黃地主").await;
    let station = app.station(org, lo, "HTTP-0001", "嘉義四號案場").await;
    let opts = expect_status!(app.get(&admin, &app.e(org, "/binding-options?kind=workflow")).await, 200);
    let bill_def = opts["items"].as_array().unwrap().iter().find(|o| o["key"] == "bill-approval").unwrap()["id"].as_i64().unwrap();
    expect_status!(
        app.post(&admin, &app.e(org, &format!("/stations/{station}/bindings")), json!({ "kind": "workflow", "docType": "electricityBill", "versionId": bill_def, "validFrom": "2000-01-01" })).await,
        200
    );
    let opts = expect_status!(app.get(&admin, &app.e(org, "/binding-options?kind=rating")).await, 200);
    let strat = opts["items"][0]["id"].as_i64().unwrap();
    expect_status!(
        app.post(&admin, &app.e(org, &format!("/stations/{station}/bindings")), json!({ "kind": "rating", "versionId": strat, "validFrom": "2000-01-01" })).await,
        200
    );
    Fx { app, org, admin, clerk, manager, finance, accountant, viewer, nobody, station }
}

impl Fx {
    fn u(&self, p: &str) -> String {
        self.app.e(self.org, p)
    }

    async fn start(&self, subject: i64, amount: i64) -> i64 {
        let body = json!({ "stationKey": "HTTP-0001", "workflowKey": "bill-approval", "subjectType": "energyBill", "subjectId": subject,
                           "businessDate": "2026-01-31", "attributes": { "bill": { "totalAmount": amount } } });
        expect_status!(self.app.post(&self.clerk, &self.u("/executions"), body).await, 200)["id"].as_i64().unwrap()
    }

    async fn open_task(&self, who: &TestUser, exec: i64) -> Value {
        let v = expect_status!(self.app.get(who, &self.u(&format!("/executions/{exec}"))).await, 200);
        v["tasks"].as_array().unwrap().iter().rev().find(|t| t["state"] == "ready" || t["state"] == "reserved").cloned().unwrap()
    }

    async fn step(&self, who: &TestUser, exec: i64, action: Value) -> Value {
        let t = self.open_task(who, exec).await;
        let id = t["id"].as_i64().unwrap();
        if t["state"] == "ready" {
            expect_status!(self.app.post(who, &self.u(&format!("/tasks/{id}/claim")), json!({})).await, 200);
        }
        expect_status!(self.app.post(who, &self.u(&format!("/tasks/{id}/actions")), action).await, 200)
    }
}

/// 每個閘：權限不足的角色一律 403，未登入 401（照 App 5 的閘常數）。
#[actix_web::test]
async fn gates_reject_the_wrong_roles() {
    let f = fixture().await;
    let app = &f.app;
    let bill = app.bill(f.org, Some(f.station), "2026-01-01", "2026-01-31", Some("5000"), Some("100")).await;
    let exec = f.start(bill, 5000).await;
    let task = f.open_task(&f.clerk, exec).await["id"].as_i64().unwrap();

    let gets = [
        "/departments", "/members", "/assignable-roles", "/audit-events", "/audit-actors", "/dashboard", "/calendar-events",
        "/me/landing", "/workbench", "/executions?scope=mine", "/workflows", "/stations", "/station-list", "/unbound-stations",
        "/landowners", "/delegations",
    ];
    for p in gets {
        expect_status!(app.get(&f.nobody, &f.u(p)).await, 403);
        let (s, _) = TestApp::send(app.req(None, Method::GET, &f.u(p))).await;
        assert_eq!(s.as_u16(), 401, "{p}");
    }
    // 治理類：只有能管部門／成員的人。
    for p in ["/departments", "/members", "/assignable-roles"] {
        expect_status!(app.get(&f.clerk, &f.u(p)).await, 403);
        expect_status!(app.get(&f.viewer, &f.u(p)).await, 403);
    }
    expect_status!(app.get(&f.clerk, &f.u("/audit-events")).await, 403);
    expect_status!(app.get(&f.viewer, &f.u("/audit-events")).await, 200);
    // 唯讀角色：看得到全組織，但不能動。
    expect_status!(app.get(&f.viewer, &f.u("/executions")).await, 200);
    let v = expect_status!(app.get(&f.clerk, &f.u("/executions")).await, 403);
    assert_eq!(v["code"], "view_all_required", "scope=all 不可靜默降級");
    expect_status!(app.get(&f.clerk, &f.u("/executions?scope=nope")).await, 400);
    expect_status!(app.get(&f.clerk, &f.u("/executions?scope=mine&status=bogus")).await, 400);
    expect_status!(app.get(&f.clerk, &f.u("/executions?scope=mine&page=x")).await, 400);
    let start = json!({ "stationKey": "HTTP-0001", "workflowKey": "bill-approval", "subjectType": "energyBill", "subjectId": "9", "businessDate": "2026-01-31" });
    expect_status!(app.post(&f.viewer, &f.u("/executions"), start).await, 403);
    expect_status!(app.post(&f.viewer, &f.u(&format!("/tasks/{task}/claim")), json!({})).await, 403);
    // 管理動作：flow:admin（只有 admin）。
    for p in [format!("/executions/{exec}/force-advance"), format!("/executions/{exec}/terminate"), format!("/tasks/{task}/reassign"), "/side-effects/1/release".into()] {
        expect_status!(app.post(&f.clerk, &f.u(&p), json!({ "reason": "x", "toNodeKey": "payment", "toUserId": f.clerk.id })).await, 403);
        expect_status!(app.post(&f.viewer, &f.u(&p), json!({ "reason": "x" })).await, 403);
    }
    expect_status!(app.post(&f.clerk, &f.u(&format!("/tasks/{task}/abort-decision")), json!({ "approved": true })).await, 403);
    expect_status!(app.post(&f.clerk, &f.u("/setup/install-defaults"), json!({})).await, 403);
    // CXO 有治理權但沒有系統設定權。
    let cxo = app.member(f.org, "cxo", Some("energy:cxo")).await;
    expect_status!(app.post(&cxo, &f.u("/setup/install-defaults"), json!({})).await, 403);
    expect_status!(app.get(&cxo, &f.u("/binding-options?kind=workflow")).await, 403);
    expect_status!(app.get(&cxo, &f.u("/departments")).await, 200);
    // 幫別人設代理需要代理管理權；替自己設只要 flow:act。
    let other_delegation = json!({ "delegatorUserId": f.manager.id, "delegateUserId": f.clerk.id, "validFrom": "2026-01-01" });
    expect_status!(app.post(&f.clerk, &f.u("/delegations"), other_delegation.clone()).await, 403);
    expect_status!(app.post(&cxo, &f.u("/delegations"), other_delegation).await, 200);
    let mine = json!({ "delegatorUserId": f.clerk.id, "delegateUserId": f.manager.id, "validFrom": "2026-01-01" });
    expect_status!(app.post(&f.clerk, &f.u("/delegations"), mine.clone()).await, 200);
    expect_status!(app.post(&f.viewer, &f.u("/delegations"), json!({ "delegatorUserId": f.viewer.id, "delegateUserId": f.clerk.id, "validFrom": "2026-01-01" })).await, 403);
}

/// 跨組織：別的組織的人拿自己的組織過閘，指向本組織的資源一律被擋（403 *_not_in_org，或查無此筆）。
#[actix_web::test]
async fn cross_org_access_is_blocked() {
    let f = fixture().await;
    let app = &f.app;
    let bill = app.bill(f.org, Some(f.station), "2026-01-01", "2026-01-31", Some("5000"), Some("100")).await;
    let exec = f.start(bill, 5000).await;
    let task = f.open_task(&f.clerk, exec).await["id"].as_i64().unwrap();
    let lo: i64 = app.scalar_i64(&format!("SELECT landowner_id FROM tbl_energy_station WHERE id = {}", f.station)).await;
    let def: i64 = app.scalar_i64(&format!("SELECT id FROM tbl_energy_workflow_definition WHERE organization_id = {} AND key = 'bill-approval'", f.org)).await;
    let deleg = expect_status!(
        app.post(&f.clerk, &f.u("/delegations"), json!({ "delegatorUserId": f.clerk.id, "delegateUserId": f.manager.id, "validFrom": "2026-01-01" })).await,
        200
    )["id"]
        .as_i64()
        .unwrap();

    let (outsider, other) = app.energy_org("http-other").await;
    // 直接拿本組織的路徑：他在這裡沒有任何權限。
    expect_status!(app.get(&outsider, &f.u("/executions?scope=mine")).await, 403);
    expect_status!(app.get(&outsider, &f.u(&format!("/executions/{exec}"))).await, 403);
    let o = |p: &str| app.e(other, p);
    let cases: Vec<(Method, String, &str)> = vec![
        (Method::GET, o(&format!("/executions/{exec}")), "execution_not_in_org"),
        (Method::GET, o(&format!("/executions/{exec}/graph")), "execution_not_in_org"),
        (Method::GET, o(&format!("/executions/{exec}/actions")), "execution_not_in_org"),
        (Method::GET, o(&format!("/executions/{exec}/lineage")), "execution_not_in_org"),
        (Method::POST, o(&format!("/tasks/{task}/claim")), "execution_not_in_org"),
        (Method::POST, o(&format!("/executions/{exec}/abort")), "execution_not_in_org"),
        (Method::POST, o(&format!("/executions/{exec}/terminate")), "execution_not_in_org"),
        (Method::GET, o(&format!("/stations/{}/overview", f.station)), "station_not_in_org"),
        (Method::GET, o(&format!("/stations/{}/bindings", f.station)), "station_not_in_org"),
        (Method::GET, o(&format!("/stations/{}/geocode", f.station)), "station_not_in_org"),
        (Method::GET, o(&format!("/landowners/{lo}")), "landowner_not_in_org"),
        (Method::GET, o(&format!("/workflows/{def}/graph")), "definition_not_in_org"),
    ];
    for (m, p, code) in cases {
        let (s, v) = TestApp::send(app.req(Some(&outsider), m.clone(), &p).json(&json!({ "reason": "跨組織" }))).await;
        assert_eq!((s.as_u16(), v["code"].as_str()), (403, Some(code)), "{m} {p}: {v}");
    }
    let v = expect_status!(app.post(&outsider, &o(&format!("/delegations/{deleg}/revoke")), json!({})).await, 404);
    assert_eq!(v["code"], "delegation_not_found");
    let v = expect_status!(app.post(&outsider, &o("/lineage"), json!({ "upstreamExecutionId": exec, "downstreamExecutionId": exec })).await, 403);
    assert_eq!(v["code"], "execution_not_in_org");
    // 他自己組織的清單與節點負載不含本組織的單。
    let v = expect_status!(app.get(&outsider, &o("/executions")).await, 200);
    assert!(v["items"].as_array().unwrap().is_empty());
    let v = expect_status!(app.get(&outsider, &o(&format!("/workflows/{def}/nodes/clerk-review/tasks"))).await, 200);
    assert!(v["items"].as_array().unwrap().is_empty());
}

/// 內建定義裝好、綁好之後，整條預設流程（含計算節點）走得完；工作台分區與登入落點由後端決定。
#[actix_web::test]
async fn installed_defaults_run_end_to_end_with_workbench() {
    let f = fixture().await;
    let app = &f.app;
    let bill = app.bill(f.org, Some(f.station), "2026-01-01", "2026-01-31", Some("50000"), Some("1000")).await;
    let exec = f.start(bill, 50000).await;

    let land = |u: &TestUser| {
        let app = &f.app;
        let url = f.u("/me/landing");
        let u = TestUser { id: u.id, email: u.email.clone(), token: u.token.clone() };
        async move { expect_status!(app.get(&u, &url).await, 200)["reason"].as_str().unwrap().to_string() }
    };
    assert_eq!(land(&f.admin).await, "admin");
    assert_eq!(land(&f.finance).await, "finance");
    assert_eq!(land(&f.manager).await, "departmentManager");
    assert_eq!(land(&f.clerk).await, "operator");
    assert_eq!(land(&f.viewer).await, "cxo");

    let wb = expect_status!(app.get(&f.manager, &f.u("/workbench")).await, 200);
    let to_approve = wb["sections"].as_array().unwrap().iter().find(|s| s["key"] == "toApprove").expect("等我核准");
    assert!(to_approve["rows"].as_array().unwrap().iter().any(|r| r["executionId"] == exec));
    assert_eq!(wb["identity"]["isDepartmentManager"], true);

    f.step(&f.manager, exec, json!({ "actionKey": "submit" })).await;
    // 主管退回 → 承辦的工作台出現「被退回要我補」，理由與退回的人印在列上。
    f.step(&f.manager, exec, json!({ "actionKey": "sendBack", "reason": "金額請再確認" })).await;
    let wb = expect_status!(app.get(&f.clerk, &f.u("/workbench")).await, 200);
    let sent = wb["sections"].as_array().unwrap().iter().find(|s| s["key"] == "sentBack").expect("被退回要我補");
    let row = &sent["rows"][0];
    assert_eq!((row["rejectReason"].clone(), row["rejectedBy"].clone()), (json!("金額請再確認"), json!("manager")));

    f.step(&f.clerk, exec, json!({ "actionKey": "submit" })).await;
    let v = expect_status!(app.get(&f.manager, &f.u(&format!("/executions/{exec}/actions"))).await, 200);
    assert_eq!(v["claimable"], true);
    assert!(v["availableActions"].as_array().unwrap().iter().any(|a| a["actionKey"] == "approve"));
    let v = expect_status!(app.get(&f.clerk, &f.u(&format!("/executions/{exec}/actions"))).await, 200);
    assert_eq!(v["actionBlocked"]["code"], "excludedOwner", "防自核要說得出原因");
    assert_eq!(v["claimable"], false);
    f.step(&f.manager, exec, json!({ "actionKey": "approve" })).await;
    f.step(&f.finance, exec, json!({ "actionKey": "release" })).await;
    let wb = expect_status!(app.get(&f.accountant, &f.u("/workbench")).await, 200);
    let pay = wb["sections"].as_array().unwrap().iter().find(|s| s["key"] == "toPay").expect("等我付款");
    assert_eq!(pay["overdueDays"], 5);
    let d = expect_status!(app.get(&f.admin, &f.u("/dashboard?month=2026-01")).await, 200);
    assert_eq!((d["releasedUnpaidCount"].clone(), d["payableThisMonth"].clone()), (json!(1), json!("5000.00")));
    f.step(&f.accountant, exec, json!({ "actionKey": "markPaid", "note": "TX-1" })).await;
    let v = expect_status!(app.get(&f.admin, &f.u(&format!("/executions/{exec}/revertable?nodeKey=manager-approve"))).await, 200);
    assert_eq!((v["allowed"].clone(), v["reason"].clone()), (json!(false), json!("side_effect_lock")));
    let se = v["blockedBy"][0]["id"].as_i64().unwrap();
    let v = expect_status!(app.post(&f.admin, &f.u(&format!("/side-effects/{se}/release")), json!({ "reason": "" })).await, 400);
    assert_eq!(v["code"], "invalid_body");
    expect_status!(app.post(&f.admin, &f.u(&format!("/side-effects/{se}/release")), json!({ "reason": "按錯了" })).await, 200);
    f.step(&f.accountant, exec, json!({ "actionKey": "markPaid", "note": "TX-2" })).await;
    f.step(&f.accountant, exec, json!({ "actionKey": "submit" })).await;
    let v = expect_status!(app.get(&f.viewer, &f.u(&format!("/executions/{exec}"))).await, 200);
    assert_eq!(v["status"], "completed");
    assert_eq!(v["mode"], "readonly");
    let g = expect_status!(app.get(&f.viewer, &f.u(&format!("/executions/{exec}/graph"))).await, 200);
    assert!(g["nodes"].as_array().unwrap().iter().all(|n| n["state"] != "active"));

    let o = expect_status!(app.get(&f.viewer, &f.u("/workflows")).await, 200);
    let def = o["definitions"].as_array().unwrap().iter().find(|d| d["key"] == "bill-approval").unwrap();
    assert_eq!(def["bindings"][0]["stationKey"], "HTTP-0001");
    let g = expect_status!(app.get(&f.viewer, &f.u(&format!("/workflows/{}/graph", def["id"]))).await, 200);
    assert!(g["nodes"].as_array().unwrap().iter().all(|n| n["state"] == "definition"));
    let unbound = expect_status!(app.get(&f.viewer, &f.u("/unbound-stations")).await, 200);
    assert!(unbound["items"].as_array().unwrap().is_empty(), "兩種都綁了就不在漏綁清單");
}

/// 旁路動作與代理的 HTTP 端點：中斷申請與裁定、終止、代理建立／列出／收回、血緣連結與追溯。
#[actix_web::test]
async fn side_channels_over_http() {
    let f = fixture().await;
    let app = &f.app;
    let b1 = app.bill(f.org, Some(f.station), "2026-01-01", "2026-01-31", Some("5000"), Some("100")).await;
    let b2 = app.bill(f.org, Some(f.station), "2026-02-01", "2026-02-28", Some("5000"), Some("100")).await;
    let e1 = f.start(b1, 5000).await;
    let e2 = f.start(b2, 5000).await;

    expect_status!(app.post(&f.clerk, &f.u(&format!("/executions/{e1}/abort")), json!({})).await, 400);
    expect_status!(app.post(&f.clerk, &f.u(&format!("/executions/{e1}/abort")), json!({ "reason": "客戶取消" })).await, 200);
    let v = expect_status!(app.get(&f.admin, &f.u(&format!("/executions/{e1}"))).await, 200);
    assert_eq!(v["status"], "abortPending");
    let approval = v["tasks"].as_array().unwrap().iter().find(|t| t["nodeType"] == "abortApproval").unwrap()["id"].as_i64().unwrap();
    let list = expect_status!(app.get(&f.admin, &f.u("/executions?status=running")).await, 200);
    assert!(list["items"].as_array().unwrap().iter().any(|r| r["id"] == e1), "待中斷核可算進行中");
    expect_status!(app.post(&f.admin, &f.u(&format!("/tasks/{approval}/abort-decision")), json!({ "approved": true })).await, 200);
    let v = expect_status!(app.post(&f.admin, &f.u(&format!("/tasks/{approval}/abort-decision")), json!({ "approved": true })).await, 409);
    assert_eq!(v["code"], "abort_already_decided");
    let v = expect_status!(app.get(&f.admin, &f.u(&format!("/executions/{e1}"))).await, 200);
    assert_eq!(v["status"], "aborted");
    let v = expect_status!(app.post(&f.admin, &f.u(&format!("/executions/{e1}/terminate")), json!({ "reason": "x" })).await, 409);
    assert_eq!(v["code"], "execution_not_running", "已結束的單不可再終止");

    let v = expect_status!(app.post(&f.clerk, &f.u("/lineage"), json!({ "upstreamExecutionId": e1, "downstreamExecutionId": e2, "relationKind": "correction" })).await, 200);
    assert!(v["id"].as_i64().unwrap() > 0);
    let v = expect_status!(app.get(&f.clerk, &f.u(&format!("/executions/{e2}/lineage?direction=up"))).await, 200);
    assert_eq!(v["items"][0]["executionId"], e1);
    assert_eq!(v["items"][0]["relationKind"], "correction");
    let v = expect_status!(app.post(&f.clerk, &f.u("/lineage"), json!({ "upstreamExecutionId": e2, "downstreamExecutionId": e1 })).await, 400);
    assert_eq!(v["code"], "lineage_cycle");

    let id = expect_status!(
        app.post(&f.finance, &f.u("/delegations"), json!({ "delegatorUserId": f.finance.id, "delegateUserId": f.accountant.id, "validFrom": "2026-01-01", "validTo": "2026-12-31", "reason": "休假" })).await,
        200
    )["id"]
        .as_i64()
        .unwrap();
    let v = expect_status!(app.get(&f.accountant, &f.u("/delegations")).await, 200);
    let d = v["items"].as_array().unwrap().iter().find(|d| d["id"] == id).unwrap().clone();
    assert_eq!((d["active"].clone(), d["mine"].clone(), d["delegatorName"].clone()), (json!(true), json!(false), json!("finance")));
    expect_status!(app.post(&f.accountant, &f.u(&format!("/delegations/{id}/revoke")), json!({})).await, 403);
    expect_status!(app.post(&f.finance, &f.u(&format!("/delegations/{id}/revoke")), json!({})).await, 200);
    let v = expect_status!(app.post(&f.finance, &f.u(&format!("/delegations/{id}/revoke")), json!({})).await, 409);
    assert_eq!(v["code"], "delegation_already_revoked");
    let v = expect_status!(app.get(&f.finance, &f.u("/delegations")).await, 200);
    assert_eq!(v["items"].as_array().unwrap().iter().find(|d| d["id"] == id).unwrap()["active"], false);

    // 轉移與強制推進。
    let t = f.open_task(&f.admin, e2).await["id"].as_i64().unwrap();
    expect_status!(app.post(&f.admin, &f.u(&format!("/tasks/{t}/reassign")), json!({ "toUserId": f.manager.id, "reason": "承辦請假" })).await, 200);
    let v = expect_status!(app.post(&f.admin, &f.u(&format!("/executions/{e2}/force-advance")), json!({ "toNodeKey": "nope", "reason": "x" })).await, 400);
    assert_eq!(v["code"], "node_not_found");
    expect_status!(app.post(&f.admin, &f.u(&format!("/executions/{e2}/force-advance")), json!({ "toNodeKey": "finance-release", "reason": "客戶催件" })).await, 200);
    let v = expect_status!(app.get(&f.admin, &f.u(&format!("/executions/{e2}"))).await, 200);
    assert_eq!(v["currentNodeKey"], "finance-release");
    let audits = expect_status!(app.get(&f.admin, &f.u(&format!("/audit-events?type=flow.force_advanced&targetId={e2}"))).await, 200);
    assert_eq!(audits["items"][0]["severity"], "critical");
    let tasks = expect_status!(app.get(&f.admin, &f.u("/audit-events?targetType=energyWorkflowTask&type=flow.task_reassigned")).await, 200);
    assert_eq!(tasks["items"][0]["targetExecutionId"], e2, "task 類的稽核補得出它屬於哪一筆流程");
}
