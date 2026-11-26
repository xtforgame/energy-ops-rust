use serde_json::{json,Value};

fn normalize(s:&str)->String {s.chars().map(|c|match c {'臺'=>'台','\u{ff01}'..='\u{ff5e}'=>char::from_u32(c as u32-0xfee0).unwrap(),_=>c}).filter(|c|c.is_alphanumeric()).flat_map(char::to_lowercase).collect()}
fn name(v:&Value)->Option<String>{v.as_str().filter(|s|!s.trim().is_empty()&& !["-","N/A"].contains(&s.trim())).map(normalize)}
fn company(s:&str)->String {s.trim_end_matches("股份有限公司").trim_end_matches("有限公司").to_string()}
fn hints(raw:&Value,keys:&[&str])->Vec<String>{keys.iter().filter_map(|k|{let f=&raw["record"]["fields"][k];if matches!(f["status"].as_str(),Some("missing"|"not_applicable")){None}else{name(&f["value"])}}).collect()}
/// Exact normalized names only. Never promote a buyer/payee to the plant owner.
pub fn candidates(stations:&[Value],raw:&Value,meter:Option<&str>)->Value {
 if meter.is_some_and(|s|!s.trim().is_empty()){return json!({"method":"meter","candidates":[]})}
 let sites=hints(raw,&["siteName","stationName","plantName"]);
 let companies=hints(raw,&["sellerName","companyName"]);
 let (method,needles)=if !sites.is_empty(){("siteName",sites)}else{("companyName",companies.iter().map(|s|company(s)).collect())};
 let mut found=Vec::new();
 for s in stations {
  let mut names=Vec::new();
  let mut add=|v:&Value,source:&str|{if let Some(n)=name(v){names.push((if method=="companyName"{company(&n)}else{n},source.to_string()));}};
  if method=="siteName" {add(&s["name"],"station.name");for v in s["data"]["nameAliases"].as_array().into_iter().flatten(){add(v,"station.data.nameAliases");}}
  else {add(&s["companyName"],"station.spvPayee.name");add(&s["data"]["easyflowDefaults"]["companyName"],"station.easyflowDefaults.companyName");}
  for (key,import) in s["data"]["sourceImports"].as_object().into_iter().flatten(){
   if method=="siteName" {
    add(&import["station"]["name"],&format!("sourceImports.{key}.station.name"));
    for a in import["station"]["assetSources"].as_array().into_iter().flatten(){add(&a["name"],&format!("sourceImports.{key}.assetSources"));}
    for a in import["rentalSources"].as_array().into_iter().flatten(){for k in ["alias","assetName"]{add(&a[k],&format!("sourceImports.{key}.rentalSources.{k}"));}}
   }else {add(&import["station"]["spv"],&format!("sourceImports.{key}.station.spv"));}
  }
  let evidence:Vec<_>=names.into_iter().filter(|(n,_)|needles.contains(n)).map(|(n,p)|json!({"name":n,"source":p})).collect();
  if !evidence.is_empty(){found.push(json!({"id":s["id"],"name":s["name"],"customerNumber":s["customerNumber"],"evidence":evidence}));}
 }
 found.sort_by_key(|s|s["id"].as_i64());found.dedup_by_key(|s|s["id"].as_i64());
 json!({"method":method,"names":needles,"status":match found.len(){0=>"unmatched",1=>"matched",_=>"ambiguous"},"candidates":found})
}

#[cfg(test)]mod tests {
 use super::*;
 fn stations()->Vec<Value>{vec![json!({"id":1,"name":"高雄小港春源鋼鐵","companyName":"富迪能源","data":{"sourceImports":{"one":{"station":{"assetSources":[{"name":"春源鋼鐵"}]}}}}}),json!({"id":2,"name":"另一案場","companyName":"京承能源","data":{}}),json!({"id":3,"name":"第三案場","companyName":"京承能源","data":{}})]}
 fn raw(key:&str,value:&str)->Value {let mut r=json!({"record":{"fields":{}}});r["record"]["fields"][key]=json!({"value":value,"status":"extracted"});r}
 #[test]fn imported_asset_alias_matches_uniquely(){let r=candidates(&stations(),&raw("siteName"," 春源鋼鐵 "),None);assert_eq!(r["candidates"][0]["id"],1);assert_eq!(r["candidates"].as_array().unwrap().len(),1);}
 #[test]fn existing_meter_never_falls_back_to_name(){assert_eq!(candidates(&stations(),&raw("siteName","春源鋼鐵"),Some("99999999999"))["candidates"],json!([]));}
 #[test]fn shared_company_keeps_all_candidates(){let r=candidates(&stations(),&raw("sellerName","京承能源"),None);assert_eq!(r["candidates"].as_array().unwrap().len(),2);}
 #[test]fn buyer_is_not_the_plant_company(){assert_eq!(candidates(&stations(),&raw("buyerName","京承能源"),None)["candidates"],json!([]));}
 #[test]fn ambiguous_site_never_uses_company_to_guess(){let mut s=stations();s.push(json!({"id":4,"name":"春源鋼鐵","companyName":"其他公司"}));let r=candidates(&s,&raw("siteName","春源鋼鐵"),None);assert_eq!(r["candidates"].as_array().unwrap().len(),2);}
 #[test]fn unmatched_explicit_site_is_not_replaced_by_company(){let mut r=raw("siteName","未知案場");r["record"]["fields"]["sellerName"]=json!({"value":"富迪能源","status":"extracted"});assert_eq!(candidates(&stations(),&r,None)["candidates"],json!([]));}
}
