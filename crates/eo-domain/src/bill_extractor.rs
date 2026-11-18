//! OCR 邊界：「整理好的 JSON」→ `ExtractedBill[]`（來源：`common/billExtractor.ts`）。
//!
//! 原始 JSON 整包存成 `ocrRaw`、不 pin shape；只 pin 我們真的要用的抽出欄位。
//! 回傳陣列而不是單筆：一張掃描檔含多個電號只是實作細節，不是 schema 風險。

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::js_compat;
use crate::taipower_canonical::is_iso_day;

/// 抽取器的輸出 —— 我們唯一 pin 的形狀。每個欄位都可能抽不到，所以全部 optional。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedBill {
    /// 電號 —— 全系統的 join key。抽不到仍要建單（例外件）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub customer_number: Option<String>,
    /// `YYYY-MM-DD`。取版依據。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub billing_period_start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub billing_period_end: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_amount: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_kwh: Option<f64>,
    /// 台電開單日。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issued_at: Option<String>,
    /// 台電單據號 —— 外部世界本來就唯一的編號，送審時成為「單號」。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_no: Option<String>,
    /// 純提示，不參與任何判斷。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    /// 服務判定的版型；校對台靠它決定顯示哪一組欄位。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
    /// 繳費期限（高壓單有、躉購單沒有）；會計依它排序與逾期警示。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    /// 單上的契約編號原樣。對不上不猜，只標示。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_number_raw: Option<String>,
    /// 電費單明細 19 欄（卡 B）—— 欄位表驅動（`taipower_extractor::BILL_DETAIL_FIELDS`），
    /// 不逐欄手寫。鍵是我們的 camelCase 欄位名，值已經是 DB 形狀（數字／`YYYY-MM-DD`／原樣字串）。
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub detail: serde_json::Map<String, Value>,
}

impl ExtractedBill {
    pub fn is_empty(&self) -> bool {
        *self == ExtractedBill::default()
    }

    /// `extractedBillSchema` 的檢查（長度照 zod 數 UTF-16）。回傳每一條不合格的說明。
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errs = Vec::new();
        let len_between = |errs: &mut Vec<String>, name: &str, v: &Option<String>, max: usize| {
            if let Some(s) = v {
                let n = js_compat::utf16_len(s);
                if n < 1 || n > max {
                    errs.push(format!("{name}: 長度須介於 1–{max}"));
                }
            }
        };
        let dateish = |errs: &mut Vec<String>, name: &str, v: &Option<String>| {
            if let Some(s) = v {
                if !is_iso_day(s) {
                    errs.push(format!("{name}: 須為 YYYY-MM-DD"));
                }
            }
        };
        len_between(&mut errs, "customerNumber", &self.customer_number, 60);
        dateish(&mut errs, "billingPeriodStart", &self.billing_period_start);
        dateish(&mut errs, "billingPeriodEnd", &self.billing_period_end);
        for (name, v) in [("totalAmount", self.total_amount), ("generationKwh", self.generation_kwh)] {
            if v.is_some_and(|f| !f.is_finite()) {
                errs.push(format!("{name}: 須為有限數"));
            }
        }
        dateish(&mut errs, "issuedAt", &self.issued_at);
        len_between(&mut errs, "documentNo", &self.document_no, 120);
        if self.confidence.is_some_and(|c| !(0.0..=1.0).contains(&c)) {
            errs.push("confidence: 須介於 0–1".into());
        }
        len_between(&mut errs, "templateId", &self.template_id, 60);
        dateish(&mut errs, "dueDate", &self.due_date);
        len_between(&mut errs, "contractNumberRaw", &self.contract_number_raw, 120);
        if errs.is_empty() {
            Ok(())
        } else {
            Err(errs)
        }
    }
}

/// 台電的單子上金額有千分位：`1,234.5` → 1234.5。去掉之後是空的就是沒有值。
fn num(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite()),
        Value::String(s) => {
            let cleaned: String = s.chars().filter(|c| *c != ',' && !js_compat::is_js_whitespace(*c)).collect();
            if cleaned.is_empty() {
                return None;
            }
            let n = js_compat::string_to_number(&cleaned);
            n.is_finite().then_some(n)
        }
        _ => None,
    }
}

fn str_of(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => {
            let t = js_compat::trim(s);
            (!t.is_empty()).then(|| t.to_string())
        }
        n @ Value::Number(_) => Some(js_compat::to_js_string(n)),
        _ => None,
    }
}

static COMPACT_DATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([0-9]{4})([0-9]{2})([0-9]{2})$").expect("常數"));
static PARTS_DATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([0-9]{4})[-/.]([0-9]{1,2})[-/.]([0-9]{1,2})$").expect("常數"));

/// `2026/01/31`、`2026-1-3`、`20260131` → `2026-01-31`；認不得就回 None。不檢查月日範圍。
pub fn normalize_date(v: &Value) -> Option<String> {
    let s = str_of(Some(v))?;
    if let Some(c) = COMPACT_DATE.captures(&s) {
        return Some(format!("{}-{}-{}", &c[1], &c[2], &c[3]));
    }
    let c = PARTS_DATE.captures(&s)?;
    Some(format!("{}-{:0>2}-{:0>2}", &c[1], &c[2], &c[3]))
}

/// JS 的 `a ?? b ?? c`：取第一個不是 null／不存在的鍵。
fn first_present<'a>(raw: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().filter_map(|k| raw.get(k)).find(|v| !v.is_null())
}

/// v1 的抽取器：吃整理好的 JSON，不做辨識。抽不到就留空，不要猜。
///
/// 接受 `{ bills: [...] }`、陣列、或單一物件（視為一筆）。
pub fn mock_bill_extractor(ocr_json: &Value) -> Vec<ExtractedBill> {
    let single;
    let items: &[Value] = match ocr_json {
        Value::Object(m) if m.get("bills").is_some_and(Value::is_array) => {
            m["bills"].as_array().map(Vec::as_slice).unwrap_or_default()
        }
        Value::Array(a) => a.as_slice(),
        other => {
            single = [other.clone()];
            &single
        }
    };

    items
        .iter()
        .map(|raw| ExtractedBill {
            customer_number: str_of(first_present(raw, &["customerNumber", "customer_number", "電號"])),
            billing_period_start: first_present(raw, &["billingPeriodStart", "periodStart", "計費起"])
                .and_then(normalize_date),
            billing_period_end: first_present(raw, &["billingPeriodEnd", "periodEnd", "計費訖"]).and_then(normalize_date),
            total_amount: num(first_present(raw, &["totalAmount", "amount", "總金額"])),
            generation_kwh: num(first_present(raw, &["generationKwh", "kwh", "度數"])),
            issued_at: first_present(raw, &["issuedAt", "issueDate", "開單日"]).and_then(normalize_date),
            confidence: raw.get("confidence").and_then(Value::as_f64),
            ..Default::default()
        })
        // 整筆都抽不到東西就不產生 bill；抽到一半正是校對台要處理的情況。
        .filter(|b| !b.is_empty())
        .collect()
}

/// 抽取器的簽名。真 OCR 交付時只換實作。
pub type BillExtractor = fn(&Value) -> Vec<ExtractedBill>;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 來源：billExtractor.ts 的 normalizeDate 說明（三種寫法）。
    #[test]
    fn normalize_date_shapes() {
        assert_eq!(normalize_date(&json!("2026/01/31")).as_deref(), Some("2026-01-31"));
        assert_eq!(normalize_date(&json!("2026-1-3")).as_deref(), Some("2026-01-03"));
        assert_eq!(normalize_date(&json!("20260131")).as_deref(), Some("2026-01-31"));
        assert_eq!(normalize_date(&json!("115/01/31")), None);
        assert_eq!(normalize_date(&json!(20260131)).as_deref(), Some("2026-01-31"));
    }

    /// 來源：billExtractor.ts 的 mockBillExtractor（多筆、千分位、抽不到就不產生）。
    #[test]
    fn mock_extractor_handles_shapes() {
        let out = mock_bill_extractor(&json!({ "bills": [
            { "電號": " 01-2233-4455 ", "總金額": "1,234.5", "計費起": "2026/1/1" },
            { "nothing": true },
        ]}));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].customer_number.as_deref(), Some("01-2233-4455"));
        assert_eq!(out[0].total_amount, Some(1234.5));
        assert_eq!(out[0].billing_period_start.as_deref(), Some("2026-01-01"));

        // `??` 只跳過 null：空字串的 totalAmount 不會退到 amount。
        let one = mock_bill_extractor(&json!({ "totalAmount": "", "amount": 5, "kwh": 0 }));
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].total_amount, None);
        assert_eq!(one[0].generation_kwh, Some(0.0));

        assert!(mock_bill_extractor(&Value::Null).is_empty());
    }

    /// 來源：無對應 TS 測試；補驗 billExtractor.ts 的 extractedBillSchema。
    #[test]
    fn validate_follows_zod() {
        let ok = ExtractedBill { customer_number: Some("x".into()), ..Default::default() };
        assert!(ok.validate().is_ok());
        let bad = ExtractedBill { confidence: Some(1.5), due_date: Some("2026/01/01".into()), ..Default::default() };
        assert_eq!(bad.validate().unwrap_err().len(), 2);
    }
}
