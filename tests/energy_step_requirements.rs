//! 流程引擎：每一關「必傳附件類別」「必填欄位」的定義與送出前檢查（20261118-0811-d）。
//! 照 tests/energy_workflow_2c.rs 的直接引擎呼叫寫法；補件自動判定／人工選類別照
//! tests/energy_recognition.rs 的 fake_tpr::spawn 寫法另開一個 fixture。

use crate::support::energy::{code_of, date};
use crate::support::{TestApp, TestUser};
use eo_domain::workflow_defs::{ActionDef, ActionKind, AssignmentRule, NodeDef, NodeType, TransitionDef, WorkflowDefinition};
use eo_energy::fake_tpr;
use eo_energy::workflow::engine::run::{start_execution, StartExecution};
use eo_energy::workflow::engine::tasks::{self, ActionInput};
use reqwest::Method;
use serde_json::{json, Value};

/// 一關要求：至少一份 `taipowerNotice`、`totalAmount` 必填；另掛一顆不受檢查的 sideEffect 動作。
fn def() -> WorkflowDefinition {
    let mut d = WorkflowDefinition::new("step-req-test", "送出前檢查測試", "energyBill", "");
    let mut ack = ActionDef::new("ack", "flow.action.ack", ActionKind::SideEffect, 1).side_effect("test.acked", false);
    ack.who = AssignmentRule::role(&["energy:operator"]);
    d.nodes = vec![
        NodeDef::new("start", "開始", NodeType::Start),
        NodeDef::new("review", "審核", NodeType::HumanTask)
            .rule(AssignmentRule::role(&["energy:operator"]))
            .subject_edit(true)
            .requires_attachment("taipowerNotice", 1)
            .requires_field("totalAmount", "energy.bills.field.totalAmount")
            .actions(vec![ActionDef::new("pass", "flow.action.pass", ActionKind::Advance, 0), ack]),
        NodeDef::new("end", "完成", NodeType::End),
    ];
    d.transitions = vec![TransitionDef::new("start", "review", 0), TransitionDef::new("review", "end", 0)];
    d
}

struct Fx {
    app: TestApp,
    org: i64,
    owner: TestUser,
    /// 發起人：跟 `operator` 是不同人，否則防自核會讓 operator 連候選池都進不去（not_a_candidate）。
    starter: TestUser,
    operator: TestUser,
}

async fn fixture(tweak: impl FnOnce(&mut eo_platform::config::Config)) -> Fx {
    let app = TestApp::with_config(tweak).await;
    let (owner, org) = app.energy_org("step-req").await;
    let starter = app.member(org, "starter", Some("energy:operator")).await;
    let operator = app.member(org, "operator", Some("energy:operator")).await;
    Fx { app, org, owner, starter, operator }
}

impl Fx {
    fn u(&self, p: &str) -> String {
        self.app.e(self.org, p)
    }

    /// 裝定義＋綁定，開一張沒有附件、總金額空著的電費單，發起執行，回傳 (execution id, task id)。
    async fn start(&self) -> (i64, i64) {
        let def_id = self.app.install_def(&self.owner, self.org, &def(), None).await;
        self.app.wf_binding(self.org, Some("ST-STEPREQ"), None, None, def_id, "2000-01-01", None).await;
        let bill_id = self.app.bill(self.org, None, "2026-01-01", "2026-01-31", None, None).await;
        let starter_ctx = self.app.ctx(&self.starter, self.org).await;
        let ctx = self.app.ctx(&self.operator, self.org).await;
        let exec = in_tx!(self.app, tx => start_execution(&mut tx, &starter_ctx, StartExecution {
            station_key: "ST-STEPREQ".into(),
            workflow_key: def().key,
            subject_type: "energyBill".into(),
            subject_id: bill_id.to_string(),
            business_date: date("2026-01-15"),
            attributes: json!({}),
            definition: None,
            station_id: None,
        }).await)
        .unwrap();
        let task: i64 = sqlx::query_scalar(
            "SELECT id FROM tbl_energy_workflow_task WHERE execution_id = $1 AND node_key = 'review' ORDER BY id DESC LIMIT 1",
        )
        .bind(exec.id)
        .fetch_one(&self.app.core.pool)
        .await
        .unwrap();
        in_tx!(self.app, tx => tasks::claim(&mut tx, &ctx, task).await).unwrap();
        (exec.id, task)
    }

    async fn upload(&self, exec: i64, name: &str, mime: &str, bytes: &[u8], role: &str) -> (u16, Value) {
        let part = reqwest::multipart::Part::bytes(bytes.to_vec()).file_name(name.to_string()).mime_str(mime).unwrap();
        let form = reqwest::multipart::Form::new().text("role", role.to_string()).part("file", part);
        let (s, v) = TestApp::send(self.app.req(Some(&self.operator), Method::POST, &self.u(&format!("/executions/{exec}/attachments"))).multipart(form)).await;
        (s.as_u16(), v)
    }
}

const PDF: &[u8] = b"%PDF-1.4\n% step requirements test\n";

/// 來源：DESIGN.md S2——缺附件/缺欄位擋 422，補齊兩樣才放行；sideEffect 不受這道檢查影響。
#[actix_web::test]
async fn blocks_advance_until_attachment_and_field_present_but_not_side_effect() {
    let fx = fixture(|_| {}).await;
    let (exec, task) = fx.start().await;
    let ctx = fx.app.ctx(&fx.operator, fx.org).await;

    // sideEffect：缺東西也能按，證明檢查只擋 advance。
    in_tx!(fx.app, tx => tasks::perform_action(&mut tx, &ctx, task, ActionInput { action_key: "ack".into(), ..Default::default() }).await).unwrap();

    // advance：兩樣都缺 → 422，帶兩個 details。
    let err = in_tx!(fx.app, tx => tasks::perform_action(&mut tx, &ctx, task, ActionInput { action_key: "pass".into(), ..Default::default() }).await);
    let (status, code) = code_of(err);
    assert_eq!((status, code.as_str()), (422, "step_requirements_unmet"));

    // node_actions DTO：本關要求兩項都 met=false。
    let v = expect_status!(fx.app.get(&fx.operator, &fx.u(&format!("/executions/{exec}/actions"))).await, 200);
    let reqs = v["requirements"].as_array().unwrap();
    assert_eq!(reqs.len(), 2, "{v}");
    assert!(reqs.iter().all(|r| r["met"] == false), "兩項都還沒滿足：{v}");

    // 補一份 taipowerNotice（role=source 一律標這個類別，不必辨識服務）。
    let (s, up) = fx.upload(exec, "notice.pdf", "application/pdf", PDF, "source").await;
    assert_eq!(s, 200, "{up}");
    assert_eq!(up["category"], "taipowerNotice");

    // 附件滿足了，欄位還缺 —— 仍是 422，但只剩 totalAmount。
    let err = in_tx!(fx.app, tx => tasks::perform_action(&mut tx, &ctx, task, ActionInput { action_key: "pass".into(), ..Default::default() }).await);
    let (status, _) = code_of(err);
    assert_eq!(status, 422);
    let v = expect_status!(fx.app.get(&fx.operator, &fx.u(&format!("/executions/{exec}/actions"))).await, 200);
    let reqs = v["requirements"].as_array().unwrap();
    assert!(reqs.iter().find(|r| r["type"] == "attachment").unwrap()["met"] == true, "{v}");
    assert!(reqs.iter().find(|r| r["type"] == "field").unwrap()["met"] == false, "{v}");

    // 補總金額（node 開了 subject_edit，PATCH 這張單）。
    expect_status!(fx.app.patch(&fx.operator, &fx.u(&format!("/bills/{}", bill_id_of(&fx, exec).await)), json!({ "totalAmount": 999 })).await, 200);

    // 兩樣都補齊 → 通過，流程走到 end。
    in_tx!(fx.app, tx => tasks::perform_action(&mut tx, &ctx, task, ActionInput { action_key: "pass".into(), ..Default::default() }).await).unwrap();
    let (status, node): (String, Option<String>) =
        sqlx::query_as("SELECT status, current_node_key FROM tbl_energy_workflow_execution WHERE id = $1").bind(exec).fetch_one(&fx.app.core.pool).await.unwrap();
    assert_eq!((status.as_str(), node.as_deref()), ("completed", Some("end")));
}

async fn bill_id_of(fx: &Fx, exec: i64) -> i64 {
    let subject_id: String = sqlx::query_scalar("SELECT subject_id FROM tbl_energy_workflow_execution WHERE id = $1").bind(exec).fetch_one(&fx.app.core.pool).await.unwrap();
    subject_id.parse().unwrap()
}

/// 來源：DESIGN.md S3/S4——補件自動判定（檔名含範本代號→auto taipowerNotice；其他→needsCategory；
/// 沒設辨識服務→直接 needsCategory），以及 PATCH 類別（人選、非法代碼 400）。
#[actix_web::test]
async fn attachment_auto_classify_and_manual_category() {
    let (url, _fake, server) = fake_tpr::spawn(0, 0).unwrap();
    actix_web::rt::spawn(server);
    let fx = fixture(|c| c.tpr_url = Some(url)).await;
    let (exec, _task) = fx.start().await;

    // 檔名含範本代號 → 辨識服務判得出 → auto taipowerNotice。
    let (s, up) = fx.upload(exec, "renewable-surplus-補件.pdf", "application/pdf", PDF, "added").await;
    assert_eq!(s, 200, "{up}");
    assert_eq!(up["classifyState"], "recognizing", "{up}");
    let id1 = up["id"].as_i64().unwrap();
    wait_classified(&fx, id1).await;
    let row = attachment_row(&fx, id1).await;
    assert_eq!((row.0.as_deref(), row.1.as_deref()), (Some("taipowerNotice"), Some("auto")), "{row:?}");

    // 檔名沒有範本代號 → 判不出 → needsCategory。
    let (s, up2) = fx.upload(exec, "bankbook.pdf", "application/pdf", PDF, "added").await;
    assert_eq!(s, 200, "{up2}");
    let id2 = up2["id"].as_i64().unwrap();
    wait_classified(&fx, id2).await;
    let row2 = attachment_row(&fx, id2).await;
    assert_eq!((row2.0.clone(), row2.1.as_deref()), (None, Some("needsCategory")), "{row2:?}");

    // 人選類別：合法代碼成功、state 變 manual；非法代碼 400。
    expect_status!(
        fx.app.patch(&fx.operator, &fx.u(&format!("/attachments/{id2}/category")), json!({ "category": "bankbookCopy" })).await,
        200
    );
    let row2b = attachment_row(&fx, id2).await;
    assert_eq!((row2b.0.as_deref(), row2b.1.as_deref()), (Some("bankbookCopy"), Some("manual")), "{row2b:?}");
    let (s, e) = fx.app.patch(&fx.operator, &fx.u(&format!("/attachments/{id2}/category")), json!({ "category": "not_a_real_category" })).await;
    assert_eq!((s.as_u16(), e["code"].clone()), (400, json!("invalid_attachment_category")));
}

/// 沒設辨識服務時：補件直接 needsCategory，不送辨識。
#[actix_web::test]
async fn no_tpr_configured_goes_straight_to_needs_category() {
    let fx = fixture(|_| {}).await;
    let (exec, _task) = fx.start().await;
    let (s, up) = fx.upload(exec, "renewable-surplus-補件.pdf", "application/pdf", PDF, "added").await;
    assert_eq!(s, 200, "{up}");
    assert_eq!(up["classifyState"], "needsCategory", "沒設辨識服務時不該卡在 recognizing：{up}");
    assert_eq!(up["category"], Value::Null);
}

async fn attachment_row(fx: &Fx, id: i64) -> (Option<String>, Option<String>) {
    sqlx::query_as("SELECT category, data->'classify'->>'state' FROM tbl_energy_attachment WHERE id = $1").bind(id).fetch_one(&fx.app.core.pool).await.unwrap()
}

/// 背景分類是非同步的（`actix_web::rt::spawn`）——輪詢到不再是 recognizing 為止，上限 5 秒
/// （假服務 `polls_before_done=0`，第一次查詢就是終態，實際通常一兩百毫秒就done）。
async fn wait_classified(fx: &Fx, attachment_id: i64) {
    for _ in 0..50 {
        let (_, state): (Option<String>, Option<String>) = attachment_row(fx, attachment_id).await;
        if state.as_deref() != Some("recognizing") {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("attachment {attachment_id} 還卡在 recognizing（5 秒逾時）");
}
