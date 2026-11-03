//! 流程引擎 2c：五種動作、回退判定與副作用鎖、並行、中斷、代理 vs 轉移、強制推進、血緣。

use crate::support::energy::{code_of, date};
use crate::support::{TestApp, TestUser};
use eo_domain::workflow_defs::{
    ActionDef, ActionKind, AssignmentRule, JoinPolicy, NodeDef, NodeType, TargetMode, TransitionDef, WorkflowDefinition,
};
use eo_energy::common::Ctx;
use eo_energy::workflow::engine::delegation::{self, NewDelegation};
use eo_energy::workflow::engine::lineage::{self, Direction, NewLineage};
use eo_energy::workflow::engine::run::{start_execution, StartExecution};
use eo_energy::workflow::engine::tasks::{self, ActionInput};
use eo_energy::workflow::engine::admin;
use eo_energy::workflow::model;
use serde_json::json;

struct Fx {
    app: TestApp,
    org: i64,
    owner: TestUser,
    starter: TestUser,
    operator: TestUser,
    accountant: TestUser,
    admin: TestUser,
    cxo: TestUser,
    holiday: TestUser,
    stand_in: TestUser,
}

async fn fixture() -> Fx {
    let app = TestApp::new().await;
    let (owner, org) = app.energy_org("org-2c").await;
    Fx {
        starter: app.member(org, "starter", Some("energy:operator")).await,
        operator: app.member(org, "operator", Some("energy:operator")).await,
        accountant: app.member(org, "accountant", Some("energy:finance")).await,
        admin: app.member(org, "admin", Some("energy:admin")).await,
        cxo: app.member(org, "cxo", Some("energy:cxo")).await,
        holiday: app.member(org, "holiday", Some("energy:finance")).await,
        stand_in: app.member(org, "standin", None).await,
        app,
        org,
        owner,
    }
}

/// 付款流程：start → operatorCheck → payment(revertable=false) → end。「已付款」是財務按的（who ≠ 主責者也可以）。
fn payment_flow(key: &str) -> WorkflowDefinition {
    let mut d = WorkflowDefinition::new(key, "付款流程", "energyBill", "");
    let mut mark = ActionDef::new("markPaid", "flow.action.markPaid", ActionKind::SideEffect, 0).side_effect("payment.sent", true);
    mark.who = AssignmentRule::role(&["energy:finance"]);
    d.nodes = vec![
        NodeDef::new("start", "起點", NodeType::Start),
        NodeDef::new("operatorCheck", "承辦確認", NodeType::HumanTask).rule(AssignmentRule::role(&["energy:operator"])).actions(vec![
            ActionDef::new("submit", "flow.action.submit", ActionKind::Advance, 0),
            ActionDef::new("sendBack", "flow.action.sendBack", ActionKind::Revert, 1).target_mode(TargetMode::PreviousNode),
        ]),
        NodeDef::new("payment", "付款", NodeType::HumanTask)
            .revertable(false)
            .rule(AssignmentRule::role(&["energy:finance"]))
            .actions(vec![mark, ActionDef::new("submit", "flow.action.submit", ActionKind::Advance, 1)]),
        NodeDef::new("end", "終點", NodeType::End),
    ];
    d.transitions = vec![
        TransitionDef::new("start", "operatorCheck", 0),
        TransitionDef::new("operatorCheck", "payment", 0),
        TransitionDef::new("payment", "end", 0),
    ];
    d
}

impl Fx {
    async fn c(&self, u: &TestUser) -> Ctx {
        self.app.ctx(u, self.org).await
    }

    /// 安裝＋綁定一份定義，並以 starter 發起一筆。回傳 execution id。
    async fn start(&self, def: &WorkflowDefinition, subject: &str) -> i64 {
        let station = format!("ST-{subject}");
        let id = self.app.install_def(&self.owner, self.org, def, None).await;
        self.app.wf_binding(self.org, Some(&station), None, None, id, "2000-01-01", None).await;
        let ctx = self.c(&self.starter).await;
        let input = StartExecution {
            station_key: station,
            workflow_key: def.key.clone(),
            subject_type: "energyBill".into(),
            subject_id: subject.into(),
            business_date: date("2026-05-31"),
            attributes: json!({}),
            definition: None,
            station_id: None,
        };
        in_tx!(self.app, tx => start_execution(&mut tx, &ctx, input).await).unwrap().id
    }

    async fn claim(&self, u: &TestUser, task: i64) -> eo_platform::error::ApiResult<()> {
        let ctx = self.c(u).await;
        in_tx!(self.app, tx => tasks::claim(&mut tx, &ctx, task).await)
    }

    async fn press(&self, u: &TestUser, task: i64, key: &str, reason: Option<&str>, note: Option<&str>) -> eo_platform::error::ApiResult<()> {
        let ctx = self.c(u).await;
        let input = ActionInput {
            action_key: key.into(),
            reason: reason.map(String::from),
            note: note.map(String::from),
            ..Default::default()
        };
        in_tx!(self.app, tx => tasks::perform_action(&mut tx, &ctx, task, input).await)
    }

    async fn task_at(&self, exec: i64, node: &str) -> crate::support::energy::TaskLite {
        self.app.tasks(exec).await.into_iter().filter(|t| t.node_key == node).last().unwrap()
    }

    async fn can_revert(&self, exec: i64, node: &str) -> eo_energy::workflow::engine::tasks::RevertVerdict {
        let mut tx = self.app.core.pool.begin().await.unwrap();
        let e = model::execution_in_org(&mut tx, self.org, exec).await.unwrap();
        tasks::can_revert(&mut tx, &e, node).await.unwrap()
    }

    async fn audit_types(&self, target_type: &str, id: i64) -> Vec<String> {
        self.app.audit_rows(self.org, target_type, id).await.into_iter().map(|r| r.0).collect()
    }

    async fn audit_latest(&self, event: &str, id: i64) -> (serde_json::Value, Option<i64>, Option<i64>) {
        sqlx::query_as(
            "SELECT payload, actor_user_id, on_behalf_of_user_id FROM tbl_energy_audit_event
              WHERE organization_id = $1 AND type = $2 AND target_id = $3 ORDER BY id DESC LIMIT 1",
        )
        .bind(self.org)
        .bind(event)
        .bind(id.to_string())
        .fetch_one(&self.app.core.pool)
        .await
        .unwrap()
    }
}

/// 來源：workflowEngine2c.test.ts「a revertable node with no locks CAN revert」
/// 來源：workflowEngine2c.test.ts「a node marked revertable=false cannot revert (the definition half)」
/// 來源：workflowEngine2c.test.ts「sideEffect action does NOT move the node — it only records the fact」
/// 來源：workflowEngine2c.test.ts「the lock blocks reverting a DIFFERENT, still-revertable node — position is irrelevant」
/// 來源：workflowEngine2c.test.ts「pressing a revert action while locked → 403 revert_locked with the blocking rows」
/// 來源：workflowEngine2c.test.ts「releasing the lock re-opens revert; the release itself is a critical audit row」
/// 來源：workflowEngine2c.test.ts「cannot release twice」
#[actix_web::test]
async fn revert_predicate_and_side_effect_lock() {
    let f = fixture().await;
    let exec = f.start(&payment_flow("pay"), "BILL-PAY-1").await;
    let g1 = f.app.tasks(exec).await[0].id;
    f.claim(&f.operator, g1).await.unwrap();
    f.press(&f.operator, g1, "submit", None, None).await.unwrap();
    let pay = f.task_at(exec, "payment").await.id;

    assert!(f.can_revert(exec, "operatorCheck").await.allowed);
    let v = f.can_revert(exec, "payment").await;
    assert_eq!((v.allowed, v.reason), (false, Some("node_not_revertable")));

    f.claim(&f.accountant, pay).await.unwrap();
    f.press(&f.accountant, pay, "markPaid", None, Some("匯款單號 TX-0001")).await.unwrap();
    assert_eq!(f.app.exec_state(exec).await, ("running".into(), Some("payment".into())), "sideEffect 不移動節點");
    assert_eq!(f.task_at(exec, "payment").await.state, "reserved", "任務也還沒完成");
    let effects: Vec<(i64, String, bool, Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT id, side_effect_key, locks_revert, note, applied_by_user_id FROM tbl_energy_workflow_side_effect WHERE execution_id = $1",
    )
    .bind(exec)
    .fetch_all(&f.app.core.pool)
    .await
    .unwrap();
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].1, "payment.sent");
    assert!(effects[0].2, "kind=sideEffect 預設鎖回退");
    assert_eq!(effects[0].3.as_deref(), Some("匯款單號 TX-0001"));
    assert_eq!(effects[0].4, Some(f.accountant.id));

    // 最重要的一條：operatorCheck 本身可回退、流程也還停在 payment，但已付款 → 一律退不了。
    let v = f.can_revert(exec, "operatorCheck").await;
    assert_eq!((v.allowed, v.reason), (false, Some("side_effect_lock")));
    let blocked = v.blocked_by.unwrap();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].side_effect_key, "payment.sent");

    // 舊 task 已完成 → 狀態機先擋；重點是鎖存在時任何回退都不成立。
    assert!(f.press(&f.operator, g1, "sendBack", Some("要退回"), None).await.is_err());

    let se = effects[0].0;
    let admin = f.c(&f.admin).await;
    assert_eq!(code_of(in_tx!(f.app, tx => tasks::release_side_effect(&mut tx, &admin, se, "").await)), (400, "reason_required".into()));
    in_tx!(f.app, tx => tasks::release_side_effect(&mut tx, &admin, se, "錢其實沒匯出去，按錯了").await).unwrap();
    assert!(f.can_revert(exec, "operatorCheck").await.allowed, "解鎖後回退恢復");
    let (released, reason): (Option<chrono::DateTime<chrono::Utc>>, Option<String>) =
        sqlx::query_as("SELECT released_at, release_reason FROM tbl_energy_workflow_side_effect WHERE id = $1")
            .bind(se)
            .fetch_one(&f.app.core.pool)
            .await
            .unwrap();
    assert!(released.is_some(), "原列不刪，releasedAt 有值");
    assert_eq!(reason.as_deref(), Some("錢其實沒匯出去，按錯了"));
    assert_eq!(f.audit_latest("flow.side_effect_released", se).await.0["reason"], "錢其實沒匯出去，按錯了");
    assert_eq!(
        code_of(in_tx!(f.app, tx => tasks::release_side_effect(&mut tx, &admin, se, "再一次").await)),
        (409, "side_effect_already_released".into())
    );
}

/// 來源：workflowEngine2c.test.ts「revert requires a reason (seed forces requiresReason=true for kind=revert)」
/// 來源：workflowEngine2c.test.ts「previousNode revert from the first gate has nowhere to go → no_previous_node」
/// 來源：workflowEngine2c.test.ts「a non-finance user cannot press markPaid even if he owns the task (who ≠ assignee)」
/// 來源：workflowEngine2c.test.ts「setState changes a flag WITHOUT moving the node」
#[actix_web::test]
async fn action_kinds() {
    let f = fixture().await;
    let exec = f.start(&payment_flow("kind"), "BILL-KIND-1").await;
    let t = f.app.tasks(exec).await[0].id;
    f.claim(&f.operator, t).await.unwrap();
    assert_eq!(code_of(f.press(&f.operator, t, "sendBack", None, None).await), (400, "reason_required".into()));
    assert_eq!(code_of(f.press(&f.operator, t, "sendBack", Some("退回"), None).await), (409, "no_previous_node".into()));
    f.press(&f.operator, t, "submit", None, None).await.unwrap();
    let pid = f.task_at(exec, "payment").await.id;
    f.claim(&f.accountant, pid).await.unwrap();
    assert_eq!(code_of(f.press(&f.operator, pid, "markPaid", None, None).await), (403, "not_allowed_to_press".into()));

    let mut d = WorkflowDefinition::new("ss", "狀態旗標", "energyBill", "");
    d.nodes = vec![
        NodeDef::new("start", "起點", NodeType::Start),
        NodeDef::new("check", "確認", NodeType::HumanTask).rule(AssignmentRule::role(&["energy:operator"])).actions(vec![
            ActionDef::new("flagPending", "x", ActionKind::SetState, 0).state("pendingDocuments"),
            ActionDef::new("submit", "x", ActionKind::Advance, 1),
        ]),
        NodeDef::new("end", "終點", NodeType::End),
    ];
    d.transitions = vec![TransitionDef::new("start", "check", 0), TransitionDef::new("check", "end", 0)];
    let e2 = f.start(&d, "BILL-SS").await;
    let tid = f.app.tasks(e2).await[0].id;
    f.claim(&f.operator, tid).await.unwrap();
    f.press(&f.operator, tid, "flagPending", None, None).await.unwrap();
    assert_eq!(f.app.exec_state(e2).await.1.as_deref(), Some("check"), "setState 不移動節點");
    let task = &f.app.tasks(e2).await[0];
    assert_eq!(task.state, "reserved");
    assert_eq!(task.payload["stateFlags"]["pendingDocuments"], true);
    assert!(f.audit_types("energyWorkflowTask", tid).await.contains(&"flow.state_set".to_string()));
}

/// 來源：workflowEngine2c.test.ts「opens ALL matching forward edges, each with its own branchKey」
/// 來源：workflowEngine2c.test.ts「first branch to finish wins; the loser is exited with its own audit row」
#[actix_web::test]
async fn parallel_branches_first_wins() {
    let f = fixture().await;
    let mut d = WorkflowDefinition::new("par", "並行", "energyBill", "");
    let submit = || vec![ActionDef::new("submit", "x", ActionKind::Advance, 0)];
    d.nodes = vec![
        NodeDef::new("start", "起點", NodeType::Start),
        NodeDef::new("split", "分岔", NodeType::HumanTask)
            .parallel(Some(JoinPolicy::First), Some("end"))
            .rule(AssignmentRule::role(&["energy:operator"]))
            .actions(submit()),
        NodeDef::new("branchA", "A 路", NodeType::HumanTask).rule(AssignmentRule::role(&["energy:finance"])).actions(submit()),
        NodeDef::new("branchB", "B 路", NodeType::HumanTask).rule(AssignmentRule::role(&["energy:admin"])).actions(submit()),
        NodeDef::new("end", "終點", NodeType::End),
    ];
    d.transitions = vec![
        TransitionDef::new("start", "split", 0),
        TransitionDef::new("split", "branchA", 0),
        TransitionDef::new("split", "branchB", 1),
        TransitionDef::new("branchA", "end", 0),
        TransitionDef::new("branchB", "end", 0),
    ];
    let exec = f.start(&d, "BILL-PAR").await;
    let split = f.app.tasks(exec).await[0].id;
    f.claim(&f.operator, split).await.unwrap();
    f.press(&f.operator, split, "submit", None, None).await.unwrap();
    let open: Vec<_> = f.app.tasks(exec).await.into_iter().filter(|t| t.state == "ready").collect();
    let mut keys: Vec<&str> = open.iter().map(|t| t.node_key.as_str()).collect();
    keys.sort();
    assert_eq!(keys, ["branchA", "branchB"]);
    assert!(open.iter().all(|t| t.branch_key.is_some()), "並行時每條分支要有 branchKey");
    assert_eq!(f.app.exec_state(exec).await.1, None, "並行時 currentNodeKey 沒有單一答案");

    let a = f.task_at(exec, "branchA").await.id;
    let b = f.task_at(exec, "branchB").await.id;
    f.claim(&f.accountant, a).await.unwrap();
    f.press(&f.accountant, a, "submit", None, None).await.unwrap();
    let ts = f.app.tasks(exec).await;
    let ta = ts.iter().find(|t| t.id == a).unwrap();
    let tb = ts.iter().find(|t| t.id == b).unwrap();
    assert_eq!(ta.state, "completed");
    assert_eq!((tb.state.as_str(), tb.outcome.as_deref()), ("exited", Some("cancelledByJoin")));
    let (p, _, _) = f.audit_latest("flow.branch_cancelled", b).await;
    assert_eq!(p["cause"], "join");
    assert!(p["branchKey"].is_string());
    assert_eq!(f.app.exec_state(exec).await.0, "completed");
}

/// 來源：workflowEngine2c.test.ts「requestAbort needs a reason and produces an out-of-band approval task」
/// 來源：workflowEngine2c.test.ts「only admin/CXO can decide (abortApprovalRule)」
/// 來源：workflowEngine2c.test.ts「rejecting restores running with the main task untouched」
/// 來源：workflowEngine2c.test.ts「approving aborts the execution but does NOT release side effects」
#[actix_web::test]
async fn abort_path_is_independent_of_main_flow() {
    let f = fixture().await;
    let exec = f.start(&payment_flow("ab"), "BILL-AB").await;
    let main = f.app.tasks(exec).await[0].id;
    f.claim(&f.operator, main).await.unwrap();
    let op = f.c(&f.operator).await;
    assert_eq!(code_of(in_tx!(f.app, tx => admin::request_abort(&mut tx, &op, exec, Some("  ")).await)), (400, "reason_required".into()));
    in_tx!(f.app, tx => admin::request_abort(&mut tx, &op, exec, Some("客戶取消委託")).await).unwrap();
    assert_eq!(f.app.exec_state(exec).await.0, "abortPending");
    let approval: (i64, bool) = sqlx::query_as(
        "SELECT id, is_out_of_band FROM tbl_energy_workflow_task WHERE execution_id = $1 AND node_type = 'abortApproval'",
    )
    .bind(exec)
    .fetch_one(&f.app.core.pool)
    .await
    .unwrap();
    assert!(approval.1, "不在主流程路徑上");
    let m = f.app.tasks(exec).await.into_iter().find(|t| t.id == main).unwrap();
    assert_eq!((m.state.as_str(), m.assignee_user_id), ("reserved", Some(f.operator.id)), "主流程不動");

    assert_eq!(code_of(in_tx!(f.app, tx => admin::decide_abort(&mut tx, &op, approval.0, true, None).await)), (403, "not_a_candidate".into()));
    let cxo = f.c(&f.cxo).await;
    in_tx!(f.app, tx => admin::decide_abort(&mut tx, &cxo, approval.0, false, Some("還是要跑完")).await).unwrap();
    assert_eq!(f.app.exec_state(exec).await.0, "running");
    let reason: Option<String> =
        sqlx::query_scalar("SELECT abort_reason FROM tbl_energy_workflow_execution WHERE id = $1").bind(exec).fetch_one(&f.app.core.pool).await.unwrap();
    assert_eq!(reason, None, "駁回後清掉申請");
    assert_eq!(f.app.tasks(exec).await.into_iter().find(|t| t.id == main).unwrap().state, "reserved");
    assert!(f.audit_types("energyWorkflowExecution", exec).await.contains(&"flow.abort_rejected".to_string()));

    f.press(&f.operator, main, "submit", None, None).await.unwrap();
    let pay = f.task_at(exec, "payment").await.id;
    f.claim(&f.accountant, pay).await.unwrap();
    f.press(&f.accountant, pay, "markPaid", None, Some("TX-ABORT")).await.unwrap();
    in_tx!(f.app, tx => admin::request_abort(&mut tx, &op, exec, Some("真的要中斷了")).await).unwrap();
    let pending: i64 = sqlx::query_scalar(
        "SELECT id FROM tbl_energy_workflow_task WHERE execution_id = $1 AND node_type = 'abortApproval' AND state <> 'completed'",
    )
    .bind(exec)
    .fetch_one(&f.app.core.pool)
    .await
    .unwrap();
    let adm = f.c(&f.admin).await;
    in_tx!(f.app, tx => admin::decide_abort(&mut tx, &adm, pending, true, None).await).unwrap();
    assert_eq!(f.app.exec_state(exec).await.0, "aborted");
    let open: i64 = f.app.scalar_i64(&format!("SELECT COUNT(*) FROM tbl_energy_workflow_side_effect WHERE execution_id = {exec} AND released_at IS NULL")).await;
    assert_eq!(open, 1, "中斷不解鎖");
    assert_eq!(f.audit_latest("flow.abort_approved", exec).await.0["openSideEffects"], json!(["payment.sent"]));
}

/// 來源：workflowEngine2c.test.ts「without a delegation the stand-in cannot claim」
/// 來源：workflowEngine2c.test.ts「cannot delegate to yourself」
/// 來源：workflowEngine2c.test.ts「with a delegation the stand-in claims, but the task stays ON BEHALF OF the delegator」
/// 來源：workflowEngine2c.test.ts「the audit row records BOTH the delegate and the person represented」
/// 來源：workflowEngine2c.test.ts「an expired / revoked delegation stops working」
/// 來源：workflowEngine2c.test.ts「reassign MOVES ownership (a different thing from delegation)」
#[actix_web::test]
async fn delegation_is_not_reassignment() {
    let f = fixture().await;
    let exec = f.start(&payment_flow("dg"), "BILL-DG").await;
    let g1 = f.app.tasks(exec).await[0].id;
    f.claim(&f.operator, g1).await.unwrap();
    f.press(&f.operator, g1, "submit", None, None).await.unwrap();
    let pay = f.task_at(exec, "payment").await.id;
    assert_eq!(code_of(f.claim(&f.stand_in, pay).await), (403, "not_a_candidate".into()));

    let holiday = f.c(&f.holiday).await;
    let d = |from: i64, to: i64| NewDelegation {
        delegator_user_id: from,
        delegate_user_id: to,
        valid_from: date("2000-01-01"),
        valid_to: None,
        reason: Some("休假".into()),
    };
    let r = in_tx!(f.app, tx => delegation::create_delegation(&mut tx, &holiday, d(f.holiday.id, f.holiday.id)).await);
    assert_eq!(code_of(r), (400, "cannot_delegate_to_self".into()));
    let row = in_tx!(f.app, tx => delegation::create_delegation(&mut tx, &holiday, d(f.holiday.id, f.stand_in.id)).await).unwrap();

    f.claim(&f.stand_in, pay).await.unwrap();
    let t = f.task_at(exec, "payment").await;
    assert_eq!(t.assignee_user_id, Some(f.stand_in.id), "實際操作的是代理人");
    assert_eq!(t.on_behalf_of_user_id, Some(f.holiday.id), "但任務仍記在本人名下");
    f.press(&f.stand_in, pay, "submit", None, None).await.unwrap();
    let (_, actor, behalf) = f.audit_latest("flow.task_completed", pay).await;
    assert_eq!((actor, behalf), (Some(f.stand_in.id), Some(f.holiday.id)));

    in_tx!(f.app, tx => delegation::revoke_delegation(&mut tx, &holiday, row.id).await).unwrap();
    let mut tx = f.app.core.pool.begin().await.unwrap();
    assert!(delegation::delegators_for(&mut tx, f.org, f.stand_in.id, holiday.today()).await.unwrap().is_empty());
    drop(tx);
    // 過期的也不算（迄日含當天、之後失效）。
    let mut expired = d(f.holiday.id, f.stand_in.id);
    expired.valid_to = Some(date("2001-01-01"));
    in_tx!(f.app, tx => delegation::create_delegation(&mut tx, &holiday, expired).await).unwrap();
    let mut tx = f.app.core.pool.begin().await.unwrap();
    assert!(delegation::delegators_for(&mut tx, f.org, f.stand_in.id, holiday.today()).await.unwrap().is_empty());
    drop(tx);

    let e2 = f.start(&payment_flow("ra"), "BILL-RA").await;
    let t = f.app.tasks(e2).await[0].id;
    let adm = f.c(&f.admin).await;
    assert_eq!(code_of(in_tx!(f.app, tx => admin::reassign(&mut tx, &adm, t, f.stand_in.id, Some("")).await)), (400, "reason_required".into()));
    in_tx!(f.app, tx => admin::reassign(&mut tx, &adm, t, f.stand_in.id, Some("承辦離職")).await).unwrap();
    let after = &f.app.tasks(e2).await[0];
    assert_eq!((after.assignee_user_id, after.on_behalf_of_user_id), (Some(f.stand_in.id), None), "轉移不是代理");
    assert!(f.audit_types("energyWorkflowTask", t).await.contains(&"flow.task_reassigned".to_string()));
}

/// 來源：workflowEngine2c.test.ts「forceAdvance leaves a CRITICAL audit row and cancels the active tasks」
#[actix_web::test]
async fn force_advance_is_a_critical_red_flag() {
    let f = fixture().await;
    let exec = f.start(&payment_flow("fa"), "BILL-FA").await;
    let first = f.app.tasks(exec).await[0].id;
    let adm = f.c(&f.admin).await;
    assert_eq!(
        code_of(in_tx!(f.app, tx => admin::force_advance(&mut tx, &adm, exec, "payment", Some("")).await)),
        (400, "reason_required".into())
    );
    in_tx!(f.app, tx => admin::force_advance(&mut tx, &adm, exec, "payment", Some("客戶催件")).await).unwrap();
    assert_eq!(f.app.exec_state(exec).await.1.as_deref(), Some("payment"));
    assert_eq!(f.audit_latest("flow.force_advanced", exec).await.0["reason"], "客戶催件");
    let t = f.app.tasks(exec).await.into_iter().find(|t| t.id == first).unwrap();
    assert_eq!(t.state, "exited", "原本活著的 task 被取消");
    let sev: String = sqlx::query_scalar("SELECT type FROM tbl_energy_audit_event WHERE type = 'flow.force_advanced' AND target_id = $1")
        .bind(exec.to_string())
        .fetch_one(&f.app.core.pool)
        .await
        .unwrap();
    assert_eq!(eo_domain::audit_events::get_audit_event_def(&sev).unwrap().severity, eo_domain::audit_events::AuditSeverity::Critical);
}

/// 來源：workflowEngine2c.test.ts「links two executions and traces both directions」
/// 來源：workflowEngine2c.test.ts「rejects cycles (the reason is infinite TRACE loops, not cascade recursion)」
/// 來源：workflowEngine2c.test.ts「terminating the upstream does NOT touch the downstream」
/// 來源：workflowEngine2c.test.ts「relationKind is a label — nothing in the engine branches on it」
#[actix_web::test]
async fn lineage_is_provenance_not_control() {
    let f = fixture().await;
    let up = f.start(&payment_flow("ln1"), "BILL-UP").await;
    let down = f.start(&payment_flow("ln2"), "BILL-DOWN").await;
    let adm = f.c(&f.admin).await;
    let link = |u: i64, d: i64, kind: Option<&str>| NewLineage {
        upstream_execution_id: u,
        downstream_execution_id: d,
        relation_kind: kind.map(String::from),
        note: Some("由上游衍生".into()),
    };
    in_tx!(f.app, tx => lineage::link_lineage(&mut tx, &adm, link(up, down, Some("derived"))).await).unwrap();
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let ups: Vec<i64> = lineage::trace_lineage(&mut tx, f.org, down, Direction::Up, 10).await.unwrap().iter().map(|h| h.execution_id).collect();
    let downs: Vec<i64> = lineage::trace_lineage(&mut tx, f.org, up, Direction::Down, 10).await.unwrap().iter().map(|h| h.execution_id).collect();
    drop(tx);
    assert_eq!((ups, downs), (vec![up], vec![down]));
    assert_eq!(code_of(in_tx!(f.app, tx => lineage::link_lineage(&mut tx, &adm, link(down, up, None)).await)), (400, "lineage_cycle".into()));
    assert_eq!(code_of(in_tx!(f.app, tx => lineage::link_lineage(&mut tx, &adm, link(up, up, None)).await)), (400, "lineage_cycle".into()));

    let before: Vec<String> = f.app.tasks(down).await.into_iter().map(|t| t.state).collect();
    in_tx!(f.app, tx => admin::terminate(&mut tx, &adm, up, Some("上游作廢")).await).unwrap();
    assert_eq!(f.app.exec_state(up).await.0, "terminated");
    assert_eq!(f.app.exec_state(down).await.0, "running", "撤回不傳播到下游");
    let after: Vec<String> = f.app.tasks(down).await.into_iter().map(|t| t.state).collect();
    assert_eq!(before, after);

    let rev = f.start(&payment_flow("rv"), "BILL-REV").await;
    in_tx!(f.app, tx => lineage::link_lineage(&mut tx, &adm, link(down, rev, Some("reversal"))).await).unwrap();
    assert_eq!(f.app.exec_state(down).await.0, "running", "標成沖銷也不會讓上游變成什麼");
}
