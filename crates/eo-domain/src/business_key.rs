//! 業務身分 —— 「這是不是同一張單」的唯一判準（來源：`common/businessKey.ts`）。
//!
//! 正規化只做「去非英數 ＋ 轉大寫」：不補零、不猜格式，否則兩張真的不同的單會被靜默併成一張。
//! 抽不到就回 None，絕不用空字串 —— 空字串會互相碰撞，partial unique 也靠 NULL 才成立。

use serde_json::Value;

use crate::js_compat;
use crate::taipower_canonical::is_iso_day;

/// 電號正規化：`01-2233-4455` 與 `0122334455` 是同一個；空白回 None。
pub fn normalize_customer_number(raw: Option<&str>) -> Option<String> {
    let raw = raw.filter(|s| !s.is_empty())?;
    // 保留英數（有些電號含英文字母），只去掉分隔符號與空白。
    let cleaned: String = raw.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_uppercase()).collect();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// `YYYY-MM-DD`（dateonly 欄位讀出來可能帶時間，一律切前 10 碼）。
fn day(v: Option<&str>) -> Option<String> {
    let s = js_compat::utf16_prefix(v.unwrap_or(""), 10);
    is_iso_day(&s).then_some(s)
}

/// 電費單的業務身分 ＝ `bill|電號|計費期間起|計費期間迄`。三者缺一就回 None，不用部分欄位湊。
pub fn bill_business_key(
    customer_number_raw: Option<&str>,
    billing_period_start: Option<&str>,
    billing_period_end: Option<&str>,
) -> Option<String> {
    let cn = normalize_customer_number(customer_number_raw)?;
    let from = day(billing_period_start)?;
    let to = day(billing_period_end)?;
    Some(format!("bill|{cn}|{from}|{to}"))
}

/// 保證金的業務身分 ＝ `inst|合約|期數`。天生唯一；算它是為了讓兩種 subject 的去重同一套。
/// 參數是原始 JSON 值（id 可能是數字或字串），null 視同沒有。
pub fn installment_business_key(contract_id: Option<&Value>, seq: Option<&Value>) -> Option<String> {
    let c = contract_id.filter(|v| !v.is_null())?;
    let s = seq.filter(|v| !v.is_null())?;
    Some(format!("inst|{}|{}", js_compat::to_js_string(c), js_compat::to_js_string(s)))
}

/// 依 subjectType 分派；row 是 DB 列的 JSON（camelCase 屬性名，保證金的合約 id 兩種寫法都認）。
pub fn business_key_of(subject_type: &str, row: &Value) -> Option<String> {
    match subject_type {
        "energyBill" => {
            // 原本對任意值做 `String(...)`：falsy 的電號視同沒有，其餘照 JS 轉字串。
            let as_text = |k: &str| -> Option<String> {
                row.get(k).filter(|v| !v.is_null()).map(js_compat::to_js_string)
            };
            let cn = row.get("customerNumberRaw").filter(|v| js_compat::truthy(v)).map(js_compat::to_js_string);
            bill_business_key(
                cn.as_deref(),
                as_text("billingPeriodStart").as_deref(),
                as_text("billingPeriodEnd").as_deref(),
            )
        }
        "energyInstallment" => {
            let contract = row.get("contract_id").filter(|v| !v.is_null()).or_else(|| row.get("contractId"));
            installment_business_key(contract, row.get("seq"))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 來源：businessKey.ts 的 @example（同一個電號兩種寫法）。
    #[test]
    fn customer_number_examples() {
        assert_eq!(normalize_customer_number(Some("01-2233-4455")).as_deref(), Some("0122334455"));
        assert_eq!(normalize_customer_number(Some("0122334455")).as_deref(), Some("0122334455"));
        assert_eq!(normalize_customer_number(Some("  ")), None);
        assert_eq!(normalize_customer_number(Some("ab-12")).as_deref(), Some("AB12"));
        assert_eq!(normalize_customer_number(None), None);
    }

    /// 來源：無對應 TS 測試；補驗 businessKey.ts 的 billBusinessKey。
    #[test]
    fn bill_key_requires_all_three() {
        assert_eq!(
            bill_business_key(Some("01-2233-4455"), Some("2026-01-01T00:00:00Z"), Some("2026-01-31")).as_deref(),
            Some("bill|0122334455|2026-01-01|2026-01-31")
        );
        assert_eq!(bill_business_key(Some("01"), None, Some("2026-01-31")), None);
        assert_eq!(bill_business_key(Some("01"), Some("2026/01/01"), Some("2026-01-31")), None);
    }

    /// 來源：無對應 TS 測試；補驗 businessKey.ts 的 businessKeyOf／installmentBusinessKey。
    #[test]
    fn dispatch_by_subject_type() {
        let row = json!({ "contract_id": 7, "seq": 3 });
        assert_eq!(business_key_of("energyInstallment", &row).as_deref(), Some("inst|7|3"));
        let row = json!({ "contractId": "C-1", "seq": null });
        assert_eq!(business_key_of("energyInstallment", &row), None);
        let bill = json!({ "customerNumberRaw": "x1", "billingPeriodStart": "2026-01-01", "billingPeriodEnd": "2026-01-31" });
        assert_eq!(business_key_of("energyBill", &bill).as_deref(), Some("bill|X1|2026-01-01|2026-01-31"));
        assert_eq!(business_key_of("other", &bill), None);
    }
}
