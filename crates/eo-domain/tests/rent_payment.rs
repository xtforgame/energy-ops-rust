use eo_domain::rent_payment::calculate;
use serde_json::{json,Value};
fn input()->Value {json!({"periodStart":"2026-10-01","periodEnd":"2026-10-31","payee":"地主","basis":"gross","percent":"8","taxPercent":"5","rounding":"perReceipt","transfer":true,"ruleSource":"資產主檔 AY:BC","notes":[],"receipts":[
 {"source":"taipower","start":"2026-10-01","end":"2026-10-31","amount":"5481","kwh":"1085","attachmentId":3,"page":2},
 {"source":"transfer","start":"2026-10-01","end":"2026-10-31","amount":"149069","kwh":"28394","attachmentId":3,"page":3}]})}
#[test]fn madou_and_changed_income(){let mut x=input();let v=calculate(&x).unwrap();assert_eq!(v["grossRent"],"12982");assert_eq!(v["baseRent"],"12364");assert_eq!(v["tax"],"618");x["receipts"][0]["amount"]=json!("10000");assert_ne!(calculate(&x).unwrap()["grossRent"],"12982");}
#[test]fn missing_and_duplicate_do_not_produce_payable(){let mut x=input();x["receipts"].as_array_mut().unwrap().pop();assert_eq!(calculate(&x).unwrap()["grossRent"],Value::Null);let mut x=input();let duplicate=x["receipts"][0].clone();x["receipts"].as_array_mut().unwrap().push(duplicate);assert_eq!(calculate(&x).unwrap()["grossRent"],Value::Null);}
#[test]fn suao_explicit_period_total_rounding(){let mut x=input();x["periodStart"]=json!("2026-07-16");x["periodEnd"]=json!("2026-10-18");x["basis"]=json!("untaxed");x["transfer"]=json!(false);x["rounding"]=json!("total");x["receipts"]=json!([
 {"source":"taipower","start":"2026-07-16","end":"2026-08-16","amount":"393631","kwh":"75414","attachmentId":4,"page":6},
 {"source":"taipower","start":"2026-08-17","end":"2026-09-15","amount":"241015","kwh":"46175","attachmentId":4,"page":7},
 {"source":"taipower","start":"2026-09-16","end":"2026-10-18","amount":"290220","kwh":"55602","attachmentId":4,"page":8}]);let v=calculate(&x).unwrap();assert_eq!(v["grossRent"],"77688");assert_eq!(v["tax"],"3699");assert_eq!(v["income"],"924866");}
