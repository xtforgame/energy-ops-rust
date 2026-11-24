//! 移植自 `tests/taipowerExtractor.test.ts` 與 `tests/billFieldBoxes.test.ts`。
//! fixture 是真的辨識結果（`tests/fixtures/taipower-canonical.json` 由 TS fixture 原樣轉出）。

use eo_domain::bill_extractor::ExtractedBill;
use eo_domain::business_key::bill_business_key;
use eo_domain::document_ingest::{doc_type_to_workflow, EnergyDocType, BILL_DOC_TYPES};
use eo_domain::taipower_canonical::{CanonicalDocument, CanonicalValue};
use eo_domain::taipower_extractor::*;
use serde_json::{json, Value};

const FIXTURE: &str = include_str!("fixtures/taipower-canonical.json");
const SAMPLES: &str = include_str!("../../../tests/fixtures/canonical-samples.json");

fn fixture(name: &str) -> Value {
    let all: Value = serde_json::from_str(FIXTURE).expect("fixture 是合法 JSON");
    all[name].clone()
}

fn renewable() -> Value {
    fixture("RENEWABLE_DOC")
}

fn high_voltage() -> Value {
    fixture("HIGH_VOLTAGE_DOC")
}

fn one_bill(doc: &Value) -> ExtractedBill {
    let bills = taipower_bill_extractor(doc);
    assert_eq!(bills.len(), 1);
    bills.into_iter().next().unwrap()
}

#[test]
fn high_voltage_discount_and_pre_tax_total_are_mapped_to_platform_details() {
    let mut doc=high_voltage();
    doc["record"]["fields"]["meterReadingDiscount"]=json!({"value":-10,"status":"extracted"});
    doc["record"]["fields"]["taxBeforeAmount"]=json!({"value":4795,"status":"extracted"});
    let b=one_bill(&doc);
    let v=serde_json::to_value(b).unwrap();
    assert_eq!(v["meterReadingDiscount"].as_f64(),Some(-10.0));
    assert_eq!(v["taxBeforeAmount"].as_f64(),Some(4795.0));
}

/// 來源：taipowerExtractor.test.ts「躉購·有餘電 → renewableSurplus → bill-approval」
#[test]
fn renewable_surplus_goes_to_bill_approval() {
    let r = taipower_classifier(&renewable());
    assert_eq!(r.doc_type, EnergyDocType::RenewableSurplus);
    assert_eq!(r.workflow_key.as_deref(), Some("bill-approval"));
    assert_eq!(r.confidence, 1.0);
    assert!(r.reason.contains("躉購"));
}

/// 來源：taipowerExtractor.test.ts「躉購·**無餘電** → renewableNoSurplus → 付款流程（不是躉購核可流程）」
#[test]
fn renewable_no_surplus_goes_to_payment_flow() {
    let mut doc = renewable();
    doc["record"]["fields"]["purchaseEnergyFee"]["value"] = json!(0);
    doc["record"]["fields"]["purchaseKwh"]["value"] = json!(0);
    let r = taipower_classifier(&doc);
    assert_eq!(r.doc_type, EnergyDocType::RenewableNoSurplus);
    assert_eq!(r.workflow_key.as_deref(), Some("utility-bill"), "錢的方向是我們付出去 → 不可以走躉購核可流程");
}

/// 來源：taipowerExtractor.test.ts「逆變器·僅設備發電 → consumptionDevice → utility-bill（錢的方向相反）」
#[test]
fn high_voltage_device_goes_to_utility_bill() {
    let r = taipower_classifier(&high_voltage());
    assert_eq!(r.doc_type, EnergyDocType::ConsumptionDevice);
    assert_eq!(r.workflow_key.as_deref(), Some("utility-bill"));
}

/// 來源：taipowerExtractor.test.ts「逆變器·**含電器租費** → consumptionDeviceRental（多了那一列就是另一種單）」
#[test]
fn high_voltage_with_rental_row() {
    let mut doc = high_voltage();
    doc["record"]["fields"]["meterRentalFee"] = json!({
        "value": 1549, "rawValue": "1549.0", "status": "extracted",
        "evidence": { "pageNumber": 1, "region": "pdf_text", "rawText": "1549.0" },
    });
    let r = taipower_classifier(&doc);
    assert_eq!(r.doc_type, EnergyDocType::ConsumptionDeviceRental);
    // 判別依據要看得到 —— 判錯時人得知道它憑什麼那樣判。
    assert_eq!(r.signals.unwrap()["meterRentalFee"], json!(1549.0));
}

/// 來源：taipowerExtractor.test.ts「每一個認得的版型都對應到一條真的流程」
#[test]
fn every_known_template_maps_to_a_flow() {
    for t in KNOWN_TEMPLATES {
        let doc_type = template_doc_type(t).expect(t);
        assert!(doc_type_to_workflow(doc_type).is_some(), "{t}");
    }
}

/// 來源：taipowerExtractor.test.ts「細分單別**都**有預設流程（新增一種就要回來補）」。
/// 2026-11-23 加 `RetailerSale`（三家售電業的購售電單）：4 → 5，`doc_type_to_workflow` 回
/// `Some("bill-approval")`，這裡就是回來補的那一筆。
#[test]
fn every_fine_doc_type_has_a_default_flow() {
    for t in BILL_DOC_TYPES {
        assert!(doc_type_to_workflow(t).is_some(), "{t:?}");
    }
    assert_eq!(BILL_DOC_TYPES.len(), 5);
}

/// 來源：taipowerExtractor.test.ts「認不出版型 → unclassified，不 fallback 成電費單」
#[test]
fn unknown_template_is_unclassified() {
    let mut doc = renewable();
    doc["record"]["templateId"] = json!("unknown");
    // 版型欄位也要一起清掉，否則會從欄位那一側讀回來。
    doc["record"]["fields"].as_object_mut().unwrap().remove("templateId");
    let r = taipower_classifier(&doc);
    assert_eq!(r.doc_type, EnergyDocType::Unclassified);
    assert_eq!(r.workflow_key, None);
}

/// 來源：taipowerExtractor.test.ts「辨識失敗的文件 → unclassified，且理由帶得出服務給的訊息」
#[test]
fn failed_document_keeps_the_service_message() {
    let r = taipower_classifier(&json!({
        "sourceFileId": "X", "sourceFileName": "x.pdf", "status": "failed",
        "error": { "code": "password_required", "message": "PDF 需要密碼才能開啟。" },
    }));
    assert_eq!(r.doc_type, EnergyDocType::Unclassified);
    assert!(r.reason.contains("需要密碼"));
    assert_eq!(r.signals.unwrap()["errorCode"], json!("password_required"));
}

/// 來源：taipowerExtractor.test.ts「不是 canonical 的東西 → unclassified（不炸）」
#[test]
fn junk_is_unclassified() {
    for junk in [Value::Null, json!("x"), json!(42), json!({}), json!({ "bills": [] })] {
        assert_eq!(taipower_classifier(&junk).doc_type, EnergyDocType::Unclassified, "{junk}");
    }
}

/// 來源：taipowerExtractor.test.ts「電號、期間、契約編號都照原單」
#[test]
fn renewable_identity_fields() {
    let bill = one_bill(&renewable());
    assert_eq!(bill.customer_number.as_deref(), Some("00-60-1600-79-5"));
    assert_eq!(bill.billing_period_start.as_deref(), Some("2026-06-09"));
    assert_eq!(bill.billing_period_end.as_deref(), Some("2026-08-10"));
    assert_eq!(bill.contract_number_raw.as_deref(), Some("00-PV-106-0014"));
    assert_eq!(bill.template_id.as_deref(), Some("renewable_purchase"));
}

/// 來源：taipowerExtractor.test.ts「totalAmount ＝ 購電電費（未稅售電收入），generationKwh ＝ 購電度數」
#[test]
fn renewable_amount_and_kwh() {
    let bill = one_bill(&renewable());
    assert_eq!(bill.total_amount, Some(127848.0));
    assert_eq!(bill.generation_kwh, Some(21072.0));
}

/// 來源：taipowerExtractor.test.ts「新版躉購單沒有單號 → documentNo 留空，不自己編一個」
#[test]
fn renewable_has_no_document_no() {
    assert_eq!(one_bill(&renewable()).document_no, None);
}

/// 來源：taipowerExtractor.test.ts「沒有台電開單日這個欄位 → issuedAt 留空，不拿通知單年月造一個假日期」
#[test]
fn renewable_has_no_issued_at() {
    assert_eq!(one_bill(&renewable()).issued_at, None);
}

/// 來源：taipowerExtractor.test.ts「抽出來的值算得出 businessKey（送審去重靠它）」
#[test]
fn extracted_values_make_a_business_key() {
    let bill = one_bill(&renewable());
    let key = bill_business_key(
        bill.customer_number.as_deref(),
        bill.billing_period_start.as_deref(),
        bill.billing_period_end.as_deref(),
    );
    assert_eq!(key.as_deref(), Some("bill|00601600795|2026-06-09|2026-08-10"));
}

/// 來源：taipowerExtractor.test.ts「totalAmount ＝ 應繳總金額、generationKwh ＝ 用電度數（與躉購單取不同欄位）」
#[test]
fn high_voltage_amount_and_kwh() {
    let bill = one_bill(&high_voltage());
    assert_eq!(bill.total_amount, Some(5035.0));
    assert_eq!(bill.generation_kwh, Some(800.0));
}

/// 來源：taipowerExtractor.test.ts「有通知單號碼與繳費期限（會計依繳費期限排序）」
#[test]
fn high_voltage_document_no_and_due_date() {
    let bill = one_bill(&high_voltage());
    assert_eq!(bill.document_no.as_deref(), Some("E-g9115080101559"));
    assert_eq!(bill.due_date.as_deref(), Some("2026-08-20"));
}

/// 來源：taipowerExtractor.test.ts「高壓單沒有契約編號 → 留空」
#[test]
fn high_voltage_has_no_contract_number() {
    assert_eq!(one_bill(&high_voltage()).contract_number_raw, None);
}

/// 來源：taipowerExtractor.test.ts「不是 canonical → 回空陣列（不產半成品單）」
#[test]
fn non_canonical_extracts_nothing() {
    assert!(taipower_bill_extractor(&json!({ "bills": [{ "customerNumber": "x" }] })).is_empty());
    assert!(taipower_bill_extractor(&Value::Null).is_empty());
}

/// 來源：taipowerExtractor.test.ts「「無餘電」那種**負數**金額抽得出來（金額必為正是錯的假設）」
#[test]
fn negative_amount_is_extracted() {
    let mut doc = renewable();
    doc["record"]["fields"]["purchaseEnergyFee"]["value"] = json!(-132);
    assert_eq!(one_bill(&doc).total_amount, Some(-132.0));
}

/// 來源：taipowerExtractor.test.ts「asCanonicalRecord 對失敗的文件回 null」
#[test]
fn failed_document_has_no_record() {
    assert_eq!(as_canonical_record(&json!({ "status": "failed", "error": { "code": "x", "message": "y" } })), None);
}

fn field_number(doc: &Value, key: &str) -> Option<f64> {
    let rec = as_canonical_record(doc)?;
    match rec.fields.get(key)?.value.as_ref()? {
        CanonicalValue::Number(n) => n.as_f64(),
        CanonicalValue::Text(_) => None,
    }
}

/// 來源：taipowerExtractor.test.ts「現況：分潤基數 ＝ 購電電費（未稅），不是應繳總金額」
#[test]
fn revenue_share_base_is_pre_tax_purchase_fee() {
    let bill = one_bill(&renewable());
    assert_eq!(bill.total_amount, Some(127848.0), "未稅售電收入");
    let payable = field_number(&renewable(), "payableTotal");
    assert_eq!(payable, Some(133744.0), "應繳總金額");
    assert_ne!(bill.total_amount, payable);
}

/// 來源：taipowerExtractor.test.ts「三個候選金額都還留在 ocrRaw 裡（答案來了舊單重算得回來）」
#[test]
fn three_candidate_amounts_stay_in_ocr_raw() {
    for k in ["purchaseEnergyFee", "businessTax", "payableTotal"] {
        assert!(field_number(&renewable(), k).is_some(), "{k}");
    }
}

/// 來源：taipowerExtractor.test.ts「躉購與高壓是兩組不同的核心欄位」
#[test]
fn review_fields_differ_by_template() {
    let renewable = review_fields_for(Some("renewable_purchase"));
    let high = review_fields_for(Some("high_voltage"));
    assert!(renewable.contains(&"purchaseKwh"));
    assert!(!renewable.contains(&"dueDate"));
    assert!(high.contains(&"dueDate"));
    assert!(!high.contains(&"purchaseKwh"));
}

/// 來源：taipowerExtractor.test.ts「不認得的版型 → 空陣列（畫面自己決定要顯示什麼，不亂給一組）」
#[test]
fn unknown_template_has_no_review_fields() {
    assert!(review_fields_for(None).is_empty());
    assert!(review_fields_for(Some("nope")).is_empty());
}

// ---------------------------------------------------------------------------
// 真範本（canonical-samples.json）
// ---------------------------------------------------------------------------

fn samples() -> Value {
    serde_json::from_str(SAMPLES).expect("canonical-samples.json 是合法 JSON")
}

/// 來源：billFieldBoxes.test.ts「四份範本都在（少一份就是這條測試在空跑）」
#[test]
fn four_samples_present() {
    assert_eq!(samples()["documents"].as_object().unwrap().len(), 4);
}

/// 來源：billFieldBoxes.test.ts「${fileName}：每個校對欄位都有框」
#[test]
fn every_review_field_has_a_box() {
    for (file_name, doc) in samples()["documents"].as_object().unwrap() {
        let template_id = doc["record"]["fields"]["templateId"]["value"].as_str().unwrap_or("").to_string();
        assert_ne!(template_id, "", "{file_name}：這份範本沒有判出版型");

        // `issuedAt` 兩種單都沒有 —— 不是漏，是紙上就沒有。
        let review: Vec<&str> = bill_review_fields_for(Some(&template_id))
            .iter()
            .copied()
            .filter(|f| canonical_key_for(f, Some(&template_id)).is_some())
            .collect();
        assert!(!review.is_empty(), "{file_name}：校對欄位一個都沒有");

        let boxed: Vec<String> = source_field_boxes(doc, &review).into_iter().map(|b| b.field).collect();
        let missing: Vec<&&str> = review.iter().filter(|f| !boxed.iter().any(|b| b == **f)).collect();
        assert!(missing.is_empty(), "{file_name}：這些欄位認出來了卻標不到位置：{missing:?}");
    }
}

/// canonical 型別要能完整讀進四份真範本，且來回序列化不掉欄位。
/// 來源：無對應 TS 測試；補驗 taipowerCanonical.ts 的型別對 canonical-samples.json。
#[test]
fn canonical_types_read_the_real_samples() {
    for (file_name, doc) in samples()["documents"].as_object().unwrap() {
        let typed: CanonicalDocument = serde_json::from_value(doc.clone()).unwrap_or_else(|e| panic!("{file_name}: {e}"));
        let CanonicalDocument::Succeeded(s) = &typed else {
            panic!("{file_name} 應為 succeeded");
        };
        assert_eq!(s.record.fields.len(), 119, "{file_name}");
        let back = serde_json::to_value(&typed).unwrap();
        assert_eq!(&back, doc, "{file_name}：來回序列化要一模一樣");
    }
}

/// 四份真範本各自判成哪一種單（版型不等於單別）。
/// 來源：無對應 TS 測試；補驗 taipowerExtractor.ts 的 docTypeFromRecord／recognitionSummary 對真範本。
#[test]
fn real_samples_classify_into_four_doc_types() {
    let expect = [
        ("逆變器電費單(僅設備發電使用電費).pdf", EnergyDocType::ConsumptionDevice),
        ("逆變器電費單(含電表租費&設備發電使用電費).pdf", EnergyDocType::ConsumptionDeviceRental),
        ("電費單(有餘電需開發票).pdf", EnergyDocType::RenewableSurplus),
        ("電費單(無餘電需付款).pdf", EnergyDocType::RenewableNoSurplus),
    ];
    let s = samples();
    for (name, doc_type) in expect {
        let doc = &s["documents"][name];
        assert_eq!(energy_document_classifier(doc).doc_type, doc_type, "{name}");
        let summary = recognition_summary(doc).expect(name);
        assert!(summary.resolutions.values().sum::<u64>() > 0, "{name}");
        assert!(summary.template_label.is_some(), "{name}");
    }
}
