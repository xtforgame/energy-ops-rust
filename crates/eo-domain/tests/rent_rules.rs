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
#[test]
fn provisional_formulas_keep_known_math_separate_from_approval() {
 use eo_domain::rent_rules::assess;
 let mut r=json!({"status":"pending","calculationType":"percentage","basis":"untaxed","percent":"11.5","leaseEvidence":{"status":"missing","attachmentIds":[]}});
 let p=assess(&r,&json!({"incomeAmount":"16874"}));assert_eq!(p["baseRentRaw"],"1940.51");assert_eq!(p["baseRent"],"1940.51");assert!(p["grossRent"].is_null());assert_eq!(p["paymentAllowed"],false);
 r["calculationType"]=json!("fixed");r["fixedAmount"]=json!("8000");r["fixedUnit"]=json!("month");r["cycleMonths"]=json!(1);
 assert_eq!(assess(&r,&json!({}))["baseRent"],"8000");r["fixedUnit"]=json!(null);assert!(assess(&r,&json!({"periodMonths":6}))["baseRent"].is_null());
 r["calculationType"]=json!("areaRate");r["area"]=json!("10");r["areaUnit"]=json!("ping");r["rateUnit"]=json!("ping");r["unitRate"]=json!("80");r["ratePeriod"]=json!("month");assert_eq!(assess(&r,&json!({"periodMonths":2}))["baseRent"],"1600");r["rateUnit"]=json!("fenLand");assert!(assess(&r,&json!({}))["baseRent"].is_null());
 r["calculationType"]=json!("percentageFloor");r["minimumRent"]=json!("10000");r["minimumUnit"]=json!("cycle");r["minimumBasis"]=json!("untaxed");assert_eq!(assess(&r,&json!({"incomeAmount":"16874"}))["baseRent"],"10000");r["minimumBasis"]=json!("gross");assert!(assess(&r,&json!({"incomeAmount":"16874"}))["baseRent"].is_null());
}
#[test]
fn provisional_sources_preserve_coverage_and_duplicate_guards() {
 let r=json!({"status":"pending","validFrom":"2026-11-01","cycleAnchor":"2026-11-01","cycleMonths":1,"calculationType":"percentage","basis":"untaxed","percent":"11.5","transfer":false,"leaseEvidence":{"status":"missing","attachmentIds":[]}});
 let bills=json!([{"id":1,"docType":"renewableSurplus","start":"2026-11-01","end":"2026-11-30","purchaseEnergyFee":"16874","kwh":"20","attachmentId":1}]);
 let preview=eo_domain::rent_rules::preview(&r,"2026-11-01",bills.as_array().unwrap());assert_eq!(preview["baseRent"],"1940.51");assert!(preview["grossRent"].is_null());
 let mut duplicate=bills.as_array().unwrap().clone();duplicate.push(duplicate[0].clone());let p=eo_domain::rent_rules::preview(&r,"2026-11-01",&duplicate);assert!(p["baseRent"].is_null());assert!(p["partialSourceBase"].is_null());
 let mut partial=bills.clone();partial[0]["end"]=json!("2026-11-15");let p=eo_domain::rent_rules::preview(&r,"2026-11-01",partial.as_array().unwrap());assert!(p["baseRent"].is_null());assert_eq!(p["partialSourceBase"],"1940.51");
 let contributions=eo_domain::rent_rules::source_contributions(&r,&duplicate);assert_eq!(contributions[0]["sourceContributionRaw"],"1940.51");assert_eq!(contributions[0]["inputMode"],"actualBillContribution");
}

#[test]
fn provisional_per_receipt_respects_tax_and_formula_guards() {
 let mut r=json!({"status":"pending","validFrom":"2026-11-01","cycleAnchor":"2026-11-01","cycleMonths":1,"calculationType":"percentage","basis":"untaxed","percent":"100","rounding":"perReceipt","taxPercent":"5","taxMode":"included","transfer":false,"leaseEvidence":{"status":"missing","attachmentIds":[]}});
 let bills=json!([{"id":1,"docType":"renewableSurplus","start":"2026-11-01","end":"2026-11-15","purchaseEnergyFee":"50.3","kwh":"1","attachmentId":1},{"id":2,"docType":"renewableSurplus","start":"2026-11-16","end":"2026-11-30","purchaseEnergyFee":"50.3","kwh":"1","attachmentId":2}]);
 let run=|rule:&serde_json::Value|eo_domain::rent_rules::preview(rule,"2026-11-01",bills.as_array().unwrap());
 let p=run(&r);assert_eq!(p["baseRent"],"100");assert_eq!(p["tax"],"5");assert_eq!(p["grossRent"],"100");
 r["percent"]=json!("101");assert!(run(&r)["baseRent"].is_null());r["percent"]=json!("100");r["groupAllocationMissing"]=json!(true);assert!(run(&r)["baseRent"].is_null());r["groupAllocationMissing"]=json!(false);
 r["calculationType"]=json!("percentageFloor");r["minimumRent"]=json!("0");r["minimumUnit"]=json!("cycle");assert!(run(&r)["baseRent"].is_null());r["minimumBasis"]=json!("untaxed");assert_eq!(run(&r)["baseRent"],"100");r["minimumRent"]=json!("120");assert_eq!(run(&r)["baseRent"],"120");
}
