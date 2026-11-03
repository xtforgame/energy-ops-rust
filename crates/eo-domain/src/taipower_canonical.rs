//! 台電辨識服務的 canonical 結果型別（來源：`common/taipowerCanonical.ts`）。
//!
//! 只 pin「一個欄位長什麼樣」與我們真的會讀的那幾個 key；欄位清單是服務那側的契約，會長。
//! 列舉值一律保留未知字串（`Other`），服務多一個狀態不可讓整份結果讀不進來。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::js_compat;
use crate::serde_util::double_option;

string_enum! {
    /// 一個欄位的狀態。`missing` 與 `not_applicable` 是不同的事，不可都轉成空字串。
    pub enum CanonicalFieldStatus {
        Extracted => "extracted",
        OcrExtracted => "ocr_extracted",
        Edited => "edited",
        /// parser 看到欄位、且確認原單就是空的。
        SourceBlank => "source_blank",
        /// 沒取得或無法確認。
        Missing => "missing",
        /// 有候選值但不可靠。
        Ambiguous => "ambiguous",
        /// 這個版型沒有這一欄。
        NotApplicable => "not_applicable",
    }
}

string_enum! {
    /// 值是怎麼被決定的（三來源交叉驗證的結果）。
    pub enum CanonicalResolution {
        PdfText => "pdf_text",
        CrossVerified => "cross_verified",
        SingleEngine => "single_engine",
        RuleResolved => "rule_resolved",
        /// 來源不一致且無法裁決 —— 值會是 null，要人看。
        Conflict => "conflict",
        ExpectedMissing => "expected_missing",
        SourceBlank => "source_blank",
    }
}

string_enum! {
    pub enum CanonicalSource {
        PdfText => "pdf_text",
        Paddleocr => "paddleocr",
        Tesseract => "tesseract",
    }
}

string_enum! {
    pub enum ExtractionMethod {
        PdfText => "pdf_text",
        Ocr => "ocr",
    }
}

/// 版型 id 的已知值。`templateId` 本身存成字串：服務回什麼就是什麼，判讀在抽取器。
pub const TEMPLATE_LEGACY_RENEWABLE_PURCHASE: &str = "legacy_renewable_purchase";
pub const TEMPLATE_RENEWABLE_PURCHASE: &str = "renewable_purchase";
pub const TEMPLATE_HIGH_VOLTAGE: &str = "high_voltage";
pub const TEMPLATE_UNKNOWN: &str = "unknown";

/// 欄位在原檔上的位置（0–1 相對座標）。沒有＝不確定，不是 0,0。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalEvidence {
    /// 服務偶爾不給頁碼；讀的一方以第 1 頁計。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_number: Option<i64>,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub raw_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<CanonicalSource>,
    /// 外層 None ＝ 沒給；`Some(None)` ＝ 服務明確回 null。
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub confidence: Option<Option<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<CanonicalResolution>,
    #[serde(rename = "box", default, skip_serializing_if = "Option::is_none")]
    pub bbox: Option<CanonicalBox>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidates: Option<Vec<Value>>,
}

/// 欄位值：字串或數字（數字保留 JSON 原樣，整數不會變成 `127848.0`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CanonicalValue {
    Number(serde_json::Number),
    Text(String),
}

impl CanonicalValue {
    /// JS 的 `String(value)`。
    pub fn to_js_string(&self) -> String {
        match self {
            CanonicalValue::Text(s) => s.clone(),
            CanonicalValue::Number(n) => js_compat::to_js_string(&Value::Number(n.clone())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalField {
    /// `None` ＝ null（沒有值）；與空字串是不同的事。
    #[serde(default)]
    pub value: Option<CanonicalValue>,
    #[serde(default)]
    pub raw_value: String,
    pub status: CanonicalFieldStatus,
    #[serde(default)]
    pub evidence: Option<CanonicalEvidence>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalIssue {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub fields: Vec<String>,
    /// true ＝ 核心欄位有問題，不該就這樣送審。
    #[serde(default)]
    pub blocking: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalRecord {
    #[serde(default)]
    pub fields: BTreeMap<String, CanonicalField>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extraction_method: Option<ExtractionMethod>,
    /// 不要拿它當「要不要人工校對」的訊號；判準是 `issues` 裡有沒有 blocking。
    #[serde(default)]
    pub review_status: String,
    /// `None` ＝ 認不得（null）。
    #[serde(default)]
    pub template_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalDocumentSuccess {
    #[serde(default)]
    pub source_file_id: String,
    #[serde(default)]
    pub source_file_name: String,
    pub record: CanonicalRecord,
    #[serde(default)]
    pub issues: Vec<CanonicalIssue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalDocumentFailure {
    #[serde(default)]
    pub source_file_id: String,
    #[serde(default)]
    pub source_file_name: String,
    pub error: CanonicalError,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum CanonicalDocument {
    Succeeded(CanonicalDocumentSuccess),
    Failed(CanonicalDocumentFailure),
}

string_enum! {
    pub enum CanonicalJobStatus {
        Completed => "completed",
        CompletedWithIssues => "completed_with_issues",
        Failed => "failed",
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalJobResult {
    pub schema_version: String,
    pub job_id: String,
    pub status: CanonicalJobStatus,
    pub created_at: String,
    pub completed_at: String,
    pub documents: Vec<CanonicalDocument>,
}

string_enum! {
    pub enum TprJobStatus {
        Queued => "queued",
        Processing => "processing",
        Completed => "completed",
        CompletedWithIssues => "completed_with_issues",
        Failed => "failed",
    }
}

string_enum! {
    pub enum TprFileStatus {
        Queued => "queued",
        Processing => "processing",
        Succeeded => "succeeded",
        Failed => "failed",
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TprJobFile {
    pub source_file_id: String,
    pub source_file_name: String,
    pub status: TprFileStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<CanonicalError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TprJobSummary {
    pub job_id: String,
    pub status: TprJobStatus,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    pub files: Vec<TprJobFile>,
}

pub const TPR_TERMINAL_STATUSES: [&str; 3] = ["completed", "completed_with_issues", "failed"];

pub fn is_tpr_terminal(s: &str) -> bool {
    TPR_TERMINAL_STATUSES.contains(&s)
}

/// 檔案需要密碼 —— 與「檔案損壞」是兩件事，畫面上要給不同的出口。
pub const TPR_PASSWORD_REQUIRED: &str = "password_required";

// ---------------------------------------------------------------------------
// 讀 canonical 的小工具（前後端共用，不要各寫一份）
// ---------------------------------------------------------------------------

pub fn field_of<'a>(record: &'a CanonicalRecord, key: &str) -> Option<&'a CanonicalField> {
    record.fields.get(key)
}

/// 取字串值。null／欄位不存在／空白字串一律 None。
pub fn text_of(record: &CanonicalRecord, key: &str) -> Option<String> {
    let v = field_of(record, key)?.value.as_ref()?;
    let s = v.to_js_string();
    let t = js_compat::trim(&s);
    (!t.is_empty()).then(|| t.to_string())
}

/// 取數值。台電的單上金額可以是負的（無餘電時應付總金額 -132）。
///
/// 照 JS `Number(...)`：去掉逗號與空白後的**空字串是 0**，不是「沒有值」。
pub fn number_of(record: &CanonicalRecord, key: &str) -> Option<f64> {
    let v = field_of(record, key)?.value.as_ref()?;
    match v {
        CanonicalValue::Number(n) => n.as_f64().filter(|f| f.is_finite()),
        CanonicalValue::Text(s) => {
            let cleaned: String = s.chars().filter(|c| *c != ',' && !js_compat::is_js_whitespace(*c)).collect();
            let n = js_compat::string_to_number(&cleaned);
            n.is_finite().then_some(n)
        }
    }
}

/// `YYYY-MM-DD`；服務已經把民國年換算好了，這裡只做形狀確認。
pub fn date_of(record: &CanonicalRecord, key: &str) -> Option<String> {
    text_of(record, key).filter(|s| is_iso_day(s))
}

pub(crate) fn is_iso_day(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

pub fn blocking_issues(issues: &[CanonicalIssue]) -> Vec<&CanonicalIssue> {
    issues.iter().filter(|i| i.blocking).collect()
}
