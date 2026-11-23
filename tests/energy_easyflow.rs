use crate::support::TestApp;
use serde_json::json;

#[actix_web::test]
async fn easyflow_draft_is_org_scoped_read_only_and_does_not_need_landlord_contract() {
    let app = TestApp::new().await;
    let (admin,org)=app.energy_org("ef-draft").await;
    let lo=app.landowner(org,"測試公司").await;
    let station=app.station(org,lo,"11-72-8007-94-7","測試案場").await;
    sqlx::query("UPDATE tbl_energy_station SET spv_payee_id=$1,data=$2 WHERE id=$3")
      .bind(lo).bind(json!({"plantNo":"32172","invoice":{"counterpartyName":"測試台電","counterpartyTaxId":"12345678"}})).bind(station)
      .execute(&app.core.pool).await.unwrap();
    let bill=app.bill(org,Some(station),"2026-09-01","2026-09-30",Some("16735"),None).await;
    sqlx::query("UPDATE tbl_energy_bill SET doc_type='renewableSurplus',review_status='pendingContract',detail_fields=$1 WHERE id=$2")
      .bind(json!({"purchaseEnergyFee":"16874","meterRentalFee":"-983"})).bind(bill).execute(&app.core.pool).await.unwrap();
    let url=app.e(org,&format!("/bills/{bill}/easyflow-application"));
    sqlx::query("UPDATE tbl_energy_bill SET detail_fields=detail_fields-'meterRentalFee',ocr_raw=$1 WHERE id=$2")
      .bind(json!({"record":{"fields":{"meterRentalSubtotal":{"value":-983,"status":"ocr_extracted"}}}}))
      .bind(bill).execute(&app.core.pool).await.unwrap();
    let v=expect_status!(app.get(&admin,&url).await,200);
    assert!(v["fields"].as_array().unwrap().iter().any(|f| f["key"]=="meterRentalFee" && f["value"]=="-983"));
    assert_eq!(v["applicationType"],"invoice");
    assert_eq!(v["submissionAllowed"],false);
    assert!(v["fields"].as_array().unwrap().iter().any(|f| f["key"]=="siteCode" && f["value"]=="32172"));
    assert!(v["fields"].as_array().unwrap().iter().any(|f| f["key"]=="amount" && f["value"]=="16874"));
    assert!(v["fields"].as_array().unwrap().iter().any(|f| f["key"]=="counterpartyName" && f["value"]=="測試台電"));
    // Bills received before master data was imported can still be previewed, without mutating workflow.
    sqlx::query("UPDATE tbl_energy_bill SET station_id=NULL,customer_number_raw='11728007947' WHERE id=$1")
      .bind(bill).execute(&app.core.pool).await.unwrap();
    let late=expect_status!(app.get(&admin,&url).await,200);
    assert!(late["fields"].as_array().unwrap().iter().any(|f| f["key"]=="siteCode" && f["value"]=="32172"));
    let revision=late["sourceRevision"].clone();
    assert!(revision.as_str().is_some());
    expect_status!(app.post(&admin,&url,json!({"sourceRevision":revision,"draftRevision":late["draftRevision"],"action":"confirm","values":{}})).await,422);
    let batch=expect_status!(app.post(&admin,&app.e(org,"/ingest-batches"),json!({"fileCount":1})).await,200);
    let (status,attachment)=crate::energy_recognition::upload_as(&app,&admin,&app.e(org,&format!("/ingest-batches/{}/attachments",batch["id"])),"source.pdf","application/pdf",b"%PDF-1.4\n% source test\n".to_vec()).await;
    assert_eq!(status,200);
    sqlx::query("UPDATE tbl_energy_bill SET source_attachment_id=$1 WHERE id=$2").bind(attachment["id"].as_i64().unwrap()).bind(bill).execute(&app.core.pool).await.unwrap();
    let with_attachment=expect_status!(app.get(&admin,&url).await,200);
    let revision=with_attachment["sourceRevision"].clone();
    let saved=expect_status!(app.post(&admin,&url,json!({"sourceRevision":revision,"draftRevision":with_attachment["draftRevision"],"action":"save","values":{"applicant":"測試承辦","department":"測試部門"}})).await,200);
    assert!(saved["fields"].as_array().unwrap().iter().any(|f|f["key"]=="applicant"&&f["value"]=="測試承辦"));
    expect_status!(app.post(&admin,&url,json!({"sourceRevision":revision,"draftRevision":with_attachment["draftRevision"],"action":"save","values":{"applicant":"舊分頁不得覆寫"}})).await,409);
    let confirmed=expect_status!(app.post(&admin,&url,json!({"sourceRevision":revision,"draftRevision":saved["draftRevision"],"action":"confirm","values":{}})).await,200);
    assert_eq!(confirmed["applicationReview"]["state"],"confirmed_for_handoff");
    assert_eq!(confirmed["applicationReview"]["reviewedBy"],admin.id);
    assert_eq!(confirmed["submissionAllowed"],false);
    expect_status!(app.post(&admin,&url,json!({"sourceRevision":revision,"action":"save","values":{"amount":"1"}})).await,400);
    sqlx::query("UPDATE tbl_energy_bill SET detail_fields=jsonb_set(detail_fields,'{purchaseEnergyFee}','\"17000\"') WHERE id=$1").bind(bill).execute(&app.core.pool).await.unwrap();
    expect_status!(app.post(&admin,&url,json!({"sourceRevision":revision,"action":"save","values":{}})).await,409);
    let changed=expect_status!(app.get(&admin,&url).await,200);
    assert_eq!(changed["applicationReview"]["stale"],true);
    sqlx::query("UPDATE tbl_energy_bill SET detail_fields=jsonb_set(detail_fields,'{purchaseEnergyFee}','\"16874\"') WHERE id=$1").bind(bill).execute(&app.core.pool).await.unwrap();
    let after=expect_status!(app.get(&admin,&app.e(org,&format!("/bills/{bill}"))).await,200);
    assert_eq!(after["reviewStatus"],"pendingContract");
    assert!(after["executions"].as_array().unwrap().is_empty());
    let nobody=app.member(org,"no-role",None).await;
    expect_status!(app.get(&nobody,&url).await,403);
    expect_status!(app.post(&nobody,&url,json!({"sourceRevision":revision,"action":"save","values":{}})).await,403);
    let (other,org2)=app.energy_org("ef-other").await;
    expect_status!(app.get(&other,&app.e(org2,&format!("/bills/{bill}/easyflow-application"))).await,403);
    expect_status!(app.post(&other,&app.e(org2,&format!("/bills/{bill}/easyflow-application")),json!({"sourceRevision":revision,"action":"save","values":{}})).await,403);
    expect_status!(app.get(&admin,&format!("{url}?format=bad")).await,400);
    let csv=app.req(Some(&admin),reqwest::Method::GET,&format!("{url}?format=csv")).send().await.unwrap();
    assert_eq!(csv.status().as_u16(),200);
    assert!(csv.headers()["content-type"].to_str().unwrap().starts_with("text/csv"));
    let csv=csv.text().await.unwrap();
    assert!(csv.contains("16874"));
    assert!(csv.contains("來源附件清單"));
    assert!(csv.contains("待確認事項"));
}
