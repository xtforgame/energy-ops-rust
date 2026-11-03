//! 副作用與流程對象的顯示名（來源：`common/sideEffects.ts`）。
//!
//! 稽核的 `targetLabel` 是寫入時固定下來的字串快照，所以解析發生在後端。
//! 查不到就原樣回傳識別碼：寧可漏一個字串，不要吞掉一筆稽核。

use crate::js_compat;

/// 副作用識別碼 → 顯示名。
pub const SIDE_EFFECT_LABELS: [(&str, &str); 1] = [("payment.sent", "付款已送出")];

pub fn side_effect_label(key: Option<&str>) -> String {
    let k = js_compat::trim(key.unwrap_or(""));
    if k.is_empty() {
        return String::new();
    }
    SIDE_EFFECT_LABELS
        .iter()
        .find(|(id, _)| *id == k)
        .map(|(_, label)| label.to_string())
        .unwrap_or_else(|| k.to_string())
}

const SUBJECT_LABELS: [(&str, &str); 2] = [("energyBill", "電費單"), ("energyInstallment", "保證金期數")];

/// 流程單的「對象」—— 不印 model 名，也不只印一個裸數字。
pub fn subject_label(subject_type: Option<&str>, subject_id: Option<&str>) -> String {
    let t = js_compat::trim(subject_type.unwrap_or(""));
    let id = js_compat::trim(subject_id.unwrap_or(""));
    let name = SUBJECT_LABELS.iter().find(|(k, _)| *k == t).map(|(_, v)| *v).unwrap_or(t);
    match (name.is_empty(), id.is_empty()) {
        (true, true) => String::new(),
        (true, false) => format!("#{id}"),
        (false, true) => name.to_string(),
        (false, false) => format!("{name} #{id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 來源：無對應 TS 測試；補驗 sideEffects.ts 的 sideEffectLabel／subjectLabel。
    #[test]
    fn labels_fall_back_to_the_raw_key() {
        assert_eq!(side_effect_label(Some("payment.sent")), "付款已送出");
        assert_eq!(side_effect_label(Some("invoice.issued")), "invoice.issued");
        assert_eq!(side_effect_label(None), "");
        assert_eq!(subject_label(Some("energyBill"), Some("20")), "電費單 #20");
        assert_eq!(subject_label(Some("energyStation"), None), "energyStation");
        assert_eq!(subject_label(None, Some("5")), "#5");
        assert_eq!(subject_label(None, None), "");
    }
}
