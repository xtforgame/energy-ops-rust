//! 主檔讀取（地主／案場／整合頁／兩條效期軸）、儀表板、地址推估座標。

use crate::support::energy::code_of;
use crate::support::{TestApp, TestUser};
use eo_domain::geocoder::{is_usable_precision, mock_geocoder, GeocodePrecision};
use eo_energy::common::Ctx;
use eo_energy::master_data::{self, landowner_overview, station_overview};
use eo_energy::workbench;
use eo_energy::workflow::model::TaskRow;
use eo_energy::workflow::views;
use serde_json::{json, Value};

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
    let rows = master_data::list_landowners(&mut tx, &ctx.access, None, None).await.unwrap();
    let mine = rows.iter().find(|r| r["id"] == f.landowner).unwrap();
    assert_eq!((mine["stationCount"].clone(), mine["abnormalStationCount"].clone(), mine["contractCount"].clone()), (json!(2), json!(1), json!(1)));
    let other = f.app.ctx(&f.other_owner, f.other_org).await;
    let rows = master_data::list_landowners(&mut tx, &other.access, None, None).await.unwrap();
    assert!(!rows.iter().any(|r| r["id"] == f.landowner));
    assert_eq!(code_of(master_data::get_landowner(&mut tx, &other.access, f.landowner).await), (403, "landowner_not_in_org".into()));

    let st = master_data::list_stations(&mut tx, &ctx.access, None, None, None).await.unwrap();
    let s = st.iter().find(|r| r["id"] == f.station).unwrap();
    assert!(s["latitude"].as_str().unwrap().starts_with("22.678"), "座標回字串");
    assert_eq!(s["landownerName"], "陳地主");
    let q = master_data::list_stations(&mut tx, &ctx.access, Some("二號"), None, None).await.unwrap();
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

/// 來源：masterData.test.ts「dashboard 只加總**現行**的應付，不把被取代的算進去」
/// 來源：masterData.test.ts「dashboard 的月份是「業務日期」的月份，不是計算時間」
/// 來源：masterData.test.ts「卡關門檻是參數（不必改 schema 就能調）」
/// 來源：masterData.test.ts「案場狀態分布數得出來」
/// 來源：masterData.test.ts「稽核的時間區間：`to` 要含**當天整天**（不是 <= 當天 00:00）」
#[actix_web::test]
async fn dashboard_and_audit_day_range() {
    let f = fixture().await;
    f.payout("superseded", "4000.00").await;
    f.payout("active", "5000.00").await;
    let ctx = f.ctx().await;
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let d = workbench::dashboard(&mut tx, &ctx, Some("2026-03"), 3).await.unwrap();
    assert_eq!(d["payableThisMonth"], "5000.00", "被取代的不算");
    assert_eq!(d["payoutCount"], 1);
    let d = workbench::dashboard(&mut tx, &ctx, Some("2026-01"), 3).await.unwrap();
    assert_eq!((d["payableThisMonth"].clone(), d["payoutCount"].clone()), (json!("0.00"), json!(0)));
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
