//! Read-only application preparation. No workflow approval, landlord calculation or EF submission.
//! Stable field keys are the automation contract; values only come from saved final bill/master data.
use rust_decimal::Decimal;
use serde_json::{json, Value};

/// Data readiness only; never authorizes external submission or replaces workflow approval.
pub fn missing_handoff_fields(application: &Value) -> Vec<String> {
    let mut required=vec!["siteName","companyName","counterpartyName","amount","companyCode","applicant","department","applicationDate","summary"];
    match application["applicationType"].as_str() {
        Some("payment")=>required.extend(["paymentMethod","paymentTerms","voucherStatus","contractOption"]),
        Some("invoice")=>required.push("expectedReceiptDate"),
        _=>required.push("applicationType"),
    }
    let fields=application["fields"].as_array();
    let mut missing=Vec::new();
    for key in required {
        let value=fields.and_then(|f|f.iter().find(|f|f["key"]==key)).and_then(|f|f["value"].as_str()).filter(|s|!s.trim().is_empty());
        if value.is_none() || (["applicationDate","expectedReceiptDate"].contains(&key) && value.is_some_and(|v|chrono::NaiveDate::parse_from_str(v,"%Y-%m-%d").is_err())) {missing.push(key.to_string());}
    }
    if application["attachments"].as_array().is_none_or(|a|a.is_empty()){missing.push("sourceAttachment".into());}
    missing
}
use std::str::FromStr;

fn text(v: &Value) -> Option<String> {
    match v { Value::String(s) if !s.trim().is_empty() => Some(s.trim().into()), Value::Number(n) => Some(n.to_string()), _ => None }
}
fn amount(v: &Value) -> Option<String> {
    Decimal::from_str(&text(v)?.replace(',', "")).ok().map(|d| d.normalize().to_string())
}
// Only a known Taipower bill may use its printed service district as a fallback.
// Keep the original department separately; internal teams are not invoice entities.
fn taipower_counterparty(v: &Value) -> Option<String> {
    let raw = text(v)?;
    let district = raw.trim_start_matches("服務部門").trim_start_matches([':', '：']).trim();
    let end = district.find("區營業處")? + "區營業處".len();
    let district = district[..end].trim();
    Some(if district.starts_with("台灣電力") { district.to_string() }
        else { format!("台灣電力股份有限公司{district}") })
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
    for key in ["purchaseEnergyFee", "businessTax", "electricityFeeTotal", "payableTotal", "usageEnergyFee", "baseEnergyFee", "flowEnergyFee", "powerFactorAdjustmentFee", "electricityAdjustmentAmount", "meterReadingDiscount", "taxBeforeAmount", "latePaymentFee"] {
        let original=&bill["ocrRaw"]["record"]["fields"][key];
        if detail.get(key).is_none() && matches!(original["status"].as_str(),Some("extracted"|"ocr_extracted"|"confirmed")) && amount(&original["value"]).is_some() {
            detail[key]=original["value"].clone();canonical_sources.push(key);
        }
    }
    let high_voltage = bill["templateId"]=="high_voltage" || bill["ocrRaw"]["record"]["fields"]["templateId"]["value"]=="high_voltage";
    let mut fields = Vec::new();
    let retailer = bill["docType"]=="retailerSale";
    let buyer_tax = &bill["ocrRaw"]["record"]["fields"]["buyerTaxId"]["value"];
    let counterparty_tax = if retailer {buyer_tax} else {&station["counterpartyTaxId"]};

    for (key,label,v,source) in [
        ("customerNumber","電號",&bill["customerNumberRaw"],"bill.customerNumberRaw"),
        ("siteCode","案場編號",&station["projectCode"],"station.projectCode / imported plantNo"),
        ("siteName","案場名稱",&station["name"],"station.name"),
        ("counterpartyTaxId","對象統一編號",counterparty_tax,if retailer {"原單發票買受人統編"}else{"station.invoice.counterpartyTaxId"}),
        // 售電業購售電單：買方是收受發票的購電方，非電廠企業。
        ("buyerTaxId","買方統一編號",&bill["ocrRaw"]["record"]["fields"]["buyerTaxId"]["value"],"bill.ocrRaw.record.fields.buyerTaxId"),
        ("billingPeriodStart","計費期間起日",&bill["billingPeriodStart"],"bill.billingPeriodStart"),
        ("billingPeriodEnd","計費期間迄日（原單）",&bill["billingPeriodEnd"],"bill.billingPeriodEnd"),
        ("currentReadingDate","本次抄表日",&detail["currentReadingDate"],"bill.detailFields.currentReadingDate"),
        ("nextReadingDate","下期抄表日",&detail["nextReadingDate"],"bill.detailFields.nextReadingDate"),
        ("dueDate","單據繳費期限",&bill["dueDate"],"bill.dueDate"),
    ] { field(&mut fields,key,label,text(v),source); }
    // 卡 20261126-0214：spv 名字優先；沒有才用案場層設定的 easyflowDefaults.companyName（例如沒帶 spvPayee 的舊案場）。
    let company_name = text(&station["companyName"]);
    field(&mut fields,"companyName","公司名稱",company_name.clone().or_else(|| text(&station["easyflowDefaults"]["companyName"])),
        if company_name.is_some() {"station.spvPayee.name"} else {"station.data.easyflowDefaults.companyName（未設 spvPayee 時備援）"});
    let counterparty = if retailer {text(&bill["ocrRaw"]["record"]["fields"]["buyerName"]["value"])}else{text(&station["counterpartyName"])};
    field(&mut fields,"counterpartyName","收款／付款對象",counterparty.clone().or_else(|| if retailer || kind=="unconfirmed" {None}else{taipower_counterparty(&detail["serviceDepartment"])}),
        if retailer {"原單發票買受人名稱（缺值不沿用台電對象）"}else if counterparty.is_some() {"station.invoice.counterpartyName"} else {"依台電原單服務區處整理名稱；已設定的對象主檔優先"});
    field(&mut fields,"serviceDepartment","原單服務部門",text(&detail["serviceDepartment"]),"bill.detailFields.serviceDepartment（保留原文）");
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
    let inverter=matches!(bill["docType"].as_str(),Some("consumptionDevice"|"consumptionDeviceRental"));
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
        for (key,label) in [("meterRentalFee","電表租費"),("usageEnergyFee",if inverter {"逆變器使用電費"} else {"設備發電使用電費"}),("latePaymentFee","遲付費用")] {
            if let Some(d)=amount(&detail[key]).and_then(|s|Decimal::from_str(&s).ok()) {
                line_items.push(json!({"key":key,"description":format!("{}{label}",station["name"].as_str().unwrap_or("")),"currency":"TWD","amount":d.abs().normalize().to_string(),"source":format!("abs(bill.detailFields.{key})")}));
            }
        }
    }
    if kind=="payment" {
        // Payment barcode is distinct from invoice carrier/footer barcodes. Unknown is not absent.
        let barcode=detail["hasPaymentBarcode"].as_bool().or_else(||{
            let f=&bill["ocrRaw"]["record"]["fields"]["hasPaymentBarcode"];
            if matches!(f["status"].as_str(),Some("extracted"|"ocr_extracted"|"confirmed")){f["value"].as_bool()}else{None}
        });
        let invoice=text(&detail["invoiceNumber"]);
        let summary=line_items.iter().filter_map(|l|l["description"].as_str()).collect::<Vec<_>>().join("、");
        let rules=[
          ("contractOption",if inverter{Some("無合約".to_string())}else if bill["docType"]=="renewableNoSurplus"{Some("有合約／簽呈".to_string())}else{None}),
          ("paymentMethod",barcode.map(|yes|if yes{"匯款（請附存摺影本）"}else{"自動扣款"}.to_string())),
          ("paymentTerms",Some("依公司正常請款規定".to_string())),
          ("voucherStatus",match barcode{Some(false)=>Some("無，後補".to_string()),Some(true)=>invoice.map(|n|format!("有，電表租費發票{n}")),None=>None}),
          ("summary",if summary.is_empty(){None}else{Some(summary)}),
        ];
        for (key,value) in rules {if text(&station["easyflowDefaults"][key]).is_none(){if let Some(value)=value{if let Some(f)=fields.iter_mut().find(|f|f["key"]==key){f["value"]=json!(value);f["status"]=json!("available");f["source"]=json!("2026-11-25 客戶 EF 填寫規則；單別、已確認付款條碼及費用明細");}}}}
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

/// 卡 20261126-0214：只補還是空的 `applicationDate`／`expectedReceiptDate`，已有值（手填或草稿存過）的不覆蓋。
/// 呼叫端要在 `sourceRevision` 雜湊算完之後才叫這支——否則每天換日都會把既有草稿變成 stale。
pub fn fill_date_defaults(result: &mut Value, today: chrono::NaiveDate, expected_receipt_days: Option<i64>) {
    let today_str = today.format("%Y-%m-%d").to_string();
    let Some(fields) = result["fields"].as_array_mut() else { return };
    let mut application_date = None;
    for f in fields.iter_mut() {
        if f["key"]=="applicationDate" {
            if f["value"].is_null() {
                f["value"] = json!(today_str);
                f["status"] = json!("available");
                f["source"] = json!("今天（系統預設，台北時區）");
            }
            application_date = f["value"].as_str().map(String::from);
        }
    }
    let Some(application_date) = application_date else { return };
    if result["applicationType"] != "invoice" { return }
    let Some(days) = expected_receipt_days else { return };
    let Ok(base) = chrono::NaiveDate::parse_from_str(&application_date, "%Y-%m-%d") else { return };
    let receipt = (base + chrono::Duration::days(days)).format("%Y-%m-%d").to_string();
    if let Some(fields) = result["fields"].as_array_mut() {
        for f in fields.iter_mut() {
            if f["key"]=="expectedReceiptDate" && f["value"].is_null() {
                f["value"] = json!(receipt);
                f["status"] = json!("available");
                f["source"] = json!(format!("申請日期 + {days} 天（station.data.easyflowDefaults.expectedReceiptDays）"));
            }
        }
    }
}

/// UTF-8 BOM for Excel. All cells quoted; untrusted text cannot become spreadsheet formulas.
pub fn electricity_ledger_csv(p: &Value) -> String {
    let get=|key:&str|p["fields"].as_array().and_then(|f|f.iter().find(|x|x["key"]==key)).and_then(|f|text(&f["value"])).unwrap_or_default();
    let headers=["公槽","申請人","日期","公司名稱","案場名稱","電表租費","開立發票金額(發電收入)-未稅","計費區間/下期抄表日","送財會日","發票開立送件日","付款申請單送件日","發票寄送日","紙本/Mail","已傳送","已讀取","白單收據","匯款日","平台單據ID","原單計費起日","原單計費迄日","本次抄表日","下期抄表日","對照說明"];
    let rental=Decimal::from_str(&get("meterRentalFee")).ok().map(|n|n.abs().normalize().to_string()).unwrap_or_default();
    // Date convention in the customer's combined H column is unresolved; do not silently substitute reading date.
    let values=vec![String::new(),get("applicant"),String::new(),get("companyName"),get("siteName"),rental,get("purchaseEnergyFee"),String::new(),String::new(),String::new(),String::new(),String::new(),String::new(),String::new(),String::new(),String::new(),String::new(),text(&p["billId"]).unwrap_or_default(),get("billingPeriodStart"),get("billingPeriodEnd"),get("currentReadingDate"),get("nextReadingDate"),"草稿：前17欄沿用紀錄表順序；收件／送件／付款日期不推測。合併期間欄規則待確認，原始日期另列。".into()];
    fn cell(s:&str)->String {let guarded=if s.trim_start().starts_with(['=','+','-','@']){format!("'{s}")}else{s.into()};format!("\"{}\"",guarded.replace('"',"\"\""))}
    format!("\u{feff}{}\r\n{}\r\n",headers.iter().map(|s|cell(s)).collect::<Vec<_>>().join(","),values.iter().map(|s|cell(s)).collect::<Vec<_>>().join(","))
}

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
