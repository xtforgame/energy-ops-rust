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
    assert_eq!(v["baseRent"],"100");
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
    expect_status!(app.post(&nobody,&path,plan.clone()).await,403);
    let candidates=expect_status!(app.get(&admin,&format!("{path}/bills?start=2026-07-01")).await,200);
    assert_eq!(candidates["items"].as_array().unwrap().len(),2);
    let mut management=plan;
    management["transfer"]=json!(false);management["sharePercent"]=json!("8");
    management["feeKind"]=json!("management");management["monthlyMinimum"]=json!("8000");
    management["commercialOperationDate"]=json!("2026-01-01");
    expect_status!(app.post(&admin,&path,management.clone()).await,200);
    assert_eq!(expect_status!(app.get(&admin,&path).await,200)["plan"],management);
    let mut monthly=vec![];
    for (start,end,amount) in [("2026-07-01","2026-07-31","100000"),("2026-08-01","2026-08-31","200000"),("2026-09-01","2026-09-30","0")] {
        let bill=app.bill(org,Some(station),start,end,Some(amount),None).await;
        monthly.push(json!({"billId":bill,"source":"taipower","untaxed":amount,"gross":null,"kwh":"0"}));
    }
    let v=expect_status!(app.post(&admin,&preview_url,json!({"start":"2026-07-01","receipts":monthly})).await,200);
    assert_eq!(v["baseRent"],"32000");assert_eq!(v["monthlyBreakdown"].as_array().unwrap().len(),3);
    assert_eq!(v["feeKind"],"management");assert_eq!(v["paymentAllowed"],false);
    let mut invalid=management;invalid["commercialOperationDate"]=json!(null);
    expect_status!(app.post(&admin,&path,invalid).await,422);
}

/// 卡 20261126-0210 S2：合約改綁到別的案場時，rent-cycle 回 422 帶確認出口。
/// A 案場存過試算 → 合約改綁 B → 不帶確認存 B 回 422（details 有 from/to） → 帶確認存 B 回 200。
#[actix_web::test]
async fn rent_cycle_rebind_requires_confirmation() {
    let app=TestApp::new().await;
    let (admin,org)=app.energy_org("rent-rebind").await;
    let lo=app.landowner(org,"改綁地主").await;
    let station_a=app.station(org,lo,"11-72-9001-00-1","改綁案場A").await;
    let station_b=app.station(org,lo,"11-72-9001-00-2","改綁案場B").await;
    let c=expect_status!(app.post(&admin,&app.e(org,"/contracts"),json!({"contractNo":"REBIND-1","landownerId":lo,
        "validFrom":"2026-01-01","validTo":"2026-12-31","stationIds":[station_a,station_b]})).await,200);
    let id=c["id"].as_i64().unwrap();
    let path=app.e(org,&format!("/contracts/{id}/rent-cycle"));
    let plan_a=json!({"stationId":station_a,"anchor":"2026-07-01","months":3,"transfer":true,
        "kind":"revenueShare","basis":"untaxed","sharePercent":"10","area":null,
        "unitPrice":null,"unitPeriod":"month","roundingPlaces":2});
    expect_status!(app.post(&admin,&path,plan_a).await,200);

    // 合約改綁到 B：存 B 的試算，不帶確認回 422，details 有 from/to。
    let plan_b=json!({"stationId":station_b,"anchor":"2026-08-01","months":1,"transfer":false,
        "kind":"revenueShare","basis":"untaxed","sharePercent":"5","area":null,
        "unitPrice":null,"unitPeriod":"month","roundingPlaces":2});
    let blocked=expect_status!(app.post(&admin,&path,plan_b.clone()).await,422);
    assert_eq!(blocked["code"],"rent_multisite_contract_requires_confirmation","{blocked}");
    assert_eq!(blocked["details"]["fromStationId"],station_a,"{blocked}");
    assert_eq!(blocked["details"]["toStationId"],station_b,"{blocked}");

    // 帶確認覆蓋存檔：200，plan.stationId 是 B。
    let confirmed=expect_status!(app.post(&admin,&format!("{path}?confirmStationChange=true"),plan_b.clone()).await,200);
    assert_eq!(confirmed["plan"]["stationId"],station_b,"{confirmed}");
    assert_eq!(expect_status!(app.get(&admin,&path).await,200)["plan"],plan_b);
}
