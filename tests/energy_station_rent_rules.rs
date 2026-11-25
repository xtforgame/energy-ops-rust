use crate::support::TestApp;
use serde_json::json;
#[actix_web::test]
async fn reusable_rules_are_scoped_versioned_and_lease_independent() {
 let app=TestApp::new().await;let(admin,org)=app.energy_org("reusable-rules").await;
 let lo=app.landowner(org,"Synthetic owner").await;let sid=app.station(org,lo,"RULE-TEST","Synthetic station").await;
 let path=app.e(org,&format!("/stations/{sid}/rent-rules"));
 let mut rule=json!({"status":"confirmed","validFrom":"2026-10-01","cycleAnchor":"2026-10-01","cycleMonths":1,"basis":"gross","percent":"8","taxPercent":"5","rounding":"total","transfer":false,"payee":"Synthetic owner","ruleSource":"verified","leaseEvidence":{"status":"missing","attachmentIds":[]}});
 let saved=expect_status!(app.post(&admin,&path,rule.clone()).await,200);
 assert_eq!(saved["version"],1);
 expect_status!(app.post(&admin,&path,rule.clone()).await,409);
 rule["id"]=saved["id"].clone();rule["expectedVersion"]=json!(1);rule["percent"]=json!("9");
 let changed=expect_status!(app.post(&admin,&path,rule.clone()).await,200);assert_eq!(changed["version"],2);
 expect_status!(app.post(&admin,&path,rule.clone()).await,409);
 let read=expect_status!(app.get(&admin,&path).await,200);assert_eq!(read["items"].as_array().unwrap().len(),1);
 let bill=app.bill(org,Some(sid),"2026-11-01","2026-11-30",Some("99999"),Some("20")).await;
 sqlx::query("UPDATE tbl_energy_bill SET doc_type='renewableSurplus',detail_fields='{\"electricityFeeTotal\":\"1000\",\"payableTotal\":\"99999\"}'::jsonb WHERE id=$1").bind(bill).execute(&app.core.pool).await.unwrap();
 let preview=app.e(org,&format!("/bills/{bill}/rent-payment"));
 assert!(expect_status!(app.get(&admin,&preview).await,200)["items"][0]["grossRent"].is_null());
 let batch=expect_status!(app.post(&admin,&app.e(org,"/ingest-batches"),json!({"fileCount":1})).await,200);
 let(status,attachment)=crate::energy_recognition::upload_as(&app,&admin,&app.e(org,&format!("/ingest-batches/{}/attachments",batch["id"])),"synthetic.pdf","application/pdf",b"%PDF-1.4\n% synthetic\n".to_vec()).await;assert_eq!(status,200);
 sqlx::query("UPDATE tbl_energy_bill SET source_attachment_id=$1 WHERE id=$2").bind(attachment["id"].as_i64().unwrap()).bind(bill).execute(&app.core.pool).await.unwrap();
 let result=expect_status!(app.get(&admin,&preview).await,200);assert_eq!(result["items"][0]["grossRent"],"95");assert_eq!(result["items"][0]["evidenceStatus"],"missing");assert_eq!(result["items"][0]["inputMethod"],"autoBills");
 rule["expectedVersion"]=json!(2);rule["percent"]=json!("8");rule["rounding"]=json!("perReceipt");rule["transfer"]=json!(true);
 expect_status!(app.post(&admin,&path,rule.clone()).await,200);
 sqlx::query("UPDATE tbl_energy_bill SET detail_fields='{\"electricityFeeTotal\":\"5481\"}'::jsonb WHERE id=$1").bind(bill).execute(&app.core.pool).await.unwrap();
 assert!(expect_status!(app.get(&admin,&preview).await,200)["items"][0]["grossRent"].is_null());
 let transfer=app.bill(org,Some(sid),"2026-11-01","2026-11-30",Some("149069"),Some("28394")).await;
 sqlx::query("UPDATE tbl_energy_bill SET doc_type='retailerSale',detail_fields='{\"electricityFeeTotal\":\"149069\"}'::jsonb,source_attachment_id=$1 WHERE id=$2").bind(attachment["id"].as_i64().unwrap()).bind(transfer).execute(&app.core.pool).await.unwrap();
 let station_preview=app.e(org,&format!("/stations/{sid}/rent-payment"));
 assert_eq!(expect_status!(app.get(&admin,&station_preview).await,200)["items"][0]["grossRent"],"12982");
 // A manual October worksheet blocks auto quarter previews even when viewed from a November bill.
 rule["expectedVersion"]=json!(3);rule["cycleMonths"]=json!(3);expect_status!(app.post(&admin,&path,rule.clone()).await,200);
 sqlx::query("UPDATE tbl_energy_station SET data=jsonb_set(data,'{rentPaymentDrafts}','[{\"periodStart\":\"2026-10-01\",\"periodEnd\":\"2026-10-31\"}]'::jsonb) WHERE id=$1").bind(sid).execute(&app.core.pool).await.unwrap();
 let blocked=expect_status!(app.get(&admin,&preview).await,200);assert!(blocked["items"][0]["grossRent"].is_null());assert!(blocked["items"][0]["issues"][0].as_str().unwrap().contains("人工歷史"));
 let ctx=app.ctx(&admin,org).await;
 assert!(in_tx!(app,tx=>eo_energy::ingest::bills::match_station(&mut tx,&ctx,bill,sid).await).is_ok());
 let outsider=app.member(org,"no-role",None).await;expect_status!(app.post(&outsider,&path,rule).await,403);
 let(_,other)=app.energy_org("other-rules").await;
 expect_status!(app.get(&admin,&app.e(other,&format!("/stations/{sid}/rent-rules"))).await,403);
}
