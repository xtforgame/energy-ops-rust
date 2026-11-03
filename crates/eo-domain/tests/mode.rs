//! 移植自 `tests/mode.test.ts` 的純函式部分（`resolveMode` 的順序與兩個不對稱）。
//! 端到端那幾條（建單、送審、PATCH 403）要 DB 與引擎，屬於 eo-energy。

use chrono::{TimeZone, Utc};
use eo_domain::resolve_mode::*;
use serde_json::json;

fn running() -> Option<ExecutionState> {
    Some(ExecutionState { status: "running".into(), ended_at: None })
}

fn task(assignee: Option<i64>, node_name: &str, allows_subject_edit: bool) -> Option<ActiveTaskState> {
    Some(ActiveTaskState {
        state: "ready".into(),
        assignee_user_id: assignee,
        node_name: Some(node_name.into()),
        allows_subject_edit,
    })
}

fn code(v: &ModeVerdict) -> Option<ModeReasonCode> {
    v.mode_reason.as_ref().map(|r| r.code)
}

/// 來源：mode.test.ts「第 3 步排在 4/5 之前：**已完成的單，就算停在可編輯的節點也是 readonly**」
#[test]
fn finished_beats_editable_node() {
    let finished = resolve_mode(&ResolveModeInput {
        has_access: true,
        execution: Some(ExecutionState {
            status: "completed".into(),
            ended_at: Some(EndedAt::Text("2026-03-30T00:00:00Z".into())),
        }),
        // 故意給「可編輯 ＋ 我是候選人」的節點 —— 順序對的話它不該生效。
        active_task: task(None, "承辦確認", true),
        is_candidate: true,
        me: 1,
        ..Default::default()
    });
    assert_eq!(finished.mode, BillMode::Readonly);
    assert_eq!(finished.mode_reason.as_ref().unwrap().at(), Some("2026-03-30"), "日期要是 YYYY-MM-DD");
    assert!(!finished.can_edit_subject);
    assert_eq!(code(&finished), Some(ModeReasonCode::Finished));

    // 時間物件也要格式化（那才是 DB 真的回的東西）。
    let with_time = resolve_mode(&ResolveModeInput {
        has_access: true,
        execution: Some(ExecutionState {
            status: "completed".into(),
            ended_at: Some(EndedAt::Time(Utc.with_ymd_and_hms(2026, 3, 30, 12, 0, 0).unwrap())),
        }),
        me: 1,
        ..Default::default()
    });
    assert_eq!(with_time.mode_reason.as_ref().unwrap().at(), Some("2026-03-30"));

    // 中斷的單同理。
    let aborted = resolve_mode(&ResolveModeInput {
        has_access: true,
        execution: Some(ExecutionState { status: "aborted".into(), ended_at: None }),
        active_task: task(None, "x", true),
        is_candidate: true,
        me: 1,
        ..Default::default()
    });
    assert_eq!(aborted.mode, BillMode::Readonly);
    assert_eq!(code(&aborted), Some(ModeReasonCode::Aborted));
    // `at` 鍵仍在，值是 null（與 TS 的物件形狀一致）。
    assert_eq!(serde_json::to_value(&aborted.mode_reason).unwrap(), json!({ "code": "aborted", "at": null }));
}

/// 來源：mode.test.ts「唯讀身分：`viewOnly` 而不是 `notMyTurn`（後者暗示等一下會輪到他）」
#[test]
fn view_only_instead_of_not_my_turn() {
    let shape = ResolveModeInput {
        has_access: true,
        execution: running(),
        active_task: task(None, "主管核准", false),
        assignee_hint: Some("業務部主管".into()),
        me: 1,
        ..Default::default()
    };
    let with = |can_act: Option<bool>| ResolveModeInput { can_act, ..shape.clone() };
    assert_eq!(code(&resolve_mode(&with(Some(false)))), Some(ModeReasonCode::ViewOnly));
    // 沒給 canAct（舊呼叫端）與能動手的人，行為都不變。
    assert_eq!(code(&resolve_mode(&with(None))), Some(ModeReasonCode::NotMyTurn));
    assert_eq!(code(&resolve_mode(&with(Some(true)))), Some(ModeReasonCode::NotMyTurn));

    // 已完成的單對唯讀身分也要說「已完成」。
    let done = resolve_mode(&ResolveModeInput {
        execution: Some(ExecutionState {
            status: "completed".into(),
            ended_at: Some(EndedAt::Text("2026-03-30T00:00:00Z".into())),
        }),
        ..with(Some(false))
    });
    assert_eq!(code(&done), Some(ModeReasonCode::Finished));
    // 防自核比「你是唯讀」精確。
    let excluded = resolve_mode(&ResolveModeInput { is_excluded: true, ..with(Some(false)) });
    assert_eq!(code(&excluded), Some(ModeReasonCode::ExcludedOwner));

    // 說明句帶得出節點名與處理人。
    let v = resolve_mode(&with(None));
    let reason = v.mode_reason.unwrap();
    assert_eq!(reason.node_name(), Some("主管核准"));
    assert_eq!(reason.assignee_hint, Some(Some("業務部主管".into())));
}

/// 來源：mode.test.ts「canAddAttachment 比 canEditSubject **寬**：act 不能改欄位但可以補件」
#[test]
fn attachment_is_wider_than_edit() {
    let act = resolve_mode(&ResolveModeInput {
        has_access: true,
        execution: running(),
        active_task: task(None, "主管核准", false),
        is_candidate: true,
        me: 1,
        ..Default::default()
    });
    assert_eq!(act.mode, BillMode::Act);
    assert!(!act.can_edit_subject, "act 不能改欄位");
    assert!(act.can_add_attachment, "act 可以補件");

    // readonly 與 denied 都不能補件。
    let ro = resolve_mode(&ResolveModeInput {
        has_access: true,
        execution: running(),
        active_task: task(Some(999), "x", false),
        me: 1,
        ..Default::default()
    });
    assert_eq!(ro.mode, BillMode::Readonly);
    assert_eq!(code(&ro), Some(ModeReasonCode::ClaimedByOther));
    assert!(!ro.can_add_attachment);

    let denied = resolve_mode(&ResolveModeInput { has_access: false, me: 1, ..Default::default() });
    assert_eq!(denied.mode, BillMode::Denied);
    assert!(!denied.can_add_attachment);
}

/// 來源：mode.test.ts「尚未送審 + **我是送件者** → intake；別人 → readonly」（判定那一半）
#[test]
fn intake_before_submission() {
    let mine = resolve_mode(&ResolveModeInput { has_access: true, can_intake: true, me: 1, ..Default::default() });
    assert_eq!(mine.mode, BillMode::Intake);
    assert!(mine.can_edit_subject && mine.can_add_attachment);
    let other = resolve_mode(&ResolveModeInput { has_access: true, me: 2, ..Default::default() });
    assert_eq!(other.mode, BillMode::Readonly);
    assert_eq!(code(&other), Some(ModeReasonCode::NotSubmitted));
}

/// 來源：mode.test.ts「停在 allowsSubjectEdit 的節點且我是候選人 → **revise**（可改）」（判定那一半）
#[test]
fn revise_on_editable_node() {
    let v = resolve_mode(&ResolveModeInput {
        has_access: true,
        execution: running(),
        active_task: task(Some(1), "承辦確認", true),
        me: 1,
        ..Default::default()
    });
    assert_eq!(v.mode, BillMode::Revise);
    assert!(v.can_edit_subject);
    assert_eq!(v.mode_reason, None);
}

/// 來源：mode.test.ts「看得到但沒事做的人 → **readonly**（不是 denied）」（判定那一半）
#[test]
fn bystander_is_readonly_not_denied() {
    let v = resolve_mode(&ResolveModeInput {
        has_access: true,
        execution: running(),
        active_task: None,
        me: 3,
        ..Default::default()
    });
    assert_eq!(v.mode, BillMode::Readonly);
    assert_eq!(code(&v), Some(ModeReasonCode::NotMyTurn));
    assert_eq!(v.mode_reason.unwrap().node_name, Some(None));
}
