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
fn strict_preview(v:&Value,day:&str,bills:&[Value])->Value{
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

// Formula capability is independent of administrative approval and lease evidence.
fn number(v:&Value)->Option<rust_decimal::Decimal>{use std::str::FromStr;let s=if let Some(s)=v.as_str(){s.to_owned()}else{v.to_string()};rust_decimal::Decimal::from_str(&s).ok().filter(|d|*d>=rust_decimal::Decimal::ZERO&&*d<=rust_decimal::Decimal::from(1_000_000_000_000i64))}
fn amount(v:Option<rust_decimal::Decimal>)->Value{v.map(|d|json!(d.normalize().to_string())).unwrap_or(Value::Null)}
pub fn assess(v:&Value,input:&Value)->Value{
 use rust_decimal::{Decimal,RoundingStrategy};
 let mut missing:Vec<String>=Vec::new();let mut warnings:Vec<String>=Vec::new();
 let kind=v["calculationType"].as_str().unwrap_or("percentage");
 let mut get=|key:&str|{let n=number(&v[key]);if n.is_none(){missing.push(format!("缺少 {key}"));}n};
 let rate=if ["percentage","percentageFloor","annualReference"].contains(&kind){get("percent").filter(|n|*n<=Decimal::from(100))}else{None};
 let invalid_rate=rate.is_none()&&["percentage","percentageFloor","annualReference"].contains(&kind)&&number(&v["percent"]).is_some();
 let fixed=if kind=="fixed"{get("fixedAmount")}else{None};
 let area=if kind=="areaRate"{get("area")}else{None};let unit_rate=if kind=="areaRate"{get("unitRate")}else{None};
 let minimum=if kind=="percentageFloor"{get("minimumRent")}else{None};
 let annual=if kind=="annualReference"{get("annualRent")}else{None};let tolerance=if kind=="annualReference"{get("tolerancePercent")}else{None};
 if invalid_rate{missing.push("比例必須介於 0 與 100".into())}
 let months=number(&input["periodMonths"]).or_else(||number(&v["cycleMonths"])).filter(|m|*m>Decimal::ZERO&&*m<=Decimal::from(120));
 let factor=|unit:&Value|->Option<Decimal>{match unit.as_str(){Some("month")=>months,Some("year")=>months.map(|m|m/Decimal::from(12)),Some("cycle")=>Some(Decimal::ONE),_=>None}};
 let basis_ok=["gross","untaxed"].contains(&v["basis"].as_str().unwrap_or(""));
 let income=number(&input["incomeAmount"]);
 if rate.is_some()&&!basis_ok{missing.push("缺少收入基底 basis".into())}
 if rate.is_some()&&income.is_none(){missing.push("缺少試算收入 incomeAmount".into())}
 let proportional=if basis_ok{income.zip(rate).map(|(i,r)|i*r/Decimal::from(100))}else{None};
 let mut raw=match kind{
 "percentage"=>proportional,
 "fixed"=>{if factor(&v["fixedUnit"]).is_none(){missing.push("缺少固定租金單位或試算月數 fixedUnit / periodMonths".into())}fixed.zip(factor(&v["fixedUnit"])).map(|(a,b)|a*b)},
 "areaRate"=>{if v.get("rateAreaUnit").unwrap_or(&v["rateUnit"]).as_str().is_none(){missing.push("缺少單價面積單位 rateAreaUnit".into());None}else if v.get("rateAreaUnit").unwrap_or(&v["rateUnit"]).as_str().is_some_and(|u|Some(u)!=v["areaUnit"].as_str()){missing.push("面積與單價單位不同，缺少換算依據".into());None}else if v["areaUnit"].as_str().unwrap_or("").is_empty(){missing.push("缺少面積單位 areaUnit".into());None}else{if factor(&v["ratePeriod"]).is_none(){missing.push("缺少單價期間或試算月數 ratePeriod / periodMonths".into())}area.zip(unit_rate).zip(factor(&v["ratePeriod"])).map(|((a,r),f)|a*r*f)}},
 "percentageFloor"=>{if !["gross","untaxed"].contains(&v["minimumBasis"].as_str().unwrap_or("")){missing.push("最低額收入基底 minimumBasis 待確認".into());None}else if v["minimumBasis"].as_str().is_some_and(|b|Some(b)!=v["basis"].as_str()){missing.push("最低額與比例租金稅基不同，待確認稅務及比較方式".into());None}else{if factor(&v["minimumUnit"]).is_none(){missing.push("缺少最低額期間或試算月數 minimumUnit / periodMonths".into())}proportional.zip(minimum.zip(factor(&v["minimumUnit"]))).map(|(p,(m,f))|p.max(m*f))}},
 "annualReference"=>{match (proportional,annual,tolerance){(Some(p),Some(a),Some(t)) if a>=Decimal::new(1,2)&&t>Decimal::ZERO&&t<=Decimal::from(100)=>{if v["rounding"]!="total"{missing.push("年度調整需確認總額取整方式".into());None}else if (p.round_dp_with_strategy(0,RoundingStrategy::MidpointAwayFromZero)*Decimal::from(100)/a-Decimal::from(100)).abs()<t{Some(a)}else{missing.push("年度調整級距及邊界適用方式待確認".into());None}},_=>None}},
 _=>{missing.push("計算類型尚未支援".into());None}};
 if v["groupAllocationMissing"]==true {missing.push("共同地主整組金額，個別案場分配待確認".into());raw=None;}
 let rounding=v["rounding"].as_str().unwrap_or("");
 let rounded=if rounding=="total"{raw.map(|n|n.round_dp_with_strategy(0,RoundingStrategy::MidpointAwayFromZero))}else if rounding=="perReceipt"&&rate.is_some(){warnings.push("逐筆取整須提供逐筆收入；目前顯示未取整基數".into());None}else{if rounding!="perReceipt"{warnings.push("取整方式待確認；先顯示未取整基數".into())}raw};
 let base=rounded.or(raw);
 let tax_rate=number(&v["taxPercent"]).filter(|n|*n<=Decimal::from(100));
 if tax_rate.is_none(){warnings.push("稅務待確認；僅顯示租金基數".into())}
 let tax_raw=base.zip(tax_rate).map(|(b,t)|if v["taxMode"]=="included"{b-b/(Decimal::ONE+t/Decimal::from(100))}else{b*t/Decimal::from(100)});
 let gross_raw=base.zip(tax_raw).map(|(b,t)|if v["taxMode"]=="included"{b}else{b+t});
 let final_rounding=rounding=="total"||(rounding=="perReceipt"&&rate.is_none());
 let tax=if final_rounding{tax_raw.map(|t|t.round_dp_with_strategy(0,RoundingStrategy::MidpointAwayFromZero))}else{None};
 if v["status"]!="confirmed"{warnings.push("規則尚待確認；試算不代表核准付款".into())}
 if v["validFrom"].is_null(){warnings.push("適用起日待確認".into())}
 if v["cycleAnchor"].is_null(){warnings.push("週期錨點待確認；無法自動組成整期".into())}
 if v["leaseEvidence"]["status"]!="complete"{warnings.push("租約附件尚未完整".into())}
 let val=|k:&str|v[k].as_str().map(str::to_owned).unwrap_or_else(||if v[k].is_null(){"待確認".into()}else{v[k].to_string()});
 let summary=match kind{"percentage"=>format!("{}收入 × {}%",if v["basis"]=="untaxed"{"未稅"}else if v["basis"]=="gross"{"含稅"}else{"基底待確認"},val("percent")),"fixed"=>format!("固定租金 {} / {}",val("fixedAmount"),val("fixedUnit")),"areaRate"=>format!("{} {} × {} / {}",val("area"),val("areaUnit"),val("unitRate"),val("ratePeriod")),"percentageFloor"=>format!("收入 × {}%，最低 {} / {}",val("percent"),val("minimumRent"),val("minimumUnit")),"annualReference"=>format!("收入 × {}%，參考年租金 {}，調整門檻 {}%",val("percent"),val("annualRent"),val("tolerancePercent")),_=>"公式待確認".into()};
 for x in v["sourceMissing"].as_array().into_iter().flatten(){if let Some(t)=x.as_str(){warnings.push(format!("來源待補：{t}"));}}
 for x in v["sourceWarnings"].as_array().into_iter().flatten(){if let Some(t)=x.as_str(){warnings.push(t.to_owned());}}
 json!({"status":v["status"],"state":if raw.is_some(){"provisionalCalculation"}else{"pending"},"formulaSummary":summary,"readiness":if raw.is_some(){"calculable"}else{"missingInputs"},"knownInputs":v,"missingInputs":missing,"requirements":missing,"issues":missing,"warnings":warnings,"baseRentRaw":amount(raw.take()),"baseRent":amount(base),"tax":amount(tax),"grossRent":amount(base.zip(tax).map(|(a,b)|if v["taxMode"]=="included"{a}else{a+b})),"taxRaw":amount(tax_raw),"grossRentRaw":amount(gross_raw),"referenceRent":amount(proportional),"annualRent":v["annualRent"],"partyGroup":v["partyGroup"],"paymentAllowed":false,"inputMode":"manualTrial","inputMethod":"manualTrial","lines":[],"income":amount(income),"periodMonths":input["periodMonths"]})
}

pub fn preview(v:&Value,day:&str,bills:&[Value])->Value {
 let mut assessment=assess(v,&json!({}));
 if v["status"]=="confirmed" {let mut result=strict_preview(v,day,bills);for key in ["formulaSummary","knownInputs","status"]{result[key]=assessment[key].clone();}result["inputMode"]=json!("autoBills");result["baseRentRaw"]=result["baseRent"].clone();return result;}
 assessment["inputMode"]=json!("autoBills");assessment["inputMethod"]=json!("autoBills");
 if ["fixed","areaRate"].contains(&v["calculationType"].as_str().unwrap_or("")){return assessment;}
 let (a,b)=match cycle(v,day){Ok(c)=>c,Err(_)=>{assessment["sourceBlockers"]=json!(["缺少可用週期或錨點，無法自動組成整期"]);return assessment}};
 let mut checker=v.clone();checker["status"]=json!("confirmed");checker["calculationType"]=json!("percentage");checker["percent"]=json!("0");checker["taxPercent"]=json!("0");checker["rounding"]=json!("total");checker["payee"]=json!("來源覆蓋檢查");checker["ruleSource"]=json!("來源覆蓋檢查");
 // A surrogate checks source coverage only, never supplies missing business terms.
 if checker["validFrom"].is_null(){checker["validFrom"]=json!(a.to_string());}
 if checker["transfer"].is_null(){checker["transfer"]=json!(false);}
 let checked=strict_preview(&checker,day,bills);
 let mut blockers=checked["issues"].as_array().cloned().unwrap_or_default();
 if v["validFrom"].is_null(){blockers.push(json!("適用起日待確認，不能認定整期適用"));}
 if v["transfer"].is_null(){blockers.push(json!("轉供來源範圍待確認"));}
 let mut calculated=assess(v,&json!({"incomeAmount":checked["income"],"periodMonths":v["cycleMonths"]}));
 let lines=checked["lines"].as_array().cloned().unwrap_or_default();
 if !blockers.is_empty(){calculated["partialSourceBase"]=if blockers.iter().any(|b|b.as_str().is_some_and(|s|s.contains("重複")||s.contains("重疊"))){Value::Null}else{calculated["referenceRent"].clone()};for key in ["baseRentRaw","baseRent","tax","grossRent","taxRaw","grossRentRaw"]{calculated[key]=Value::Null;}calculated["state"]=json!("missingOrConflictingSources");}
 else if !calculated["baseRentRaw"].is_null()&&v["rounding"]=="perReceipt"&&["percentage","percentageFloor"].contains(&v["calculationType"].as_str().unwrap_or("percentage")){
   use rust_decimal::{Decimal,RoundingStrategy};
   if let Some(rate)=number(&v["percent"]).filter(|n|*n<=Decimal::from(100)){let sum:Decimal=lines.iter().filter_map(|l|number(&l["income"])).map(|n|(n*rate/Decimal::from(100)).round_dp_with_strategy(0,RoundingStrategy::MidpointAwayFromZero)).sum();
   let base=if v["calculationType"]=="percentageFloor"{number(&assess(v,&json!({"incomeAmount":"0","periodMonths":v["cycleMonths"]}))["baseRentRaw"]).map(|n|n.max(sum))}else{Some(sum)};
   let tax_raw=base.zip(number(&v["taxPercent"]).filter(|n|*n<=Decimal::from(100))).map(|(n,t)|if v["taxMode"]=="included"{n-n/(Decimal::ONE+t/Decimal::from(100))}else{n*t/Decimal::from(100)});
   let tax=tax_raw.map(|n|n.round_dp_with_strategy(0,RoundingStrategy::MidpointAwayFromZero));
   calculated["baseRent"]=amount(base);calculated["taxRaw"]=amount(tax_raw);calculated["grossRentRaw"]=amount(base.zip(tax_raw).map(|(n,t)|if v["taxMode"]=="included"{n}else{n+t}));calculated["tax"]=amount(tax);calculated["grossRent"]=amount(base.zip(tax).map(|(n,t)|if v["taxMode"]=="included"{n}else{n+t}));
   if let Some(warnings)=calculated["warnings"].as_array_mut(){warnings.retain(|w|!w.as_str().is_some_and(|s|s.starts_with("逐筆取整須")));}}
 }
 calculated["sourceBlockers"]=json!(blockers);calculated["issues"]=json!(blockers);calculated["lines"]=json!(lines);calculated["sourceBillIds"]=checked["sourceBillIds"].clone();calculated["periodStart"]=json!(a.to_string());calculated["periodEnd"]=json!(b.to_string());calculated["inputMode"]=json!("autoBills");calculated["inputMethod"]=json!("autoBills");calculated
}

/// Contributions are individually identified; they must never be summed as a full cycle.
pub fn source_contributions(v:&Value,bills:&[Value])->Value {
 json!(bills.iter().filter(|b|["renewableSurplus","renewableNoSurplus","retailerSale"].contains(&b["docType"].as_str().unwrap_or(""))).map(|b|{
 let key=if v["basis"]=="untaxed"{"purchaseEnergyFee"}else{"electricityFeeTotal"};
 let mut result=assess(v,&json!({"incomeAmount":b[key]}));
 if !["percentage","percentageFloor","annualReference"].contains(&v["calculationType"].as_str().unwrap_or("percentage")){result["baseRentRaw"]=Value::Null;result["baseRent"]=Value::Null;}
 // Floors and annual adjustments apply to a whole period, never to one bill.
 let contribution=if ["percentage","percentageFloor","annualReference"].contains(&v["calculationType"].as_str().unwrap_or("percentage")){result["referenceRent"].clone()}else{Value::Null};
 json!({"billId":b["id"],"start":b["start"],"end":b["end"],"income":b[key],"sourceContributionRaw":contribution,"inputMode":"actualBillContribution","paymentAllowed":false,"note":"單筆來源貢獻，非整期租金；重複單據不可加總"})
 }).collect::<Vec<_>>())
}
