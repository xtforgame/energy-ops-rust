use serde_json::json;
#[test]
fn recurring_cycle_requires_source_coverage_and_never_uses_payable() {
 let rule=json!({"status":"confirmed","validFrom":"2026-10-01","cycleAnchor":"2026-10-01","cycleMonths":1,"basis":"gross","percent":"8","taxPercent":"5","rounding":"total","transfer":false,"payee":"Synthetic owner","ruleSource":"verified","leaseEvidence":{"status":"missing","attachmentIds":[]}});
 let bills=json!([{"id":1,"docType":"renewableSurplus","start":"2026-11-01","end":"2026-11-30","electricityFeeTotal":"1000","payableTotal":"99999","kwh":"20","attachmentId":1}]);
 let result=eo_domain::rent_rules::preview(&rule,"2026-11-01",bills.as_array().unwrap());
 assert_eq!(result["grossRent"],"84"); assert_eq!(result["inputMethod"],"autoBills");
 let mut missing=bills.clone();missing[0]["electricityFeeTotal"]=json!(null);
 assert!(eo_domain::rent_rules::preview(&rule,"2026-11-01",missing.as_array().unwrap())["grossRent"].is_null());
 let mut duplicate=bills.as_array().unwrap().clone();duplicate.push(duplicate[0].clone());
 assert!(eo_domain::rent_rules::preview(&rule,"2026-11-01",&duplicate)["grossRent"].is_null());
 assert!(result["lines"][0]["page"].is_null());
 let mut transfer=rule.clone();transfer["transfer"]=json!(true);
 assert!(eo_domain::rent_rules::preview(&transfer,"2026-11-01",bills.as_array().unwrap())["grossRent"].is_null());
 let mut complete=bills.as_array().unwrap().clone();let mut zero=complete[0].clone();zero["id"]=json!(2);zero["attachmentId"]=json!(2);zero["docType"]=json!("retailerSale");zero["electricityFeeTotal"]=json!("0");zero["kwh"]=json!("0");complete.push(zero);
 assert_eq!(eo_domain::rent_rules::preview(&transfer,"2026-11-01",&complete)["grossRent"],"84");
 complete[1]["docType"]=json!("consumptionDevice");assert!(eo_domain::rent_rules::preview(&transfer,"2026-11-01",&complete)["grossRent"].is_null());
 let mut unknown=rule.clone();unknown["status"]=json!("pending");unknown["validFrom"]=json!(null);unknown["calculationType"]=json!("annualReference");assert!(eo_domain::rent_rules::validate(&unknown).is_ok());assert!(eo_domain::rent_rules::preview(&unknown,"2026-11-01",&[])["grossRent"].is_null());
 let mut partial=rule.clone();partial["validTo"]=json!("2026-11-15");assert!(eo_domain::rent_rules::preview(&partial,"2026-11-01",bills.as_array().unwrap())["grossRent"].is_null());
}
