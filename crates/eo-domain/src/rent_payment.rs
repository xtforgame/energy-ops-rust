//! Evidence-backed worksheet calculation; never approval, transfer or tax advice.
use chrono::{NaiveDate,Days};
use rust_decimal::{Decimal,RoundingStrategy};
use serde_json::{Value,json};
use std::{str::FromStr,collections::HashSet};
fn string<'a>(v:&'a Value,k:&str)->Result<&'a str,String>{v[k].as_str().filter(|s|!s.trim().is_empty()).ok_or(format!("缺少 {k}"))}
fn date(s:&str)->Result<NaiveDate,String>{NaiveDate::parse_from_str(s,"%Y-%m-%d").map_err(|_|"日期格式錯誤".into())}
fn money(s:&str)->Result<Decimal,String>{let d=Decimal::from_str(s).map_err(|_|"金額格式錯誤")?;if d<Decimal::ZERO || d>Decimal::from(1_000_000_000_000i64){return Err("金額超出範圍".into())}Ok(d)}
fn round(d:Decimal)->Decimal{d.round_dp_with_strategy(0,RoundingStrategy::MidpointAwayFromZero)}
pub fn calculate(v:&Value)->Result<Value,String>{
 let kind=v["calculationType"].as_str().unwrap_or("percentage");
 if !["percentage","annualReference"].contains(&kind){return Err("租金規則類型不支援，請先確認".into())}
 let from=date(string(v,"periodStart")?)?;let to=date(string(v,"periodEnd")?)?;
 if from>to || (to-from).num_days()>370 {return Err("計算期間不正確".into())}
 string(v,"payee")?;string(v,"ruleSource")?;
 if !["gross","untaxed"].contains(&string(v,"basis")?) {return Err("收入基底未指定".into())}
 let rate=money(string(v,"percent")?)?;let taxrate=money(string(v,"taxPercent")?)?;
 if rate>Decimal::from(100)||taxrate>Decimal::from(100){return Err("百分比超出範圍".into())}
 let mode=string(v,"rounding")?;if !["perReceipt","total"].contains(&mode){return Err("取整數規則未指定".into())}
 let transfer=v["transfer"].as_bool().ok_or("未指定轉供狀態")?;
 let rows=v["receipts"].as_array().ok_or("缺少來源單據")?;if rows.len()>100{return Err("單據超過100張".into())}
 let mut issues=Vec::new();let mut warnings=Vec::new();let mut income=Decimal::ZERO;let mut rounded=Decimal::ZERO;let mut lines=Vec::new();let mut seen=HashSet::new();
 for r in rows {
  let source=string(r,"source")?;if !["taipower","transfer"].contains(&source){return Err("單據來源不正確".into())}
  if source=="transfer"&&!transfer{issues.push("非轉供案場出現轉供單據".to_string())}
  let a=date(string(r,"start")?)?;let b=date(string(r,"end")?)?;
  if a>b||a<from||b>to{issues.push("來源單據超出計算期間".into())}
  let attachment=r["attachmentId"].as_i64().filter(|n|*n>0).ok_or("缺少來源附件")?;
  let page=r["page"].as_u64().filter(|n|*n>0).ok_or("缺少來源頁碼")?;
  let source_row=r["sourceRow"].as_str().unwrap_or("");
  if !seen.insert((attachment,page,source_row.to_string())){issues.push("同一來源頁／列重複選取".into())}
  let amount=money(string(r,"amount")?)?;
  let kwh=if r["kwh"].is_null()&&!source_row.is_empty(){warnings.push("試算表未列度數，明細保留待補；金額採已核對試算表，未完成原始單據度數核對".to_string());None}else{Some(money(string(r,"kwh")?)?.normalize().to_string())};
  let part=amount*rate/Decimal::from(100);income+=amount;rounded+=round(part);
  lines.push(json!({"source":source,"start":a.to_string(),"end":b.to_string(),"income":amount.normalize().to_string(),"kwh":kwh,"sourceRow":source_row,"proportionalRent":part.normalize().to_string(),"roundedRent":round(part).to_string(),"attachmentId":attachment,"page":page}));
 }
 for source in if transfer{vec!["taipower","transfer"]}else{vec!["taipower"]}{
  let mut ranges=rows.iter().filter(|r|r["source"]==source).map(|r|Ok((date(string(r,"start")?)?,date(string(r,"end")?)?))).collect::<Result<Vec<_>,String>>()?;
  ranges.sort();let mut next=from;
  for (a,b) in ranges {if a!=next{issues.push(format!("{source} 期間缺漏或重疊"))}next=b.checked_add_days(Days::new(1)).ok_or("日期超界")?;}
  if next<=to{issues.push(format!("{source} 尚未收齊；零元仍須單據"))}
 }
 let reference=if mode=="perReceipt"{rounded}else{round(income*rate/Decimal::from(100))};
 let mut base=reference;let mut annual=Value::Null;let mut ratio=Value::Null;
 if kind=="annualReference" {
  let fixed=money(string(v,"annualRent")?)?;let tolerance=money(string(v,"tolerancePercent")?)?;
  if fixed==Decimal::ZERO||tolerance==Decimal::ZERO||tolerance>Decimal::from(100)||mode!="total" {return Err("年租金或調整門檻不正確".into())}
  let percent=reference*Decimal::from(100)/fixed;
  ratio=json!(percent.round_dp_with_strategy(2,RoundingStrategy::MidpointAwayFromZero).to_string());annual=json!(fixed.to_string());
  // Only the documented unchanged band is enabled. No guessed tier or boundary behavior.
  if (percent-Decimal::from(100)).abs()>=tolerance {issues.push("參考年租金已達調整門檻；級距及邊界適用方式待確認，暫不產出應付金額".into())}
  base=fixed;
 }
 warnings.sort();warnings.dedup();
 let tax=round(base*taxrate/Decimal::from(100));
 Ok(json!({"state":if issues.is_empty(){"calculatedDraft"}else{"missingOrConflictingSources"},"paymentAllowed":false,"inputMethod":"documentVerifiedInputs","periodStart":from.to_string(),"periodEnd":to.to_string(),"payee":v["payee"],"basis":v["basis"],"percent":v["percent"],"taxPercent":v["taxPercent"],"rounding":mode,"ruleSource":v["ruleSource"],"notes":v["notes"],"issues":issues,"warnings":warnings,"lines":lines,"income":income.normalize().to_string(),"baseRent":if issues.is_empty(){Some(base.to_string())}else{None},"tax":if issues.is_empty(){Some(tax.to_string())}else{None},"grossRent":if issues.is_empty(){Some((base+tax).to_string())}else{None},"calculationType":kind,"annualRent":annual,"referenceRent":reference.to_string(),"referenceRatioPercent":ratio,"tolerancePercent":v["tolerancePercent"],"paymentDeadline":v["paymentDeadline"],"paymentDate":null,"paymentNo":null,"note":"來源經人工核對匯入，平台依參數即時計算；非全自動 OCR、非審核通過或已付款。"}))
}
