//! 單據詳情的五個模式（端到端）：intake／revise／act／readonly／denied 由後端判定。

use crate::support::energy::date;
use crate::support::{TestApp, TestUser};
use eo_domain::resolve_mode::BillMode;
use eo_domain::workflow_defs::{default_bill_approval, DEPT_ACCOUNTING, DEPT_FINANCE, DEPT_SALES};
use eo_energy::workflow::engine::admin;
use eo_energy::workflow::engine::run::{start_execution, StartExecution};
use eo_energy::workflow::engine::tasks::{self, ActionInput};
use eo_energy::workflow::mode::{mode_for, ModeOut};
use serde_json::json;

struct Fx {
    app: TestApp,
    org: i64,
    station: i64,
    cn: String,
    operator: TestUser,
    manager: TestUser,
    outsider: TestUser,
    seq: std::cell::Cell<u32>,
}

async fn fixture() -> Fx {
    let app = TestApp::new().await;
    let (owner, org) = app.energy_org("mode-org").await;
    let operator = app.member(org, "operator", None).await;
    let manager = app.member(org, "manager", None).await;
    let outsider = app.member(org, "outsider", None).await;
    let lo = app.landowner(org, "陳明德").await;
    let cn = "12-3456-7890".to_string();
    let station = app.station(org, lo, &cn, "屏東枋寮一號").await;
    // 承辦與主管都在業務部：clerk-review 指派給整個部門。
    let sales = app.department(org, DEPT_SALES, "業務部").await;
    app.dept_member(sales, &operator, "member").await;
    app.dept_member(sales, &manager, "manager").await;
    for (k, n) in [(DEPT_ACCOUNTING, "會計部門"), (DEPT_FINANCE, "財務部")] {
        let d = app.department(org, k, n).await;
        app.dept_member(d, &manager, "manager").await;
    }
    let def = app.install_def(&owner, org, &default_bill_approval(), None).await;
    app.wf_binding(org, Some(&cn), None, None, def, "2000-01-01", None).await;
    Fx { app, org, station, cn, operator, manager, outsider, seq: std::cell::Cell::new(0) }
}

impl Fx {
    async fn bill(&self) -> i64 {
        self.seq.set(self.seq.get() + 1);
        let m = self.seq.get();
        self.app.bill(self.org, Some(self.station), &format!("2026-{m:02}-01"), &format!("2026-{m:02}-28"), Some("60000.00"), Some("15000.000")).await
    }

    /// 送審（收件那條路的等價物：承辦發起這張單的流程）。
    async fn submit(&self, bill: i64) -> i64 {
        let ctx = self.app.ctx(&self.operator, self.org).await;
        let input = StartExecution {
            station_key: self.cn.clone(),
            workflow_key: "bill-approval".into(),
            subject_type: "energyBill".into(),
            subject_id: bill.to_string(),
            business_date: date("2026-01-28"),
            attributes: json!({ "bill": { "totalAmount": 60000 } }),
            definition: None,
            station_id: Some(self.station),
        };
        in_tx!(self.app, tx => start_execution(&mut tx, &ctx, input).await).unwrap().id
    }

    async fn mode(&self, u: &TestUser, bill: i64) -> ModeOut {
        let ctx = self.app.ctx(u, self.org).await;
        let mut tx = self.app.core.pool.begin().await.unwrap();
        mode_for(&mut tx, &ctx, "energyBill", bill).await.unwrap()
    }

    async fn force_to(&self, exec: i64, node: &str) {
        let ctx = self.app.ctx(&self.manager, self.org).await;
        in_tx!(self.app, tx => admin::force_advance(&mut tx, &ctx, exec, node, Some("測試：推到核可關卡")).await).unwrap();
    }
}

/// 來源：mode.test.ts「尚未送審 + **我是送件者** → intake；別人 → readonly」
#[actix_web::test]
async fn intake_for_the_submitter_only() {
    let f = fixture().await;
    let bill = f.bill().await;
    let batch: i64 = sqlx::query_scalar(
        "INSERT INTO tbl_energy_ingest_batch (organization_id, submitted_by_user_id, submitted_at, file_count, status)
         VALUES ($1, $2, now(), 1, 'ready') RETURNING id",
    )
    .bind(f.org)
    .bind(f.operator.id)
    .fetch_one(&f.app.core.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO tbl_energy_ingest_item (organization_id, batch_id, bill_id, original_filename, doc_type, workflow_key, status)
         VALUES ($1, $2, $3, 'a.jpg', 'electricityBill', 'bill-approval', 'ready')",
    )
    .bind(f.org)
    .bind(batch)
    .bind(bill)
    .execute(&f.app.core.pool)
    .await
    .unwrap();
    let mine = f.mode(&f.operator, bill).await;
    assert_eq!(mine.mode(), BillMode::Intake, "我上傳的單，我當然能校對它");
    assert!(mine.verdict.can_edit_subject);
    assert_eq!(mine.execution_id, None);
    let other = f.mode(&f.outsider, bill).await;
    assert_eq!(other.mode(), BillMode::Readonly);
    assert_eq!(serde_json::to_value(other.verdict.mode_reason.unwrap().code).unwrap(), "notSubmitted");
}

/// 來源：mode.test.ts「承辦查**自己的單**（停在核可關）→ readonly，動作列空，modeReason 有值」
/// 來源：mode.test.ts「輪到的主管 → **act**，canEditSubject=false 但 canAddAttachment=true」
/// 來源：mode.test.ts「看得到但沒事做的人 → **readonly**（不是 denied）」
#[actix_web::test]
async fn readonly_act_and_bystanders() {
    let f = fixture().await;
    let bill = f.bill().await;
    let exec = f.submit(bill).await;
    f.force_to(exec, "manager-approve").await;
    let v = f.mode(&f.operator, bill).await;
    assert_eq!(v.mode(), BillMode::Readonly);
    assert!(v.available_actions.is_empty());
    assert!(!v.verdict.can_edit_subject);
    let reason = v.verdict.mode_reason.clone().expect("readonly 一定要有說明");
    let code = serde_json::to_value(reason.code).unwrap();
    assert!(code == "excludedOwner" || code == "notMyTurn", "{code}");
    assert!(reason.node_name().is_some(), "印節點名稱不印 key");

    let v = f.mode(&f.manager, bill).await;
    assert_eq!(v.mode(), BillMode::Act);
    assert!(!v.verdict.can_edit_subject);
    assert!(v.verdict.can_add_attachment, "act 可以補件");
    assert!(!v.available_actions.is_empty());

    // 沒參與、沒 view-all → denied；給 view-all 之後是 readonly（不是 denied）。
    assert_eq!(f.mode(&f.outsider, bill).await.mode(), BillMode::Denied);
    f.app.grant_role(f.org, &f.outsider, "energy:viewer").await;
    let v = f.mode(&f.outsider, bill).await;
    assert_eq!(v.mode(), BillMode::Readonly);
    assert!(v.verdict.mode_reason.is_some());
}

/// 來源：mode.test.ts「停在 allowsSubjectEdit 的節點且我是候選人 → **revise**（可改）」
/// 來源：mode.test.ts「revise 重新送審 → execution **還是同一筆**（不開新的）」
#[actix_web::test]
async fn revise_resubmits_on_the_same_execution() {
    let f = fixture().await;
    let bill = f.bill().await;
    let first = f.submit(bill).await;
    let v = f.mode(&f.manager, bill).await;
    assert_eq!(v.mode(), BillMode::Revise, "停在可編輯的節點 → revise");
    assert!(v.verdict.can_edit_subject);
    assert!(!v.available_actions.is_empty(), "revise 要有「重新送審」可按");
    assert_eq!(v.execution_id, Some(first));

    // 重新送審走的是目前這一關的動作，不是再開一筆。
    let task = f.app.tasks(first).await.into_iter().rev().find(|t| t.state == "ready").unwrap();
    let ctx = f.app.ctx(&f.manager, f.org).await;
    in_tx!(f.app, tx => tasks::claim(&mut tx, &ctx, task.id).await).unwrap();
    let submit = ActionInput { action_key: "submit".into(), ..Default::default() };
    in_tx!(f.app, tx => tasks::perform_action(&mut tx, &ctx, task.id, submit).await).unwrap();
    let n = f
        .app
        .scalar_i64(&format!("SELECT COUNT(*) FROM tbl_energy_workflow_execution WHERE subject_type = 'energyBill' AND subject_id = '{bill}'"))
        .await;
    assert_eq!(n, 1, "不可以開出第二筆 execution");
    assert_ne!(f.app.exec_state(first).await.1.as_deref(), Some("clerk-review"));
    // 同一張單同一條流程只能一筆 running：再開一筆會被擋。
    let ctx = f.app.ctx(&f.operator, f.org).await;
    let again = StartExecution {
        station_key: f.cn.clone(),
        workflow_key: "bill-approval".into(),
        subject_type: "energyBill".into(),
        subject_id: bill.to_string(),
        business_date: date("2026-01-28"),
        attributes: json!({}),
        definition: None,
        station_id: None,
    };
    let r = in_tx!(f.app, tx => start_execution(&mut tx, &ctx, again).await);
    assert_eq!(crate::support::energy::code_of(r), (409, "already_in_review".into()));
}
