use eo_domain::easyflow::{build_application, application_csv};
use serde_json::{json, Value};

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
