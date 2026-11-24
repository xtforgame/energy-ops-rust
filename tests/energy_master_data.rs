//! 主檔讀取（地主／案場／整合頁／兩條效期軸）、儀表板、地址推估座標。

use crate::support::energy::code_of;
use crate::support::{TestApp, TestUser};
use eo_domain::geocoder::{is_usable_precision, mock_geocoder, GeocodePrecision};
use eo_energy::common::Ctx;
use eo_energy::master_data::{self, landowner_overview, station_overview};
use eo_energy::workbench;
use eo_energy::workflow::model::TaskRow;
use eo_energy::workflow::views;
use reqwest::Method;
use serde_json::{json, Value};
use std::fs;

struct Fx {
    app: TestApp,
    org: i64,
    other_org: i64,
    owner: TestUser,
    other_owner: TestUser,
    landowner: i64,
    station: i64,
    bill: i64,
}

async fn fixture() -> Fx {
    let app = TestApp::new().await;
    let (owner, org) = app.energy_org("c1-org").await;
    let (other_owner, other_org) = app.energy_org("c1-other").await;
    let landowner: i64 = sqlx::query_scalar(
        "INSERT INTO tbl_energy_landowner (organization_id, name, tax_id, contact_name, contact_phone, bank_name, bank_branch,
                                            bank_account, bank_account_name, status)
         VALUES ($1, '陳地主', 'A123456789', '陳小姐', '0912-345-678', '台灣銀行', '屏東分行', '001-234-567890', '陳地主', 'active') RETURNING id",
    )
    .bind(org)
    .fetch_one(&app.core.pool)
    .await
    .unwrap();
    let station = app.station(org, landowner, "01-0001", "屏東一號案場").await;
    app.exec_sql(&format!("UPDATE tbl_energy_station SET latitude = 22.6789012, longitude = 120.4881000 WHERE id = {station}")).await;
    let abnormal = app.station(org, landowner, "02-0001", "屏東二號案場").await;
    app.exec_sql(&format!("UPDATE tbl_energy_station SET status = 'abnormal' WHERE id = {abnormal}")).await;
    let contract = app.contract(org, landowner, "CT-C1", "2026-01-01", None).await;
    app.exec_sql(&format!("UPDATE tbl_energy_contract SET stage = 'construction', installment_total = 3 WHERE id = {contract}")).await;
    app.contract_station(contract, station, json!({}), None, None).await;
    for n in 1..=3 {
        let i = app.installment(org, contract, n, &format!("2026-0{n}-15"), "100000.00").await;
        if n == 1 {
            app.exec_sql(&format!("UPDATE tbl_energy_installment SET status = 'paid' WHERE id = {i}")).await;
        }
    }
    app.exec_sql(&format!(
        "INSERT INTO tbl_energy_calendar_event (organization_id, landowner_id, contract_id, event_date, kind, title, status)
         VALUES ({org}, {landowner}, {contract}, '2026-02-15', 'installmentDue', '保證金第 2 期到期', 'pending')"
    ))
    .await;
    let bill = app.bill(org, Some(station), "2026-01-01", "2026-02-28", Some("50000.00"), None).await;
    Fx { app, org, other_org, owner, other_owner, landowner, station, bill }
}

impl Fx {
    async fn ctx(&self) -> Ctx {
        self.app.ctx(&self.owner, self.org).await
    }
    async fn payout(&self, status: &str, amount: &str) {
        let s = self.app.strategy(self.org, "rs", "revenueShare", json!({ "sharePercent": 10 }), "一成分潤").await;
        self.app
            .exec_sql(&format!(
                "INSERT INTO tbl_energy_payout_record (organization_id, bill_id, station_id, strategy_id, strategy_type, params_snapshot,
                   input_snapshot, amount, currency, business_date, computed_at, trigger, status)
                 VALUES ({}, {}, {}, {s}, 'revenueShare', '{{\"sharePercent\":10}}', '{{}}', {amount}, 'TWD', '2026-03-01', now(), 'workflowNode', '{status}')",
                self.org, self.bill, self.station
            ))
            .await;
    }
}

/// 來源：masterData.test.ts「landowner list counts stations and flags the abnormal ones」
/// 來源：masterData.test.ts「another org never sees this landowner」
/// 來源：masterData.test.ts「cross-org detail is 403 — NOT 404 (404 would leak "this id exists")」
/// 來源：masterData.test.ts「station list carries coordinates as STRINGS (不轉 Number，地圖要精度)」
/// 來源：masterData.test.ts「detail returns money AND time in one shot」
/// 來源：masterData.test.ts「only the CURRENT payout shows up — superseded ones belong to the audit trail」
#[actix_web::test]
async fn landowners_and_stations() {
    let f = fixture().await;
    let ctx = f.ctx().await;
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let rows = master_data::list_landowners(&mut tx, &ctx.access, None, None, None).await.unwrap();
    let mine = rows.iter().find(|r| r["id"] == f.landowner).unwrap();
    assert_eq!((mine["stationCount"].clone(), mine["abnormalStationCount"].clone(), mine["contractCount"].clone()), (json!(2), json!(1), json!(1)));
    let other = f.app.ctx(&f.other_owner, f.other_org).await;
    let rows = master_data::list_landowners(&mut tx, &other.access, None, None, None).await.unwrap();
    assert!(!rows.iter().any(|r| r["id"] == f.landowner));
    assert_eq!(code_of(master_data::get_landowner(&mut tx, &other.access, f.landowner).await), (403, "landowner_not_in_org".into()));

    let st = master_data::list_stations(&mut tx, &ctx.access, None, None, None, None, None).await.unwrap();
    let s = st.iter().find(|r| r["id"] == f.station).unwrap();
    assert!(s["latitude"].as_str().unwrap().starts_with("22.678"), "座標回字串");
    assert_eq!(s["landownerName"], "陳地主");
    let q = master_data::list_stations(&mut tx, &ctx.access, Some("二號"), None, None, None, None).await.unwrap();
    assert_eq!(q.len(), 1);

    let d = master_data::get_landowner(&mut tx, &ctx.access, f.landowner).await.unwrap();
    assert_eq!((d["bankName"].clone(), d["bankAccount"].clone()), (json!("台灣銀行"), json!("001-234-567890")));
    assert_eq!(d["stations"].as_array().unwrap().len(), 2);
    let c = &d["contracts"][0];
    assert_eq!((c["stage"].clone(), c["installmentTotal"].clone(), c["installmentPaid"].clone()), (json!("construction"), json!(3), json!(1)));
    assert_eq!(c["installments"].as_array().unwrap().len(), 3);
    assert_eq!(c["stationName"], "屏東一號案場", "契約涵蓋的案場");
    assert_eq!(d["calendarEvents"][0]["kind"], "installmentDue");
    let b = d["recentBills"].as_array().unwrap().iter().find(|b| b["id"] == f.bill).unwrap();
    assert_eq!(b["stationName"], "屏東一號案場");
    assert!(b["payoutAmount"].is_null(), "還沒算應付 → null，不是 0");
    drop(tx);

    f.payout("superseded", "4000.00").await;
    f.payout("active", "5000.00").await;
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let d = master_data::get_landowner(&mut tx, &ctx.access, f.landowner).await.unwrap();
    let b = d["recentBills"].as_array().unwrap().iter().find(|b| b["id"] == f.bill).unwrap();
    assert_eq!((b["payoutAmount"].clone(), b["payoutStrategyType"].clone()), (json!("5000.00"), json!("revenueShare")));
}

async fn task_on(f: &Fx, subject: &str, node_name: &str, state: &str, assignee: Option<i64>) -> TaskRow {
    let def: i64 = sqlx::query_scalar(
        "INSERT INTO tbl_energy_workflow_definition (organization_id, key, version, name, subject_type, status)
         VALUES ($1, $2, 1, '測試流程', 'energyBill', 'published') RETURNING id",
    )
    .bind(f.org)
    .bind(format!("def-{subject}"))
    .fetch_one(&f.app.core.pool)
    .await
    .unwrap();
    f.app.exec_sql(&format!("INSERT INTO tbl_energy_workflow_node (definition_id, node_key, name, node_type) VALUES ({def}, 'gate1', '{node_name}', 'humanTask')")).await;
    let exec: i64 = sqlx::query_scalar(
        "INSERT INTO tbl_energy_workflow_execution (organization_id, definition_id, subject_type, subject_id, business_date, status, current_node_key, station_key, started_at)
         VALUES ($1, $2, 'energyBill', $3, '2026-03-01', 'running', 'gate1', 'test', now()) RETURNING id",
    )
    .bind(f.org)
    .bind(def)
    .bind(subject)
    .fetch_one(&f.app.core.pool)
    .await
    .unwrap();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO tbl_energy_workflow_task (execution_id, node_key, node_type, seq, state, assignee_user_id, created_at)
         VALUES ($1, 'gate1', 'humanTask', 1, $2, $3, now()) RETURNING id",
    )
    .bind(exec)
    .bind(state)
    .bind(assignee)
    .fetch_one(&f.app.core.pool)
    .await
    .unwrap();
    let mut tx = f.app.core.pool.begin().await.unwrap();
    eo_energy::workflow::model::task_by_id(&mut tx, id).await.unwrap().unwrap()
}

/// 來源：masterData.test.ts「decorateMyTasks survives a NON-NUMERIC subjectId (不可炸 column "nan")」
/// 來源：masterData.test.ts「a numeric subjectId DOES bring the bill summary along」
#[actix_web::test]
async fn task_rows_survive_non_numeric_subjects() {
    let f = fixture().await;
    let ctx = f.ctx().await;
    let t = task_on(&f, "BILL-2026-05-0001", "承辦確認", "ready", None).await;
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let rows = views::decorate_tasks(&mut tx, &ctx, f.owner.id, &[t]).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0]["subject"].is_null(), "非數字 subjectId → 沒有單據摘要，但不可以炸");
    assert_eq!(rows[0]["nodeName"], "承辦確認");
    // 這列沒有候選人，擁有者認領不了：claimable 是「我認領得了嗎」，不是「task 是不是 ready」
    // （App 5 在這支回 true、在 executions/{id}/actions 回 false，這裡統一成後者）。
    assert_eq!((rows[0]["claimable"].clone(), rows[0]["claimedByMe"].clone()), (json!(false), json!(false)));
    drop(tx);
    let t = task_on(&f, &f.bill.to_string(), "財務放行", "reserved", Some(f.owner.id)).await;
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let rows = views::decorate_tasks(&mut tx, &ctx, f.owner.id, &[t]).await.unwrap();
    let s = &rows[0]["subject"];
    assert_eq!(s["billId"], f.bill);
    assert_eq!(s["stationName"], "屏東一號案場");
    assert_eq!(s["customerNumber"], "01-0001", "電號優先用指認到的案場");
    assert_eq!(s["totalAmount"], "50000.00");
    assert_eq!(rows[0]["claimedByMe"], true);
    assert!(rows[0]["waitingDays"].is_number());
}

/// 來源：masterData.test.ts「dashboard 的月份是「業務日期」的月份，不是計算時間」
/// 來源：masterData.test.ts「卡關門檻是參數（不必改 schema 就能調）」
/// 來源：masterData.test.ts「案場狀態分布數得出來」
/// 來源：masterData.test.ts「稽核的時間區間：`to` 要含**當天整天**（不是 <= 當天 00:00）」
/// G4：拿掉「dashboard 只加總現行應付、不把被取代的算進去」—— 首頁不再查應付，這段斷言跟著拿掉
/// （`payout_provenance_and_immutability`／`energy_rating_bindings.rs` 還驗應付紀錄本身的現行判定）。
#[actix_web::test]
async fn dashboard_and_audit_day_range() {
    let f = fixture().await;
    let ctx = f.ctx().await;
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let strict = workbench::dashboard(&mut tx, &ctx, Some("2026-03"), 0).await.unwrap();
    let loose = workbench::dashboard(&mut tx, &ctx, Some("2026-03"), 3650).await.unwrap();
    assert_eq!((strict["stuckDays"].clone(), loose["stuckDays"].clone()), (json!(0), json!(3650)));
    assert!(loose["stuckCount"].as_i64() <= strict["stuckCount"].as_i64());
    let d = workbench::dashboard(&mut tx, &ctx, Some("2026-03"), 3).await.unwrap();
    assert_eq!(d["stationTotal"], 2);
    assert_eq!(d["stationByStatus"], json!({ "operating": 1, "abnormal": 1 }));
    drop(tx);

    // 稽核區間：今天到今天要含今天整天。先留一筆稽核（推估座標會寫）。
    let geo = f.app.station(f.org, f.landowner, "GEO-0", "定位").await;
    f.app.exec_sql(&format!("UPDATE tbl_energy_station SET address = '屏東縣枋寮鄉中山路 100 號' WHERE id = {geo}")).await;
    in_tx!(f.app, tx => master_data::apply_geocode(&mut tx, &ctx, geo).await).unwrap();
    let today = eo_energy::common::day(ctx.now().date_naive());
    let all = expect_status!(f.app.get(&f.owner, &f.app.e(f.org, "/audit-events")).await, 200);
    let todays = expect_status!(f.app.get(&f.owner, &f.app.e(f.org, &format!("/audit-events?from={today}&to={today}"))).await, 200);
    assert!(!all["items"].as_array().unwrap().is_empty());
    assert_eq!(all["items"].as_array().unwrap().len(), todays["items"].as_array().unwrap().len());
    expect_status!(f.app.get(&f.owner, &f.app.e(f.org, "/audit-events?from=-01")).await, 400);
    expect_status!(f.app.get(&f.owner, &f.app.e(f.org, "/calendar-events?to=2026-1")).await, 400);
    let cal = expect_status!(f.app.get(&f.owner, &f.app.e(f.org, "/calendar-events?from=2026-02-01&to=2026-02-15")).await, 200);
    assert_eq!(cal["items"][0]["landownerName"], "陳地主");
}

/// 來源：masterData.test.ts「mockGeocoder 查不到就回 null —— **絕不**回退成行政區中心點」
/// 來源：masterData.test.ts「precision 是一級欄位：地段地號只回 locality，不可當座標用」
/// 來源：masterData.test.ts「沒地址 / 查不到 / 精度不足 —— suggest 都回 usable=false 並說明理由」
/// 來源：masterData.test.ts「suggest 不寫入 —— 只是建議值」
/// 來源：masterData.test.ts「apply 才寫入，並記下 source / precision（換 provider 時才知道哪些該重跑）」
/// 來源：masterData.test.ts「精度不足時 apply 直接拒絕（422），不會「先寫進去再說」」
/// 來源：masterData.test.ts「`geocodeSource=manual` 的座標**永遠優先** —— 自動 geocode 不覆蓋」
#[actix_web::test]
async fn geocode_suggest_and_apply() {
    assert!(mock_geocoder("完全查無此地 1234").is_none());
    assert!(mock_geocoder("").is_none());
    let r = mock_geocoder("台南市七股區大潭段123地號").unwrap();
    assert_eq!(r.precision, GeocodePrecision::Locality);
    assert!(!is_usable_precision(r.precision));
    assert_eq!(mock_geocoder("屏東縣枋寮鄉中山路 100 號").unwrap().precision, GeocodePrecision::Exact);

    let f = fixture().await;
    let ctx = f.ctx().await;
    let mk = |addr: Option<&'static str>, n: &'static str| {
        let app = &f.app;
        let org = f.org;
        let lo = f.landowner;
        async move {
            let id = app.station(org, lo, &format!("CN-{n}"), n).await;
            if let Some(a) = addr {
                app.exec_sql(&format!("UPDATE tbl_energy_station SET address = '{a}' WHERE id = {id}")).await;
            }
            id
        }
    };
    let no_addr = mk(None, "noaddr").await;
    let not_found = mk(Some("火星第三區"), "nf").await;
    let coarse = mk(Some("台南市七股區大潭段123地號"), "coarse").await;
    let good = mk(Some("屏東縣枋寮鄉中山路 100 號"), "good").await;
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let s = |v: Value| v["reason"].as_str().map(String::from);
    assert_eq!(s(master_data::suggest_coordinates(&mut tx, &ctx.access, no_addr).await.unwrap()).as_deref(), Some("no_address"));
    assert_eq!(s(master_data::suggest_coordinates(&mut tx, &ctx.access, not_found).await.unwrap()).as_deref(), Some("not_found"));
    let c = master_data::suggest_coordinates(&mut tx, &ctx.access, coarse).await.unwrap();
    assert_eq!((c["usable"].clone(), c["reason"].clone(), c["result"]["precision"].clone()), (json!(false), json!("too_coarse"), json!("locality")));
    let g = master_data::suggest_coordinates(&mut tx, &ctx.access, good).await.unwrap();
    assert_eq!(g["usable"], true);
    drop(tx);
    let lat: Option<String> = sqlx::query_scalar("SELECT latitude::text FROM tbl_energy_station WHERE id = $1").bind(good).fetch_one(&f.app.core.pool).await.unwrap();
    assert!(lat.is_none(), "suggest 不寫入");

    // 採用：HTTP（會改資料 → act 的閘）。
    let viewer = f.app.member(f.org, "viewer", Some("energy:viewer")).await;
    expect_status!(f.app.post(&viewer, &f.app.e(f.org, &format!("/stations/{good}/geocode")), json!({})).await, 403);
    let v = expect_status!(f.app.post(&f.owner, &f.app.e(f.org, &format!("/stations/{good}/geocode")), json!({})).await, 200);
    assert!((v["latitude"].as_str().unwrap().parse::<f64>().unwrap() - 22.3654).abs() < 0.0001);
    assert_eq!((v["geocodePrecision"].clone(), v["geocodeSource"].clone()), (json!("exact"), json!("mock")));
    let geocoded_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT geocoded_at FROM tbl_energy_station WHERE id = $1").bind(good).fetch_one(&f.app.core.pool).await.unwrap();
    assert!(geocoded_at.is_some());
    let ev = f.app.audit_rows(f.org, "energyStation", good).await;
    assert_eq!(ev.len(), 1, "改座標一定要留痕");
    assert_eq!((ev[0].1["source"].clone(), ev[0].1["precision"].clone()), (json!("mock"), json!("exact")));
    assert!(ev[0].1["changes"]["latitude"]["to"].is_string());
    assert_eq!(ev[0].3, Some(f.owner.id), "操作者來自 session");

    let v = expect_status!(f.app.post(&f.owner, &f.app.e(f.org, &format!("/stations/{coarse}/geocode")), json!({})).await, 422);
    assert_eq!(v["code"], "geocode_too_coarse");
    let lat: Option<String> = sqlx::query_scalar("SELECT latitude::text FROM tbl_energy_station WHERE id = $1").bind(coarse).fetch_one(&f.app.core.pool).await.unwrap();
    assert!(lat.is_none());

    let manual = mk(Some("屏東縣枋寮鄉中山路 100 號"), "manual").await;
    f.app.exec_sql(&format!("UPDATE tbl_energy_station SET latitude = 22.1111111, longitude = 120.2222222, geocode_precision = 'exact', geocode_source = 'manual' WHERE id = {manual}")).await;
    let v = expect_status!(f.app.post(&f.owner, &f.app.e(f.org, &format!("/stations/{manual}/geocode")), json!({})).await, 409);
    assert_eq!(v["code"], "manual_coordinates_protected");
    let lat: String = sqlx::query_scalar("SELECT latitude::text FROM tbl_energy_station WHERE id = $1").bind(manual).fetch_one(&f.app.core.pool).await.unwrap();
    assert_eq!(lat, "22.1111111");
}

/// 卡 20261118-0427-a：案場 POST/PATCH＋專案編號、電號一對多＋重疊 409、受款對象類型篩選、
/// 地主分配＋稽核、沒有 `energy:master:manage` 的人寫入回 403（對應 S1～S4，含 DESIGN.md「合理測試」）。
#[actix_web::test]
async fn master_data_writes() {
    let app = TestApp::new().await;
    let (owner, org) = app.energy_org("carda-org").await;
    let viewer = app.member(org, "carda-viewer", Some("energy:viewer")).await;
    let base = app.e(org, "");

    // S3：受款對象 — 先建一家子公司（代號 ZA）給案場當 SPV，再驗類型篩選。
    let sub = expect_status!(
        app.post(&owner, &format!("{base}/landowners"), json!({ "name": "測試子公司甲", "partyType": "subsidiary", "code": "ZA" })).await,
        200
    );
    let sub_id = sub["id"].as_i64().unwrap();
    let agency = expect_status!(app.post(&owner, &format!("{base}/landowners"), json!({ "name": "測試機關", "partyType": "agency" })).await, 200);
    let agency_id = agency["id"].as_i64().unwrap();
    expect_status!(app.post(&viewer, &format!("{base}/landowners"), json!({ "name": "沒權限建的" })).await, 403);
    let filtered = expect_status!(app.get(&owner, &format!("{base}/landowners?partyType=subsidiary")).await, 200);
    let items = filtered["items"].as_array().unwrap();
    assert!(items.iter().any(|i| i["id"] == sub_id));
    assert!(!items.iter().any(|i| i["id"] == agency_id));
    let edited = expect_status!(
        app.patch(&owner, &format!("{base}/landowners/{sub_id}"), json!({ "name": "測試子公司甲", "partyType": "subsidiary", "code": "ZA", "taxId": "12345678" })).await,
        200
    );
    assert_eq!(edited["taxId"], "12345678");

    // S1：案場 — 新增時算出專案編號，改型別後專案編號跟著變；沒有 master:manage 回 403。
    let lo = expect_status!(app.post(&owner, &format!("{base}/landowners"), json!({ "name": "測試地主" })).await, 200);
    let landowner_id = lo["id"].as_i64().unwrap();
    expect_status!(
        app.post(&viewer, &format!("{base}/stations"), json!({ "name": "x", "customerNumber": "99-1", "landownerId": landowner_id })).await,
        403
    );
    let created = expect_status!(
        app.post(
            &owner,
            &format!("{base}/stations"),
            json!({
                "name": "測試案場", "customerNumber": "99-9000-0001-00", "landownerId": landowner_id,
                "siteType": "fishery", "spvPayeeId": sub_id, "gridDate": "2025-03-01", "serialNo": 7,
            }),
        )
        .await,
        200
    );
    let station_id = created["id"].as_i64().unwrap();
    assert_eq!(created["projectCode"], "114BZA0007", "(2025-1911)+B(漁電)+ZA+0007");
    let updated = expect_status!(
        app.patch(&owner, &format!("{base}/stations/{station_id}"), json!({
            "name": "測試案場", "siteType": "rooftop", "spvPayeeId": sub_id, "gridDate": "2025-03-01", "serialNo": 7, "landownerId": landowner_id,
        })).await,
        200
    );
    assert_eq!(updated["projectCode"], "114RZA0007", "改型別後型別碼跟著變");

    // S2：電號一對多 — 加第二個電號，收件比對找得到；同電號期間重疊回 409。
    let meters = expect_status!(
        app.post(&owner, &format!("{base}/stations/{station_id}/meters"), json!({ "customerNumber": "99-9000-0002-00", "usage": "consumption" })).await,
        200
    );
    assert_eq!(meters["items"].as_array().unwrap().len(), 2, "主電號＋新加的");
    let overlap = expect_status!(
        app.post(&owner, &format!("{base}/stations/{station_id}/meters"), json!({ "customerNumber": "99-9000-0002-00", "usage": "wholesale" })).await,
        409
    );
    assert_eq!(overlap["code"], "meter_overlap");
    let mut tx = app.core.pool.begin().await.unwrap();
    let hit = eo_energy::ingest::bills::find_station_by_customer_number(&mut tx, org, "99-9000-0002-00").await.unwrap();
    assert_eq!(hit.map(|s| s.id), Some(station_id), "第二個電號的單也對得到案場");
    drop(tx);

    // S4：地主分配 — 兩位地主 60/40，結束一位要留異動紀錄；每次寫入都有稽核。
    let lo2 = expect_status!(app.post(&owner, &format!("{base}/landowners"), json!({ "name": "測試地主二" })).await, 200);
    let landowner2_id = lo2["id"].as_i64().unwrap();
    let alloc = expect_status!(
        app.post(&owner, &format!("{base}/stations/{station_id}/allocations"), json!({ "landownerId": landowner2_id, "method": "ratio", "ratio": "40", "validFrom": "2026-01-01" })).await,
        200
    );
    assert_eq!(alloc["items"].as_array().unwrap().len(), 2, "主要受款對象 100%＋新加的 40%");
    let list = expect_status!(app.get(&owner, &format!("{base}/stations/{station_id}/allocations")).await, 200);
    let second = list["items"].as_array().unwrap().iter().find(|a| a["landownerId"] == landowner2_id).unwrap();
    let alloc_id = second["id"].as_i64().unwrap();
    expect_status!(
        app.patch(&owner, &format!("{base}/allocations/{alloc_id}"), json!({ "validTo": "2026-06-01", "changeReason": "測試結束" })).await,
        200
    );
    let history = expect_status!(app.get(&owner, &format!("{base}/stations/{station_id}/allocation-history")).await, 200);
    let types: Vec<&str> = history["items"].as_array().unwrap().iter().map(|h| h["type"].as_str().unwrap()).collect();
    assert!(types.contains(&"allocation.created"), "{types:?}");
    assert!(types.contains(&"allocation.ended"), "{types:?}");
}

/// 卡 20261118-0810-a2 S1／S2：合約 CRUD（含 `stationIds` 同步、驗證）、合約檔上傳／下載。
#[actix_web::test]
async fn contracts_and_attachments() {
    let app = TestApp::new().await;
    let (owner, org) = app.energy_org("carda2-org").await;
    let base = app.e(org, "");

    let lo = expect_status!(app.post(&owner, &format!("{base}/landowners"), json!({ "name": "合約地主" })).await, 200);
    let landowner_id = lo["id"].as_i64().unwrap();
    let st1 = app.station(org, landowner_id, "99-c-0001", "合約案場一").await;
    let st2 = app.station(org, landowner_id, "99-c-0002", "合約案場二").await;

    // S1：新增合約、連兩個案場。
    let created = expect_status!(
        app.post(
            &owner,
            &format!("{base}/contracts"),
            json!({
                "contractNo": "CT-A2-001", "contractType": "土地租賃", "landownerId": landowner_id,
                "stationIds": [st1, st2], "signedOn": "2026-01-01", "validFrom": "2026-01-01", "validTo": "2030-12-31",
                "renewalRemindOn": "2030-06-01", "termsVersion": "v1", "paymentFrequency": "quarterly", "paymentTerms": "每季月底付款",
            }),
        )
        .await,
        200
    );
    let contract_id = created["id"].as_i64().unwrap();
    assert_eq!(created["stations"].as_array().unwrap().len(), 2, "{created}");
    assert_eq!(created["paymentFrequency"], "quarterly");

    let bad_freq = expect_status!(
        app.post(&owner, &format!("{base}/contracts"), json!({ "contractNo": "CT-BAD", "landownerId": landowner_id, "validFrom": "2026-01-01", "paymentFrequency": "weekly" })).await,
        400
    );
    assert_eq!(bad_freq["code"], "bad_payment_frequency");

    let bad_range = expect_status!(
        app.post(&owner, &format!("{base}/contracts"), json!({ "contractNo": "CT-BAD2", "landownerId": landowner_id, "validFrom": "2026-01-01", "validTo": "2025-01-01" })).await,
        400
    );
    assert_eq!(bad_range["code"], "bad_date_range");

    // 編輯：整包送上來，改到期日、把案場清單收斂成只剩 st1（軟刪 st2 那一列，不硬刪）。
    let updated = expect_status!(
        app.patch(
            &owner,
            &format!("{base}/contracts/{contract_id}"),
            json!({
                "contractNo": "CT-A2-001", "landownerId": landowner_id, "stationIds": [st1],
                "validFrom": "2026-01-01", "validTo": "2031-12-31", "paymentFrequency": "quarterly",
            }),
        )
        .await,
        200
    );
    assert_eq!(updated["validTo"], "2031-12-31");
    assert_eq!(updated["stations"].as_array().unwrap().len(), 1, "{updated}");

    // S2：上傳合約 PDF，清單出現一筆，下載回來位元組一致。
    const PDF: &[u8] = b"%PDF-1.4\n% carda2 contract test\n";
    let mut form = reqwest::multipart::Form::new();
    let part = reqwest::multipart::Part::bytes(PDF.to_vec()).file_name("contract.pdf").mime_str("application/pdf").unwrap();
    form = form.part("file", part);
    let (s, v) = TestApp::send(app.req(Some(&owner), Method::POST, &format!("{base}/contracts/{contract_id}/attachments")).multipart(form)).await;
    assert_eq!(s.as_u16(), 200, "{v}");
    let attachment_id = v["id"].as_i64().unwrap();

    let list = expect_status!(app.get(&owner, &format!("{base}/contracts/{contract_id}/attachments")).await, 200);
    assert_eq!(list["items"].as_array().unwrap().len(), 1, "{list}");

    let dl = app.req(Some(&owner), Method::GET, &format!("{base}/attachments/{attachment_id}/download")).send().await.unwrap();
    assert_eq!(dl.status().as_u16(), 200);
    assert_eq!(dl.bytes().await.unwrap().as_ref(), PDF);

    let got = expect_status!(app.get(&owner, &format!("{base}/contracts/{contract_id}")).await, 200);
    assert_eq!(got["attachmentCount"], 1, "{got}");
}

/// 卡 20261118-0810-a2 S3：發電案場清單 Excel 匯入。壞檔 dryRun 回 4 種錯碼且 DB 不變；
/// 好檔 dryRun 不寫、正式匯入新增 8 個案場、10 個電號；再匯一次好檔不重複建立。
#[actix_web::test]
async fn station_import() {
    let app = TestApp::new().await;
    let (owner, org) = app.energy_org("carda2-import-org").await;
    let base = app.e(org, "");
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    let good = fs::read(format!("{fixtures}/stations-import-fake.xlsx")).expect("好檔：先跑 evidence/make-fake-xlsx.py 產生 tests/fixtures");
    let bad = fs::read(format!("{fixtures}/stations-import-fake-bad.xlsx")).expect("壞檔：先跑 evidence/make-fake-xlsx.py 產生 tests/fixtures");

    // 壞檔的 meter_taken 那一列撞的是示範資料既有電號（demo_seed.rs 的 s1 主電號）；
    // 測試環境不跑 demo seed，所以在這個組織自己先用 API 建一座同電號的案場來撞
    // （走 API 才會像 create_station 一樣同時補一列 tbl_energy_station_meter；
    // `app.station()` 那個測試小工具只插 tbl_energy_station，少了 meter 那一列，
    // 匯入檢查不到既有 meter，反而會因為 DB 的 customer_number 唯一鍵直接炸裂）。
    let existing_lo = expect_status!(app.post(&owner, &format!("{base}/landowners"), json!({ "name": "既有地主" })).await, 200);
    expect_status!(
        app.post(
            &owner,
            &format!("{base}/stations"),
            json!({ "name": "既有案場", "customerNumber": "00-60-1600-79-5", "landownerId": existing_lo["id"] }),
        )
        .await,
        200
    );

    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_energy_station WHERE organization_id = $1")
        .bind(org)
        .fetch_one(&app.core.pool)
        .await
        .unwrap();

    // 壞檔 dryRun：4 種錯碼，DB 筆數不變。
    let mut form = reqwest::multipart::Form::new();
    form = form.part("file", reqwest::multipart::Part::bytes(bad.clone()).file_name("bad.xlsx").mime_str("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet").unwrap());
    form = form.text("dryRun", "1");
    let (s, v) = TestApp::send(app.req(Some(&owner), Method::POST, &format!("{base}/imports/stations")).multipart(form)).await;
    assert_eq!(s.as_u16(), 200, "{v}");
    assert_eq!(v["dryRun"], true);
    let codes: Vec<&str> = v["errors"].as_array().unwrap().iter().map(|e| e["code"].as_str().unwrap()).collect();
    assert!(codes.contains(&"required"), "{codes:?}");
    assert!(codes.contains(&"bad_number"), "{codes:?}");
    assert!(codes.contains(&"duplicate_meter"), "{codes:?}");
    assert!(codes.contains(&"meter_taken"), "{codes:?}");
    let after_bad: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_energy_station WHERE organization_id = $1")
        .bind(org)
        .fetch_one(&app.core.pool)
        .await
        .unwrap();
    assert_eq!(before, after_bad, "壞檔 dryRun 不該動到任何一列");

    // 好檔 dryRun：不寫。
    let mut form = reqwest::multipart::Form::new();
    form = form.part("file", reqwest::multipart::Part::bytes(good.clone()).file_name("good.xlsx").mime_str("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet").unwrap());
    form = form.text("dryRun", "1");
    let (s, v) = TestApp::send(app.req(Some(&owner), Method::POST, &format!("{base}/imports/stations")).multipart(form)).await;
    assert_eq!(s.as_u16(), 200, "{v}");
    assert_eq!(v["errors"].as_array().unwrap().len(), 0, "{v}");
    assert_eq!(v["created"]["stations"], 8, "{v}");
    assert_eq!(v["created"]["meters"], 10, "{v}");
    let after_dry: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_energy_station WHERE organization_id = $1")
        .bind(org)
        .fetch_one(&app.core.pool)
        .await
        .unwrap();
    assert_eq!(before, after_dry, "好檔 dryRun 也不該真的寫進去");

    // 正式匯入：8 個案場、10 個電號；受款對象清單出現子公司與台電區處。
    let mut form = reqwest::multipart::Form::new();
    form = form.part("file", reqwest::multipart::Part::bytes(good.clone()).file_name("good.xlsx").mime_str("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet").unwrap());
    form = form.text("dryRun", "0");
    let (s, v) = TestApp::send(app.req(Some(&owner), Method::POST, &format!("{base}/imports/stations")).multipart(form)).await;
    assert_eq!(s.as_u16(), 200, "{v}");
    assert_eq!(v["created"]["stations"], 8, "{v}");
    assert_eq!(v["created"]["meters"], 10, "{v}");
    let after_real: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_energy_station WHERE organization_id = $1")
        .bind(org)
        .fetch_one(&app.core.pool)
        .await
        .unwrap();
    assert_eq!(after_real, before + 8, "真的新增了 8 個案場");
    let invoice_sources: Vec<serde_json::Value> = sqlx::query_scalar("SELECT data->'invoice' FROM tbl_energy_station WHERE organization_id=$1 AND data ? 'plantNo'")
        .bind(org).fetch_all(&app.core.pool).await.unwrap();
    assert!(!invoice_sources.is_empty());
    assert!(invoice_sources.iter().all(|v| v.get("counterpartyName").is_some() && v.get("counterpartyTaxId").is_some()), "匯入時必須保留開票對象及統編來源");

    let payees = expect_status!(app.get(&owner, &format!("{base}/landowners")).await, 200);
    let names: Vec<&str> = payees["items"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert!(names.iter().any(|n| n.contains("假控股")), "{names:?}");
    assert!(names.iter().any(|n| n.contains("假台電")), "{names:?}");

    // 再匯一次好檔：案場算 updated，電號已在同一案場就略過，不重複建立。
    let mut form = reqwest::multipart::Form::new();
    form = form.part("file", reqwest::multipart::Part::bytes(good).file_name("good.xlsx").mime_str("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet").unwrap());
    form = form.text("dryRun", "0");
    let (s, v) = TestApp::send(app.req(Some(&owner), Method::POST, &format!("{base}/imports/stations")).multipart(form)).await;
    assert_eq!(s.as_u16(), 200, "{v}");
    assert_eq!(v["created"]["stations"], 0, "{v}");
    assert_eq!(v["created"]["meters"], 0, "{v}");
    assert_eq!(v["updated"]["stations"], 8, "{v}");
    let after_reimport: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_energy_station WHERE organization_id = $1")
        .bind(org)
        .fetch_one(&app.core.pool)
        .await
        .unwrap();
    assert_eq!(after_reimport, after_real, "重匯同一份好檔不重複建立案場");
}

/// 來源：masterDataOverview.test.ts「landownerOverview 呼叫得到（這條就是在擋 ReferenceError 型的 500）」
/// 來源：masterDataOverview.test.ts「案場上帶得出「適用哪版流程 / 哪版策略」（陷阱 #27：兩張表的鍵不一樣）」
/// 來源：masterDataOverview.test.ts「stationOverview 兩條軸都有東西，且標得出「現在生效的是哪一段」」
/// 來源：masterDataOverview.test.ts「沒有綁定的案場：兩條軸都空（這是誠實，不是壞掉）」
/// 來源：masterDataOverview.test.ts「跨組織拿案場是 403（不是 404 —— 404 會洩漏「這個 id 存在」）」
/// 來源：masterDataOverview.test.ts「別種 subject 的流程不會混進 inFlight（subjectId 的 id 空間會撞）」
#[actix_web::test]
async fn overview_pages() {
    let app = TestApp::new().await;
    let (owner, org) = app.energy_org("c7b-org").await;
    let lo = app.landowner(org, "林地主").await;
    let station = app.station(org, lo, "07-0001", "嘉義六腳一號").await;
    let bare = app.station(org, lo, "08-0001", "台南七股一號").await;
    let def: i64 = sqlx::query_scalar(
        "INSERT INTO tbl_energy_workflow_definition (organization_id, key, version, name, subject_type, status, published_at)
         VALUES ($1, 'bill-approval', 3, '電費單審批', 'energyBill', 'published', now()) RETURNING id",
    )
    .bind(org)
    .fetch_one(&app.core.pool)
    .await
    .unwrap();
    let strat = app.strategy(org, "revenue-share", "revenueShare", json!({ "sharePercent": 10 }), "售電分潤 10%").await;
    // 流程綁定比電號（不填 station_id），策略綁定比 station_id。
    app.wf_binding(org, Some("07-0001"), None, None, def, "2000-01-01", None).await;
    app.strategy_binding(org, station, strat, "2000-01-01", None).await;
    app.strategy_binding(org, station, strat, "1990-01-01", Some("2000-01-01")).await;
    let ctx = app.ctx(&owner, org).await;
    let mut tx = app.core.pool.begin().await.unwrap();
    let o = landowner_overview(&mut tx, &ctx, lo, 3).await.unwrap();
    assert_eq!(o["id"], lo);
    assert_eq!(o["stations"].as_array().unwrap().len(), 2);
    assert!(o["inFlight"].is_array() && o["stuckDays"].is_number());
    let by = |id: i64| o["stations"].as_array().unwrap().iter().find(|s| s["id"] == id).unwrap().clone();
    let bound = by(station);
    assert_eq!((bound["workflow"]["name"].clone(), bound["workflow"]["version"].clone()), (json!("電費單審批"), json!(3)));
    assert_eq!(bound["strategy"]["name"], "售電分潤 10%");
    assert!(by(bare)["workflow"].is_null() && by(bare)["strategy"].is_null());

    let s = station_overview(&mut tx, &ctx, station).await.unwrap();
    assert_eq!(s["workflowAxis"].as_array().unwrap().len(), 1);
    assert_eq!((s["workflowAxis"][0]["name"].clone(), s["workflowAxis"][0]["active"].clone()), (json!("電費單審批"), json!(true)));
    let axis = s["strategyAxis"].as_array().unwrap();
    assert_eq!(axis.len(), 2);
    let actives: Vec<&Value> = axis.iter().filter(|a| a["active"] == true).collect();
    assert_eq!(actives.len(), 1);
    assert!(actives[0]["validTo"].is_null());
    assert!(axis[0]["validFrom"].as_str() < axis[1]["validFrom"].as_str(), "由舊到新");
    let b = station_overview(&mut tx, &ctx, bare).await.unwrap();
    assert!(b["workflowAxis"].as_array().unwrap().is_empty() && b["strategyAxis"].as_array().unwrap().is_empty());
    drop(tx);

    let (other_owner, other_org) = app.energy_org("c7b-other").await;
    let v = expect_status!(app.get(&other_owner, &app.e(other_org, &format!("/stations/{station}/overview"))).await, 403);
    assert_eq!(v["code"], "station_not_in_org");

    // 保證金那條流程用同一個 id 字串當 subjectId —— 不可混進「現在正在發生」。
    let bill = app.bill(org, Some(station), "2026-01-01", "2026-02-28", Some("50000.00"), None).await;
    app.exec_sql(&format!(
        "INSERT INTO tbl_energy_workflow_execution (organization_id, definition_id, subject_type, subject_id, station_key, started_at, status, current_node_key, business_date)
         VALUES ({org}, {def}, 'energyInstallment', '{bill}', '07-0001', now(), 'running', 'finance-release', '2026-03-01')"
    ))
    .await;
    let mut tx = app.core.pool.begin().await.unwrap();
    let o = landowner_overview(&mut tx, &ctx, lo, 3).await.unwrap();
    assert!(o["inFlight"].as_array().unwrap().is_empty());
    let v = expect_status!(app.get(&owner, &app.e(org, &format!("/landowners/{lo}/overview"))).await, 200);
    assert_eq!(v["id"], lo);
}

/// 卡 20261124-2316：案場／地主／合約各自獨立，沒有電號也能建檔，陌生電號人工繫結後下月自動對到。
/// S1：不帶電號／地主建案場，沒有電號案場建工作流綁定回 409。
/// S2：合約掛多位地主，依任一位地主篩都找得到，拿掉一位不影響另一位與案場。
/// S3：一次建案場、地主（一舊一新）、合約（`/onboard`），分配照填的比例逐筆寫。
/// S4：候選清單裡沒合約的案場灰掉標字（`hasContract: false`）、直接送出也 409；
/// 繫結成功寫電號表、補主電號；下一期同電號 ingest 自動對到，不進待指認。
#[actix_web::test]
async fn masters_independent_unknown_meter() {
    let app = TestApp::new().await;
    let (owner, org) = app.energy_org("mim-org").await;
    let base = app.e(org, "");

    // ── S1：不帶電號、不帶地主也能建案場 ──
    let s1 = expect_status!(app.post(&owner, &format!("{base}/stations"), json!({ "name": "S1 無電號案場" })).await, 200);
    let s1_id = s1["id"].as_i64().unwrap();
    assert!(s1["landownerId"].is_null(), "{s1}");
    let list = expect_status!(app.get(&owner, &format!("{base}/station-list")).await, 200);
    let row = list["items"].as_array().unwrap().iter().find(|r| r["id"] == s1_id).unwrap();
    assert_eq!(row["meterCount"], 0, "{row}");

    // 連帶：沒有電號的案場不能建工作流綁定（執行用主電號當 key）。
    app.install_def(&owner, org, &eo_domain::workflow_defs::default_bill_approval(), None).await;
    let opts = expect_status!(app.get(&owner, &format!("{base}/binding-options?kind=workflow")).await, 200);
    let bill_def = opts["items"].as_array().unwrap().iter().find(|o| o["key"] == "bill-approval").unwrap()["id"].as_i64().unwrap();
    let blocked_bind = expect_status!(
        app.post(
            &owner,
            &format!("{base}/stations/{s1_id}/bindings"),
            json!({ "kind": "workflow", "docType": "electricityBill", "versionId": bill_def, "validFrom": "2000-01-01" }),
        )
        .await,
        409
    );
    assert_eq!(blocked_bind["code"], "station_has_no_meter");

    // ── S2：合約掛多位地主 ──
    let lo1 = app.landowner(org, "地主甲").await;
    let lo2 = app.landowner(org, "地主乙").await;
    let s2_station = app.station(org, lo1, "99-s2-0001", "S2 案場").await;
    let created = expect_status!(
        app.post(
            &owner,
            &format!("{base}/contracts"),
            json!({ "contractNo": "CT-MIM-S2", "landownerIds": [lo1, lo2], "stationIds": [s2_station], "validFrom": "2026-01-01" }),
        )
        .await,
        200
    );
    let contract_id = created["id"].as_i64().unwrap();
    assert_eq!(created["landowners"].as_array().unwrap().len(), 2, "{created}");
    let by_lo2 = expect_status!(app.get(&owner, &format!("{base}/contracts?landownerId={lo2}")).await, 200);
    assert!(by_lo2["items"].as_array().unwrap().iter().any(|c| c["id"] == contract_id), "{by_lo2}");
    let updated = expect_status!(
        app.patch(
            &owner,
            &format!("{base}/contracts/{contract_id}"),
            json!({ "contractNo": "CT-MIM-S2", "landownerIds": [lo1], "stationIds": [s2_station], "validFrom": "2026-01-01" }),
        )
        .await,
        200
    );
    assert_eq!(updated["landowners"].as_array().unwrap().len(), 1, "{updated}");
    let still = expect_status!(app.get(&owner, &format!("{base}/stations/{s2_station}/overview")).await, 200);
    assert!(still["landownerId"].is_number(), "拿掉一位地主不影響案場：{still}");

    // ── S3：一次建案場、地主（一舊一新）、合約 ──
    let onboarded = expect_status!(
        app.post(
            &owner,
            &format!("{base}/onboard"),
            json!({
                "station": { "name": "S3 路竹三爺二期（示範）" },
                "landowners": [ { "id": lo1, "ratio": 60 }, { "name": "S3 新地主", "ratio": 40 } ],
                "contract": { "contractNo": "CT-MIM-S3", "validFrom": "2026-01-01" },
            }),
        )
        .await,
        200
    );
    let s3_id = onboarded["id"].as_i64().unwrap();
    assert!(onboarded["landownerIds"].as_array().unwrap().len() == 2, "{onboarded}");
    assert!(onboarded["contractId"].as_i64().unwrap() > 0, "{onboarded}");
    let allocs = expect_status!(app.get(&owner, &format!("{base}/stations/{s3_id}/allocations")).await, 200);
    assert_eq!(allocs["items"].as_array().unwrap().len(), 2, "{allocs}");

    // ── S4：陌生電號繫結，候選灰掉沒合約的案場、直接送出 409；成功後寫電號表、下一期自動對到 ──
    let no_contract_station = app.station(org, lo1, "99-s4-nc-0001", "S4 沒合約案場").await;
    let bill1 = app.bill(org, None, "2026-06-01", "2026-06-30", Some("1000"), None).await;
    app.exec_sql(&format!("UPDATE tbl_energy_bill SET customer_number_raw = '99-3217-2001' WHERE id = {bill1}")).await;

    let candidates = expect_status!(app.get(&owner, &format!("{base}/bills/{bill1}/match-candidates")).await, 200);
    let items = candidates["items"].as_array().unwrap();
    let nc = items.iter().find(|c| c["id"] == no_contract_station).unwrap();
    assert_eq!(nc["hasContract"], false, "{nc}");
    let s3c = items.iter().find(|c| c["id"] == s3_id).unwrap();
    assert_eq!(s3c["hasContract"], true, "{s3c}");

    let blocked = expect_status!(app.post(&owner, &format!("{base}/bills/{bill1}/match"), json!({ "stationId": no_contract_station })).await, 409);
    assert_eq!(blocked["code"], "station_has_no_contract");

    let matched = expect_status!(app.post(&owner, &format!("{base}/bills/{bill1}/match"), json!({ "stationId": s3_id })).await, 200);
    assert_eq!(matched["reviewStatus"], "pendingReview", "{matched}");
    let meters = expect_status!(app.get(&owner, &format!("{base}/stations/{s3_id}/meters")).await, 200);
    assert_eq!(meters["items"].as_array().unwrap().len(), 1, "繫結成功要寫電號表：{meters}");
    let overview = expect_status!(app.get(&owner, &format!("{base}/stations/{s3_id}/overview")).await, 200);
    assert_eq!(overview["customerNumber"], "99-3217-2001", "要補主電號，「沒有電號」才會消失：{overview}");

    // 下一期同電號：ingest 自動對到這座案場與合約，不進待指認。
    let r = expect_status!(
        app.post(
            &owner,
            &format!("{base}/bills/ingest"),
            json!({ "ocrJson": { "bills": [{ "customerNumber": "99-3217-2001", "計費起": "2026/07/01", "計費訖": "2026/07/31", "totalAmount": 2000 }] } }),
        )
        .await,
        200
    );
    assert_eq!(r["matched"], 1, "{r}");
    let bill2 = r["billIds"][0].as_i64().unwrap();
    let bill2_detail = expect_status!(app.get(&owner, &format!("{base}/bills/{bill2}")).await, 200);
    assert_eq!(bill2_detail["stationId"], s3_id, "{bill2_detail}");
    assert_eq!(bill2_detail["reviewStatus"], "pendingReview", "{bill2_detail}");
}
