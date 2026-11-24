//! Parse an annual rental worksheet as evidence, then recalculate with an existing rule.
//! A worksheet is not proof that its underlying monthly bills or meter readings were checked.
use chrono::NaiveDate;
use regex::Regex;
use serde_json::{json, Value};

fn date(s: &str) -> Result<NaiveDate, String> {
    let p = s.split('/').map(str::parse::<i32>).collect::<Result<Vec<_>, _>>().map_err(|_| "日期無法解析")?;
    let y = if p[0] < 1911 { p[0] + 1911 } else { p[0] };
    NaiveDate::from_ymd_opt(y, p[1] as u32, p[2] as u32).ok_or("日期不合法".into())
}
fn amount(s: &str) -> Result<i64, String> { s.replace(',', "").parse().map_err(|_| "金額無法解析".into()) }
fn declared(text: &str, label: &str) -> Result<i64, String> {
    let re = Regex::new(&format!(r"(?m)^{}\s+([\d,]+)\s*$", regex::escape(label))).unwrap();
    let values = re.captures_iter(text).map(|c| amount(&c[1])).collect::<Result<Vec<_>, _>>()?;
    if values.len() != 1 { return Err(format!("{label} 未辨識到唯一數值")); }
    Ok(values[0])
}
pub fn is_worksheet(text: &str) -> bool { ["參考年租金計算表", "台電購電電費", "轉供電費"].iter().all(|s| text.contains(s)) }
pub fn title(text: &str) -> &str { text.lines().find(|s| s.contains("參考年租金計算表")).unwrap_or("").trim() }

pub fn extract(text: &str, attachment: i64) -> Result<Value, String> {
    if !is_worksheet(text) || attachment <= 0 { return Err("不是可支援的租金計算表".into()); }
    let re = Regex::new(r"(?m)^\s*(\d{3,4}/\d{1,2}/\d{1,2})\s+(\d{3,4}/\d{1,2}/\d{1,2})\s+(\d+)\s+([\d,]+)\s+([\d,]+)\s*$").unwrap();
    let mut receipts = Vec::new(); let mut total = 0i64;
    for (i, c) in re.captures_iter(text).enumerate() {
        let a = date(&c[1])?; let b = date(&c[2])?;
        if b < a || c[3].parse::<i64>().ok() != Some((b-a).num_days()+1) { return Err("日期與天數不符".into()); }
        for (source, value) in [("taipower", &c[4]), ("transfer", &c[5])] {
            let n = amount(value)?; total = total.checked_add(n).ok_or("金額超界")?;
            receipts.push(json!({"source":source,"start":a.to_string(),"end":b.to_string(),"amount":n.to_string(),"kwh":null,"attachmentId":attachment,"page":1,"sourceRow":format!("OCR-row-{}-{source}",i+1)}));
        }
    }
    if receipts.is_empty() || receipts.len()>100 { return Err("未辨識到可用明細列".into()); }
    if total != declared(text,"總電費")? { return Err("明細加總與總電費不符，請核對來源".into()); }
    Ok(json!({"periodStart":receipts.first().unwrap()["start"],"periodEnd":receipts.last().unwrap()["end"],"receipts":receipts,"annualRent":declared(text,"經營年租金/年")?.to_string(),"declaredReference":declared(text,"參考年租金")?.to_string(),"declaredPayable":declared(text,"本次應繳納金額為")?.to_string()}))
}

pub fn apply(rule: &Value, sheet: &Value) -> Result<Value, String> {
    if rule["periodStart"] != sheet["periodStart"] || rule["periodEnd"] != sheet["periodEnd"] || rule["annualRent"] != sheet["annualRent"] || rule["calculationType"] != "annualReference" { return Err("計算表與既有期間／合約年租金不符，待確認".into()); }
    let mut draft = rule.clone(); draft["receipts"] = sheet["receipts"].clone();
    draft["inputMethod"] = json!("ocrWorksheet");
    if let Some(notes)=draft["notes"].as_array_mut() {for n in notes {if n.as_str().is_some_and(|s|s.starts_with("來源為人工核對")){*n=json!("本次來源為首頁計算表 OCR，尚未逐張核對後附全年電費單。");}}}

    let result = crate::rent_payment::calculate(&draft)?;
    if result["referenceRent"] != sheet["declaredReference"] || result["grossRent"] != sheet["declaredPayable"] || result["state"] != "calculatedDraft" { return Err("平台重算與表上參考租金／應付金額不符，待確認".into()); }
    Ok(draft)
}

#[cfg(test)] mod tests {
    use super::*;
    const TEXT: &str = "測試第一期參考年租金計算表\n台電購電電費 轉供電費\n115/01/01 115/01/31 31 0 1,000\n總電費 1,000\n經營年租金/年 100\n參考年租金 100\n本次應繳納金額為 100";
    #[test] fn parses_explicit_zero_and_rejects_mismatch() { let s=extract(TEXT,7).unwrap(); assert_eq!(s["receipts"][0]["amount"],"0");assert_eq!(s["periodStart"],"2026-01-01");assert!(extract(&TEXT.replace("總電費 1,000","總電費 1,001"),7).is_err());assert!(extract(&TEXT.replace("31 0","30 0"),7).is_err()); }
    #[test] fn requires_existing_rule_and_recalculates() {let s=extract(TEXT,7).unwrap();let r=json!({"periodStart":"2026-01-01","periodEnd":"2026-01-31","annualRent":"100","calculationType":"annualReference","payee":"測試","ruleSource":"合約","basis":"gross","percent":"10","taxPercent":"0","rounding":"total","transfer":true,"tolerancePercent":"5"});assert!(apply(&r,&s).is_ok());let mut bad=s.clone();bad["declaredPayable"]=json!("101");assert!(apply(&r,&bad).is_err());}
}
