use crate::support::TestApp;
use serde_json::json;

#[actix_web::test]
async fn rent_cycle_persists_draft_and_validates_org_station_and_contract_dates() {
    let app=TestApp::new().await;
    let (admin,org)=app.energy_org("rent-cycle").await;
    let lo=app.landowner(org,"測試地主").await;
    let station=app.station(org,lo,"11-72-8007-94-7","週期案場").await;
    let c=expect_status!(app.post(&admin,&app.e(org,"/contracts"),json!({"contractNo":"CYCLE-1","landownerId":lo,
        "validFrom":"2026-01-01","validTo":"2026-12-31","stationIds":[station]})).await,200);
    let id=c["id"].as_i64().unwrap();
    let path=app.e(org,&format!("/contracts/{id}/rent-cycle"));
    let plan=json!({"stationId":station,"anchor":"2026-07-01","months":3,"transfer":true,
        "kind":"revenueShare","basis":"untaxed","sharePercent":"10","area":null,
        "unitPrice":null,"unitPeriod":"month","roundingPlaces":2});
    expect_status!(app.post(&admin,&path,plan.clone()).await,200);
    assert_eq!(expect_status!(app.get(&admin,&path).await,200)["plan"],plan);
    let b1=app.bill(org,Some(station),"2026-07-01","2026-09-30",Some("900"),None).await;
    let b2=app.bill(org,Some(station),"2026-07-01","2026-09-30",Some("0"),None).await;
    let rows=json!([{"billId":b1,"source":"taipower","untaxed":"1000","gross":"1050","kwh":"300"},
        {"billId":b2,"source":"transfer","untaxed":"0","gross":"0","kwh":"0"}]);
    let preview_url=format!("{path}/preview");
    let v=expect_status!(app.post(&admin,&preview_url,json!({"start":"2026-07-01","receipts":rows})).await,200);
    assert_eq!(v["baseRent"],"100.00");
    assert_eq!(v["paymentAllowed"],false);
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM tbl_energy_payout_record WHERE organization_id=$1").bind(org).fetch_one(&app.core.pool).await.unwrap();
    assert_eq!(count,0);
    expect_status!(app.post(&admin,&preview_url,json!({"start":"2027-01-01","receipts":[]})).await,422);
    let other_station=app.station(org,lo,"11-72-8007-95-0","別案場").await;
    let foreign=app.bill(org,Some(other_station),"2026-07-01","2026-09-30",Some("0"),None).await;
    expect_status!(app.post(&admin,&preview_url,json!({"start":"2026-07-01","receipts":[{"billId":foreign,"source":"transfer"}]})).await,422);
    let (outsider,org2)=app.energy_org("rent-other").await;
    expect_status!(app.get(&outsider,&app.e(org2,&format!("/contracts/{id}/rent-cycle"))).await,403);
    let nobody=app.member(org,"rent-no-role",None).await;
    expect_status!(app.post(&nobody,&path,plan).await,403);
    let candidates=expect_status!(app.get(&admin,&format!("{path}/bills?start=2026-07-01")).await,200);
    assert_eq!(candidates["items"].as_array().unwrap().len(),2);
}
