//! 台電辨識結果 → 我們的單別與 `ExtractedBill`（來源：`common/taipowerExtractor.ts`）。
//!
//! 兩者吃的都是服務回來的 canonical document（存進 `energyBill.ocrRaw`）。
//! 版型 → 單別一對一，不猜；認不得就是 `unclassified`，絕不 fallback 成電費單。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::bill_extractor::{mock_bill_extractor, ExtractedBill};
use crate::document_ingest::{doc_type_to_workflow, mock_classifier, ClassifyResult, EnergyDocType};
use crate::taipower_canonical::{
    blocking_issues, date_of, number_of, text_of, CanonicalDocument, CanonicalRecord, TEMPLATE_HIGH_VOLTAGE,
    TEMPLATE_UNKNOWN,
};

/// 版型 → 粗分單別。細分要看欄位（`doc_type_from_record`）。
pub fn template_doc_type(template_id: &str) -> Option<EnergyDocType> {
    match template_id {
        "renewable_purchase" | "legacy_renewable_purchase" => Some(EnergyDocType::ElectricityBill),
        "high_voltage" => Some(EnergyDocType::ConsumptionBill),
        _ => None,
    }
}

/// 所有認得的版型（測試與畫面用）。
pub const KNOWN_TEMPLATES: [&str; 3] = ["renewable_purchase", "legacy_renewable_purchase", "high_voltage"];

/// 單別的中文名 —— 畫面與稽核都不可以出現 raw docType。
pub fn doc_type_label(t: EnergyDocType) -> &'static str {
    match t {
        EnergyDocType::RenewableSurplus => "躉購電費單（有餘電·需開發票）",
        EnergyDocType::RenewableNoSurplus => "躉購電費單（無餘電·需付款）",
        EnergyDocType::ConsumptionDevice => "逆變器電費單（僅設備發電使用電費）",
        EnergyDocType::ConsumptionDeviceRental => "逆變器電費單（含電器租費）",
        EnergyDocType::ElectricityBill => "電費單（躉購）",
        EnergyDocType::ConsumptionBill => "用電電費單",
        EnergyDocType::DepositRequest => "保證金請款單",
        EnergyDocType::Unclassified => "未判定",
    }
}

pub fn template_label(template_id: &str) -> Option<&'static str> {
    match template_id {
        "renewable_purchase" => Some("再生能源購售電通知單"),
        "legacy_renewable_purchase" => Some("再生能源電能躉購電費通知單（舊版）"),
        "high_voltage" => Some("繳費通知單（高壓電力用戶）"),
        _ => None,
    }
}

/// 原始 JSON 長得像不像 canonical document（判讀器分流用，與 TS 的判準相同）。
pub fn looks_canonical(ocr_raw: &Value) -> bool {
    match ocr_raw.get("status").and_then(Value::as_str) {
        Some("succeeded") => ocr_raw.get("record").is_some_and(|r| r.is_object() || r.is_array()),
        Some("failed") => ocr_raw.get("error").is_some_and(crate::js_compat::truthy),
        _ => false,
    }
}

/// 從存下來的 `ocrRaw` 取回 canonical document。
///
/// 長得像 canonical 但讀不進型別（必要欄位缺、型別不對）時也回 None ——
/// TS 會照樣把它當 canonical 硬讀；這裡寧可當成「沒有辨識結果」。
pub fn as_canonical_document(ocr_raw: &Value) -> Option<CanonicalDocument> {
    if !looks_canonical(ocr_raw) {
        return None;
    }
    serde_json::from_value(ocr_raw.clone()).ok()
}

pub fn as_canonical_record(ocr_raw: &Value) -> Option<CanonicalRecord> {
    match as_canonical_document(ocr_raw)? {
        CanonicalDocument::Succeeded(s) => Some(s.record),
        CanonicalDocument::Failed(_) => None,
    }
}

/// 版型：先看 record 宣告的，`unknown`／沒有時再看同名欄位（parser 也會把它當欄位給）。
pub fn template_id_of(record: &CanonicalRecord) -> Option<String> {
    if let Some(declared) = record.template_id.as_deref() {
        if !declared.is_empty() && declared != TEMPLATE_UNKNOWN {
            return Some(declared.to_string());
        }
    }
    text_of(record, "templateId").filter(|s| s != TEMPLATE_UNKNOWN)
}

/// 版型不等於單別：同一版型底下，後續作業不同的單要分開（看紙上真的印了什麼）。
///
/// | 版型 | 判別 | 單別 |
/// |---|---|---|
/// | `renewable_purchase` | 購電電費或度數 > 0 | `renewableSurplus` |
/// | `renewable_purchase` | 兩者都抽到但不 > 0 | `renewableNoSurplus` |
/// | `high_voltage` | 沒有電器租費那一列 | `consumptionDevice` |
/// | `high_voltage` | 電器租費 ≠ 0 | `consumptionDeviceRental` |
pub fn doc_type_from_record(record: &CanonicalRecord) -> Option<EnergyDocType> {
    let template_id = template_id_of(record)?;

    if template_id == TEMPLATE_HIGH_VOLTAGE {
        // 抽不到就是沒有那一列（另一張範本連欄位都不存在）。
        let rental = number_of(record, "meterRentalFee");
        return Some(if rental.is_some_and(|r| r != 0.0) {
            EnergyDocType::ConsumptionDeviceRental
        } else {
            EnergyDocType::ConsumptionDevice
        });
    }

    // 躉購：有沒有賣出去的電。（不認得的版型也走這段，與 TS 相同。）
    let fee = number_of(record, "purchaseEnergyFee");
    let kwh = number_of(record, "purchaseKwh");
    if fee.is_some_and(|f| f > 0.0) || kwh.is_some_and(|k| k > 0.0) {
        return Some(EnergyDocType::RenewableSurplus);
    }
    // 兩個都抽不到時不要猜「無餘電」：那會把一張躉購單送進付款流程。
    if fee.is_none() && kwh.is_none() {
        return template_doc_type(&template_id);
    }
    Some(EnergyDocType::RenewableNoSurplus)
}

fn num_or_null(v: Option<f64>) -> Value {
    v.and_then(serde_json::Number::from_f64).map(Value::Number).unwrap_or(Value::Null)
}

/// 分類器：版型就是答案。`confidence` 一律 1 或 0 —— 給 0.73 只會讓人以為那是機率。
pub fn taipower_classifier(ocr_raw: &Value) -> ClassifyResult {
    let Some(doc) = as_canonical_document(ocr_raw) else {
        return ClassifyResult::unclassified("沒有辨識結果可判讀".into(), None);
    };
    let doc = match doc {
        CanonicalDocument::Failed(f) => {
            return ClassifyResult::unclassified(
                format!("辨識失敗：{}", f.error.message),
                Some(json!({ "errorCode": f.error.code })),
            );
        }
        CanonicalDocument::Succeeded(s) => s,
    };

    let template_id = template_id_of(&doc.record);
    let doc_type = doc_type_from_record(&doc.record);
    let (Some(template_id), Some(doc_type)) = (template_id.clone(), doc_type) else {
        return ClassifyResult::unclassified(
            "辨識服務認不出這是哪一種單".into(),
            Some(json!({ "templateId": template_id })),
        );
    };

    let blocking: Vec<&str> = blocking_issues(&doc.issues).iter().map(|i| i.code.as_str()).collect();
    let mut signals = json!({
        "templateId": template_id,
        // 判別單別時看的兩個數字 —— 判錯時人要看得出它憑什麼那樣判。
        "purchaseEnergyFee": num_or_null(number_of(&doc.record, "purchaseEnergyFee")),
        "meterRentalFee": num_or_null(number_of(&doc.record, "meterRentalFee")),
        "blockingIssues": blocking,
    });
    // TS 沒有這個鍵時 JSON 裡就不出現（undefined 不序列化），不印成 null。
    if let Some(m) = doc.record.extraction_method.as_ref() {
        signals["extractionMethod"] = json!(m.as_str());
    }
    ClassifyResult {
        doc_type,
        // 這裡回的是預設流程；真正要開哪些由綁定決定。
        workflow_key: doc_type_to_workflow(doc_type).map(str::to_string),
        confidence: 1.0,
        reason: format!("辨識為「{}」", doc_type_label(doc_type)),
        signals: Some(signals),
    }
}

/// 從 canonical record 依 `BILL_DETAIL_FIELDS` 表撈出 19 個明細欄位（表驅動，不逐欄手寫）。
/// `contractNumberRaw` 也走這張表：`ExtractedBill.contract_number_raw` 是另一個具名欄位（只用在
/// `ocr_extracted` 序列化與長度檢查），跟這裡寫進 `detail`／`detail_fields` 的鍵不衝突，兩邊同一個來源值，
/// 不會互相蓋掉（REVIEW-2.md：S3 要明細區看得到契約編號的辨識值，漏掉算缺陷）。
fn detail_of(record: &CanonicalRecord) -> Map<String, Value> {
    let mut out = Map::new();
    for f in BILL_DETAIL_FIELDS {
        let v = match f.kind {
            DetailFieldKind::Text => text_of(record, f.canonical_key).map(Value::from),
            DetailFieldKind::Date => date_of(record, f.canonical_key).map(Value::from),
            DetailFieldKind::Num => number_of(record, f.canonical_key).and_then(serde_json::Number::from_f64).map(Value::Number),
        };
        if let Some(v) = v {
            out.insert(f.camel.to_string(), v);
        }
    }
    out
}

/// 抽取器：canonical → `ExtractedBill`。
///
/// `totalAmount` 依單別取不同欄位：躉購取購電電費（未稅售電收入，分潤基數仍待客戶確認），
/// 高壓用電取應繳總金額。三個候選金額都還在 `ocrRaw`，答案回來時不必重跑辨識。
pub fn taipower_bill_extractor(ocr_json: &Value) -> Vec<ExtractedBill> {
    let Some(record) = as_canonical_record(ocr_json) else {
        return vec![];
    };
    let template_id = template_id_of(&record);
    let is_consumption = template_id.as_deref() == Some(TEMPLATE_HIGH_VOLTAGE);

    let out = ExtractedBill {
        customer_number: text_of(&record, "accountNumber"),
        billing_period_start: date_of(&record, "billingStartDate"),
        billing_period_end: date_of(&record, "billingEndDate"),
        total_amount: number_of(&record, if is_consumption { "payableTotal" } else { "purchaseEnergyFee" }),
        generation_kwh: number_of(&record, if is_consumption { "consumptionKwh" } else { "purchaseKwh" }),
        // 兩種單都沒有台電開單日；不拿通知單年月造一個假日期。
        issued_at: None,
        // 躉購新版單沒有單號（服務回 not_applicable）—— 不自己編一個。
        document_no: text_of(&record, "billNumber"),
        template_id,
        due_date: date_of(&record, "dueDate"),
        contract_number_raw: text_of(&record, "contractNumber"),
        confidence: None,
        detail: detail_of(&record),
    };
    if out.is_empty() {
        vec![]
    } else {
        vec![out]
    }
}

/// 欄位的值怎麼存（DB 型別，驅動 migration／抽取／比對三邊，不逐欄手寫 match）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailFieldKind {
    Text,
    Date,
    Num,
}

/// 電費單明細一欄：我們的欄位名（camel）、DB 欄名（snake，對應 0104 的欄）、辨識鍵、型別。
#[derive(Debug, Clone, Copy)]
pub struct DetailField {
    pub camel: &'static str,
    pub snake: &'static str,
    pub canonical_key: &'static str,
    pub kind: DetailFieldKind,
}

/// 卡 B：電費單明細 19 欄（整併版「資料庫欄位字典」DB-088～DB-107 扣主鍵，加「契約編號」「區處」）。
/// 這張表驅動：0104 migration 的欄名、抽取時從 canonical 讀哪個鍵、校對台顯示與比對 —— 都讀它，不逐欄手寫。
pub const BILL_DETAIL_FIELDS: &[DetailField] = &[
    DetailField { camel: "noticeYearMonth", snake: "notice_year_month", canonical_key: "noticeYearMonth", kind: DetailFieldKind::Text },
    DetailField { camel: "currentReadingDate", snake: "current_reading_date", canonical_key: "currentReadingDate", kind: DetailFieldKind::Date },
    DetailField { camel: "nextReadingDate", snake: "next_reading_date", canonical_key: "nextReadingDate", kind: DetailFieldKind::Date },
    DetailField { camel: "purchaseKwh", snake: "purchase_kwh", canonical_key: "purchaseKwh", kind: DetailFieldKind::Num },
    DetailField { camel: "consumptionKwh", snake: "consumption_kwh", canonical_key: "consumptionKwh", kind: DetailFieldKind::Num },
    DetailField { camel: "purchaseEnergyFee", snake: "purchase_energy_fee", canonical_key: "purchaseEnergyFee", kind: DetailFieldKind::Num },
    DetailField { camel: "businessTax", snake: "business_tax", canonical_key: "businessTax", kind: DetailFieldKind::Num },
    DetailField { camel: "electricityFeeTotal", snake: "electricity_fee_total", canonical_key: "electricityFeeTotal", kind: DetailFieldKind::Num },
    DetailField { camel: "meterRentalFee", snake: "meter_rental_fee", canonical_key: "meterRentalFee", kind: DetailFieldKind::Num },
    DetailField { camel: "usageEnergyFee", snake: "usage_energy_fee", canonical_key: "usageEnergyFee", kind: DetailFieldKind::Num },
    DetailField { camel: "baseEnergyFee", snake: "base_energy_fee", canonical_key: "baseEnergyFee", kind: DetailFieldKind::Num },
    DetailField { camel: "flowEnergyFee", snake: "flow_energy_fee", canonical_key: "flowEnergyFee", kind: DetailFieldKind::Num },
    DetailField { camel: "powerFactorAdjustmentFee", snake: "power_factor_adjustment_fee", canonical_key: "powerFactorAdjustmentFee", kind: DetailFieldKind::Num },
    DetailField { camel: "electricityAdjustmentAmount", snake: "electricity_adjustment_amount", canonical_key: "electricityAdjustmentAmount", kind: DetailFieldKind::Num },
    DetailField { camel: "meterReadingDiscount", snake: "meter_reading_discount", canonical_key: "meterReadingDiscount", kind: DetailFieldKind::Num },
    DetailField { camel: "taxBeforeAmount", snake: "tax_before_amount", canonical_key: "taxBeforeAmount", kind: DetailFieldKind::Num },
    DetailField { camel: "payableTotal", snake: "payable_total", canonical_key: "payableTotal", kind: DetailFieldKind::Num },
    DetailField { camel: "carrierNumber", snake: "carrier_number", canonical_key: "carrierIdentity", kind: DetailFieldKind::Text },
    DetailField { camel: "invoiceNumber", snake: "invoice_number", canonical_key: "rentalInvoiceNumber", kind: DetailFieldKind::Text },
    DetailField { camel: "contractNumberRaw", snake: "contract_number_raw", canonical_key: "contractNumber", kind: DetailFieldKind::Text },
    DetailField { camel: "serviceDepartment", snake: "service_department", canonical_key: "serviceDepartment", kind: DetailFieldKind::Text },
];

/// 校對台要並排的 canonical 欄位，依版型而異。
pub fn review_fields_for(template_id: Option<&str>) -> &'static [&'static str] {
    match template_id {
        Some("renewable_purchase") => &[
            "accountNumber", "contractNumber", "companyTaxId",
            "billingStartDate", "billingEndDate",
            "purchaseKwh", "purchaseEnergyFee", "businessTax",
            "electricityFeeTotal", "meterRentalSubtotal", "payableTotal",
        ],
        Some("legacy_renewable_purchase") => &[
            "billNumber", "accountNumber", "siteName", "companyTaxId",
            "billingStartDate", "billingEndDate", "contractNumber",
            "purchaseKwh", "purchaseEnergyFee", "businessTax",
            "electricityFeeTotal", "meterRentalSubtotal", "payableTotal",
        ],
        Some("high_voltage") => &[
            "billNumber", "accountNumber", "companyTaxId",
            "billingStartDate", "billingEndDate", "dueDate",
            "consumptionKwh", "businessTax", "taxBeforeAmount", "payableTotal",
        ],
        _ => &[],
    }
}

/// 高壓用電單（我們付台電）看繳費期限。
pub const CONSUMPTION_REVIEW_FIELDS: [&str; 6] = [
    "customerNumberRaw", "billingPeriodStart", "billingPeriodEnd", "totalAmount", "generationKwh", "dueDate",
];

/// 躉購單與所有認不出版型的單。電號排第一個：它決定這張單配到哪座案場、走哪條流程。
pub const DEFAULT_REVIEW_FIELDS: [&str; 6] = [
    "customerNumberRaw", "billingPeriodStart", "billingPeriodEnd", "totalAmount", "generationKwh", "issuedAt",
];

/// 後端寫入與變更標記掃的是兩組的聯集 —— 版型判錯時，改過的欄位才不會靜默地存不進去。
pub const ALL_BILL_REVIEW_FIELDS: [&str; 8] = [
    "customerNumberRaw", "billingPeriodStart", "billingPeriodEnd", "totalAmount", "generationKwh", "issuedAt", "dueDate", "documentNo",
];

/// 欄位名 → `ocrExtracted` 裡的鍵（兩邊不同名是歷史的結果）。
pub fn extracted_key_alias(field: &str) -> &str {
    match field {
        "customerNumberRaw" => "customerNumber",
        other => other,
    }
}

/// 從 `ocrExtracted` 取某個欄位的辨識值（處理上面那張對照表）。
pub fn extracted_value_of<'a>(extracted: &'a Value, field: &str) -> Option<&'a Value> {
    extracted.get(extracted_key_alias(field))
}

/// `bill_doc_type` 要讀的三個欄位（呼叫端從 DB 列取出）。
#[derive(Debug, Clone, Copy, Default)]
pub struct BillDocTypeSource<'a> {
    pub doc_type: Option<&'a str>,
    pub template_id: Option<&'a str>,
    pub ocr_raw: Option<&'a Value>,
}

/// 這張單現在算哪一種（人改過就是人改的那個）。
///
/// 推不出來就回 `electricityBill`（舊資料唯一存在的形態）—— 但不可寫死它，
/// 寫死會讓高壓用電單走進躉購流程，反過來算出要付給地主多少。
pub fn bill_doc_type(bill: BillDocTypeSource<'_>) -> EnergyDocType {
    if let Some(stored) = bill.doc_type.and_then(EnergyDocType::parse) {
        if stored != EnergyDocType::Unclassified {
            return stored;
        }
    }
    recognized_doc_type(bill).unwrap_or(EnergyDocType::ElectricityBill)
}

/// 辨識當初判成哪一種（不看人工修正）。認不出版型時回 None —— 「沒判出來」與「判成躉購」是兩件事。
pub fn recognized_doc_type(bill: BillDocTypeSource<'_>) -> Option<EnergyDocType> {
    if let Some(record) = bill.ocr_raw.and_then(as_canonical_record) {
        if let Some(fine) = doc_type_from_record(&record) {
            return Some(fine);
        }
    }
    template_doc_type(bill.template_id.unwrap_or(""))
}

/// 這張單在校對台上要並排哪幾個我們的欄位。
pub fn bill_review_fields_for(template_id: Option<&str>) -> &'static [&'static str] {
    if template_id == Some(TEMPLATE_HIGH_VOLTAGE) {
        &CONSUMPTION_REVIEW_FIELDS
    } else {
        &DEFAULT_REVIEW_FIELDS
    }
}

/// 由內容決定判讀器：是 canonical 就用版型判，不是才退回規則式關鍵詞判讀。
pub fn energy_document_classifier(ocr_raw: &Value) -> ClassifyResult {
    if looks_canonical(ocr_raw) {
        taipower_classifier(ocr_raw)
    } else {
        mock_classifier(ocr_raw)
    }
}

/// 同上：canonical 走 taipower 映射，其餘走欄位名相容抽取。
pub fn energy_bill_extractor(ocr_json: &Value) -> Vec<ExtractedBill> {
    if looks_canonical(ocr_json) {
        taipower_bill_extractor(ocr_json)
    } else {
        mock_bill_extractor(ocr_json)
    }
}

/// 我們的欄位名 → canonical 的欄位名。只有一份：抽取、校對台高亮、差異提示都讀它。
pub fn canonical_key_for(field: &str, template_id: Option<&str>) -> Option<&'static str> {
    let is_consumption = template_id == Some(TEMPLATE_HIGH_VOLTAGE);
    match field {
        // 兩個名字都要認：只認一個的話，點「電號」那一列不會有任何反應。
        "customerNumber" | "customerNumberRaw" => Some("accountNumber"),
        "billingPeriodStart" => Some("billingStartDate"),
        "billingPeriodEnd" => Some("billingEndDate"),
        "totalAmount" => Some(if is_consumption { "payableTotal" } else { "purchaseEnergyFee" }),
        "generationKwh" => Some(if is_consumption { "consumptionKwh" } else { "purchaseKwh" }),
        "documentNo" => Some("billNumber"),
        "dueDate" => Some("dueDate"),
        "contractNumberRaw" => Some("contractNumber"),
        // 兩種單都沒有台電開單日 —— 不要對到一個看起來很像的欄位。
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceFieldBox {
    pub field: String,
    /// 0 起算的頁索引。
    pub page: i64,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// 欄位在紙上的位置。沒有座標就不回 —— 指錯位置比沒有位置更糟。
pub fn source_field_boxes(ocr_raw: &Value, fields: &[&str]) -> Vec<SourceFieldBox> {
    let Some(record) = as_canonical_record(ocr_raw) else {
        return vec![];
    };
    let template_id = template_id_of(&record);
    let mut out = Vec::new();
    for field in fields {
        let Some(key) = canonical_key_for(field, template_id.as_deref()) else {
            continue;
        };
        let Some(ev) = record.fields.get(key).and_then(|f| f.evidence.as_ref()) else {
            continue;
        };
        let Some(b) = ev.bbox.as_ref() else {
            continue;
        };
        out.push(SourceFieldBox {
            field: field.to_string(),
            page: (ev.page_number.unwrap_or(1) - 1).max(0),
            x: b.x,
            y: b.y,
            width: b.width,
            height: b.height,
        });
    }
    out
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecognitionIssueView {
    pub code: String,
    pub message: String,
    pub fields: Vec<String>,
    pub blocking: bool,
}

/// 「這份辨識結果可不可信」的一句話所需的資料。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecognitionSummary {
    pub template_id: Option<String>,
    pub template_label: Option<String>,
    pub extraction_method: Option<String>,
    /// 服務自己列出來的問題。blocking ＝ 核心欄位有問題，不該就這樣送審。
    pub issues: Vec<RecognitionIssueView>,
    pub blocking_count: usize,
    /// 每個欄位的值是怎麼被決定的，按 `resolution` 計數（三來源全跑唯一被人看見的地方）。
    pub resolutions: BTreeMap<String, u64>,
    /// 辨識失敗時的錯誤碼（例如 `password_required`）。
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

/// 從 `ocrRaw` 整理出畫面要的那幾件事。不看 `reviewStatus`，判準是 blocking issue。
pub fn recognition_summary(ocr_raw: &Value) -> Option<RecognitionSummary> {
    let doc = match as_canonical_document(ocr_raw)? {
        CanonicalDocument::Failed(f) => {
            return Some(RecognitionSummary {
                template_id: None,
                template_label: None,
                extraction_method: None,
                issues: vec![],
                blocking_count: 0,
                resolutions: BTreeMap::new(),
                error_code: Some(f.error.code),
                error_message: Some(f.error.message),
            });
        }
        CanonicalDocument::Succeeded(s) => s,
    };
    let template_id = template_id_of(&doc.record);
    let mut resolutions = BTreeMap::new();
    for f in doc.record.fields.values() {
        // 沒有 evidence 的欄位不計 —— 它們不是「某個來源看到的值」，混進來會讓比例失真。
        let Some(r) = f.evidence.as_ref().and_then(|e| e.resolution.as_ref()) else {
            continue;
        };
        if r.as_str().is_empty() {
            continue;
        }
        *resolutions.entry(r.as_str().to_string()).or_insert(0) += 1;
    }
    let issues = doc
        .issues
        .iter()
        .map(|i| RecognitionIssueView {
            code: i.code.clone(),
            message: i.message.clone(),
            fields: i.fields.clone(),
            blocking: i.blocking,
        })
        .collect();
    Some(RecognitionSummary {
        template_label: template_id.as_deref().and_then(template_label).map(str::to_string),
        template_id,
        extraction_method: doc.record.extraction_method.as_ref().map(|m| m.as_str().to_string()),
        issues,
        blocking_count: blocking_issues(&doc.issues).len(),
        resolutions,
        error_code: None,
        error_message: None,
    })
}
