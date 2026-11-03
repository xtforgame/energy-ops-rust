//! 稽核事件 registry（來源：`common/auditEvents.ts`，以及 `energy/AuditLogger.ts` 的純函式）。
//!
//! 新增事件類型＝在這裡加一筆常數。三個約束讓它不會變成爛帳：
//! 1. type 白名單 —— 未註冊的 type 直接拒絕。
//! 2. payload 有 per-type 白名單 —— 沒宣告的鍵一律丟棄。
//! 3. 查詢維度一律是真欄位 —— payload 只放「點開才看的細節」。
//!
//! 命名文法 `<domain>.<object>.<action>`，動詞用過去式：稽核記的是已發生的事實。
//! zod schema 以小型描述語言（`Ty`／`Field`）重寫，涵蓋原檔用到的所有構件。

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use thiserror::Error;

use crate::js_compat;

/// 呈現分級。不入庫 —— 存進列裡會在調整分級時產生新舊不一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditSeverity {
    Info,
    Notable,
    Critical,
}

/// 數值的額外限制（zod 的 `.int()`／`.nonnegative()`／`.positive()`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumRule {
    Any,
    Int,
    IntNonNegative,
    IntPositive,
}

/// payload 欄位的型別。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ty {
    /// 字串，至少 `min` 個 UTF-16 code unit。
    Str { min: usize },
    Num(NumRule),
    Bool,
    /// 只收 null（`z.null()`）。
    Null,
    Enum(&'static [&'static str]),
    Array(&'static Ty),
    /// `z.record(z.string(), X)`。
    Record(&'static Ty),
    /// 巢狀物件：同樣只留宣告的鍵。
    Object(&'static [Field]),
    /// 依序試，第一個通過的為準。
    Union(&'static [Ty]),
    /// 任何值（含 null）。
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Field {
    pub key: &'static str,
    pub ty: Ty,
    /// 可以不給（zod `.optional()`）。
    pub optional: bool,
    /// 可以給 null（zod `.nullable()`）。
    pub nullable: bool,
}

const fn req(key: &'static str, ty: Ty) -> Field {
    Field { key, ty, optional: false, nullable: false }
}
const fn opt(key: &'static str, ty: Ty) -> Field {
    Field { key, ty, optional: true, nullable: false }
}
const fn nul(key: &'static str, ty: Ty) -> Field {
    Field { key, ty, optional: false, nullable: true }
}
const fn opt_nul(key: &'static str, ty: Ty) -> Field {
    Field { key, ty, optional: true, nullable: true }
}

const STR: Ty = Ty::Str { min: 0 };
const STR1: Ty = Ty::Str { min: 1 };
const NUM: Ty = Ty::Num(NumRule::Any);
const INT: Ty = Ty::Num(NumRule::Int);
const UINT: Ty = Ty::Num(NumRule::IntNonNegative);
const PINT: Ty = Ty::Num(NumRule::IntPositive);
const BOOL: Ty = Ty::Bool;
const STRS: Ty = Ty::Array(&STR);
const INTS: Ty = Ty::Array(&INT);
const NUM_RECORD: Ty = Ty::Record(&NUM);
const MEMBER_ROLE: Ty = Ty::Enum(&["manager", "member"]);

/// 「差異對」：只存真的變了的欄位。機敏欄位不走這裡，降級成 `changedFields`。
pub const CHANGES: Ty = Ty::Record(&Ty::Object(&[opt("from", Ty::Unknown), opt("to", Ty::Unknown)]));

/// 跨門檻修正：存當下算出來的關卡清單 —— 流程定義會換版，事後重算會得到不同答案。
const ROUTE_GATE: &[Field] = &[
    req("field", STR),
    req("before", Ty::Union(&[STR, NUM, Ty::Null])),
    req("after", Ty::Union(&[STR, NUM, Ty::Null])),
    req("gatesBefore", STRS),
    req("gatesAfter", STRS),
    req("gatesDelta", Ty::Object(&[req("added", STRS), req("removed", STRS)])),
    opt("gateNames", Ty::Record(&STR)),
];

/// 效期綁定的三個動作，兩種 kind 同形狀。
const BINDING: &[Field] = &[
    req("kind", Ty::Enum(&["workflow", "rating"])),
    req("stationId", INT),
    opt("stationKey", STR),
    opt("stationName", STR),
    req("targetKey", STR),
    opt("toVersion", INT),
    opt("fromVersion", INT),
    opt("validFrom", STR),
    opt_nul("validTo", STR),
    opt("isBackdated", BOOL),
    opt("affectedDraftBills", INT),
    opt("runningExecutions", INT),
    opt("reason", STR),
];

const DEPARTMENT: &[Field] = &[req("departmentKey", STR), opt("departmentName", STR)];

const DEPARTMENT_MEMBER: &[Field] = &[
    req("departmentKey", STR),
    opt("departmentName", STR),
    req("memberUserId", INT),
    opt("memberLabel", STR),
    req("role", MEMBER_ROLE),
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AuditEventDef {
    /// 事件命名空間，例 `department.created`。
    pub event_type: &'static str,
    /// 目標的 model 名（camelCase）。
    pub target_type: &'static str,
    /// payload 白名單。
    pub payload: &'static [Field],
    /// i18n key。
    pub summary_key: &'static str,
    pub severity: AuditSeverity,
    /// true → fail-closed（稽核寫失敗則整個 transaction rollback）；false → best-effort。
    pub has_side_effect: bool,
}

const fn ev(
    event_type: &'static str,
    target_type: &'static str,
    payload: &'static [Field],
    summary_key: &'static str,
    severity: AuditSeverity,
    has_side_effect: bool,
) -> AuditEventDef {
    AuditEventDef { event_type, target_type, payload, summary_key, severity, has_side_effect }
}

use AuditSeverity::{Critical, Info, Notable};

/// 只宣告真的有人寫的 type —— 宣告了卻沒人寫，看起來有、實際上永遠是空的。
pub static ENERGY_AUDIT_EVENTS: &[AuditEventDef] = &[
    // ── 組織架構 ──
    ev("department.created", "energyDepartment", DEPARTMENT, "audit.type.department.created", Info, true),
    ev(
        "department.updated",
        "energyDepartment",
        &[req("departmentKey", STR), opt("departmentName", STR), opt("changes", CHANGES)],
        "audit.type.department.updated",
        Info,
        true,
    ),
    ev(
        "department.deleted",
        "energyDepartment",
        &[req("departmentKey", STR), opt("departmentName", STR), opt("removedMembers", INT)],
        "audit.type.department.deleted",
        Notable,
        true,
    ),
    ev("department.member_added", "energyDepartment", DEPARTMENT_MEMBER, "audit.type.department.memberAdded", Info, true),
    ev(
        "department.member_removed",
        "energyDepartment",
        &[
            req("departmentKey", STR),
            opt("departmentName", STR),
            req("memberUserId", INT),
            opt("memberLabel", STR),
            opt("role", MEMBER_ROLE),
        ],
        "audit.type.department.memberRemoved",
        Info,
        true,
    ),
    // 主管異動會改變主責者候選池 → notable。
    ev(
        "department.manager_changed",
        "energyDepartment",
        &[
            req("departmentKey", STR),
            opt("departmentName", STR),
            req("memberUserId", INT),
            opt("memberLabel", STR),
            req("role", MEMBER_ROLE),
            req("previousRole", MEMBER_ROLE),
        ],
        "audit.type.department.managerChanged",
        Notable,
        true,
    ),
    // ── 權限：target 是 user，回答「這個人的權限歷程」──
    ev(
        "member.role_changed",
        "user",
        &[
            req("action", Ty::Enum(&["granted", "revoked"])),
            req("roleUniqueName", STR),
            opt("roleName", STR),
            opt("grantId", INT),
            opt("reason", STR),
        ],
        "audit.type.member.roleChanged",
        Critical,
        true,
    ),
    // ── 流程引擎 ──
    ev(
        "flow.started",
        "energyWorkflowExecution",
        &[
            req("workflowKey", STR),
            req("definitionId", INT),
            req("version", INT),
            req("businessDate", STR),
            req("stationKey", STR),
            req("subjectType", STR),
            req("subjectId", STR),
        ],
        "audit.type.flow.started",
        Info,
        true,
    ),
    ev(
        "flow.candidates_resolved",
        "energyWorkflowTask",
        &[
            req("nodeKey", STR),
            req("potentialOwners", INTS),
            req("excludedOwners", INTS),
            req("resolvedBy", STR),
            // 解析出 0 人不是「沒事」，是要有人處理的狀態。
            opt("empty", BOOL),
        ],
        "audit.type.flow.candidatesResolved",
        Info,
        false,
    ),
    ev("flow.task_claimed", "energyWorkflowTask", &[req("nodeKey", STR)], "audit.type.flow.taskClaimed", Info, false),
    ev("flow.task_released", "energyWorkflowTask", &[req("nodeKey", STR)], "audit.type.flow.taskReleased", Info, false),
    ev(
        "flow.task_completed",
        "energyWorkflowTask",
        &[req("nodeKey", STR), req("outcome", STR), opt_nul("nextNodeKey", STR), opt("reason", STR)],
        "audit.type.flow.taskCompleted",
        Info,
        true,
    ),
    ev("flow.completed", "energyWorkflowExecution", &[req("endNodeKey", STR)], "audit.type.flow.completed", Info, true),
    // 回退／駁回沒有理由等於沒有稽核價值 → reason 必填。
    ev(
        "flow.task_sent_back",
        "energyWorkflowTask",
        &[
            req("nodeKey", STR),
            req("targetNodeKey", STR),
            req("targetMode", STR),
            req("actionKey", STR),
            req("reason", STR1),
        ],
        "audit.type.flow.taskSentBack",
        Notable,
        true,
    ),
    // 每條被取消的分支各一筆，不可只記一筆總結。
    ev(
        "flow.branch_cancelled",
        "energyWorkflowTask",
        &[
            req("nodeKey", STR),
            opt_nul("branchKey", STR),
            req("stateWhenCancelled", STR),
            req("cause", Ty::Enum(&["revert", "join", "abort", "terminate"])),
        ],
        "audit.type.flow.branchCancelled",
        Notable,
        true,
    ),
    ev(
        "flow.task_reassigned",
        "energyWorkflowTask",
        &[req("nodeKey", STR), nul("fromUserId", INT), req("toUserId", INT), req("reason", STR1)],
        "audit.type.flow.taskReassigned",
        Notable,
        true,
    ),
    // 改狀態不移動節點。
    ev(
        "flow.state_set",
        "energyWorkflowTask",
        &[req("nodeKey", STR), req("actionKey", STR), req("stateKey", STR), opt("value", Ty::Unknown)],
        "audit.type.flow.stateSet",
        Info,
        false,
    ),
    // 「已付款」按下去的那一筆；note 是人輸入的匯款單號，系統無從驗證。
    ev(
        "flow.side_effect_applied",
        "energyWorkflowSideEffect",
        &[
            req("sideEffectKey", STR),
            req("executionId", INT),
            req("nodeKey", STR),
            req("locksRevert", BOOL),
            opt("note", STR),
        ],
        "audit.type.flow.sideEffectApplied",
        Notable,
        true,
    ),
    // 管理員解除回退鎖 —— 全系統最需要被看見的一筆。
    ev(
        "flow.side_effect_released",
        "energyWorkflowSideEffect",
        &[req("sideEffectKey", STR), req("executionId", INT), req("reason", STR1)],
        "audit.type.flow.sideEffectReleased",
        Critical,
        true,
    ),
    ev(
        "flow.force_advanced",
        "energyWorkflowExecution",
        &[nul("fromNodeKey", STR), req("toNodeKey", STR), req("reason", STR1)],
        "audit.type.flow.forceAdvanced",
        Critical,
        true,
    ),
    ev("flow.terminated", "energyWorkflowExecution", &[req("reason", STR1)], "audit.type.flow.terminated", Notable, true),
    ev(
        "flow.abort_requested",
        "energyWorkflowExecution",
        &[req("reason", STR1), req("approvalTaskId", INT)],
        "audit.type.flow.abortRequested",
        Notable,
        true,
    ),
    // 中斷不解除副作用：已付款後中斷仍然中斷，但那筆副作用留在清單裡。
    ev(
        "flow.abort_approved",
        "energyWorkflowExecution",
        &[opt("reason", STR), req("requestReason", STR), opt("openSideEffects", STRS)],
        "audit.type.flow.abortApproved",
        Critical,
        true,
    ),
    ev(
        "flow.abort_rejected",
        "energyWorkflowExecution",
        &[opt("reason", STR), req("requestReason", STR)],
        "audit.type.flow.abortRejected",
        Notable,
        true,
    ),
    ev(
        "flow.rerun",
        "energyWorkflowExecution",
        &[req("supersedesExecutionId", INT), req("reason", STR1)],
        "audit.type.flow.rerun",
        Notable,
        true,
    ),
    ev(
        "delegation.created",
        "energyWorkflowDelegation",
        &[
            req("delegatorUserId", INT),
            req("delegateUserId", INT),
            req("validFrom", STR),
            opt_nul("validTo", STR),
            opt("reason", STR),
        ],
        "audit.type.delegation.created",
        Notable,
        true,
    ),
    ev(
        "delegation.revoked",
        "energyWorkflowDelegation",
        &[req("delegatorUserId", INT), req("delegateUserId", INT)],
        "audit.type.delegation.revoked",
        Notable,
        true,
    ),
    // 純紀錄，不觸發行為。
    ev(
        "lineage.linked",
        "energyExecutionLineage",
        &[
            req("upstreamExecutionId", INT),
            req("downstreamExecutionId", INT),
            req("relationKind", STR),
            opt("note", STR),
        ],
        "audit.type.lineage.linked",
        Info,
        false,
    ),
    // ── 流程綁定的三個具名動作（不做 CRUD）──
    ev("flow.binding_created", "energyWorkflowBinding", BINDING, "audit.type.flow.bindingCreated", Info, true),
    ev("flow.binding_switched", "energyWorkflowBinding", BINDING, "audit.type.flow.bindingSwitched", Notable, true),
    // critical —— 解除之後這個案場收不了單。
    ev("flow.binding_ended", "energyWorkflowBinding", BINDING, "audit.type.flow.bindingEnded", Critical, true),
    // ── 附件 ──
    // 兩種歸屬二擇一：流程中掛的有 executionId；收件時的原始掃描檔只有 ingestBatchId。
    ev(
        "attachment.uploaded",
        "energyAttachment",
        &[
            opt_nul("executionId", INT),
            opt_nul("ingestBatchId", INT),
            opt_nul("taskId", INT),
            req("role", STR),
            req("originalFilename", STR),
            req("size", INT),
            req("hash", STR),
            req("mimeType", STR),
        ],
        "audit.type.attachment.uploaded",
        Info,
        true,
    ),
    // 補件：流程中才補上的，通常是有人要求的結果；稽核頁按 type 篩，所以分開記。
    ev(
        "flow.attachment_added",
        "energyAttachment",
        &[
            req("executionId", INT),
            opt_nul("taskId", INT),
            req("role", STR),
            req("originalFilename", STR),
            req("size", INT),
        ],
        "audit.type.flow.attachment_added",
        Info,
        true,
    ),
    // 唯一記錄「讀取」的事件：下載一份合約 ≈ 匯出。
    ev(
        "attachment.downloaded",
        "energyAttachment",
        &[req("executionId", INT)],
        "audit.type.attachment.downloaded",
        Info,
        false,
    ),
    ev(
        "attachment.superseded",
        "energyAttachment",
        &[req("replacedByAttachmentId", INT)],
        "audit.type.attachment.superseded",
        Notable,
        true,
    ),
    // 不是刪檔 —— 檔案與 attachment 列都留著。
    ev(
        "attachment.link_removed",
        "energyAttachmentLink",
        &[req("attachmentId", INT), opt_nul("taskId", INT), req("role", STR), req("reason", STR1)],
        "audit.type.attachment.linkRemoved",
        Notable,
        true,
    ),
    // ── 電費單 ──
    // 辨識判不出來改由人工建單：事後看到沒有辨識值的單，第一個問題是「它哪來的」。
    ev(
        "bill.created_manually",
        "energyBill",
        &[req("itemId", INT), req("docType", STR)],
        "audit.type.bill.created_manually",
        Notable,
        true,
    ),
    ev(
        "bill.matched",
        "energyBill",
        &[
            opt_nul("customerNumberRaw", STR),
            opt_nul("stationId", INT),
            opt_nul("previousStationId", INT),
            opt("extractedFields", STRS),
            opt_nul("sourceAttachmentId", INT),
            // true ＝ 人工指認。
            opt("manual", BOOL),
        ],
        "audit.type.bill.matched",
        Info,
        true,
    ),
    // 不是錯誤，是要有人處理的狀態（例外件）。
    ev(
        "bill.unmatched",
        "energyBill",
        &[
            opt_nul("customerNumberRaw", STR),
            opt("stationId", Ty::Null),
            opt("extractedFields", STRS),
            opt_nul("sourceAttachmentId", INT),
        ],
        "audit.type.bill.unmatched",
        Notable,
        true,
    ),
    ev(
        "bill.corrected",
        "energyBill",
        &[req("changes", CHANGES), opt("markReviewed", BOOL), opt("ocrExtracted", Ty::Record(&Ty::Unknown))],
        "audit.type.bill.corrected",
        Info,
        true,
    ),
    // 人工修正改變了審批路線。危險的方向是「少一關」：那是規避大額控管最省事的繞法。
    ev("bill.route_gate_added", "energyBill", ROUTE_GATE, "audit.type.bill.route_gate_added", Notable, true),
    ev("bill.route_gate_removed", "energyBill", ROUTE_GATE, "audit.type.bill.route_gate_removed", Critical, true),
    // ── 送件批次：沒產出單的檔案在 bills 表裡查不到，只有這一筆答得完整 ──
    ev(
        "ingest.batch_classified",
        "energyIngestBatch",
        &[req("fileCount", UINT), opt("byStatus", NUM_RECORD)],
        "audit.type.ingest.batchClassified",
        Info,
        true,
    ),
    // ── 台電辨識服務 ──
    ev(
        "ingest.recognition_requested",
        "energyIngestBatch",
        &[req("jobId", STR1), req("fileCount", UINT), opt("withPassword", UINT)],
        "audit.type.ingest.recognitionRequested",
        Info,
        true,
    ),
    ev(
        "ingest.recognition_completed",
        "energyIngestBatch",
        &[
            req("jobId", STR1),
            req("documents", UINT),
            req("failed", UINT),
            opt("byTemplate", NUM_RECORD),
            opt("blockingIssues", UINT),
            opt("elapsedMs", UINT),
        ],
        "audit.type.ingest.recognitionCompleted",
        Info,
        true,
    ),
    // 外部管線直接灌一份辨識結果：信任來源不同，與 recognition_completed 分開記。
    ev(
        "ingest.recognition_supplied",
        "energyIngestItem",
        &[
            req("itemId", PINT),
            req("batchId", PINT),
            req("docType", STR1),
            opt_nul("billId", PINT),
            req("status", STR1),
        ],
        "audit.type.ingest.recognitionSupplied",
        Info,
        true,
    ),
    // notable：檔案還在，但沒有人會知道它卡住了，除非這一筆被看見。
    ev(
        "ingest.recognition_failed",
        "energyIngestBatch",
        &[opt("jobId", STR), req("code", STR1), opt("message", STR), opt("itemCount", UINT)],
        "audit.type.ingest.recognitionFailed",
        Notable,
        true,
    ),
    // 服務那側的第二份原始檔已刪除 ——「刪掉了沒」必須是查得到的事實。
    ev(
        "ingest.recognition_purged",
        "energyIngestBatch",
        &[req("jobId", STR1), req("ok", BOOL)],
        "audit.type.ingest.recognitionPurged",
        Info,
        false,
    ),
    // ── 案場座標：要記從哪來，換 geocoder 時才知道哪些該重跑 ──
    ev(
        "station.geocoded",
        "energyStation",
        &[
            req("changes", CHANGES),
            opt("address", STR),
            opt("precision", STR),
            opt("source", STR),
            opt("matchedAddress", STR),
        ],
        "audit.type.station.geocoded",
        Info,
        true,
    ),
    // ── 計算策略與應付 ──
    ev(
        "payout.calculated",
        "energyPayoutRecord",
        &[
            req("billId", INT),
            // 參數可能來自契約（沒有策略版本可引）→ 可空；paramsSource 說得出是哪一種。
            nul("strategyId", INT),
            nul("version", INT),
            opt_nul("contractId", INT),
            opt("paramsSource", Ty::Enum(&["contract", "stationStrategy"])),
            req("strategyType", STR),
            // 金額用字串 —— JSON number 會把 decimal 變成浮點。
            req("amount", STR),
            req("trigger", Ty::Enum(&["workflowNode", "manualRecompute", "correction"])),
            opt_nul("supersedesRecordId", INT),
        ],
        "audit.type.payout.calculated",
        Info,
        true,
    ),
    // 算不出來也要留痕 —— 否則「為什麼這張單沒有應付金額」永遠查不到。
    ev(
        "payout.calculation_failed",
        "energyBill",
        &[req("reason", STR), opt("details", Ty::Unknown), req("trigger", STR)],
        "audit.type.payout.calculationFailed",
        Notable,
        false,
    ),
    ev(
        "payout.superseded",
        "energyPayoutRecord",
        &[req("replacedByRecordId", INT)],
        "audit.type.payout.superseded",
        Notable,
        true,
    ),
    // 新版本上線＝影響往後所有計算。
    ev(
        "strategy.published",
        "energyRatingStrategy",
        &[req("key", STR), req("version", INT), req("strategyType", STR)],
        "audit.type.strategy.published",
        Notable,
        true,
    ),
    ev("strategy.binding_created", "energyRatingStrategyBinding", BINDING, "audit.type.strategy.bindingCreated", Info, true),
    ev(
        "strategy.binding_switched",
        "energyRatingStrategyBinding",
        BINDING,
        "audit.type.strategy.bindingSwitched",
        Notable,
        true,
    ),
    // critical —— 解除之後這個案場算不出錢。
    ev(
        "strategy.binding_ended",
        "energyRatingStrategyBinding",
        BINDING,
        "audit.type.strategy.bindingEnded",
        Critical,
        true,
    ),
];

/// 取事件定義；找不到回 None（寫入端據此拒絕未註冊的 type）。
pub fn get_audit_event_def(event_type: &str) -> Option<&'static AuditEventDef> {
    ENERGY_AUDIT_EVENTS.iter().find(|d| d.event_type == event_type)
}

pub fn is_known_audit_event_type(event_type: &str) -> bool {
    get_audit_event_def(event_type).is_some()
}

pub fn audit_event_types() -> Vec<&'static str> {
    ENERGY_AUDIT_EVENTS.iter().map(|d| d.event_type).collect()
}

/// 這個 type 是不是 fail-closed。不認得的 type 當 fail-closed（稽核的預設方向）。
pub fn is_fail_closed(event_type: &str) -> bool {
    get_audit_event_def(event_type).map(|d| d.has_side_effect).unwrap_or(true)
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum AuditError {
    #[error("Unknown audit event type \"{0}\" — declare it in audit_events first")]
    UnknownType(String),
    #[error("Audit payload for \"{event_type}\" failed validation: {issues:?}")]
    InvalidPayload { event_type: String, issues: Vec<String> },
}

fn check(ty: &Ty, v: &Value, path: &str, issues: &mut Vec<String>) -> Option<Value> {
    let fail = |issues: &mut Vec<String>, what: &str| {
        issues.push(format!("{path}: {what}"));
        None
    };
    match ty {
        Ty::Unknown => Some(v.clone()),
        Ty::Str { min } => match v {
            Value::String(s) if js_compat::utf16_len(s) >= *min => Some(v.clone()),
            Value::String(_) => fail(issues, &format!("至少 {min} 個字")),
            _ => fail(issues, "須為字串"),
        },
        Ty::Num(rule) => {
            let Some(n) = v.as_f64() else {
                return fail(issues, "須為數字");
            };
            let ok = match rule {
                NumRule::Any => true,
                NumRule::Int => js_compat::is_integer(n),
                NumRule::IntNonNegative => js_compat::is_integer(n) && n >= 0.0,
                NumRule::IntPositive => js_compat::is_integer(n) && n > 0.0,
            };
            if ok {
                Some(v.clone())
            } else {
                fail(issues, "數值不合規則")
            }
        }
        Ty::Bool => match v {
            Value::Bool(_) => Some(v.clone()),
            _ => fail(issues, "須為布林"),
        },
        Ty::Null => match v {
            Value::Null => Some(Value::Null),
            _ => fail(issues, "須為 null"),
        },
        Ty::Enum(options) => match v {
            Value::String(s) if options.contains(&s.as_str()) => Some(v.clone()),
            _ => fail(issues, &format!("須為 {options:?} 之一")),
        },
        Ty::Array(inner) => {
            let Value::Array(items) = v else {
                return fail(issues, "須為陣列");
            };
            let before = issues.len();
            let out: Vec<Value> = items
                .iter()
                .enumerate()
                .filter_map(|(i, x)| check(inner, x, &format!("{path}[{i}]"), issues))
                .collect();
            (issues.len() == before).then_some(Value::Array(out))
        }
        Ty::Record(inner) => {
            let Value::Object(m) = v else {
                return fail(issues, "須為物件");
            };
            let before = issues.len();
            let mut out = Map::new();
            for (k, x) in m {
                if let Some(c) = check(inner, x, &format!("{path}.{k}"), issues) {
                    out.insert(k.clone(), c);
                }
            }
            (issues.len() == before).then_some(Value::Object(out))
        }
        Ty::Object(fields) => {
            let Value::Object(m) = v else {
                return fail(issues, "須為物件");
            };
            strip_object(fields, m, path, issues).map(Value::Object)
        }
        Ty::Union(options) => {
            for o in options.iter() {
                let mut scratch = Vec::new();
                if let Some(ok) = check(o, v, path, &mut scratch) {
                    return Some(ok);
                }
            }
            fail(issues, "不符合任何允許的型別")
        }
    }
}

fn strip_object(fields: &[Field], m: &Map<String, Value>, path: &str, issues: &mut Vec<String>) -> Option<Map<String, Value>> {
    let before = issues.len();
    let mut out = Map::new();
    for f in fields {
        let p = if path.is_empty() { f.key.to_string() } else { format!("{path}.{}", f.key) };
        match m.get(f.key) {
            // `unknown` 本身就收 undefined，所以「必填的 unknown」也可以不給。
            None if f.optional || matches!(f.ty, Ty::Unknown) => {}
            None => issues.push(format!("{p}: 必填")),
            Some(Value::Null) if f.nullable => {
                out.insert(f.key.into(), Value::Null);
            }
            Some(v) => {
                if let Some(c) = check(&f.ty, v, &p, issues) {
                    out.insert(f.key.into(), c);
                }
            }
        }
    }
    (issues.len() == before).then_some(out)
}

/// 驗證並清洗 payload：未註冊的 type 拒絕、形狀不對拒絕、沒宣告的鍵（含巢狀物件裡的）丟棄。
/// `null` 視同 `{}`。
pub fn sanitize_payload(event_type: &str, payload: &Value) -> Result<Value, AuditError> {
    let def = get_audit_event_def(event_type).ok_or_else(|| AuditError::UnknownType(event_type.into()))?;
    let empty = Map::new();
    let obj = match payload {
        Value::Null => &empty,
        Value::Object(m) => m,
        _ => {
            return Err(AuditError::InvalidPayload { event_type: event_type.into(), issues: vec!["payload 須為物件".into()] });
        }
    };
    let mut issues = Vec::new();
    match strip_object(def.payload, obj, "", &mut issues) {
        Some(out) if issues.is_empty() => Ok(Value::Object(out)),
        _ => Err(AuditError::InvalidPayload { event_type: event_type.into(), issues }),
    }
}

/// payload 上限 32KB，超過就換成固定形狀的標記。
pub const PAYLOAD_MAX_BYTES: usize = 32 * 1024;

/// 截斷過大的 payload。不做「聰明的部分保留」—— 那會讓 payload 的形狀不可預期；
/// 直接換成標記 ＋ 前 1KB（UTF-16 code unit）的字串樣本。
pub fn truncate_payload(payload: Value) -> Value {
    let payload = if payload.is_null() { Value::Object(Map::new()) } else { payload };
    let serialized = serde_json::to_string(&payload).unwrap_or_default();
    if serialized.len() <= PAYLOAD_MAX_BYTES {
        return payload;
    }
    json!({
        "_truncated": true,
        "_originalBytes": serialized.len(),
        "_sample": js_compat::utf16_prefix(&serialized, 1024),
    })
}

/// 差異對的一邊：None ＝ 該側沒有這個鍵（JS 的 undefined，序列化時省略）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffResult {
    pub changes: Map<String, Value>,
    pub changed_fields: Vec<String>,
}

/// JSON 意義上的相等（數字依數值比，`1` 與 `1.0` 相同）。
fn json_eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        (Some(Value::Array(x)), Some(Value::Array(y))) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| json_eq(Some(p), Some(q)))
        }
        (Some(Value::Object(x)), Some(Value::Object(y))) => {
            x.len() == y.len() && x.iter().all(|(k, v)| json_eq(Some(v), y.get(k)))
        }
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// 從 before/after 算差異對：只回真的變了的欄位；機敏欄位只記「動過」，不記前後值
/// （稽核表不該變成機敏資料的第二個副本）。
pub fn diff_changes(before: Option<&Map<String, Value>>, after: Option<&Map<String, Value>>, sensitive_fields: &[&str]) -> DiffResult {
    let empty = Map::new();
    let b = before.unwrap_or(&empty);
    let a = after.unwrap_or(&empty);
    let mut keys: Vec<&String> = b.keys().collect();
    keys.extend(a.keys().filter(|k| !b.contains_key(*k)));

    let mut out = DiffResult::default();
    for k in keys {
        if json_eq(b.get(k), a.get(k)) {
            continue;
        }
        if sensitive_fields.contains(&k.as_str()) {
            out.changed_fields.push(k.clone());
        } else {
            let change = Change { from: b.get(k).cloned(), to: a.get(k).cloned() };
            out.changes.insert(k.clone(), serde_json::to_value(change).unwrap_or(Value::Null));
        }
    }
    out
}
