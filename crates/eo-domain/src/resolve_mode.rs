//! 單據詳情頁的五個模式 —— 後端是唯一真相（來源：`energy/workflow/resolveMode.ts`）。
//!
//! | 模式 | 誰 | 欄位 | 動作列 |
//! |---|---|---|---|
//! | `intake` | 承辦，尚未送審 | 可編輯 | 存草稿／送審 |
//! | `revise` | 承辦，被退回後 | 可編輯 | 存草稿／重新送審 |
//! | `act` | 輪到他的人 | 唯讀 | 由 availableActions 決定 |
//! | `readonly` | 看得到但沒事做的人 | 唯讀 | 無 —— 改印 modeReason |
//! | `denied` | 不該看到的人 | — | 403 |

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::js_compat;
use crate::serde_util::double_option;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BillMode {
    Intake,
    Revise,
    Act,
    Readonly,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModeReasonCode {
    NotMyTurn,
    ExcludedOwner,
    ClaimedByOther,
    Finished,
    Aborted,
    ViewOnly,
    NotSubmitted,
}

/// `readonly` 一定要帶它：什麼都不能按又不解釋的頁面，使用者只會以為它壞了。
///
/// 外層 None ＝ 這個鍵不出現；`Some(None)` ＝ 出現且為 null（與 TS 的物件形狀一致）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeReason {
    pub code: ModeReasonCode,
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub node_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub assignee_hint: Option<Option<String>>,
    /// `finished` 用：完成日期 `YYYY-MM-DD`。
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub at: Option<Option<String>>,
}

impl ModeReason {
    fn code(code: ModeReasonCode) -> Self {
        Self { code, node_name: None, assignee_hint: None, at: None }
    }

    /// 這一句說明的節點名（沒有就 None）。
    pub fn node_name(&self) -> Option<&str> {
        self.node_name.as_ref().and_then(|n| n.as_deref())
    }

    pub fn at(&self) -> Option<&str> {
        self.at.as_ref().and_then(|n| n.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeVerdict {
    pub mode: BillMode,
    pub mode_reason: Option<ModeReason>,
    pub can_edit_subject: bool,
    pub can_add_attachment: bool,
}

/// 結束時間：DB 讀出來是時間，API 進來可能是字串。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EndedAt {
    Time(DateTime<Utc>),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionState {
    pub status: String,
    #[serde(default)]
    pub ended_at: Option<EndedAt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveTaskState {
    pub state: String,
    pub assignee_user_id: Option<i64>,
    #[serde(default)]
    pub node_name: Option<String>,
    /// 這一關的 `allowsSubjectEdit`。
    pub allows_subject_edit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveModeInput {
    /// 已經過存取檢查？false 就是 `denied`。
    pub has_access: bool,
    /// 還沒送審時為 None。
    pub execution: Option<ExecutionState>,
    /// 目前活躍的那一關。
    pub active_task: Option<ActiveTaskState>,
    /// 我是不是這一關的候選人。
    pub is_candidate: bool,
    /// 我是不是被排除的（防自核）。
    pub is_excluded: bool,
    /// 送審前用：我是送件者、或有 `energy:flow:act`。
    pub can_intake: bool,
    /// 我有沒有 `energy:flow:act`。只影響說法不影響權限；None 沿用舊行為。
    #[serde(default)]
    pub can_act: Option<bool>,
    /// 這一關由誰處理（給說明句用）。
    #[serde(default)]
    pub assignee_hint: Option<String>,
    pub me: i64,
}

/// 日期 → `YYYY-MM-DD`（UTC）。不可寫成「切前十個字」：時間物件轉字串會切出 `Sat Aug 29`。
///
/// 字串照 JS `new Date(...)` 的常見格式解析；不帶時區的日期時間以 UTC 計
/// （JS 會以伺服器本地時區計，純邏輯層沒有本地時區）；認不得就退回切前十個字。
fn iso_day(v: Option<&EndedAt>) -> Option<String> {
    match v? {
        EndedAt::Time(t) => Some(t.format("%Y-%m-%d").to_string()),
        EndedAt::Text(s) if s.is_empty() => None,
        EndedAt::Text(s) => {
            if let Ok(t) = DateTime::parse_from_rfc3339(s) {
                return Some(t.with_timezone(&Utc).format("%Y-%m-%d").to_string());
            }
            if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                return Some(d.format("%Y-%m-%d").to_string());
            }
            for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M:%S%.f"] {
                if let Ok(t) = NaiveDateTime::parse_from_str(s, fmt) {
                    return Some(t.format("%Y-%m-%d").to_string());
                }
            }
            Some(js_compat::utf16_prefix(s, 10))
        }
    }
}

/// 已結束的 execution 狀態 —— 這幾種任何人都不能改。
const FINISHED: [&str; 3] = ["completed", "aborted", "terminated"];

fn readonly(reason: ModeReason) -> ModeVerdict {
    ModeVerdict { mode: BillMode::Readonly, mode_reason: Some(reason), can_edit_subject: false, can_add_attachment: false }
}

/// 六步，第一個成立就回。順序不可調換：第 3 步（已結束）必須排在 4/5 之前，
/// 否則一張走完的單若剛好停在可改單的節點上，那個節點的候選人還能改它 —— 而它已經付過錢了。
pub fn resolve_mode(input: &ResolveModeInput) -> ModeVerdict {
    // 1. 看不到就是看不到。
    if !input.has_access {
        return ModeVerdict { mode: BillMode::Denied, mode_reason: None, can_edit_subject: false, can_add_attachment: false };
    }

    // 2. 還沒送審。
    let Some(execution) = input.execution.as_ref() else {
        if input.can_intake {
            return ModeVerdict { mode: BillMode::Intake, mode_reason: None, can_edit_subject: true, can_add_attachment: true };
        }
        return readonly(ModeReason::code(ModeReasonCode::NotSubmitted));
    };

    // 3. 已完成／已中斷。
    if FINISHED.contains(&execution.status.as_str()) {
        let code = if execution.status == "completed" { ModeReasonCode::Finished } else { ModeReasonCode::Aborted };
        return readonly(ModeReason { at: Some(iso_day(execution.ended_at.as_ref())), ..ModeReason::code(code) });
    }

    let task = input.active_task.as_ref();
    let with_base = |code| ModeReason {
        node_name: Some(task.and_then(|t| t.node_name.clone())),
        assignee_hint: Some(input.assignee_hint.clone()),
        ..ModeReason::code(code)
    };

    // 沒有活躍的關卡（例如計算節點正在跑）—— 誰都沒事做。
    let Some(task) = task else {
        return readonly(with_base(ModeReasonCode::NotMyTurn));
    };

    // 別人已經認領了 —— 那不是權限不足，是別人在處理。
    if task.assignee_user_id.is_some_and(|a| a != input.me) {
        return readonly(with_base(ModeReasonCode::ClaimedByOther));
    }
    // 防自核：發起人被排除。
    if input.is_excluded {
        return readonly(with_base(ModeReasonCode::ExcludedOwner));
    }

    let mine_now = input.is_candidate || task.assignee_user_id == Some(input.me);
    if !mine_now {
        // 唯讀身分要說「你是唯讀身分」：「這一關由 X 處理」暗示等一下會輪到他，而他永遠不會。
        if input.can_act == Some(false) {
            return readonly(with_base(ModeReasonCode::ViewOnly));
        }
        return readonly(with_base(ModeReasonCode::NotMyTurn));
    }

    // 4. 這一關允許改單 → revise（被退回後承辦回到 clerk-review 就是走這裡）。
    if task.allows_subject_edit {
        return ModeVerdict { mode: BillMode::Revise, mode_reason: None, can_edit_subject: true, can_add_attachment: true };
    }

    // 5. 輪到我但不能改欄位。可以補件 —— 主管說「缺一張證明」時要有地方上傳。
    ModeVerdict { mode: BillMode::Act, mode_reason: None, can_edit_subject: false, can_add_attachment: true }
}
