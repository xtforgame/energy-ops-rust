//! 流程單清單：以 execution 為主表、subject 多型展開；狀態五個值、篩選、等待天數、可視度、單號。

use crate::support::{TestApp, TestUser};
use chrono::Duration;
use eo_energy::workflow::list::{self, ListFilter};
use eo_energy::workflow::views;
use serde_json::{json, Value};

struct Fx {
    app: TestApp,
    org: i64,
    landowner: i64,
    station: i64,
    member: TestUser,
    outsider: TestUser,
    def_v1: i64,
    def_v2: i64,
    deposit_def: i64,
    bill_exec: i64,
    old_version_exec: i64,
    deposit_exec: i64,
    abort_pending_exec: i64,
    completed_exec: i64,
    bill_of_exec: i64,
}

async fn def_row(app: &TestApp, org: i64, key: &str, version: i32, name: &str, subject: &str, status: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO tbl_energy_workflow_definition (organization_id, key, version, name, subject_type, status, published_at)
         VALUES ($1, $2, $3, $4, $5, $6, now()) RETURNING id",
    )
    .bind(org)
    .bind(key)
    .bind(version)
    .bind(name)
    .bind(subject)
    .bind(status)
    .fetch_one(&app.core.pool)
    .await
    .unwrap()
}

/// 一筆 execution ＋ 它現在開著的那一關（含候選人）；開關時間往回撥。
async fn mk_exec(
    app: &TestApp,
    org: i64,
    def: i64,
    subject_type: &str,
    subject_id: i64,
    status: &str,
    node: &str,
    days_ago: i64,
    candidate: Option<i64>,
    station_fk: Option<i64>,
) -> i64 {
    let at = app.clock_now() - Duration::days(days_ago);
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO tbl_energy_workflow_execution
           (organization_id, definition_id, station_key, station_id, subject_type, subject_id, business_date, status, current_node_key, started_at)
         VALUES ($1, $2, 'P2-STATION', $3, $4, $5, '2026-03-01', $6, $7, $8) RETURNING id",
    )
    .bind(org)
    .bind(def)
    .bind(station_fk)
    .bind(subject_type)
    .bind(subject_id.to_string())
    .bind(status)
    .bind(node)
    .bind(at)
    .fetch_one(&app.core.pool)
    .await
    .unwrap();
    if status == "completed" {
        return id;
    }
    let task: i64 = sqlx::query_scalar(
        "INSERT INTO tbl_energy_workflow_task (execution_id, node_key, node_type, seq, state, created_at)
         VALUES ($1, $2, 'humanTask', 1, 'ready', $3) RETURNING id",
    )
    .bind(id)
    .bind(node)
    .bind(at)
    .fetch_one(&app.core.pool)
    .await
    .unwrap();
    if let Some(u) = candidate {
        sqlx::query("INSERT INTO tbl_energy_workflow_task_candidate (task_id, user_id, kind, resolved_by) VALUES ($1, $2, 'potentialOwner', 'test-fixture')")
            .bind(task)
            .bind(u)
            .execute(&app.core.pool)
            .await
            .unwrap();
    }
    id
}

async fn fixture() -> Fx {
    let app = TestApp::new().await;
    let (_owner, org) = app.energy_org("p2-org").await;
    let member = app.member(org, "member", None).await;
    let outsider = app.member(org, "outsider", None).await;
    let landowner = app.landowner(org, "林地主").await;
    let station = app.station(org, landowner, "P2-STATION", "屏東枋寮一號").await;
    let def_v1 = def_row(&app, org, "bill-approval", 1, "電費單審批", "energyBill", "superseded").await;
    let def_v2 = def_row(&app, org, "bill-approval", 2, "電費單審批", "energyBill", "published").await;
    let deposit_def = def_row(&app, org, "deposit-request", 1, "保證金請款", "energyInstallment", "published").await;
    for (d, k, n) in [(def_v2, "review", "校對"), (deposit_def, "submit", "送出請款")] {
        sqlx::query("INSERT INTO tbl_energy_workflow_node (definition_id, node_key, node_type, name) VALUES ($1, $2, 'humanTask', $3)")
            .bind(d)
            .bind(k)
            .bind(n)
            .execute(&app.core.pool)
            .await
            .unwrap();
    }
    let bill_a = app.bill(org, Some(station), "2026-01-01", "2026-02-28", Some("138195"), None).await;
    let bill_b = app.bill(org, Some(station), "2026-01-01", "2026-02-28", Some("60000"), None).await;
    let bill_c = app.bill(org, Some(station), "2026-01-01", "2026-02-28", Some("41000"), None).await;
    let contract = app.contract(org, landowner, "CT-P2", "2025-01-01", None).await;
    app.contract_station(contract, station, json!({}), None, None).await;
    let inst = app.installment(org, contract, 1, "2026-04-30", "500000").await;
    let bill_exec = mk_exec(&app, org, def_v2, "energyBill", bill_a, "running", "review", 5, Some(member.id), Some(station)).await;
    let old_version_exec = mk_exec(&app, org, def_v1, "energyBill", bill_b, "running", "review", 2, None, Some(station)).await;
    // 舊列沒有 station_id —— 逼案場篩選走 subject 推導。
    let deposit_exec = mk_exec(&app, org, deposit_def, "energyInstallment", inst, "running", "submit", 12, None, None).await;
    let abort_pending_exec = mk_exec(&app, org, def_v2, "energyBill", bill_c, "abortPending", "review", 3, None, Some(station)).await;
    let bill_d = app.bill(org, Some(station), "2026-01-01", "2026-02-28", Some("7000"), None).await;
    let completed_exec = mk_exec(&app, org, def_v2, "energyBill", bill_d, "completed", "done", 30, None, Some(station)).await;
    Fx {
        app,
        org,
        landowner,
        station,
        member,
        outsider,
        def_v1,
        def_v2,
        deposit_def,
        bill_exec,
        old_version_exec,
        deposit_exec,
        abort_pending_exec,
        completed_exec,
        bill_of_exec: bill_a,
    }
}

impl Fx {
    async fn list(&self, who: &TestUser, view_all: bool, f: ListFilter) -> Value {
        let ctx = self.app.ctx(who, self.org).await;
        let mut tx = self.app.core.pool.begin().await.unwrap();
        list::list(&mut tx, &ctx, view_all, &f).await.unwrap()
    }

    async fn all(&self, f: ListFilter) -> Value {
        self.list(&self.outsider, true, f).await
    }
}

fn running() -> ListFilter {
    ListFilter { status: Some("running".into()), ..Default::default() }
}

fn ids(v: &Value) -> Vec<i64> {
    v["items"].as_array().unwrap().iter().map(|r| r["id"].as_i64().unwrap()).collect()
}

fn row(v: &Value, id: i64) -> Value {
    v["items"].as_array().unwrap().iter().find(|r| r["id"] == id).cloned().unwrap()
}

/// 來源：executionList.test.ts「保證金那類（subject＝energyInstallment）撈得到 —— 「查 bills 再 join」會讓這條紅」
/// 來源：executionList.test.ts「「待中斷核可」算進行中（寫成 status:'running' 會讓它整批消失）」
/// 來源：executionList.test.ts「已完成頁籤拿得到 completed」
/// 來源：executionList.test.ts「workflowKey 篩選涵蓋**所有版本**（只取現行版會漏掉跑在舊版上的單）」
/// 來源：executionList.test.ts「「等了幾天」算得出實際數字，而且可排序 / 可篩選」
/// 來源：executionList.test.ts「案場篩選：execution 自己的 station_id **與** subject 推導出來的都要算」
/// 來源：executionList.test.ts「地主篩選（P5「看他的所有單」的落點）」
/// 來源：executionList.test.ts「節點篩選（P9 節點 drawer「看全部」的落點）」
/// 來源：executionList.test.ts「不接受 `unsubmitted` —— 需要「這東西不存在」的頁籤＝主體選錯了」
/// 來源：executionList.test.ts「`executionId` 指名某一筆時**不套 status**（從電費單點過來的那條連結）」
#[actix_web::test]
async fn polymorphic_list_statuses_and_filters() {
    let f = fixture().await;
    let r = f.all(running()).await;
    let dep = row(&r, f.deposit_exec);
    assert_eq!(dep["workflowName"], "保證金請款");
    assert_eq!(dep["workflowKey"], "deposit-request");
    assert_eq!(dep["subject"]["kind"], "energyInstallment");
    assert_eq!(dep["subject"]["amount"], "500000.00");
    assert!(dep["subject"]["contractNo"].is_string());
    assert!(dep["subject"]["stationName"].is_string(), "保證金那列的案場名不可以是空的");
    assert!(dep["subject"].get("route").is_none());

    let i = ids(&r);
    assert!(i.contains(&f.abort_pending_exec), "abortPending 必須看得到");
    assert!(i.contains(&f.bill_exec));
    assert!(!i.contains(&f.completed_exec));
    let done = ids(&f.all(ListFilter { status: Some("completed".into()), ..Default::default() }).await);
    assert!(done.contains(&f.completed_exec) && !done.contains(&f.bill_exec));

    let r = f.all(ListFilter { workflow_key: Some("bill-approval".into()), ..running() }).await;
    let i = ids(&r);
    assert!(i.contains(&f.old_version_exec) && i.contains(&f.bill_exec) && !i.contains(&f.deposit_exec));
    assert_eq!(row(&r, f.old_version_exec)["version"], 1);

    let r = f.all(ListFilter { sort: Some("waitingDays:desc".into()), ..running() }).await;
    let w = row(&r, f.deposit_exec)["waitingDays"].as_i64().unwrap();
    assert!((11..=13).contains(&w), "等了 12 天：{w}");
    assert_eq!(ids(&r)[0], f.deposit_exec, "最久的在最前面");
    assert_eq!(ids(&f.all(ListFilter { waiting_days_min: Some(10), ..running() }).await), vec![f.deposit_exec]);

    let i = ids(&f.all(ListFilter { station_id: Some(f.station), ..running() }).await);
    assert!(i.contains(&f.deposit_exec), "靠契約涵蓋的案場推導出來的也要在");
    assert!(i.contains(&f.bill_exec));

    let r = f.all(ListFilter { landowner_id: Some(f.landowner), ..running() }).await;
    assert!(!ids(&r).is_empty());
    assert!(r["items"].as_array().unwrap().iter().all(|x| x["subject"]["landownerId"] == f.landowner));

    let r = f.all(ListFilter { workflow_key: Some("bill-approval".into()), node_key: Some("review".into()), ..running() }).await;
    assert!(r["items"].as_array().unwrap().iter().all(|x| x["nodeKey"] == "review"));
    assert_eq!(row(&r, f.bill_exec)["nodeName"], "校對");

    let r = f.all(ListFilter { status: Some("unsubmitted".into()), ..Default::default() }).await;
    assert_ne!(r["tab"], "unsubmitted");
    assert!(ids(&r).iter().all(|id| *id > 0));

    assert_eq!(ids(&f.all(ListFilter { execution_id: Some(f.completed_exec), ..running() }).await), vec![f.completed_exec]);
    let _ = (f.def_v1, f.def_v2, f.deposit_def);
}

/// 來源：executionList.test.ts「沒有 view-all 的人只看得到自己參與的（前端不做第二套過濾）」
/// 來源：executionList.test.ts「`scope=mine` 對**有 view-all 的人也要過濾**（否則那一格等於全組織）」
/// 來源：executionList.test.ts「`scope=mine` 預設**含已完成**（那一格問的是「我碰過什麼」）」
/// 來源：executionList.test.ts「「曾在候選池但沒認領」也算參與過（只看目前 task 的實作會漏掉）」
/// 來源：executionList.test.ts「跨組織看不到（org 由 server 端推導，不信 client）」
#[actix_web::test]
async fn visibility_and_scope() {
    let f = fixture().await;
    let all = f.list(&f.outsider, true, running()).await;
    let mine = f.list(&f.member, false, running()).await;
    let none = f.list(&f.outsider, false, running()).await;
    assert_eq!(ids(&mine), vec![f.bill_exec], "候選池裡的人在動手之前就看得到");
    assert!(ids(&none).is_empty());
    assert!(ids(&all).len() > ids(&mine).len());

    let mine_scope = ListFilter { scope: Some("mine".into()), ..Default::default() };
    let r = f.list(&f.outsider, true, mine_scope.clone()).await;
    assert_eq!(r["scope"], "mine");
    assert!(ids(&r).is_empty(), "view-all 也不能讓「我參與過的」變成全組織");
    assert!(!ids(&f.list(&f.outsider, true, ListFilter { scope: Some("all".into()), ..Default::default() }).await).is_empty());

    let r = f.list(&f.member, false, mine_scope.clone()).await;
    assert_eq!(r["tab"], "all");
    let scope_bill = f.app.bill(f.org, Some(f.station), "2027-01-01", "2027-02-28", Some("1234"), None).await;
    let done = mk_exec(&f.app, f.org, f.def_v2, "energyBill", scope_bill, "completed", "end", 1, None, Some(f.station)).await;
    f.app.exec_sql(&format!("UPDATE tbl_energy_workflow_execution SET started_by_user_id = {} WHERE id = {done}", f.member.id)).await;
    assert!(ids(&f.list(&f.member, false, mine_scope.clone()).await).contains(&done), "已完成的單也算「我參與過的」");
    let r = f.list(&f.member, false, ListFilter { scope: Some("mine".into()), status: Some("running".into()), ..Default::default() }).await;
    assert!(!ids(&r).contains(&done));

    let other = f.app.member(f.org, "other", None).await;
    let b2 = f.app.bill(f.org, Some(f.station), "2027-01-01", "2027-02-28", Some("1234"), None).await;
    let e = mk_exec(&f.app, f.org, f.def_v2, "energyBill", b2, "running", "clerk-review", 1, Some(f.outsider.id), Some(f.station)).await;
    f.app.exec_sql(&format!("UPDATE tbl_energy_workflow_task SET assignee_user_id = {}, state = 'completed' WHERE execution_id = {e}", other.id)).await;
    assert!(ids(&f.list(&f.outsider, false, mine_scope).await).contains(&e), "候選池那一條一旦成立就永久成立");

    // 跨組織：在別的組織看不到這些單。
    let (other_owner, other_org) = f.app.energy_org("p2-other").await;
    let ctx = f.app.ctx(&other_owner, other_org).await;
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let r = list::list(&mut tx, &ctx, true, &running()).await.unwrap();
    assert!(ids(&r).is_empty());
}

/// 來源：executionList.test.ts「controller 拿得到服務（class field 會遮蔽同名 getter —— 這條就是在擋那個 500）」
/// 來源：executionList.test.ts「`getOne` 走**同一份 decorate**（P12 詳情頁的資料來源）」
/// 來源：executionList.test.ts「清單的每一列都不帶前端路由（導覽是前端的決定）」
/// 來源：executionList.test.ts「每一種 subjectType 都展開得出摘要（多型不可以只做一半）」
/// 來源：executionList.test.ts「工作台 DTO 也要展開保證金（前一版 subject 是 null → 點了沒反應）」
/// 來源：executionList.test.ts「單號：`documentNo` 有值就用它，沒有就落回 `#id`（連結仍走 id）」
/// 來源：executionList.test.ts「單號是**正規欄位**：組織內唯一、查得到、貼網址開得起來」
#[actix_web::test]
async fn detail_shares_decorate_and_document_numbers() {
    let f = fixture().await;
    // 端點那條線：HTTP 打得到、回得出東西。
    let admin = f.app.member(f.org, "viewer", Some("energy:viewer")).await;
    let v = expect_status!(f.app.get(&admin, &f.app.e(f.org, "/executions?status=running")).await, 200);
    assert!(v["items"].is_array());

    let r = f.all(running()).await;
    let ctx = f.app.ctx(&f.outsider, f.org).await;
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let one = list::find_execution(&mut tx, &ctx, &f.deposit_exec.to_string()).await.unwrap().unwrap();
    let one = list::decorate(&mut tx, &ctx, &[one]).await.unwrap().remove(0);
    let from_list = row(&r, f.deposit_exec);
    assert_eq!(json!(one.workflow_name), from_list["workflowName"]);
    assert_eq!(json!(one.node_name), from_list["nodeName"]);
    assert_eq!(one.subject["kind"], "energyInstallment");
    drop(tx);

    for x in r["items"].as_array().unwrap() {
        assert!(x["subject"].get("route").is_none());
        let flat = x.to_string();
        assert!(!regex_like_route(&flat), "回應裡不可以出現前端路由字串：{flat}");
        assert!(x["subject"]["kind"].is_string(), "execution #{} 的 subject 沒展開", x["id"]);
        assert_eq!(x["displayId"], format!("#{}", x["id"]));
    }
    let kinds: Vec<&str> = r["items"].as_array().unwrap().iter().filter_map(|x| x["subject"]["kind"].as_str()).collect();
    assert!(kinds.contains(&"energyBill") && kinds.contains(&"energyInstallment"));

    // 工作台 DTO 也要展開保證金。
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let tasks = eo_energy::workflow::model::tasks_of(&mut tx, f.deposit_exec).await.unwrap();
    assert!(!tasks.is_empty());
    let member = f.app.ctx(&f.member, f.org).await;
    let rows = views::decorate_tasks(&mut tx, &member, f.member.id, &tasks).await.unwrap();
    assert_eq!(rows[0]["subject"]["kind"], "energyInstallment");
    assert!(rows[0]["subject"]["contractNo"].is_string());
    assert!(rows[0]["subject"]["totalAmount"].as_str().unwrap().parse::<f64>().unwrap() > 0.0);
    drop(tx);

    // 單號住在 subject 上：查得到、貼網址開得起來、跨組織查不到。
    f.app.exec_sql(&format!("UPDATE tbl_energy_bill SET document_no = 'TPC-0001' WHERE id = {}", f.bill_of_exec)).await;
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let by_no = list::find_execution(&mut tx, &ctx, "TPC-0001").await.unwrap().unwrap();
    assert_eq!(by_no.id, f.bill_exec);
    let d = list::decorate(&mut tx, &ctx, &[by_no]).await.unwrap().remove(0);
    assert_eq!(d.display_id, "TPC-0001");
    drop(tx);
    assert_eq!(ids(&f.all(ListFilter { document_no: Some("TPC-0001".into()), ..running() }).await), vec![f.bill_exec]);
    let v = expect_status!(f.app.get(&admin, &f.app.e(f.org, "/executions/TPC-0001")).await, 200);
    assert_eq!(v["id"], f.bill_exec);
    expect_status!(f.app.get(&admin, &f.app.e(f.org, "/executions/NO-SUCH-DOC")).await, 404);

    // 唯一只保證在「已通過」之間：未通過的第二列建得起來，兩張都通過就撞 unique。
    let second: Result<i64, sqlx::Error> = sqlx::query_scalar(
        "INSERT INTO tbl_energy_bill (organization_id, station_id, document_no, total_amount) VALUES ($1, $2, 'TPC-0001', 1) RETURNING id",
    )
    .bind(f.org)
    .bind(f.station)
    .fetch_one(&f.app.core.pool)
    .await;
    assert!(second.is_ok());
    f.app.exec_sql(&format!("UPDATE tbl_energy_bill SET approval_state = 'approved' WHERE id = {}", f.bill_of_exec)).await;
    let dup = sqlx::query("INSERT INTO tbl_energy_bill (organization_id, station_id, document_no, total_amount, approval_state) VALUES ($1, $2, 'TPC-0001', 1, 'approved')")
        .bind(f.org)
        .bind(f.station)
        .execute(&f.app.core.pool)
        .await;
    assert_eq!(dup.unwrap_err().as_database_error().unwrap().code().as_deref(), Some("23505"));
    let mut tx = f.app.core.pool.begin().await.unwrap();
    assert_eq!(list::find_execution(&mut tx, &ctx, "TPC-0001").await.unwrap().unwrap().id, f.bill_exec, "命中已通過的那一張");
}

/// 路徑長得像前端路由（`"/energyXxxConsole/`、`"/energyXxxs/`）就算違規。
fn regex_like_route(s: &str) -> bool {
    let mut rest = s;
    while let Some(i) = rest.find("\"/energy") {
        let tail = &rest[i + 8..];
        let word: String = tail.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
        let after = &tail[word.len()..];
        if after.starts_with('/') && (word.ends_with("Console") || word.ends_with('s')) {
            return true;
        }
        rest = &rest[i + 1..];
    }
    false
}
