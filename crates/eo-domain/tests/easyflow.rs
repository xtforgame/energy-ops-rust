use eo_domain::easyflow::{build_application, application_csv};
use serde_json::{json, Value};

#[test]
fn canonical_amounts_fill_absent_details_but_never_replace_explicit_corrections() {
    let bill=json!({"docType":"renewableSurplus","detailFields":{"businessTax":null},"ocrRaw":{"record":{"fields":{
        "purchaseEnergyFee":{"value":5220,"status":"extracted"},
        "businessTax":{"value":261,"status":"extracted"},
        "electricityFeeTotal":{"value":5481,"status":"extracted"},
        "payableTotal":{"value":5228,"status":"ocr_extracted"}
    }}}});
    let p=build_application(&bill,&json!({}),&json!([]));
    assert_eq!(value(&p,"purchaseEnergyFee"),"5220");
    assert_eq!(value(&p,"electricityFeeTotal"),"5481");
    assert_eq!(value(&p,"payableTotal"),"5228");
    assert_eq!(value(&p,"businessTax"),Value::Null);
}

#[test]
fn handoff_requires_operator_fields_but_invoice_does_not_require_payment_options() {
    use eo_domain::easyflow::missing_handoff_fields;
    let mut p=json!({"applicationType":"invoice","fields":[],"attachments":[{"id":1}]});
    assert!(missing_handoff_fields(&p).contains(&"applicant".to_string()));
    let fields=["siteName","companyName","counterpartyName","companyCode","applicant","department","applicationDate","summary","amount","expectedReceiptDate"];
    p["fields"]=json!(fields.map(|k|json!({"key":k,"value":if k=="applicationDate"||k=="expectedReceiptDate"{"2026-11-24"}else{"test"}})));
    assert!(missing_handoff_fields(&p).is_empty());
    p["applicationType"]=json!("payment");
    assert!(missing_handoff_fields(&p).contains(&"paymentMethod".to_string()));
}

fn value(p: &Value, key: &str) -> Value {
    p["fields"].as_array().unwrap().iter().find(|f| f["key"] == key).unwrap()["value"].clone()
}

#[test]
fn invoice_uses_untaxed_income_not_net_payable_and_preserves_dates() {
    let bill = json!({"id": 42, "docType":"renewableSurplus", "customerNumberRaw":"11-72-8007-94-7",
        "billingPeriodStart":"2026-09-01", "billingPeriodEnd":"2026-09-30", "totalAmount":"16735",
        "detailFields":{"purchaseEnergyFee":"16874", "businessTax":"844", "electricityFeeTotal":"17718",
          "meterRentalFee":"-983", "currentReadingDate":"2026-10-01", "nextReadingDate":"2026-11-01"}});
    let station = json!({"projectCode":"32172", "name":"測試案場", "companyName":"測試公司", "counterpartyName":"台電測試區處", "counterpartyTaxId":"12345678"});
    let p = build_application(&bill, &station, &json!([]));
    assert_eq!(p["applicationType"], "invoice");
    assert_eq!(value(&p,"amount"), "16874");
    assert_eq!(value(&p,"siteCode"), "32172");
    assert_eq!(value(&p,"billingPeriodEnd"), "2026-09-30");
    assert_eq!(value(&p,"currentReadingDate"), "2026-10-01");
    assert_eq!(value(&p,"meterRentalFee"), "-983");
    assert_eq!(value(&p,"summary"), "測試案場發電收入");
    assert_eq!(p["submissionAllowed"], false);
    assert!(p["pending"].as_array().unwrap().iter().any(|x| x["key"] == "ledgerPeriodConvention"));
}

#[test]
fn payment_exports_positive_total_without_guessing_missing_or_unknown_fields() {
    let p = build_application(&json!({"id":1,"docType":"renewableNoSurplus","totalAmount":"-983",
      "detailFields":{"purchaseEnergyFee":0,"meterRentalFee":"-983"}}), &json!(null), &json!([]));
    assert_eq!(p["applicationType"], "payment");
    assert_eq!(value(&p,"amount"), "983");
    assert_eq!(value(&p,"purchaseEnergyFee"), "0");
    assert_eq!(value(&p,"companyName"), Value::Null);
    assert_eq!(value(&p,"paymentMethod"), Value::Null);
    let unknown = build_application(&json!({"docType":"unclassified", "totalAmount":"100"}), &json!(null), &json!([]));
    assert_eq!(unknown["applicationType"], "unconfirmed");
    assert_eq!(value(&unknown,"amount"), Value::Null);
}

#[test]
fn invalid_amounts_do_not_become_zero_and_csv_does_not_execute_formulas() {
    let p = build_application(&json!({"docType":"renewableSurplus","detailFields":{"purchaseEnergyFee":"oops"}}),
      &json!({"name":"=HYPERLINK(\"bad\")"}), &json!([]));
    assert_eq!(value(&p,"amount"), Value::Null);
    let csv = application_csv(&p);
    assert!(csv.starts_with('\u{feff}'));
    assert!(csv.contains("'=HYPERLINK"));
    assert!(csv.contains("待確認"));
}

#[test]
fn payment_line_items_must_reconcile_and_configured_options_have_provenance() {
    let p = build_application(&json!({"docType":"consumptionDeviceRental","totalAmount":"250",
      "detailFields":{"meterRentalFee":"163","usageEnergyFee":"87"}}),
      &json!({"easyflowDefaults":{"companyCode":"002","paymentMethod":"匯款"}}), &json!([]));
    assert_eq!(p["lineItems"][0]["amount"],"163");
    assert_eq!(p["lineItems"][1]["amount"],"87");
    assert_eq!(p["lineItemsReconciled"],true);
    assert_eq!(value(&p,"companyCode"),"002");
    let partial = build_application(&json!({"docType":"consumptionDeviceRental","totalAmount":"250",
      "detailFields":{"meterRentalFee":"163"}}), &json!({}), &json!([]));
    assert_eq!(partial["lineItemsReconciled"],false);
}

#[test]
fn canonical_rental_subtotal_and_net_payable_keep_their_actual_meaning() {
    let bill=json!({"docType":"renewableSurplus","totalAmount":"16874",
      "detailFields":{"purchaseEnergyFee":16874,"payableTotal":16735},
      "ocrRaw":{"record":{"fields":{"meterRentalSubtotal":{"value":-983,"status":"ocr_extracted"}}}}});
    let p=build_application(&bill,&json!({}),&json!([]));
    assert_eq!(value(&p,"meterRentalFee"),"-983");
    assert_eq!(value(&p,"payableTotal"),"16735");
    assert_eq!(value(&p,"amount"),"16874");
    let p=build_application(&json!({"docType":"renewableNoSurplus","totalAmount":0,
      "detailFields":{"payableTotal":-250,"meterRentalFee":-163,"usageEnergyFee":-87}}),&json!({}),&json!([]));
    assert_eq!(value(&p,"amount"),"250");
    assert_eq!(p["lineItemsReconciled"],true);
}

#[test]
fn high_voltage_preserves_discounts_and_reconciles_display_precision_without_double_tax() {
    for (detail,discount,net,total,sum,rounding) in [
        (json!({"baseEnergyFee":1285.7,"flowEnergyFee":3720,"powerFactorAdjustmentFee":-45.8,"electricityAdjustmentAmount":85,"businessTax":240}),-10,4795,5035,"5034.9","0.1"),
        (json!({"baseEnergyFee":514.2,"flowEnergyFee":2711.2,"powerFactorAdjustmentFee":-49.2,"electricityAdjustmentAmount":54.8,"meterRentalFee":1549,"businessTax":227}),-10,4543,4770,"4770","0"),
    ] {
        let mut bill=json!({"docType":"consumptionDeviceRental","templateId":"high_voltage","detailFields":detail,
          "ocrRaw":{"record":{"fields":{"meterReadingDiscount":{"value":discount,"status":"extracted"},"taxBeforeAmount":{"value":net,"status":"extracted"}}}}});
        bill["detailFields"]["payableTotal"]=json!(total);
        let p=build_application(&bill,&json!({}),&json!([]));
        assert_eq!(p["lineItemsReconciled"],true);
        assert_eq!(p["reconciliation"]["lineSum"],sum);
        assert_eq!(p["reconciliation"]["roundingDifference"],rounding);
        assert_eq!(p["reconciliation"]["taxBreakdownReconciled"],true);
        assert_eq!(value(&p,"meterReadingDiscount"),"-10");
        assert!(!p["lineItems"].as_array().unwrap().iter().any(|x|x["key"]=="businessTax"));
        assert!(p["lineItems"].as_array().unwrap().iter().any(|x|x["key"]=="meterReadingDiscount"&&x["amount"]=="-10"));
        // Explicit corrections/clears win over old canonical data and do not silently reconcile.
        bill["detailFields"]["meterReadingDiscount"]=Value::Null;
        let cleared=build_application(&bill,&json!({}),&json!([]));
        assert_eq!(value(&cleared,"meterReadingDiscount"),Value::Null);
        assert_eq!(cleared["lineItemsReconciled"],false);
        bill["detailFields"]["meterReadingDiscount"]=json!(-10);
        bill["detailFields"]["businessTax"]=json!(999);
        assert_eq!(build_application(&bill,&json!({}),&json!([]))["lineItemsReconciled"],false);
    }
}
#[test]
fn retailer_application_uses_current_buyer_not_legacy_taipower_recipient() {
 let bill=serde_json::json!({"docType":"retailerSale","detailFields":{"purchaseEnergyFee":"100"},"ocrRaw":{"record":{"fields":{"buyerTaxId":{"value":"12345678"},"buyerName":{"value":"測試購電股份有限公司"}}}}});
 let station=serde_json::json!({"counterpartyTaxId":"99999999","counterpartyName":"舊台電對象"});
 let app=eo_domain::easyflow::build_application(&bill,&station,&serde_json::json!([]));
 let fields=app["fields"].as_array().unwrap();
 assert_eq!(fields.iter().find(|f|f["key"]=="counterpartyTaxId").unwrap()["value"],"12345678");
 assert_eq!(fields.iter().find(|f|f["key"]=="counterpartyName").unwrap()["value"],"測試購電股份有限公司");
}
#[test]
fn ledger_keeps_template_columns_and_does_not_invent_submission_dates() {
 let p=serde_json::json!({"billId":1,"fields":[{"key":"siteName","value":"=danger"},{"key":"meterRentalFee","value":"-983"},{"key":"purchaseEnergyFee","value":"16874"},{"key":"billingPeriodEnd","value":"2026-09-30"},{"key":"currentReadingDate","value":"2026-10-01"}]});
 let csv=eo_domain::easyflow::electricity_ledger_csv(&p);
 assert!(csv.contains("\"'\u{3d}danger\""));assert!(csv.contains("\"983\",\"16874\",\"\""));
 assert!(csv.contains("\"2026-09-30\",\"2026-10-01\""));
}

/// 卡 20261126-0214 S2：companyName 優先序。spv 名字優先；沒帶 spvPayee 的案場才用 `data.easyflowDefaults.companyName` 備援。
#[test]
fn company_name_prefers_spv_then_easyflow_default() {
    let bill = json!({"docType":"renewableSurplus","detailFields":{"purchaseEnergyFee":"100"}});
    // 有 spv 名字：優先用它，即使 easyflowDefaults 也設了一個不同的值。
    let with_spv = build_application(&bill, &json!({"companyName":"綠順科技股份有限公司","easyflowDefaults":{"companyName":"測試填值-備援公司"}}), &json!([]));
    assert_eq!(value(&with_spv,"companyName"), "綠順科技股份有限公司");
    // 沒有 spv 名字：備援到 easyflowDefaults.companyName。
    let without_spv = build_application(&bill, &json!({"easyflowDefaults":{"companyName":"測試填值-備援公司"}}), &json!([]));
    assert_eq!(value(&without_spv,"companyName"), "測試填值-備援公司");
    // 兩者都沒有：維持 None（不猜）。
    let neither = build_application(&bill, &json!({}), &json!([]));
    assert_eq!(value(&neither,"companyName"), Value::Null);
}

/// 卡 20261126-0214 S2：日期預設只補空欄，不覆蓋已存在的值；收款日 = 申請日 + 天數。
#[test]
fn fill_date_defaults_only_fills_blank_dates_and_adds_receipt_days() {
    use eo_domain::easyflow::fill_date_defaults;
    let today = chrono::NaiveDate::parse_from_str("2026-11-26", "%Y-%m-%d").unwrap();
    let bill = json!({"docType":"renewableSurplus","detailFields":{"purchaseEnergyFee":"100"}});
    let mut p = build_application(&bill, &json!({}), &json!([]));
    assert_eq!(value(&p,"applicationDate"), Value::Null);
    fill_date_defaults(&mut p, today, Some(30));
    assert_eq!(value(&p,"applicationDate"), "2026-11-26");
    assert_eq!(value(&p,"expectedReceiptDate"), "2026-12-26");
    // 已經有值的欄位不被覆蓋。
    if let Some(fields) = p["fields"].as_array_mut() {
        for f in fields { if f["key"]=="applicationDate" { f["value"]=json!("2026-01-01"); } }
    }
    fill_date_defaults(&mut p, today, Some(30));
    assert_eq!(value(&p,"applicationDate"), "2026-01-01");
}

#[test]
fn retailer_identity_is_independent_of_taipower_and_other_providers(){
 use eo_domain::business_key::{bill_business_key,source_bill_key};
 let key=bill_business_key(Some("11-72-8007-94-7"),Some("2026-09-01"),Some("2026-09-30"));
 let tp=source_bill_key(key.clone(),Some("renewableSurplus"),Some("renewable_purchase"));
 let a=source_bill_key(key.clone(),Some("retailerSale"),Some("retailer_apollo"));
 let b=source_bill_key(key.clone(),Some("retailerSale"),Some("retailer_inventec"));
 assert_ne!(tp,a);assert_ne!(a,b);assert_eq!(a,source_bill_key(key,Some("retailerSale"),Some("retailer_apollo")));
 assert_eq!(source_bill_key(None,Some("retailerSale"),None),None);
}

#[test]
fn service_team_is_not_the_invoice_counterparty() {
 let bill=json!({"docType":"renewableSurplus","detailFields":{"serviceDepartment":"服務部門:新營區營業處核算(收費)課"}});
 let p=build_application(&bill,&json!({}),&json!([]));
 assert_eq!(value(&p,"counterpartyName"),"台灣電力股份有限公司新營區營業處");
 assert_eq!(value(&p,"serviceDepartment"),"服務部門:新營區營業處核算(收費)課");
 let p=build_application(&bill,&json!({"counterpartyName":"已確認買受人"}),&json!([]));
 assert_eq!(value(&p,"counterpartyName"),"已確認買受人");
 let mut other=bill.clone();other["docType"]=json!("retailerSale");
 assert_eq!(value(&build_application(&other,&json!({}),&json!([])),"counterpartyName"),Value::Null);
 other["docType"]=json!("unclassified");
 assert_eq!(value(&build_application(&other,&json!({}),&json!([])),"counterpartyName"),Value::Null);
}

#[test]
fn customer_payment_rules_keep_components_and_unknown_barcode_distinct() {
 let station=json!({"name":"測試案場"});
 let mut b=json!({"docType":"consumptionDeviceRental","detailFields":{"meterRentalFee":20,"usageEnergyFee":100,"latePaymentFee":5,"payableTotal":125,"hasPaymentBarcode":true,"invoiceNumber":"BBR123456"}});
 let p=build_application(&b,&station,&json!([]));
 assert_eq!(value(&p,"contractOption"),"無合約");
 assert_eq!(value(&p,"paymentMethod"),"匯款（請附存摺影本）");
 assert_eq!(value(&p,"paymentTerms"),"依公司正常請款規定");
 assert_eq!(value(&p,"voucherStatus"),"有，電表租費發票BBR123456");
 assert_eq!(p["lineItems"].as_array().unwrap().len(),3);
 assert_eq!(p["lineItems"][1]["description"],"測試案場逆變器使用電費");
 assert_eq!(p["lineItemsReconciled"],true);
 b["detailFields"]["hasPaymentBarcode"]=Value::Null;
 let p=build_application(&b,&station,&json!([]));
 assert_eq!(value(&p,"paymentMethod"),Value::Null);
 b["docType"]=json!("renewableNoSurplus");b["detailFields"]["hasPaymentBarcode"]=json!(false);
 let p=build_application(&b,&station,&json!([]));
 assert_eq!(value(&p,"contractOption"),"有合約／簽呈");
 assert_eq!(value(&p,"paymentMethod"),"自動扣款");
 assert_eq!(value(&p,"voucherStatus"),"無，後補");
}
