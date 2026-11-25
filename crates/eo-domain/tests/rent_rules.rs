use serde_json::json;
#[test]
fn confirmed_known_fixed_formula_is_supported_without_income_bills() {
 let r=json!({"status":"confirmed","validFrom":"2026-11-01","cycleAnchor":"2026-11-01","cycleMonths":1,"calculationType":"fixed","fixedAmount":"100","fixedUnit":"month","taxPercent":"5","rounding":"total","payee":"Owner","ruleSource":"verified","leaseEvidence":{"status":"missing","attachmentIds":[]}});
 assert!(eo_domain::rent_rules::validate(&r).is_ok());
 let p=eo_domain::rent_rules::preview(&r,"2026-11-01",&[]);assert_eq!(p["grossRent"],"105");assert_eq!(p["paymentAllowed"],false);
}
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

#[test]
fn known_group_rent_is_a_single_reference_never_station_payable() {
 let mut r=json!({"status":"pending","calculationType":"fixed","fixedAmount":"213549","fixedUnit":"year","cycleMonths":12,"groupAllocationMissing":true,"sharedGroupKey":"puxin","sharedGroupLabel":"埔心墓","sharedGroupMembers":["A","B"],"taxPercent":"5","rounding":"total"});
 let p=eo_domain::rent_rules::preview(&r,"2026-11-01",&[]);
 assert_eq!(p["groupReferenceRent"],"213549");assert_eq!(p["readiness"],"groupReference");assert_eq!(p["sharedGroupMembers"],r["sharedGroupMembers"]);assert_eq!(p["sharedGroupKey"],"puxin");
 for key in ["baseRent","baseRentRaw","tax","grossRent","grossRentRaw"]{assert!(p[key].is_null(),"{key}");}
 assert_eq!(p["paymentAllowed"],false);assert_eq!(p["missingInputs"],json!([]));
 assert_eq!(eo_domain::rent_rules::assess(&r,&json!({"periodMonths":6}))["groupReferenceRent"],"106774.5");
 r["fixedAmount"]=json!("200000");assert_eq!(eo_domain::rent_rules::preview(&r,"2026-11-01",&[])["groupReferenceRent"],"200000");
 r["fixedUnit"]=json!("month");r["fixedAmount"]=json!("100");assert_eq!(eo_domain::rent_rules::assess(&r,&json!({"periodMonths":6}))["groupReferenceRent"],"600");
 r["fixedUnit"]=json!("cycle");r["cycleMonths"]=json!(6);assert_eq!(eo_domain::rent_rules::assess(&r,&json!({"periodMonths":6}))["groupReferenceRent"],"100");
}
#[test]
fn unequal_installments_preserve_annual_amount_without_inventing_half_year_amount() {
 let r=json!({"status":"pending","calculationType":"fixed","fixedAmount":"1700000","fixedUnit":"year","cycleMonths":6,"periodAllocationUnknown":true});
 for input in [json!({}),json!({"periodMonths":6}),json!({"periodMonths":1})]{let p=eo_domain::rent_rules::assess(&r,&input);assert_eq!(p["annualRentReference"],"1700000");assert!(p["baseRent"].is_null());assert_eq!(p["missingInputs"],json!(["分期算法待確認；已知年額不推定平均分期"]));assert_eq!(p["paymentAllowed"],false);}
 assert!(eo_domain::rent_rules::preview(&r,"2026-11-01",&[])["baseRent"].is_null());
 let p=eo_domain::rent_rules::assess(&r,&json!({"periodMonths":12}));assert_eq!(p["baseRent"],"1700000");assert_eq!(p["missingInputs"],json!([]));
}

#[test]
fn non_percentage_rent_has_no_per_bill_contribution_rows() {
 let bills=json!([{"id":1,"docType":"renewableSurplus","purchaseEnergyFee":"1000"}]);
 for kind in ["fixed","areaRate","pendingUnknown"] {assert_eq!(eo_domain::rent_rules::source_contributions(&json!({"calculationType":kind}),bills.as_array().unwrap()),json!([]));}
 let p=eo_domain::rent_rules::source_contributions(&json!({"calculationType":"percentageFloor","basis":"untaxed","percent":"8"}),bills.as_array().unwrap());assert_eq!(p[0]["sourceContributionRaw"],"80");
}

#[test]
fn fixed_cycle_rent_does_not_mislabel_a_different_trial_period() {
 let r=json!({"status":"pending","calculationType":"fixed","fixedAmount":"37500","fixedUnit":"cycle","cycleMonths":3});
 for input in [json!({}),json!({"periodMonths":3})]{let p=eo_domain::rent_rules::assess(&r,&input);assert_eq!(p["baseRent"],"37500");assert_eq!(p["missingInputs"],json!([]));}
 for months in [1,6,12]{let p=eo_domain::rent_rules::assess(&r,&json!({"periodMonths":months}));assert!(p["baseRent"].is_null());assert!(p["baseRentRaw"].is_null());assert!(p["grossRent"].is_null());assert_eq!(p["cycleRentReference"],"37500");assert!(p["missingInputs"][0].as_str().unwrap().contains("試算月數與付款週期不同"));assert_eq!(p["paymentAllowed"],false);}
}

#[test]
fn owner_built_not_applicable_rent_is_not_a_zero_payment_or_missing_formula() {
 for status in ["pending","confirmed"] {
 let r=json!({"status":status,"calculationType":"notApplicable","rentRequired":false});
 for p in [eo_domain::rent_rules::assess(&r,&json!({"incomeAmount":"10000"})),eo_domain::rent_rules::preview(&r,"2026-11-01",&[])] {
 assert_eq!(p["state"],"notApplicable");assert_eq!(p["readiness"],"notApplicable");assert!(p["baseRent"].is_null());assert!(p["grossRent"].is_null());assert_eq!(p["missingInputs"],json!([]));assert_eq!(p["paymentAllowed"],false);assert!(p["formulaSummary"].as_str().unwrap().contains("不需付租金"));
 }
 assert_eq!(eo_domain::rent_rules::source_contributions(&r,&[json!({"docType":"renewableSurplus"})]),json!([]));
 }
 let r=json!({"calculationType":"notApplicable"});assert_ne!(eo_domain::rent_rules::assess(&r,&json!({}))["state"],"notApplicable");
}
#[test]
fn confirmed_extended_formulas_keep_source_and_annual_boundaries() {
 let mut r=json!({"status":"confirmed","validFrom":"2026-11-01","cycleAnchor":"2026-11-01","cycleMonths":1,"calculationType":"areaRate","area":"10","areaUnit":"ping","rateAreaUnit":"ping","unitRate":"20","ratePeriod":"month","basis":"untaxed","percent":"10","taxPercent":"5","rounding":"total","transfer":false,"payee":"Owner","ruleSource":"verified","leaseEvidence":{"status":"missing","attachmentIds":[]}});
 assert!(eo_domain::rent_rules::validate(&r).is_ok());assert_eq!(eo_domain::rent_rules::preview(&r,"2026-11-01",&[])["grossRent"],"210");
 let mut bills=json!([{"id":1,"docType":"renewableSurplus","start":"2026-11-01","end":"2026-11-30","purchaseEnergyFee":"1000","kwh":"20","attachmentId":1}]);
 r["calculationType"]=json!("percentageFloor");r["minimumRent"]=json!("200");r["minimumUnit"]=json!("cycle");r["minimumBasis"]=json!("untaxed");assert!(eo_domain::rent_rules::validate(&r).is_ok());assert_eq!(eo_domain::rent_rules::preview(&r,"2026-11-01",bills.as_array().unwrap())["grossRent"],"210");assert!(eo_domain::rent_rules::preview(&r,"2026-11-01",&[])["grossRent"].is_null());
 let duplicate=vec![bills[0].clone(),bills[0].clone()];assert!(eo_domain::rent_rules::preview(&r,"2026-11-01",&duplicate)["grossRent"].is_null());
 r["status"]=json!("pending");let p=eo_domain::rent_rules::preview(&r,"2026-11-01",bills.as_array().unwrap());assert_eq!(p["status"],"pending");assert_eq!(p["grossRent"],"210");
 r["status"]=json!("confirmed");r["calculationType"]=json!("annualReference");r["annualRent"]=json!("100");r["tolerancePercent"]=json!("5");r["cycleMonths"]=json!(12);bills[0]["end"]=json!("2027-10-31");assert!(eo_domain::rent_rules::validate(&r).is_ok());assert_eq!(eo_domain::rent_rules::preview(&r,"2026-11-01",bills.as_array().unwrap())["grossRent"],"105");bills[0]["purchaseEnergyFee"]=json!("1050");assert!(eo_domain::rent_rules::preview(&r,"2026-11-01",bills.as_array().unwrap())["grossRent"].is_null());
}
#[test]
fn confirmed_annual_preview_preserves_reference_ratio() {
 let r=json!({"status":"confirmed","validFrom":"2026-01-01","cycleAnchor":"2026-01-01","cycleMonths":12,"calculationType":"annualReference","annualRent":"100","tolerancePercent":"5","basis":"untaxed","percent":"10","taxPercent":"0","rounding":"total","transfer":false,"payee":"Owner","ruleSource":"verified","leaseEvidence":{"status":"missing","attachmentIds":[]}});
 let bills=json!([{"id":1,"docType":"renewableSurplus","start":"2026-01-01","end":"2026-12-31","purchaseEnergyFee":"1000","kwh":"20","attachmentId":1}]);
 let p=eo_domain::rent_rules::preview(&r,"2026-01-01",bills.as_array().unwrap());assert_eq!(p["referenceRatioPercent"],"100");assert_eq!(p["state"],"calculatedDraft");
}
#[test]
fn source_lines_show_actual_percentage_contributions_not_coverage_probe_zero() {
 let r=json!({"status":"confirmed","validFrom":"2026-11-01","cycleAnchor":"2026-11-01","cycleMonths":1,"calculationType":"percentage","basis":"gross","percent":"8","taxPercent":"5","rounding":"perReceipt","transfer":true,"payee":"麻豆","ruleSource":"verified","leaseEvidence":{"status":"missing","attachmentIds":[]}});
 let bills=json!([{"id":1,"docType":"renewableSurplus","start":"2026-11-01","end":"2026-11-30","electricityFeeTotal":"5481","kwh":"1","attachmentId":1},{"id":2,"docType":"retailerSale","start":"2026-11-01","end":"2026-11-30","electricityFeeTotal":"149069","kwh":"1","attachmentId":2}]);
 let p=eo_domain::rent_rules::preview(&r,"2026-11-01",bills.as_array().unwrap());assert_eq!(p["grossRent"],"12982");assert_eq!(p["lines"][0]["proportionalRent"],"438.48");assert_eq!(p["lines"][0]["roundedRent"],"438");assert_eq!(p["lines"][1]["proportionalRent"],"11925.52");assert_eq!(p["lines"][1]["roundedRent"],"11926");
 for kind in ["percentageFloor","annualReference"] {let mut pending=r.clone();pending["status"]=json!("pending");pending["calculationType"]=json!(kind);let p=eo_domain::rent_rules::preview(&pending,"2026-11-01",bills.as_array().unwrap());assert_eq!(p["lines"][0]["proportionalRent"],"438.48");assert_eq!(p["lines"][0]["contributionOnly"],true);}
}
