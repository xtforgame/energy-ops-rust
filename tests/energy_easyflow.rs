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
    let v=expect_status!(app.get(&admin,&url).await,200);
    assert_eq!(v["applicationType"],"invoice");
    assert_eq!(v["submissionAllowed"],false);
    assert!(v["fields"].as_array().unwrap().iter().any(|f| f["key"]=="siteCode" && f["value"]=="32172"));
    assert!(v["fields"].as_array().unwrap().iter().any(|f| f["key"]=="amount" && f["value"]=="16874"));
    assert!(v["fields"].as_array().unwrap().iter().any(|f| f["key"]=="counterpartyName" && f["value"]=="測試台電"));
    let after=expect_status!(app.get(&admin,&app.e(org,&format!("/bills/{bill}"))).await,200);
    assert_eq!(after["reviewStatus"],"pendingContract");
    assert!(after["executions"].as_array().unwrap().is_empty());
    let nobody=app.member(org,"no-role",None).await;
    expect_status!(app.get(&nobody,&url).await,403);
    let (other,org2)=app.energy_org("ef-other").await;
    expect_status!(app.get(&other,&app.e(org2,&format!("/bills/{bill}/easyflow-application"))).await,403);
    expect_status!(app.get(&admin,&format!("{url}?format=bad")).await,400);
    let csv=app.req(Some(&admin),reqwest::Method::GET,&format!("{url}?format=csv")).send().await.unwrap();
    assert_eq!(csv.status().as_u16(),200);
    assert!(csv.headers()["content-type"].to_str().unwrap().starts_with("text/csv"));
    let csv=csv.text().await.unwrap();
    assert!(csv.contains("16874"));
    assert!(csv.contains("來源附件清單"));
    assert!(csv.contains("待確認事項"));
}
