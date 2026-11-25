use crate::support::TestApp;
use serde_json::json;
#[actix_web::test]
async fn rental_handoff_persists_versions_and_any_group_member_can_confirm(){
 let app=TestApp::new().await;let(admin,org)=app.energy_org("rental-handoff").await;
 let owner=app.landowner(org,"Synthetic owner").await;let sid=app.station(org,owner,"HANDOFF","Synthetic station").await;
 let rule=json!({"status":"confirmed","validFrom":"2026-11-01","cycleAnchor":"2026-11-01","cycleMonths":1,"calculationType":"fixed","fixedAmount":"100","fixedUnit":"month","taxPercent":"5","rounding":"total","payee":"Synthetic owner","ruleSource":"verified","leaseEvidence":{"status":"missing","attachmentIds":[]}});
 expect_status!(app.post(&admin,&app.e(org,&format!("/stations/{sid}/rent-rules")),rule).await,200);

 let read=app.e(org,&format!("/stations/{sid}/rent-payment"));let write=format!("{read}/application");let read=format!("{read}?periodStart=2026-11-01");
 let batch=expect_status!(app.post(&admin,&app.e(org,"/ingest-batches"),json!({"fileCount":1})).await,200);
 let(status,attachment)=crate::energy_recognition::upload_as(&app,&admin,&app.e(org,&format!("/ingest-batches/{}/attachments",batch["id"])),"voucher.pdf","application/pdf",b"%PDF-1.4\n% synthetic voucher\n".to_vec()).await;assert_eq!(status,200);
 let item=expect_status!(app.get(&admin,&read).await,200)["items"][0].clone();
 let mut body=json!({"ruleId":item["ruleId"],"periodStart":item["periodStart"],"periodEnd":item["periodEnd"],"sourceRevision":item["application"]["sourceRevision"],"draftRevision":0,"fields":{"applicant":"Synthetic clerk"},"invoiceTiming":"later","voucherAttachmentIds":[attachment["id"]],"action":"save"});
 let saved=expect_status!(app.post(&admin,&write,body.clone()).await,200);assert_eq!(saved["application"]["draftRevision"],1);assert_eq!(saved["invoiceTiming"],"later");assert_eq!(saved["application"]["fields"]["applicant"],"Synthetic clerk");
 expect_status!(app.post(&admin,&write,body.clone()).await,409);
 let clerk=app.member(org,"rent-clerk",Some("energy:operator")).await;
 body["draftRevision"]=json!(1);body["action"]=json!("confirm");expect_status!(app.post(&clerk,&write,body.clone()).await,403);
 let dept:i64=sqlx::query_scalar("INSERT INTO tbl_energy_department(organization_id,key,name) VALUES($1,$2,'Rental review') RETURNING id").bind(org).bind(eo_domain::workflow_defs::DEPT_RENT_REVIEW).fetch_one(&app.core.pool).await.unwrap();
 sqlx::query("INSERT INTO mn_energy_department_member(department_id,user_id,role) VALUES($1,$2,'member')").bind(dept).bind(clerk.id).execute(&app.core.pool).await.unwrap();
 let confirmed=expect_status!(app.post(&clerk,&write,body.clone()).await,200);assert_eq!(confirmed["application"]["state"],"dataConfirmed");assert_eq!(confirmed["application"]["confirmedBy"],clerk.id);assert!(!confirmed["application"]["missingFields"].as_array().unwrap().is_empty());assert_eq!(confirmed["paymentAllowed"],false);
 let bill_count:i64=sqlx::query_scalar("SELECT count(*) FROM tbl_energy_bill WHERE station_id=$1").bind(sid).fetch_one(&app.core.pool).await.unwrap();assert_eq!(bill_count,0);
 let refresh=expect_status!(app.get(&admin,&read).await,200);assert_eq!(refresh["items"][0]["application"]["state"],"dataConfirmed");
 sqlx::query("UPDATE tbl_energy_attachment SET deleted_at=clock_timestamp() WHERE id=$1").bind(attachment["id"].as_i64().unwrap()).execute(&app.core.pool).await.unwrap();
 let missing_voucher=expect_status!(app.get(&admin,&read).await,200);assert_eq!(missing_voucher["items"][0]["application"]["state"],"stale");body["draftRevision"]=json!(2);expect_status!(app.post(&clerk,&write,body.clone()).await,404);
 sqlx::query("UPDATE tbl_energy_station_rent_rule SET rule=jsonb_set(rule,'{fixedAmount}','\"101\"'::jsonb),version=version+1 WHERE station_id=$1").bind(sid).execute(&app.core.pool).await.unwrap();
 let stale=expect_status!(app.get(&admin,&read).await,200);assert_eq!(stale["items"][0]["application"]["state"],"stale");assert_eq!(stale["items"][0]["application"]["fields"]["applicant"],"Synthetic clerk");
 body["draftRevision"]=json!(2);expect_status!(app.post(&clerk,&write,body.clone()).await,409);
 sqlx::query("UPDATE tbl_energy_station_rent_rule SET rule=jsonb_set(rule,'{status}','\"pending\"'::jsonb),status='pending',version=version+1 WHERE station_id=$1").bind(sid).execute(&app.core.pool).await.unwrap();
 let pending=expect_status!(app.get(&admin,&read).await,200)["items"][0].clone();body["sourceRevision"]=pending["application"]["sourceRevision"].clone();expect_status!(app.post(&clerk,&write,body).await,422);

}
