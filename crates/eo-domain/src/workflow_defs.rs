//! 流程定義的資料型別與內建的五份 seed 定義（來源：`energy/workflow/seedDefinitions.ts`）。
//!
//! 定義即資料：客戶給出實際階段後改這裡的常數、出新版本即可，引擎一行不動。
//! 這裡的型別是**套用過 seed 預設值之後**的形狀（`seedWorkflowDefinition` 寫進 DB 的那一份），
//! 引擎讀 DB 列或讀這裡的常數，看到的是同一件事。

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::conditions::{Condition, ConditionOp, ConditionalEdge};
use crate::document_ingest::EnergyDocType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeType {
    Start,
    HumanTask,
    Calculation,
    AbortApproval,
    End,
}

/// 動作的五種 kind。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionKind {
    Advance,
    Revert,
    SetState,
    SideEffect,
    Abort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TargetMode {
    Transition,
    PreviousNode,
    ExplicitNode,
    CancelExecution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransitionKind {
    #[default]
    Forward,
    SendBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JoinPolicy {
    First,
    All,
    Quorum,
}

/// 主責者解析不到部門主管時怎麼辦。`skipNode` 本版不實作（跳關＝無人核准就放行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagerFallback {
    Role,
    Error,
    SkipNode,
    Other(String),
}

impl ManagerFallback {
    pub fn as_str(&self) -> &str {
        match self {
            ManagerFallback::Role => "role",
            ManagerFallback::Error => "error",
            ManagerFallback::SkipNode => "skipNode",
            ManagerFallback::Other(s) => s,
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "role" => ManagerFallback::Role,
            "error" => ManagerFallback::Error,
            "skipNode" => ManagerFallback::SkipNode,
            other => ManagerFallback::Other(other.into()),
        }
    }
}

/// 指派規則（節點的 `assignmentRule`、動作的 `who`、定義的 `abortApprovalRule` 共用）。
///
/// JSON 形狀 `{ "type": ..., "params": {...} }`；`{}` ＝ 沒有規則。
/// 未給的參數保留成 None，讓引擎套自己的預設（例如 R1 fallback 預設升到 `energy:cxo`）。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "Value", into = "Value")]
pub enum AssignmentRule {
    /// 沒有規則（`{}` 或 `type: 'none'`）。
    #[default]
    None,
    /// R3：指定角色（陣列 —— 中斷核可要一次收 admin＋CXO）。
    Role { role_unique_names: Option<Vec<String>> },
    /// R2：指定部門；`scope` 是 `managers` 或 `all`（引擎預設 `all`）。
    Department { department_key: Option<String>, scope: Option<String> },
    /// R1：發起人的部門主管，含 fallback。
    DepartmentManagerOfInitiator { fallback: Option<ManagerFallback>, role_unique_names: Option<Vec<String>> },
    /// 引擎不認得的 type —— 原樣保留，解析時報 `unsupported_assignment_rule`。
    Unsupported { rule_type: String, raw: Value },
}

impl AssignmentRule {
    pub fn role(names: &[&str]) -> Self {
        AssignmentRule::Role { role_unique_names: Some(names.iter().map(|s| s.to_string()).collect()) }
    }

    pub fn department(key: &str, scope: &str) -> Self {
        AssignmentRule::Department { department_key: Some(key.into()), scope: Some(scope.into()) }
    }

    pub fn manager_of_initiator(fallback: ManagerFallback, names: &[&str]) -> Self {
        AssignmentRule::DepartmentManagerOfInitiator {
            fallback: Some(fallback),
            role_unique_names: Some(names.iter().map(|s| s.to_string()).collect()),
        }
    }

    /// 規則的 type 名（`none` 表示沒有規則）。
    pub fn rule_type(&self) -> &str {
        match self {
            AssignmentRule::None => "none",
            AssignmentRule::Role { .. } => "role",
            AssignmentRule::Department { .. } => "department",
            AssignmentRule::DepartmentManagerOfInitiator { .. } => "departmentManagerOfInitiator",
            AssignmentRule::Unsupported { rule_type, .. } => rule_type,
        }
    }
}

fn string_list(v: Option<&Value>) -> Option<Vec<String>> {
    v?.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
}

impl From<Value> for AssignmentRule {
    fn from(v: Value) -> Self {
        let rule_type = v.get("type").and_then(Value::as_str).filter(|s| !s.is_empty());
        let params = v.get("params").cloned().unwrap_or(Value::Null);
        let p = |k: &str| params.get(k);
        match rule_type {
            None | Some("none") => AssignmentRule::None,
            Some("role") => AssignmentRule::Role { role_unique_names: string_list(p("roleUniqueNames")) },
            Some("department") => AssignmentRule::Department {
                department_key: p("departmentKey").and_then(Value::as_str).map(str::to_string),
                scope: p("scope").and_then(Value::as_str).map(str::to_string),
            },
            Some("departmentManagerOfInitiator") => AssignmentRule::DepartmentManagerOfInitiator {
                fallback: p("fallback").and_then(Value::as_str).map(ManagerFallback::parse),
                role_unique_names: string_list(p("roleUniqueNames")),
            },
            Some(other) => AssignmentRule::Unsupported { rule_type: other.into(), raw: v.clone() },
        }
    }
}

impl From<AssignmentRule> for Value {
    fn from(r: AssignmentRule) -> Value {
        let with = |t: &str, params: Map<String, Value>| json!({ "type": t, "params": params });
        let mut params = Map::new();
        match r {
            AssignmentRule::None => Value::Object(Map::new()),
            AssignmentRule::Role { role_unique_names } => {
                if let Some(n) = role_unique_names {
                    params.insert("roleUniqueNames".into(), json!(n));
                }
                with("role", params)
            }
            AssignmentRule::Department { department_key, scope } => {
                if let Some(k) = department_key {
                    params.insert("departmentKey".into(), json!(k));
                }
                if let Some(s) = scope {
                    params.insert("scope".into(), json!(s));
                }
                with("department", params)
            }
            AssignmentRule::DepartmentManagerOfInitiator { fallback, role_unique_names } => {
                if let Some(f) = fallback {
                    params.insert("fallback".into(), json!(f.as_str()));
                }
                if let Some(n) = role_unique_names {
                    params.insert("roleUniqueNames".into(), json!(n));
                }
                with("departmentManagerOfInitiator", params)
            }
            AssignmentRule::Unsupported { raw, .. } => raw,
        }
    }
}

/// 節點上的按鈕。動作是定義的一部分，不是 UI 自己長出來的。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionDef {
    pub action_key: String,
    pub label_key: String,
    pub kind: ActionKind,
    #[serde(default)]
    pub target_mode: Option<TargetMode>,
    #[serde(default)]
    pub target_node_key: Option<String>,
    /// 誰能按。`{}` ＝ 這一關的主責者；可以與主責者不同（「已付款」是會計按的）。
    #[serde(default)]
    pub who: AssignmentRule,
    pub requires_reason: bool,
    pub locks_revert: bool,
    #[serde(default)]
    pub side_effect_key: Option<String>,
    #[serde(default)]
    pub state_key: Option<String>,
    pub sort_order: i64,
}

impl ActionDef {
    /// 套 seed 預設：advance 預設走 transition；revert／abort 的理由強制必填；sideEffect 預設鎖回退。
    pub fn new(action_key: &str, label_key: &str, kind: ActionKind, sort_order: i64) -> Self {
        Self {
            action_key: action_key.into(),
            label_key: label_key.into(),
            kind,
            target_mode: (kind == ActionKind::Advance).then_some(TargetMode::Transition),
            target_node_key: None,
            who: AssignmentRule::None,
            requires_reason: matches!(kind, ActionKind::Revert | ActionKind::Abort),
            locks_revert: kind == ActionKind::SideEffect,
            side_effect_key: None,
            state_key: None,
            sort_order,
        }
    }

    pub fn target_mode(mut self, mode: TargetMode) -> Self {
        self.target_mode = Some(mode);
        self
    }

    pub fn side_effect(mut self, key: &str, locks_revert: bool) -> Self {
        self.side_effect_key = Some(key.into());
        self.locks_revert = locks_revert;
        self
    }

    pub fn state(mut self, key: &str) -> Self {
        self.state_key = Some(key.into());
        self
    }
}

/// 這一關要求的附件類別與份數。`min` 是至少要幾份（不是上限）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentRequirement {
    /// 封閉代碼：見 `eo_domain::attachment_limits::AttachmentCategory`。
    pub category: String,
    pub min: i64,
}

/// 這一關要求必填的單據欄位。`path` 是單據表（`to_jsonb` 之後）的欄位名，snake_case。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldRequirement {
    pub path: String,
    pub label_key: String,
}

/// 一關的「必傳附件類別」「必填欄位」。空＝這一關沒有要求（舊流程、大多數節點）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRequirements {
    #[serde(default)]
    pub attachments: Vec<AttachmentRequirement>,
    #[serde(default)]
    pub fields: Vec<FieldRequirement>,
}

impl StepRequirements {
    pub fn is_empty(&self) -> bool {
        self.attachments.is_empty() && self.fields.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDef {
    pub node_key: String,
    pub name: String,
    pub node_type: NodeType,
    #[serde(default)]
    pub assignment_rule: AssignmentRule,
    /// 預設 false ＝ 發起人不得核自己的單。
    pub allow_self_approval: bool,
    pub revertable: bool,
    /// 停在這一關時單的欄位可以被改（`revise` 模式靠它判定，不硬編節點名）。
    pub allows_subject_edit: bool,
    pub allow_jump_target: bool,
    pub parallel: bool,
    #[serde(default)]
    pub join_policy: Option<JoinPolicy>,
    #[serde(default)]
    pub join_node_key: Option<String>,
    /// `calculation` 節點要呼叫的服務；漏了它，計算節點會變成什麼都不做的空節點。
    #[serde(default)]
    pub service_key: Option<String>,
    #[serde(default)]
    pub layout: Value,
    #[serde(default)]
    pub actions: Vec<ActionDef>,
    /// 這一關的「必傳附件類別」「必填欄位」；`advance` 送出前的最後一道檢查就靠它。
    #[serde(default)]
    pub requirements: StepRequirements,
}

impl NodeDef {
    /// 套 seed 預設：不可自核、可回退、不可改單、不可跳入、非並行。
    pub fn new(node_key: &str, name: &str, node_type: NodeType) -> Self {
        Self {
            node_key: node_key.into(),
            name: name.into(),
            node_type,
            assignment_rule: AssignmentRule::None,
            allow_self_approval: false,
            revertable: true,
            allows_subject_edit: false,
            allow_jump_target: false,
            parallel: false,
            join_policy: None,
            join_node_key: None,
            service_key: None,
            layout: json!({}),
            actions: vec![],
            requirements: StepRequirements::default(),
        }
    }

    pub fn rule(mut self, rule: AssignmentRule) -> Self {
        self.assignment_rule = rule;
        self
    }

    pub fn layout(mut self, x: i64, y: i64) -> Self {
        self.layout = json!({ "x": x, "y": y });
        self
    }

    pub fn self_approval(mut self, allow: bool) -> Self {
        self.allow_self_approval = allow;
        self
    }

    pub fn subject_edit(mut self, allow: bool) -> Self {
        self.allows_subject_edit = allow;
        self
    }

    /// 這一關要求至少 `min` 份某類別的附件（`category` 是封閉代碼）。
    pub fn requires_attachment(mut self, category: &str, min: i64) -> Self {
        self.requirements.attachments.push(AttachmentRequirement { category: category.into(), min });
        self
    }

    /// 這一關要求單據本體的某個欄位非空；`label_key` 是畫面上「本關要求」清單要顯示的 i18n key。
    pub fn requires_field(mut self, path: &str, label_key: &str) -> Self {
        self.requirements.fields.push(FieldRequirement { path: path.into(), label_key: label_key.into() });
        self
    }

    pub fn revertable(mut self, revertable: bool) -> Self {
        self.revertable = revertable;
        self
    }

    pub fn service(mut self, key: &str) -> Self {
        self.service_key = Some(key.into());
        self
    }

    /// 並行節點：沒指定 join 政策時預設 `first`。
    pub fn parallel(mut self, join_policy: Option<JoinPolicy>, join_node_key: Option<&str>) -> Self {
        self.parallel = true;
        self.join_policy = Some(join_policy.unwrap_or(JoinPolicy::First));
        self.join_node_key = join_node_key.map(str::to_string);
        self
    }

    pub fn actions(mut self, actions: Vec<ActionDef>) -> Self {
        self.actions = actions;
        self
    }

    pub fn action(&self, action_key: &str) -> Option<&ActionDef> {
        self.actions.iter().find(|a| a.action_key == action_key)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransitionDef {
    pub from_node_key: String,
    pub to_node_key: String,
    #[serde(default)]
    pub kind: TransitionKind,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub condition: Condition,
    /// 回退邊的理由預設必填。
    pub requires_reason: bool,
    #[serde(default)]
    pub sort_order: i64,
}

impl TransitionDef {
    pub fn new(from: &str, to: &str, sort_order: i64) -> Self {
        Self {
            from_node_key: from.into(),
            to_node_key: to.into(),
            kind: TransitionKind::Forward,
            label: None,
            condition: Condition::Always,
            requires_reason: false,
            sort_order,
        }
    }

    pub fn send_back(mut self) -> Self {
        self.kind = TransitionKind::SendBack;
        self.requires_reason = true;
        self
    }

    pub fn when(mut self, label: &str, condition: Condition) -> Self {
        self.label = Some(label.into());
        self.condition = condition;
        self
    }
}

impl ConditionalEdge for TransitionDef {
    fn condition(&self) -> Option<&Condition> {
        Some(&self.condition)
    }

    fn sort_order(&self) -> i64 {
        self.sort_order
    }
}

/// 一份流程定義（一個版本）。版號、狀態、發佈者由 DB 配，不在這裡。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDefinition {
    pub key: String,
    pub name: String,
    pub subject_type: String,
    #[serde(default)]
    pub description: Option<String>,
    pub abort_enabled: bool,
    pub abort_approval_rule: AssignmentRule,
    pub nodes: Vec<NodeDef>,
    pub transitions: Vec<TransitionDef>,
}

impl WorkflowDefinition {
    /// 套 seed 預設：可中斷；中斷核可者＝最高負責人（admin＋CXO）。
    pub fn new(key: &str, name: &str, subject_type: &str, description: &str) -> Self {
        Self {
            key: key.into(),
            name: name.into(),
            subject_type: subject_type.into(),
            description: Some(description.into()),
            abort_enabled: true,
            abort_approval_rule: AssignmentRule::role(&["energy:admin", "energy:cxo"]),
            nodes: vec![],
            transitions: vec![],
        }
    }

    pub fn node(&self, node_key: &str) -> Option<&NodeDef> {
        self.nodes.iter().find(|n| n.node_key == node_key)
    }

    pub fn outgoing(&self, node_key: &str) -> Vec<&TransitionDef> {
        self.transitions.iter().filter(|t| t.from_node_key == node_key).collect()
    }
}

/// seed 時順手建的那一段綁定（`(案場, 單別)` → 這份定義）。案場由呼叫端給。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeedBinding {
    pub workflow_key: String,
    /// None ＝ 不分單別（舊行為）。
    pub doc_type: Option<EnergyDocType>,
    pub valid_from: String,
    pub note: String,
}

/// seed 綁定沒指定起日時的預設。
pub const DEFAULT_BINDING_VALID_FROM: &str = "2000-01-01";

fn binding(def: &WorkflowDefinition, doc_type: Option<EnergyDocType>, note: &str) -> SeedBinding {
    SeedBinding {
        workflow_key: def.key.clone(),
        doc_type,
        valid_from: DEFAULT_BINDING_VALID_FROM.into(),
        note: note.into(),
    }
}

// ---------------------------------------------------------------------------
// 內建定義 —— 全部是預設值不是需求
// ---------------------------------------------------------------------------

/// 加簽門檻（待客戶確認前的預設）。它是 seed 常數，改它不用改程式、不用改表。
pub const EXEC_APPROVAL_THRESHOLD: i64 = 100_000;

/// 部門 key。seed 部門時要用同一組。
pub const DEPT_SALES: &str = "sales";
pub const DEPT_ACCOUNTING: &str = "accounting";
/// 財務部（`deposit-request` 的放行關）。
pub const DEPT_FINANCE: &str = "finance";
/// 電費單平台資料審核人員（Q-04：單一審核步驟，任一位通過即完成）。
pub const DEPT_BILL_REVIEW: &str = "bill-reviewers";
/// 租金平台資料審核人員（Q-04：名單先建好，租金審核流程待 R08 的租金付款申請單）。
pub const DEPT_RENT_REVIEW: &str = "rent-reviewers";

fn submit(sort: i64) -> ActionDef {
    ActionDef::new("submit", "flow.action.submit", ActionKind::Advance, sort)
}

fn approve() -> ActionDef {
    ActionDef::new("approve", "flow.action.approve", ActionKind::Advance, 0)
}

fn release() -> ActionDef {
    ActionDef::new("release", "flow.action.release", ActionKind::Advance, 0)
}

fn send_back_previous() -> ActionDef {
    ActionDef::new("sendBack", "flow.action.sendBack", ActionKind::Revert, 1).target_mode(TargetMode::PreviousNode)
}

fn request_abort(sort: i64) -> ActionDef {
    ActionDef::new("requestAbort", "flow.action.requestAbort", ActionKind::Abort, sort)
}

/// 付款關：一顆按鈕的兩件事拆成兩顆 —— 先標副作用（鎖回退）、再推進。
/// 順序反過來就會出現「已放行但沒有付款紀錄」的鬼單。
fn mark_paid_then_submit() -> Vec<ActionDef> {
    vec![
        ActionDef::new("markPaid", "flow.action.markPaid", ActionKind::SideEffect, 0).side_effect("payment.sent", true),
        submit(1),
    ]
}

fn cxo_fallback_manager_rule() -> AssignmentRule {
    // R1：發起人的部門主管；解不到（發起人自己就是主管）→ 升到 CXO。
    AssignmentRule::manager_of_initiator(ManagerFallback::Role, &["energy:cxo"])
}

/// Slice 2b 的最小流程：start → 承辦確認 → 財務核准 → end。沒有回退邊。
pub fn minimal_bill_approval() -> WorkflowDefinition {
    let mut d = WorkflowDefinition::new(
        "bill-approval",
        "電費單核可（最小版）",
        "energyBill",
        "Slice 2b 的 walking skeleton：承辦確認 → 財務核准。",
    );
    d.nodes = vec![
        NodeDef::new("start", "起點", NodeType::Start).layout(0, 0),
        NodeDef::new("operatorCheck", "承辦確認", NodeType::HumanTask)
            .rule(AssignmentRule::role(&["energy:operator"]))
            .layout(200, 0)
            .actions(vec![
                submit(0),
                ActionDef::new("flagPending", "flow.action.flagPending", ActionKind::SetState, 1).state("pendingDocuments"),
                request_abort(2),
            ]),
        NodeDef::new("financeApproval", "財務核准", NodeType::HumanTask)
            .rule(AssignmentRule::role(&["energy:finance"]))
            .layout(400, 0)
            .actions(vec![
                approve(),
                send_back_previous(),
                ActionDef::new("cancelAll", "flow.action.cancelAll", ActionKind::Revert, 2)
                    .target_mode(TargetMode::CancelExecution),
            ]),
        NodeDef::new("end", "終點", NodeType::End).layout(600, 0),
    ];
    d.transitions = vec![
        TransitionDef::new("start", "operatorCheck", 0),
        TransitionDef::new("operatorCheck", "financeApproval", 0),
        TransitionDef::new("financeApproval", "end", 0),
    ];
    d
}

pub fn minimal_bill_approval_binding() -> SeedBinding {
    binding(&minimal_bill_approval(), None, "Slice 2b walking skeleton")
}

/// 七節點預設流程：
/// ```text
/// start → clerk-review → manager-approve ─(金額 ≥ 門檻)→ exec-approve ─┐
///                                 └───────(預設邊)──────────────────────┴→ calc → finance-release → payment → end
/// ```
/// Q-04（2026-11-16）起不再內建安裝，換成 `platform_review_bill()`；七關樣本留給引擎測試。
pub fn default_bill_approval() -> WorkflowDefinition {
    let mut d = WorkflowDefinition::new(
        "bill-approval",
        "電費單核可流程",
        "energyBill",
        "DESIGN.md §3 的七節點預設流程（§九-1 的預設值）。",
    );
    d.nodes = vec![
        NodeDef::new("start", "開始", NodeType::Start).layout(0, 120),
        // 被退回後承辦要能改單（revise 模式）；這一關主要是「改」，所以允許自核。
        // 四眼原則的價值在核可環節：manager-approve／finance-release／payment 一律不可自核。
        NodeDef::new("clerk-review", "承辦確認", NodeType::HumanTask)
            .subject_edit(true)
            .self_approval(true)
            .rule(AssignmentRule::department(DEPT_SALES, "all"))
            .layout(160, 120)
            .actions(vec![submit(0), request_abort(9)]),
        NodeDef::new("manager-approve", "主管核准", NodeType::HumanTask)
            .rule(cxo_fallback_manager_rule())
            .layout(320, 120)
            .actions(vec![approve(), send_back_previous()]),
        NodeDef::new("exec-approve", "高層加簽", NodeType::HumanTask)
            .rule(AssignmentRule::role(&["energy:cxo"]))
            .layout(480, 40)
            .actions(vec![approve(), send_back_previous()]),
        // 流程呼叫計算，不內含計算。
        NodeDef::new("calc", "計算應付", NodeType::Calculation).service("rating.computePayout").layout(640, 120),
        NodeDef::new("finance-release", "財務放行", NodeType::HumanTask)
            .rule(AssignmentRule::role(&["energy:finance"]))
            .layout(800, 120)
            .actions(vec![release(), send_back_previous()]),
        // 付款這一關不可回退 —— 錢出去了就不能當沒發生。
        NodeDef::new("payment", "付款", NodeType::HumanTask)
            .revertable(false)
            .rule(AssignmentRule::department(DEPT_ACCOUNTING, "all"))
            .layout(960, 120)
            .actions(mark_paid_then_submit()),
        NodeDef::new("end", "完成", NodeType::End).layout(1120, 120),
    ];
    d.transitions = vec![
        TransitionDef::new("start", "clerk-review", 0),
        TransitionDef::new("clerk-review", "manager-approve", 0),
        TransitionDef::new("manager-approve", "exec-approve", 0).when(
            &format!("金額 ≥ {EXEC_APPROVAL_THRESHOLD}"),
            Condition::leaf("bill.totalAmount", ConditionOp::Gte, json!(EXEC_APPROVAL_THRESHOLD)),
        ),
        // 無條件的預設邊放最後（sortOrder 較大）—— 第一個成立的贏。
        TransitionDef::new("manager-approve", "calc", 10),
        TransitionDef::new("exec-approve", "calc", 0),
        TransitionDef::new("calc", "finance-release", 0),
        TransitionDef::new("finance-release", "payment", 0),
        TransitionDef::new("payment", "end", 0),
    ];
    d
}

/// 綁在躉購那一家族：細分的 `renewableSurplus` 會回退到這裡。
pub fn default_bill_approval_binding() -> SeedBinding {
    binding(&default_bill_approval(), Some(EnergyDocType::ElectricityBill), "DESIGN.md §3 預設流程")
}

/// Q-04（2026-11-16 Marrash 確認）：單一審核步驟，電費單平台資料審核人員任一位通過即完成。
/// 平台審核通過不代表 EasyFlow 已核准或已付款。
/// G4（2026-11-23 使用者拍板）：通過不再算應付，租金改走週期試算（rent_cycle）；已安裝的庫由
/// migration 0108 自動把這個形狀（platform-review + rating.computePayout）換到新版、拿掉計算節點。
pub fn platform_review_bill() -> WorkflowDefinition {
    let mut d = WorkflowDefinition::new(
        "bill-approval",
        "電費單平台資料審核",
        "energyBill",
        "Q-04（2026-11-16 Marrash 確認）：單一審核步驟，電費單平台資料審核人員任一位通過即完成。平台審核通過不代表 EasyFlow 已核准或已付款。",
    );
    d.nodes = vec![
        NodeDef::new("start", "開始", NodeType::Start).layout(0, 120),
        NodeDef::new("platform-review", "平台資料審核", NodeType::HumanTask)
            .rule(AssignmentRule::department(DEPT_BILL_REVIEW, "all"))
            .layout(220, 120)
            // 使用者 2026-11-18 拍板：這一關開放改欄位、預設要求台電電費通知單＋電號／計費起迄日／
            // 應付總金額／繳費期限。
            .subject_edit(true)
            .requires_attachment("taipowerNotice", 1)
            .requires_field("customerNumberRaw", "energy.bills.field.customerNumberRaw")
            .requires_field("billingPeriodStart", "energy.bills.field.billingPeriodStart")
            .requires_field("billingPeriodEnd", "energy.bills.field.billingPeriodEnd")
            .requires_field("totalAmount", "energy.bills.field.totalAmount")
            .requires_field("dueDate", "energy.bills.field.dueDate")
            .actions(vec![ActionDef::new("pass", "flow.action.pass", ActionKind::Advance, 0)]),
        NodeDef::new("end", "平台審核通過", NodeType::End).layout(440, 120),
    ];
    d.transitions = vec![
        TransitionDef::new("start", "platform-review", 0),
        TransitionDef::new("platform-review", "end", 0),
    ];
    d
}

/// 電費資料審核與地主租金分離；同一位承辦可收件及確認，仍須屬於審核群組。
pub fn bill_data_review() -> WorkflowDefinition {
    let mut d = platform_review_bill();
    d.key = "bill-data-review".into();
    d.description = Some("電費單獨立審核；不要求地主租約，不計算租金，不送出 EasyFlow。".into());
    d.nodes.retain(|n| n.node_key != "calc");
    for n in &mut d.nodes {
        if n.node_key == "platform-review" {
            n.allow_self_approval = true;
            // 原單不一定有繳費期限（有餘電為收款），不列為所有單別必填。
            n.requirements.fields.retain(|f| f.path != "dueDate");
        }
    }
    d.transitions = vec![TransitionDef::new("start", "platform-review", 0), TransitionDef::new("platform-review", "end", 0)];
    d
}

pub fn platform_review_bill_binding() -> SeedBinding {
    binding(&platform_review_bill(), Some(EnergyDocType::ElectricityBill), "Q-04 平台資料審核")
}

/// 保證金請款（三節點）。沒有計算節點：金額來自合約，不用算也不取掃描件的。
pub fn deposit_request() -> WorkflowDefinition {
    let mut d = WorkflowDefinition::new(
        "deposit-request",
        "保證金請款流程",
        // subject 是既有的那一期，不是新造的單。
        "energyInstallment",
        "DESIGN.md §3.2 的三節點流程。沒有計算節點：金額來自合約。",
    );
    d.nodes = vec![
        // 送審關同樣可改單（它是 start 節點，退回會回到這裡）。
        NodeDef::new("submit", "送審", NodeType::Start).subject_edit(true).layout(0, 120),
        NodeDef::new("finance-release", "財務放行", NodeType::HumanTask)
            .rule(AssignmentRule::department(DEPT_FINANCE, "all"))
            .layout(220, 120)
            .actions(vec![release(), send_back_previous()]),
        NodeDef::new("pay", "付款", NodeType::HumanTask)
            .rule(AssignmentRule::department(DEPT_ACCOUNTING, "all"))
            .revertable(false)
            .layout(440, 120)
            .actions(mark_paid_then_submit()),
        NodeDef::new("end", "完成", NodeType::End).layout(660, 120),
    ];
    d.transitions = vec![
        TransitionDef::new("submit", "finance-release", 0),
        TransitionDef::new("finance-release", "pay", 0),
        TransitionDef::new("pay", "end", 0),
    ];
    d
}

pub fn deposit_request_binding() -> SeedBinding {
    binding(&deposit_request(), Some(EnergyDocType::DepositRequest), "DESIGN.md §3.2 保證金請款流程")
}

/// 用電電費單（我們付台電）。沒有 `calc` 節點 —— 對要付出去的錢算分潤沒有意義，
/// 這件事由流程定義保證，不是由某個 if 保證。也沒有加簽門檻：金額是台電開的。
pub fn utility_bill() -> WorkflowDefinition {
    let mut d = WorkflowDefinition::new(
        "utility-bill",
        "用電電費單付款流程",
        "energyBill",
        "RECOGNITION.md §9-1 的四關流程。沒有計算節點：這是我們要付出去的錢。",
    );
    d.nodes = vec![
        // start 是起點不是關卡：把承辦確認定義成 start 的話，那一關整個不存在。
        NodeDef::new("start", "開始", NodeType::Start).layout(0, 120),
        NodeDef::new("clerk-review", "承辦確認", NodeType::HumanTask)
            .subject_edit(true)
            .self_approval(true)
            .rule(AssignmentRule::department(DEPT_SALES, "all"))
            .layout(180, 120)
            .actions(vec![submit(0), request_abort(9)]),
        NodeDef::new("manager-approve", "主管核准", NodeType::HumanTask)
            .rule(cxo_fallback_manager_rule())
            .layout(400, 120)
            .actions(vec![approve(), send_back_previous()]),
        NodeDef::new("finance-release", "財務放行", NodeType::HumanTask)
            .rule(AssignmentRule::department(DEPT_FINANCE, "all"))
            .layout(600, 120)
            .actions(vec![release(), send_back_previous()]),
        NodeDef::new("payment", "付款", NodeType::HumanTask)
            .rule(AssignmentRule::department(DEPT_ACCOUNTING, "all"))
            .revertable(false)
            .layout(800, 120)
            .actions(mark_paid_then_submit()),
        NodeDef::new("end", "完成", NodeType::End).layout(1000, 120),
    ];
    d.transitions = vec![
        TransitionDef::new("start", "clerk-review", 0),
        TransitionDef::new("clerk-review", "manager-approve", 0),
        TransitionDef::new("manager-approve", "finance-release", 0),
        TransitionDef::new("finance-release", "payment", 0),
        TransitionDef::new("payment", "end", 0),
    ];
    d
}

/// 綁在粗分上：無餘電、僅設備發電、含電器租費三種的家族都是 `consumptionBill`。
pub fn utility_bill_binding() -> SeedBinding {
    binding(&utility_bill(), Some(EnergyDocType::ConsumptionBill), "RECOGNITION.md §9-1 用電電費單付款流程")
}

/// 開立發票（躉購·有餘電的第二條線）。與核可平行：開票卡住不該擋整張單結案。
pub fn invoice_issue() -> WorkflowDefinition {
    let mut d = WorkflowDefinition::new(
        "invoice-issue",
        "開立發票流程",
        "energyBill",
        "躉購有餘電的單：開發票給台電。與核可流程平行。",
    );
    d.nodes = vec![
        NodeDef::new("start", "開始", NodeType::Start).layout(0, 120),
        // 發票開出去了就不能當沒發生（同付款關的理由）。
        NodeDef::new("issue-invoice", "開立發票", NodeType::HumanTask)
            .rule(AssignmentRule::department(DEPT_ACCOUNTING, "all"))
            .revertable(false)
            .layout(220, 120)
            .actions(vec![
                ActionDef::new("markInvoiced", "flow.action.markInvoiced", ActionKind::SideEffect, 0)
                    .side_effect("invoice.issued", true),
                submit(1),
            ]),
        NodeDef::new("end", "完成", NodeType::End).layout(440, 120),
    ];
    d.transitions = vec![TransitionDef::new("start", "issue-invoice", 0), TransitionDef::new("issue-invoice", "end", 0)];
    d
}

/// 只綁躉購·有餘電那一種單 —— 無餘電那張不會因此多開一條開票流程。
pub fn invoice_issue_binding() -> SeedBinding {
    binding(&invoice_issue(), Some(EnergyDocType::RenewableSurplus), "躉購有餘電：開發票給台電（與核可流程平行）")
}

/// 五份內建定義與各自的 seed 綁定。`minimal` 與 `platform_review_bill` 同 key（`bill-approval`），是兩個版本的候選。
/// Q-04（2026-11-16）起內建安裝換成單一審核步驟；舊的七關 `default_bill_approval()` 不再內建安裝，留給引擎測試。
pub fn builtin_definitions() -> Vec<(WorkflowDefinition, SeedBinding)> {
    vec![
        (minimal_bill_approval(), minimal_bill_approval_binding()),
        (platform_review_bill(), platform_review_bill_binding()),
        (deposit_request(), deposit_request_binding()),
        (utility_bill(), utility_bill_binding()),
        (invoice_issue(), invoice_issue_binding()),
    ]
}
