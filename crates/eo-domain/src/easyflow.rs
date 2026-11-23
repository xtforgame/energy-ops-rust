//! Read-only application preparation. No workflow approval, landlord calculation or EF submission.
//! Stable field keys are the automation contract; values only come from saved final bill/master data.
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;

fn text(v: &Value) -> Option<String> {
    match v { Value::String(s) if !s.trim().is_empty() => Some(s.trim().into()), Value::Number(n) => Some(n.to_string()), _ => None }
}
fn amount(v: &Value) -> Option<String> {
    Decimal::from_str(&text(v)?.replace(',', "")).ok().map(|d| d.normalize().to_string())
}
fn field(fields: &mut Vec<Value>, key: &str, label: &str, value: Option<String>, source: &str) {
    fields.push(json!({"key":key,"label":label,"status":if value.is_some(){"available"}else{"pending"},"value":value,"source":source}));
}

pub fn build_application(bill: &Value, station: &Value, attachments: &Value) -> Value {
    let kind = match bill["docType"].as_str().unwrap_or("") {
        "renewableSurplus" | "retailerSale" => "invoice",
        "renewableNoSurplus" | "consumptionDevice" | "consumptionDeviceRental" | "consumptionBill" => "payment",
        _ => "unconfirmed",
    };
    let mut detail = bill["detailFields"].clone();
    if !detail.is_object() { detail = json!({}); }
    // Legacy platform detail omitted this canonical field. Use OCR's explicit subtotal only
    // when the saved field is absent; an explicitly cleared/corrected field always wins.
    let subtotal = &bill["ocrRaw"]["record"]["fields"]["meterRentalSubtotal"];
    let rental_from_subtotal = detail.get("meterRentalFee").is_none()
        && matches!(subtotal["status"].as_str(), Some("ocr_extracted" | "extracted" | "confirmed"))
        && amount(&subtotal["value"]).is_some();
    if rental_from_subtotal { detail["meterRentalFee"] = subtotal["value"].clone(); }
    // Old bills predate these detail mappings. Preserve explicit corrections, including null.
    let mut canonical_sources = Vec::new();
    for key in ["meterReadingDiscount", "taxBeforeAmount"] {
        let original=&bill["ocrRaw"]["record"]["fields"][key];
        if detail.get(key).is_none() && matches!(original["status"].as_str(),Some("extracted"|"ocr_extracted"|"confirmed")) && amount(&original["value"]).is_some() {
            detail[key]=original["value"].clone();canonical_sources.push(key);
        }
    }
    let high_voltage = bill["templateId"]=="high_voltage" || bill["ocrRaw"]["record"]["fields"]["templateId"]["value"]=="high_voltage";
    let mut fields = Vec::new();
    for (key,label,v,source) in [
        ("customerNumber","電號",&bill["customerNumberRaw"],"bill.customerNumberRaw"),
        ("siteCode","案場編號",&station["projectCode"],"station.projectCode / imported plantNo"),
        ("siteName","案場名稱",&station["name"],"station.name"),
        ("companyName","公司名稱",&station["companyName"],"station.spvPayee.name"),
        ("counterpartyTaxId","對象統一編號",&station["counterpartyTaxId"],"station.invoice.counterpartyTaxId"),
        // 售電業購售電單：買方統編是我們自己（跟 station 設定無關），直接從辨識結果取；取不到就標待確認。
        ("buyerTaxId","買方統一編號",&bill["ocrRaw"]["record"]["fields"]["buyerTaxId"]["value"],"bill.ocrRaw.record.fields.buyerTaxId"),
        ("billingPeriodStart","計費期間起日",&bill["billingPeriodStart"],"bill.billingPeriodStart"),
        ("billingPeriodEnd","計費期間迄日（原單）",&bill["billingPeriodEnd"],"bill.billingPeriodEnd"),
        ("currentReadingDate","本次抄表日",&detail["currentReadingDate"],"bill.detailFields.currentReadingDate"),
        ("nextReadingDate","下期抄表日",&detail["nextReadingDate"],"bill.detailFields.nextReadingDate"),
        ("dueDate","單據繳費期限",&bill["dueDate"],"bill.dueDate"),
    ] { field(&mut fields,key,label,text(v),source); }
    let counterparty = text(&station["counterpartyName"]);
    field(&mut fields,"counterpartyName","收款／付款對象",counterparty.clone().or_else(|| text(&detail["serviceDepartment"])),
        if counterparty.is_some() {"station.invoice.counterpartyName"} else {"bill.detailFields.serviceDepartment（原單台電服務部門，正式名稱請核對）"});
    for (key,label) in [("purchaseEnergyFee","購電電費（未稅）"),("businessTax","營業稅"),("electricityFeeTotal","電費合計"),
        ("meterRentalFee","電表租費（原始正負號）"),("usageEnergyFee","設備發電使用電費"),("payableTotal","應付總金額（明細）"),
        ("baseEnergyFee","基本電費（約定）"),("flowEnergyFee","流動電費"),("powerFactorAdjustmentFee","功率因數調整費"),
        ("meterReadingDiscount","電子帳單優惠減收金額"),("electricityAdjustmentAmount","電價調整金額"),("taxBeforeAmount","稅前應繳總金額")] {
        let source = if key == "meterRentalFee" && rental_from_subtotal { "bill.ocrRaw.record.fields.meterRentalSubtotal（原單電表租費小計）".into() }
            else if canonical_sources.contains(&key) {format!("bill.ocrRaw.record.fields.{key}（舊單據原始辨識值）")}
            else {format!("bill.detailFields.{key}")};
        field(&mut fields,key,label,amount(&detail[key]),&source);
    }
    field(&mut fields,"totalAmount","平台計算基準金額",amount(&bill["totalAmount"]),"bill.totalAmount（躉購單可能是未稅收入，非原單應付額）");
    let payment_total = if detail.get("payableTotal").is_some() { amount(&detail["payableTotal"]) } else {amount(&bill["totalAmount"])};
    // No fallback from net total to invoice revenue. Unknown is never zero.
    let application_amount = match kind {
        "invoice" => amount(&detail["purchaseEnergyFee"]),
        "payment" => payment_total.clone().and_then(|s| Decimal::from_str(&s).ok()).map(|d| d.abs().normalize().to_string()),
        _ => None,
    };
    field(&mut fields,"amount",if kind=="invoice"{"開立金額（未稅）"}else{"付款申請總金額"}, application_amount,
        if kind=="invoice"{"bill.detailFields.purchaseEnergyFee"}else{"abs(bill.detailFields.payableTotal)，舊資料無此欄才取 bill.totalAmount"});
    field(&mut fields,"currency","幣別",Some("TWD".into()),"Taiwan electricity bill scope");
    for (key,label) in [("companyCode","EasyFlow 公司代碼"),("applicant","申請人"),("department","申請部門"),
        ("applicationDate","申請日期"),("summary","申請摘要／項目說明"),("expectedReceiptDate","預計收款日"),
        ("paymentMethod","付款方式"),("paymentTerms","付款條件"),("voucherStatus","憑證狀態"),("contractOption","有合約／簽呈選項")] {
        let configured = text(&station["easyflowDefaults"][key]);
        let generated = if key == "summary" && kind != "unconfirmed" {
            text(&station["name"]).map(|name| format!("{name}{}", if kind == "invoice" { "發電收入" } else { "電費付款" }))
        } else { None };
        let source = if configured.is_none() && generated.is_some() { "依案場名稱與申請類型產生摘要".into() }
            else { format!("station.data.easyflowDefaults.{key}（未設定則待確認）") };
        field(&mut fields,key,label,configured.or(generated),&source);
    }
    let mut line_items = Vec::new();
    if kind=="payment" && high_voltage {
        for (key,label) in [("baseEnergyFee","基本電費（約定）"),("flowEnergyFee","流動電費"),("powerFactorAdjustmentFee","功率因數調整費"),
            ("meterReadingDiscount","電子帳單優惠減收金額"),("meterRentalFee","電器租費"),("electricityAdjustmentAmount","電價調整金額")] {
            if let Some(d)=amount(&detail[key]) {
                let source=if canonical_sources.contains(&key){format!("bill.ocrRaw.record.fields.{key}")}else{format!("bill.detailFields.{key}")};
                line_items.push(json!({"key":key,"description":label,"amount":d,"source":source}));
            }
        }
    } else if kind=="payment" {
        for (key,label) in [("meterRentalFee","電表租費"),("usageEnergyFee","設備發電使用電費")] {
            if let Some(d)=amount(&detail[key]).and_then(|s|Decimal::from_str(&s).ok()) {
                line_items.push(json!({"key":key,"description":label,"amount":d.abs().normalize().to_string(),"source":format!("abs(bill.detailFields.{key})")}));
            }
        }
    }
    let line_sum: Decimal = line_items.iter().filter_map(|x|x["amount"].as_str().and_then(|s|Decimal::from_str(s).ok())).sum();
    let total=payment_total.as_deref().and_then(|s|Decimal::from_str(s).ok());
    let rounded_sum=line_sum.round_dp_with_strategy(0,rust_decimal::RoundingStrategy::MidpointAwayFromZero);
    let tax_reconciled=amount(&detail["taxBeforeAmount"]).and_then(|s|Decimal::from_str(&s).ok())
        .zip(amount(&detail["businessTax"]).and_then(|s|Decimal::from_str(&s).ok())).zip(total)
        .is_some_and(|((net,tax),gross)|net+tax==gross);
    let required_present=["baseEnergyFee","flowEnergyFee"].iter().all(|k|amount(&detail[*k]).is_some());
    let explicit_missing=["powerFactorAdjustmentFee","meterReadingDiscount","meterRentalFee","electricityAdjustmentAmount"].iter()
        .any(|k|detail.get(*k).is_some()&&amount(&detail[*k]).is_none());
    let reconciled = kind=="payment" && !line_items.is_empty() && if high_voltage {
        required_present && !explicit_missing && tax_reconciled && total.is_some_and(|d|d==rounded_sum)
    } else {total.is_some_and(|d|d.abs()==line_sum)};
    let reconciliation=if high_voltage {json!({"basis":"signed_bill_charges_including_tax","lineSum":line_sum.normalize().to_string(),
       "roundedLineSum":rounded_sum.normalize().to_string(),"roundingDifference":(rounded_sum-line_sum).normalize().to_string(),
       "taxBreakdownReconciled":tax_reconciled,"message":"依原單計費項目保留正負號加總，按整元核對總額；稅前總額＋營業稅另行驗算，營業稅不重複加入明細。"})}else{Value::Null};
    let mut pending = vec![
        json!({"key":"workflowApproval","message":"這是資料準備草稿，不代表審核通過，不可據此自動送出 EasyFlow。"}),
        json!({"key":"ledgerPeriodConvention","message":"紀錄表計費迄日採原單計費日或本次抄表日待確認；兩個日期分別保留。"}),
        json!({"key":"applicationOptions","message":"申請人、部門、公司代碼、日期及選項依實際作業補齊；範例選項不套用所有單據。"}),
    ];
    if kind=="unconfirmed" { pending.push(json!({"key":"applicationType","message":"單別尚不能決定付款或開票，請先校對單別。"})); }
    if kind=="payment" && !reconciled { pending.push(json!({"key":"lineItems","message":"已取得的費用明細尚未與付款總額對平；可能含補收、調整或其他電費，不自動猜拆分。"})); }
    for f in &fields {
        if ["amount","customerNumber","siteCode","siteName","companyName","counterpartyName","counterpartyTaxId"].contains(&f["key"].as_str().unwrap_or("")) && f["value"].is_null() {
            pending.push(json!({"key":f["key"],"message":format!("{}缺少來源值",f["label"].as_str().unwrap_or(""))}));
        }
    }
    if attachments.as_array().is_none_or(|a| a.is_empty()) { pending.push(json!({"key":"sourceAttachment","message":"尚未取得來源附件，請補上原始單據。"})); }
    json!({"schemaVersion":"easyflow.application.v1","state":"draft","submissionAllowed":false,
        "billId":bill["id"],"reviewStatus":bill["reviewStatus"],"approvalState":bill["approvalState"],
        "applicationType":kind,"fields":fields,"lineItems":line_items,"lineItemsReconciled":reconciled,"reconciliation":reconciliation,
        "attachments":attachments,"pending":pending})
}

/// UTF-8 BOM for Excel. All cells quoted; untrusted text cannot become spreadsheet formulas.
pub fn application_csv(p: &Value) -> String {
    fn cell(s: &str) -> String {
        let guarded = if s.trim_start().starts_with(['=', '+', '-', '@']) { format!("'{s}") } else { s.into() };
        format!("\"{}\"",guarded.replace('"',"\"\""))
    }
    let mut out = "\u{feff}欄位鍵,欄位名稱,值,狀態,資料來源\r\n".to_string();
    if let Some(fields)=p["fields"].as_array() {
        for f in fields {
            let status = if f["status"]=="available" {"已取得（草稿）"} else {"待確認"};
            let row = [text(&f["key"]).unwrap_or_default(),text(&f["label"]).unwrap_or_default(),text(&f["value"]).unwrap_or_default(),status.into(),text(&f["source"]).unwrap_or_default()];
            out.push_str(&row.iter().map(|s| cell(s)).collect::<Vec<_>>().join(","));out.push_str("\r\n");
        }
    }
    for (key,label) in [("schemaVersion","格式版本"),("billId","平台單據 ID"),("applicationType","申請類型"),("state","資料狀態")] {
        let row=[key.to_string(),label.to_string(),text(&p[key]).unwrap_or_default(),"草稿，未送出".into(),"platform".into()];
        out.push_str(&row.iter().map(|s|cell(s)).collect::<Vec<_>>().join(","));out.push_str("\r\n");
    }
    for (section,label) in [("pending","待確認事項"),("attachments","來源附件清單"),("lineItems","費用明細"),("reconciliation","明細對帳與整元差額")] {
        let row=[section.to_string(),label.to_string(),p[section].to_string(),"草稿，未送出".into(),"platform".into()];
        out.push_str(&row.iter().map(|s|cell(s)).collect::<Vec<_>>().join(","));out.push_str("\r\n");
    }
    out
}
