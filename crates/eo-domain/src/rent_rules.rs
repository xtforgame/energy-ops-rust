//! Recurring rental worksheets use final bill fields, never payable totals or lease-file presence.
use chrono::{Datelike,Days,Months,NaiveDate};
use serde_json::{json,Value};
fn date(v:&Value)->Result<NaiveDate,String>{NaiveDate::parse_from_str(v.as_str().unwrap_or(""),"%Y-%m-%d").map_err(|_|"日期格式不正確".into())}
pub fn validate(v:&Value)->Result<(),String>{
 if !["pending","confirmed"].contains(&v["status"].as_str().unwrap_or("")){return Err("規則狀態不正確".into())}
 if !["missing","excerpt","complete"].contains(&v["leaseEvidence"]["status"].as_str().unwrap_or("")){return Err("租約附件狀態不正確".into())}
 let ids=v["leaseEvidence"]["attachmentIds"].as_array().ok_or("缺少附件清單")?;
 if ids.len()>100 || ids.iter().any(|i|i.as_i64().unwrap_or(0)<=0){return Err("附件清單不正確".into())}
 if v["leaseEvidence"]["status"]!="missing"&&ids.is_empty(){return Err("節錄或完整租約須指定附件".into())}
 if !v["validFrom"].is_null(){date(&v["validFrom"])?;}
 if !v["validTo"].is_null(){let end=date(&v["validTo"])?;if !v["validFrom"].is_null()&&end<date(&v["validFrom"])?{return Err("規則生效期間不正確".into())}}
 if v["status"]=="pending"{return Ok(())}
 date(&v["validFrom"])?;
 if v["calculationType"].as_str().unwrap_or("percentage")!="percentage"{return Err("此類型尚待確認，請儲存待確認規則".into())}
 let anchor=date(&v["cycleAnchor"])?;
 if anchor.day()>28||!(1..=12).contains(&v["cycleMonths"].as_u64().unwrap_or(0)){return Err("週期須為1至12個月，錨點日期須為1至28日".into())}
 let mut sample=v.clone();sample["periodStart"]=json!(anchor.to_string());sample["periodEnd"]=json!(anchor.to_string());sample["receipts"]=json!([]);
 crate::rent_payment::calculate(&sample)?;Ok(())
}
pub fn cycle(v:&Value,day:&str)->Result<(NaiveDate,NaiveDate),String>{
 let anchor=date(&v["cycleAnchor"])?;let day=date(&json!(day))?;
 let months=v["cycleMonths"].as_u64().filter(|n|(1..=12).contains(n)).ok_or("週期未確認")? as u32;
 let delta=(day.year()-anchor.year())*12+day.month() as i32-anchor.month() as i32;
 let mut n=delta.div_euclid(months as i32);if day.day()<anchor.day(){n=(delta-1).div_euclid(months as i32)}
 let offset=n*months as i32;
 let start=if offset>=0{anchor.checked_add_months(Months::new(offset as u32))}else{anchor.checked_sub_months(Months::new((-offset) as u32))}.ok_or("日期超界")?;
 let end=start.checked_add_months(Months::new(months)).and_then(|d|d.checked_sub_days(Days::new(1))).ok_or("日期超界")?;Ok((start,end))
}
pub fn pending(issue:&str)->Value{json!({"state":"pending","inputMethod":"autoBills","paymentAllowed":false,"baseRent":null,"tax":null,"grossRent":null,"issues":[issue],"requirements":[issue],"lines":[]})}
pub fn preview(v:&Value,day:&str,bills:&[Value])->Value{
 let run=||->Result<Value,String>{validate(v)?;if v["status"]!="confirmed"{return Err("租金規則待確認".into())}
 let(a,b)=cycle(v,day)?;
 if a<date(&v["validFrom"])?||(!v["validTo"].is_null()&&b>date(&v["validTo"])?){return Err("規則未涵蓋完整計算週期".into())}
 let mut input=v.clone();input["periodStart"]=json!(a.to_string());input["periodEnd"]=json!(b.to_string());let mut rows=Vec::new();
 for bill in bills{
 let kind=bill["docType"].as_str().unwrap_or("");if !["renewableSurplus","renewableNoSurplus","retailerSale"].contains(&kind){continue}
 if date(&bill["end"]).is_ok_and(|end|end<a)||date(&bill["start"]).is_ok_and(|start|start>b){continue}
 let start=date(&bill["start"]).map_err(|_|format!("單據 {} 缺少有效起日",bill["id"]))?;let end=date(&bill["end"]).map_err(|_|format!("單據 {} 缺少有效迄日",bill["id"]))?;if end<a||start>b{continue}
 let key=if v["basis"]=="gross"{"electricityFeeTotal"}else{"purchaseEnergyFee"};
 let amount=&bill[key];let text=|x:&Value|if x.is_number(){json!(x.to_string())}else{x.clone()};
 if bill["kwh"].is_null(){return Err("缺少來源單據度數".into())}
 rows.push(json!({"source":if kind=="retailerSale"{"transfer"}else{"taipower"},"start":bill["start"],"end":bill["end"],"amount":text(amount),"kwh":text(&bill["kwh"]),"attachmentId":bill["attachmentId"],"page":bill["page"].as_u64().unwrap_or(1),"sourceRow":format!("bill:{}",bill["id"])}));
 }
 input["receipts"]=json!(rows);let mut result=crate::rent_payment::calculate(&input)?;result["inputMethod"]=json!("autoBills");result["requirements"]=result["issues"].clone();result["note"]=json!("依已確認規則與電費單最終欄位自動試算；非審核通過或已付款。");Ok(result)};
 let mut result=run().unwrap_or_else(|e|pending(&e));
 if let Some(lines)=result["lines"].as_array_mut(){for line in lines{let original=bills.iter().find(|b|line["sourceRow"]==format!("bill:{}",b["id"]));line["page"]=original.map(|b|b["page"].clone()).unwrap_or(Value::Null);line["billId"]=original.map(|b|b["id"].clone()).unwrap_or(Value::Null);}}
 result["sourceBillIds"]=json!(bills.iter().filter(|b|["renewableSurplus","renewableNoSurplus","retailerSale"].contains(&b["docType"].as_str().unwrap_or(""))).filter(|b|cycle(v,day).is_ok_and(|(a,z)|b["start"].as_str().is_some_and(|s|s<=z.to_string().as_str())&&b["end"].as_str().is_some_and(|s|s>=a.to_string().as_str()))).map(|b|b["id"].clone()).collect::<Vec<_>>());
 result["evidenceStatus"]=v["leaseEvidence"]["status"].clone();result["leaseEvidence"]=v["leaseEvidence"].clone();result
}
