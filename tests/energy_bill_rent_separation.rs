use crate::support::TestApp;
use eo_energy::workflow::engine::tasks::{self,ActionInput};
use eo_domain::workflow_defs::DEPT_BILL_REVIEW;
use serde_json::json;

#[actix_web::test]
async fn bill_review_without_rent_contract_rules_or_station_binding() {
 let app=TestApp::new().await; let (admin,org)=app.energy_org("split-review").await;
 let dept=app.department(org,DEPT_BILL_REVIEW,"電費審核").await;
 app.dept_member(dept,&admin,"member").await;
 for n in 0..4 {let u=app.member(org,&format!("reviewer-{n}"),Some("energy:operator")).await;app.dept_member(dept,&u,"member").await;}
 let lo=app.landowner(org,"測試").await;let station=app.station(org,lo,"SPLIT-1","測試案場").await;
 let bill=app.bill(org,Some(station),"2026-09-01","2026-09-30",Some("5035"),None).await;
 sqlx::query("UPDATE tbl_energy_bill SET review_status='pendingContract',doc_type='consumptionDevice' WHERE id=$1").bind(bill).execute(&app.core.pool).await.unwrap();
 let batch=expect_status!(app.post(&admin,&app.e(org,"/ingest-batches"),json!({"fileCount":1})).await,200);
 let (s,a)=crate::energy_recognition::upload_as(&app,&admin,&app.e(org,&format!("/ingest-batches/{}/attachments",batch["id"])),"source.pdf","application/pdf",b"%PDF-1.4\n% test\n".to_vec()).await;assert_eq!(s,200);
 sqlx::query("UPDATE tbl_energy_bill SET source_attachment_id=$1 WHERE id=$2").bind(a["id"].as_i64().unwrap()).bind(bill).execute(&app.core.pool).await.unwrap();
 let started=expect_status!(app.post(&admin,&app.e(org,&format!("/bills/{bill}/submit")),json!({})).await,200);
 let exec=started["id"].as_i64().unwrap();let task=app.tasks(exec).await.into_iter().find(|t|t.node_key=="platform-review").unwrap();
 assert_eq!(app.candidates(task.id).await.iter().filter(|c| c.0=="potentialOwner").count(),5);
 let outsider=app.member(org,"outside",Some("energy:operator")).await;
 expect_status!(app.post(&outsider,&app.e(org,&format!("/tasks/{}/claim",task.id)),json!({})).await,403);
 let ctx=app.ctx(&admin,org).await;
 in_tx!(app,tx=>tasks::claim(&mut tx,&ctx,task.id).await).unwrap();
 in_tx!(app,tx=>tasks::perform_action(&mut tx,&ctx,task.id,ActionInput{action_key:"pass".into(),..Default::default()}).await).unwrap();
 let state:String=sqlx::query_scalar("SELECT approval_state FROM tbl_energy_bill WHERE id=$1").bind(bill).fetch_one(&app.core.pool).await.unwrap();assert_eq!(state,"approved");
 assert!(!app.tasks(exec).await.iter().any(|t|t.node_key=="calc"));
 expect_status!(app.post(&admin,&app.e(org,&format!("/bills/{bill}/submit")),json!({})).await,409);

 // 收件台入口也必須走同一條獨立審核；不受舊 workflowPinned 設定影響。
 let b2=app.bill(org,Some(station),"2026-10-01","2026-10-31",Some("4770"),None).await;
 sqlx::query("UPDATE tbl_energy_bill SET review_status='pendingContract',doc_type='renewableSurplus',source_attachment_id=$1 WHERE id=$2").bind(a["id"].as_i64().unwrap()).bind(b2).execute(&app.core.pool).await.unwrap();
 let item:i64=sqlx::query_scalar("INSERT INTO tbl_energy_ingest_item (organization_id,batch_id,attachment_id,original_filename,doc_type,workflow_key,status,data,bill_id) VALUES ($1,$2,$3,'source.pdf','renewableSurplus','bill-approval','extracted','{\"workflowPinned\":true}'::jsonb,$4) RETURNING id")
    .bind(org).bind(batch["id"].as_i64().unwrap()).bind(a["id"].as_i64().unwrap()).bind(b2).fetch_one(&app.core.pool).await.unwrap();
 let ex=in_tx!(app,tx=>eo_energy::ingest::service::submit_item(&mut tx,&ctx,item,None).await).unwrap();
 assert_eq!(ex.len(),1);assert_eq!(ex[0].workflow_key.as_deref(),Some("bill-data-review"));
 assert!(in_tx!(app,tx=>eo_energy::ingest::service::submit_bill(&mut tx,&ctx,b2,None).await).is_err());
 let t=app.tasks(ex[0].id).await.into_iter().find(|t|t.node_key=="platform-review").unwrap();
 in_tx!(app,tx=>tasks::claim(&mut tx,&ctx,t.id).await).unwrap();
 in_tx!(app,tx=>tasks::perform_action(&mut tx,&ctx,t.id,ActionInput{action_key:"pass".into(),..Default::default()}).await).unwrap();
 assert!(!app.tasks(ex[0].id).await.iter().any(|t|t.node_key=="calc"));
}
