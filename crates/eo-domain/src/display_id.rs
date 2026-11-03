//! 流程單的「單號」—— 對外指認用的代號（來源：`common/displayId.ts`）。
//!
//! 有真的就用真的（`documentNo`，例如台電單據號），沒有就用 `#<執行 id>`。
//! 單號是純顯示，不是路由的一部分。

use serde::{Deserialize, Serialize};

use crate::js_compat;

/// id 可能是數字或字串（DB id 讀出來的型別不一定）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IdLike {
    Number(i64),
    Text(String),
}

impl std::fmt::Display for IdLike {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IdLike::Number(n) => write!(f, "{n}"),
            IdLike::Text(s) => f.write_str(s),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayIdSource {
    #[serde(default)]
    pub id: Option<IdLike>,
    /// 正規欄位（`energyWorkflowExecution.documentNo`）—— 組織內唯一、可搜尋。
    #[serde(default)]
    pub document_no: Option<String>,
}

/// 顯示用的單號。只接受非空白的 documentNo：空白單號欄比顯示 `#4` 更難查。
pub fn get_display_id(source: Option<&DisplayIdSource>) -> String {
    let Some(source) = source else {
        return String::new();
    };
    if let Some(no) = source.document_no.as_deref() {
        let t = js_compat::trim(no);
        if !t.is_empty() {
            return t.to_string();
        }
    }
    source.id.as_ref().map(|id| format!("#{id}")).unwrap_or_default()
}

/// 網址片段是 DB id 還是單號：純數字才當 id（查不到就 404，不會拿錯一筆）。
pub fn is_db_id(raw: Option<&str>) -> bool {
    let t = js_compat::trim(raw.unwrap_or(""));
    !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit())
}

/// 要寫進 `documentNo` 的值。抽不到就回 None —— unique index 對 NULL 不衝突，空字串會。
pub fn normalize_document_no(raw: Option<&str>) -> Option<String> {
    let t = js_compat::trim(raw?);
    (!t.is_empty()).then(|| t.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 來源：displayId.ts 的 @example。
    #[test]
    fn display_id_examples() {
        let plain = DisplayIdSource { id: Some(IdLike::Number(4)), document_no: None };
        assert_eq!(get_display_id(Some(&plain)), "#4");
        let with_no = DisplayIdSource { id: Some(IdLike::Number(4)), document_no: Some("TP-2026-03-A".into()) };
        assert_eq!(get_display_id(Some(&with_no)), "TP-2026-03-A");
        let blank = DisplayIdSource { id: Some(IdLike::Number(4)), document_no: Some("  ".into()) };
        assert_eq!(get_display_id(Some(&blank)), "#4");
        assert_eq!(get_display_id(None), "");
        assert_eq!(get_display_id(Some(&DisplayIdSource::default())), "");
    }

    /// 來源：無對應 TS 測試；補驗 displayId.ts 的 isDbId／normalizeDocumentNo。
    #[test]
    fn db_id_vs_document_no() {
        assert!(is_db_id(Some(" 42 ")));
        assert!(!is_db_id(Some("TP-2026-03-A")));
        assert!(!is_db_id(Some("")));
        assert!(!is_db_id(None));
        assert_eq!(normalize_document_no(Some("  ")), None);
        assert_eq!(normalize_document_no(Some(" A1 ")).as_deref(), Some("A1"));
    }
}
