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

/// 卡 20261126-0416 S1／S2／S3：同案場兩份 R-A 合約（公司、個人地主）各存同一期零單據草稿，
/// 合併申請回 1 張、2 列、合計相加；第三份 R-0 合約進 notPayable，不進任何一期申請。
#[actix_web::test]
async fn payment_draft_without_bills_and_merged_rent_application_excludes_r0() {
    let app=TestApp::new().await;
    let (admin,org)=app.energy_org("rent-merge").await;
    // 公司地主（統編 8 碼）、個人地主（非 8 碼）、R-0 合約隨意掛一位地主即可。
    let company_lo:i64=sqlx::query_scalar("INSERT INTO tbl_energy_landowner (organization_id,name,status,tax_id) VALUES ($1,$2,'active',$3) RETURNING id")
        .bind(org).bind("合併測試公司地主").bind("12345678").fetch_one(&app.core.pool).await.unwrap();
    let individual_lo:i64=sqlx::query_scalar("INSERT INTO tbl_energy_landowner (organization_id,name,status,tax_id) VALUES ($1,$2,'active',$3) RETURNING id")
        .bind(org).bind("合併測試個人地主").bind("A123456789").fetch_one(&app.core.pool).await.unwrap();
    let r0_lo=app.landowner(org,"合併測試R0地主").await;
    let station=app.station(org,company_lo,"11-72-8100-00-1","合併測試案場").await;

    let make=|contract_no:&str,landowner:i64,rent_calc_code:&str| json!({"contractNo":contract_no,"landownerIds":[landowner],
        "validFrom":"2026-01-01","validTo":"2026-12-31","stationIds":[station],"rentCalcCode":rent_calc_code});
    let ca=expect_status!(app.post(&admin,&app.e(org,"/contracts"),make("MERGE-A",company_lo,"R-A")).await,200);
    let cb=expect_status!(app.post(&admin,&app.e(org,"/contracts"),make("MERGE-B",individual_lo,"R-A")).await,200);
    expect_status!(app.post(&admin,&app.e(org,"/contracts"),make("MERGE-C",r0_lo,"R-0")).await,200);
    let (ca,cb)=(ca["id"].as_i64().unwrap(),cb["id"].as_i64().unwrap());

    let plan=json!({"stationId":station,"anchor":"2026-07-01","months":3,"transfer":false,
        "kind":"fixedAnnual","basis":"untaxed","sharePercent":null,"area":null,
        "unitPrice":null,"unitPeriod":"month","roundingPlaces":0,"annualAmount":"4000"});
    for id in [ca,cb] {
        expect_status!(app.post(&admin,&app.e(org,&format!("/contracts/{id}/rent-cycle")),plan.clone()).await,200);
    }

    // S2：零單據（receipts:[]）存成付款草稿，200，不經 POST /bills/{id}/rent-payment。
    let draft_a=expect_status!(app.post(&admin,&app.e(org,&format!("/contracts/{ca}/rent-cycle/payment-draft")),
        json!({"start":"2026-07-01","receipts":[],"invoiceTiming":"before"})).await,200);
    assert_eq!(draft_a["baseRent"],"1000","{draft_a}");
    let draft_b=expect_status!(app.post(&admin,&app.e(org,&format!("/contracts/{cb}/rent-cycle/payment-draft")),
        json!({"start":"2026-07-01","receipts":[]})).await,200);
    assert_eq!(draft_b["baseRent"],"1000","{draft_b}");
    let bill_count:i64=app.scalar_i64(&format!("SELECT count(*) FROM tbl_energy_bill WHERE organization_id={org}")).await;
    assert_eq!(bill_count,0);

    // S1／S3：合併申請回 1 張、2 列、合計相加；R-0 在 notPayable、不進任何一期申請。
    let app_url=app.e(org,&format!("/stations/{station}/rent-application"));
    let v=expect_status!(app.get(&admin,&app_url).await,200);
    let apps=v["applications"].as_array().unwrap();
    assert_eq!(apps.len(),1,"{v}");
    let entries=apps[0]["entries"].as_array().unwrap();
    assert_eq!(entries.len(),2,"{v}");
    assert_eq!(apps[0]["total"],"2000","{v}");
    assert_eq!(apps[0]["summary"],"支付合併測試案場租金（2026-07-01～2026-09-30）");
    let vouchers:Vec<&str>=entries.iter().map(|e|e["voucher"].as_str().unwrap()).collect();
    assert!(vouchers.contains(&"有，發票"),"{v}");
    assert!(vouchers.contains(&"有，租金計算表"),"{v}");
    let not_payable=v["notPayable"].as_array().unwrap();
    assert_eq!(not_payable.len(),1,"{v}");
    assert_eq!(not_payable[0]["contractNo"],"MERGE-C");
    assert!(apps[0]["entries"].as_array().unwrap().iter().all(|e|e["contractNo"]!="MERGE-C"),"{v}");
}
