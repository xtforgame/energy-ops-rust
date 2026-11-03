//! 計算服務（應付紀錄不可變、取版按業務日期、算不出來不寫 0）與效期綁定管理（綁定／換版／解除、預覽、漏綁）。

use crate::support::energy::{code_of, date};
use crate::support::{TestApp, TestUser};
use eo_domain::audit_events::{get_audit_event_def, AuditSeverity};
use eo_energy::bindings::{self, Kind};
use eo_energy::common::Ctx;
use eo_energy::rating;
use serde_json::{json, Value};

struct Rx {
    app: TestApp,
    org: i64,
    finance: TestUser,
    station: i64,
    bill: i64,
}

async fn rating_fixture() -> Rx {
    let app = TestApp::new().await;
    let (_owner, org) = app.energy_org("b3-org").await;
    let finance = app.member(org, "finance", Some("energy:finance")).await;
    let lo = app.landowner(org, "陳地主").await;
    let station = app.station(org, lo, "CN-B3-0001", "台南三號案場").await;
    let s = app.strategy(org, "station-payout", "revenueShare", json!({ "sharePercent": 12.5 }), "售電分潤 12.5%").await;
    app.strategy_binding(org, station, s, "2000-01-01", None).await;
    let bill = app.bill(org, Some(station), "2026-01-01", "2026-01-31", Some("98000"), Some("4321.5")).await;
    Rx { app, org, finance, station, bill }
}

impl Rx {
    async fn ctx(&self) -> Ctx {
        self.app.ctx(&self.finance, self.org).await
    }
    async fn compute(&self, bill: i64, trigger: &str, key: Option<&str>) -> eo_platform::error::ApiResult<rating::PayoutRecord> {
        let ctx = self.ctx().await;
        in_tx!(self.app, tx => rating::compute_for_bill_with_key(&mut tx, &ctx, bill, trigger, None, key).await)
    }
    async fn station(&self, cn: &str) -> i64 {
        let lo = self.app.landowner(self.org, &format!("地主 {cn}")).await;
        self.app.station(self.org, lo, cn, &format!("案場 {cn}")).await
    }
}

/// 來源：rating.test.ts「computes and stores the FULL provenance (US-6: 財務要敢放行)」
/// 來源：rating.test.ts「the input snapshot carries ONLY the required fields (no wholesale copy)」
/// 來源：rating.test.ts「recomputing creates a NEW row and only flips the two pointer columns on the old one」
/// 來源：rating.test.ts「the history shows every attempt, newest first」
#[actix_web::test]
async fn payout_provenance_and_immutability() {
    let f = rating_fixture().await;
    let r = f.compute(f.bill, "manualRecompute", None).await.unwrap();
    assert_eq!(r.amount_str(), "12250.00");
    assert_eq!(r.strategy_type, "revenueShare");
    assert_eq!(r.params_snapshot["sharePercent"], 12.5, "參數快照");
    assert!(r.input_snapshot["totalAmount"].as_str().unwrap().contains("98000"), "輸入快照");
    assert_eq!(r.input_snapshot["billingPeriodEnd"], "2026-01-31");
    assert_eq!(r.business_date, date("2026-01-31"));
    assert!(r.strategy_id.unwrap() > 0, "指向版本列不是 key");
    assert_eq!((r.trigger.as_str(), r.status.as_str()), ("manualRecompute", "active"));
    let audit = f.app.audit_rows(f.org, "energyPayoutRecord", r.id).await;
    let calc = audit.iter().find(|a| a.0 == "payout.calculated").unwrap();
    assert_eq!(calc.1["amount"], "12250.00", "金額用字串，不用 JSON number");
    assert_eq!(calc.1["version"], 1);

    let mut keys: Vec<String> = r.input_snapshot.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["billingPeriodEnd", "billingPeriodStart", "totalAmount"], "不整包複製 bill");

    // 改策略（新版本）＋改綁定 → 重算：新一列、舊列只動兩個指標欄。
    let v2 = f.app.strategy(f.org, "station-payout", "revenueShare", json!({ "sharePercent": 15 }), "售電分潤 15%").await;
    f.app.exec_sql(&format!("UPDATE tbl_energy_rating_strategy_binding SET valid_to = '2026-01-01' WHERE station_id = {} AND valid_to IS NULL", f.station)).await;
    f.app.strategy_binding(f.org, f.station, v2, "2026-01-01", None).await;
    let after = f.compute(f.bill, "correction", None).await.unwrap();
    assert_ne!(after.id, r.id);
    assert_eq!(after.amount_str(), "14700.00", "98000 × 15%");
    assert_eq!(after.supersedes_record_id, Some(r.id));
    let (amount, status, superseded, params): (String, String, Option<chrono::DateTime<chrono::Utc>>, Value) =
        sqlx::query_as("SELECT amount::text, status, superseded_at, params_snapshot FROM tbl_energy_payout_record WHERE id = $1")
            .bind(r.id)
            .fetch_one(&f.app.core.pool)
            .await
            .unwrap();
    assert_eq!(amount, "12250.00", "舊筆的金額永不改變");
    assert_eq!(status, "superseded");
    assert!(superseded.is_some());
    assert_eq!(params["sharePercent"], 12.5);
    assert!(f.app.audit_rows(f.org, "energyPayoutRecord", r.id).await.iter().any(|a| a.0 == "payout.superseded"), "取代也要留痕");

    let ctx = f.ctx().await;
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let history = rating::history_for_bill(&mut tx, &ctx.access, f.bill).await.unwrap();
    assert!(history.len() > 1);
    assert_eq!((history[0].status.as_str(), history[1].status.as_str()), ("active", "superseded"));
    assert!(history.iter().any(|h| h.amount == "12250.00"));
    let current = rating::current_for_bill(&mut tx, &ctx.access, f.bill).await.unwrap().unwrap();
    assert_eq!(current.id, after.id);
}

/// 來源：rating.test.ts「resolves the strategy by BUSINESS date, not now()」
/// 來源：rating.test.ts「no binding for that date is an ERROR (not a silent fallback)」
/// 來源：rating.test.ts「a bill spanning two bindings is DETECTABLE (v1 does not split — it records honestly)」
#[actix_web::test]
async fn strategy_by_business_date_and_spanning_detection() {
    let f = rating_fixture().await;
    let st = f.station("CN-BD").await;
    let a = f.app.strategy(f.org, "bd", "revenueShare", json!({ "sharePercent": 12.5 }), "a").await;
    let b = f.app.strategy(f.org, "bd", "revenueShare", json!({ "sharePercent": 15 }), "b").await;
    f.app.strategy_binding(f.org, st, a, "2000-01-01", Some("2026-01-01")).await;
    f.app.strategy_binding(f.org, st, b, "2026-01-01", None).await;
    let ctx = f.ctx().await;
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let pct = |s: &rating::StrategyRow| s.params["sharePercent"].as_f64().unwrap();
    assert_eq!(pct(&rating::resolve_strategy(&mut tx, &ctx.access, st, Some("bd"), date("2025-12-31")).await.unwrap()), 12.5, "補登去年的單走舊費率");
    assert_eq!(pct(&rating::resolve_strategy(&mut tx, &ctx.access, st, Some("bd"), date("2026-06-30")).await.unwrap()), 15.0);
    assert_eq!(pct(&rating::resolve_strategy(&mut tx, &ctx.access, st, Some("bd"), date("2026-01-01")).await.unwrap()), 15.0, "半開區間：生效當天算新的");
    let none = f.station("CN-NOSTRAT").await;
    let e = rating::resolve_strategy(&mut tx, &ctx.access, none, None, date("2026-01-31")).await;
    assert_eq!(code_of(e).1, "no_strategy_binding_for_business_date");
    drop(tx);

    let spanning = f.app.bill(f.org, Some(st), "2025-12-15", "2026-01-14", Some("1000"), None).await;
    let r = f.compute(spanning, "manualRecompute", Some("bd")).await.unwrap();
    assert_eq!(r.params_snapshot["sharePercent"], 15, "用期末命中的那一版算整張單");
    let mut tx = f.app.core.pool.begin().await.unwrap();
    assert!(rating::spans_multiple_bindings(&mut tx, &ctx.access, &r).await.unwrap(), "看得出來跨約了");
    drop(tx);
    let inside = f.app.bill(f.org, Some(st), "2026-02-01", "2026-02-28", Some("1000"), None).await;
    let r2 = f.compute(inside, "manualRecompute", Some("bd")).await.unwrap();
    let mut tx = f.app.core.pool.begin().await.unwrap();
    assert!(!rating::spans_multiple_bindings(&mut tx, &ctx.access, &r2).await.unwrap());
}

/// 來源：rating.test.ts「a bill with no station cannot be computed (exception must be matched first)」
/// 來源：rating.test.ts「a missing required field fails LOUDLY and still leaves an audit trail」
#[actix_web::test]
async fn failures_throw_and_never_write_zero() {
    let f = rating_fixture().await;
    let orphan = f.app.bill(f.org, None, "2026-01-01", "2026-01-31", Some("98000"), None).await;
    assert_eq!(code_of(f.compute(orphan, "manualRecompute", None).await).1, "bill_has_no_station");
    let n = f.app.scalar_i64(&format!("SELECT COUNT(*) FROM tbl_energy_payout_record WHERE bill_id = {orphan}")).await;
    assert_eq!(n, 0, "不留半筆");

    let st = f.station("CN-KWH").await;
    let s = f.app.strategy(f.org, "kwh", "generationUnitPrice", json!({ "unitPrice": 1.85 }), "kwh").await;
    f.app.strategy_binding(f.org, st, s, "2000-01-01", None).await;
    let no_kwh = f.app.bill(f.org, Some(st), "2026-01-01", "2026-01-31", Some("98000"), None).await;
    assert_eq!(code_of(f.compute(no_kwh, "workflowNode", Some("kwh")).await).1, "missing_bill_fields");
    let n = f.app.scalar_i64(&format!("SELECT COUNT(*) FROM tbl_energy_payout_record WHERE bill_id = {no_kwh}")).await;
    assert_eq!(n, 0, "沒有金額 0 的紀錄");
    // 交易 rollback 了，「為什麼算不出來」仍查得到。
    let row = f.app.audit_rows(f.org, "energyBill", no_kwh).await.into_iter().find(|r| r.0 == "payout.calculation_failed").expect("算不出來也要留痕");
    assert_eq!(row.2, "failed");
    assert_eq!(row.1["reason"], "missing_bill_fields");
}

// ── 綁定 ──

struct Bx {
    app: TestApp,
    org: i64,
    admin: TestUser,
    cxo: TestUser,
    bare: i64,
    def_v1: i64,
    def_v2: i64,
    strat_v1: i64,
    strat_v2: i64,
}

async fn binding_fixture() -> Bx {
    let app = TestApp::new().await;
    let (admin, org) = app.energy_org("c7e-org").await;
    let cxo = app.member(org, "cxo", Some("energy:cxo")).await;
    let lo = app.landowner(org, "陳地主").await;
    let bare = app.station(org, lo, "11-0001", "未綁定案場").await;
    let mut d = eo_domain::workflow_defs::default_bill_approval();
    let def_v1 = app.install_def(&admin, org, &d, None).await;
    d.name = "電費單核可流程（2026 版）".into();
    let def_v2 = app.install_def(&admin, org, &d, Some(def_v1)).await;
    let strat_v1 = app.strategy(org, "revenue-share", "revenueShare", json!({ "sharePercent": 10 }), "售電分潤 10%").await;
    let strat_v2 = app.strategy(org, "revenue-share", "generationUnitPrice", json!({ "unitPrice": 1.9 }), "售電單價 1.9 元/度").await;
    Bx { app, org, admin, cxo, bare, def_v1, def_v2, strat_v1, strat_v2 }
}

impl Bx {
    async fn ctx(&self) -> Ctx {
        self.app.ctx(&self.admin, self.org).await
    }
    async fn latest(&self, event: &str) -> Option<(String, Value)> {
        sqlx::query_as("SELECT summary, payload FROM tbl_energy_audit_event WHERE organization_id = $1 AND type = $2 ORDER BY id DESC LIMIT 1")
            .bind(self.org)
            .bind(event)
            .fetch_optional(&self.app.core.pool)
            .await
            .unwrap()
    }
}

/// 來源：bindings.test.ts「權限對得上，且**不新增任何 permission**（CXO 也不行）」
/// 來源：bindings.test.ts「六個 audit type 的 severity 是同形狀的（不可一邊三個一邊一個）」
#[actix_web::test]
async fn binding_permissions_and_severities() {
    assert_eq!(Kind::Workflow.permission(), "energy:workflow:manage");
    assert_eq!(Kind::Rating.permission(), "energy:rating:manage");
    for p in ["flow", "strategy"] {
        assert_eq!(get_audit_event_def(&format!("{p}.binding_created")).unwrap().severity, AuditSeverity::Info);
        assert_eq!(get_audit_event_def(&format!("{p}.binding_switched")).unwrap().severity, AuditSeverity::Notable);
        assert_eq!(get_audit_event_def(&format!("{p}.binding_ended")).unwrap().severity, AuditSeverity::Critical);
    }
    // CXO 沒有 govern:system → 按不到綁定按鈕；看得到綁定，但 canManage 是 false。
    let f = binding_fixture().await;
    let body = json!({ "kind": "workflow", "versionId": f.def_v1, "validFrom": "2000-01-01" });
    expect_status!(f.app.post(&f.cxo, &f.app.e(f.org, &format!("/stations/{}/bindings", f.bare)), body.clone()).await, 403);
    let v = expect_status!(f.app.get(&f.cxo, &f.app.e(f.org, &format!("/stations/{}/bindings", f.bare))).await, 200);
    assert_eq!(v["canManage"], json!({ "workflow": false, "rating": false }));
    let v = expect_status!(f.app.post(&f.admin, &f.app.e(f.org, &format!("/stations/{}/bindings", f.bare)), body).await, 200);
    assert_eq!(v["workflow"].as_array().unwrap().len(), 1);
    expect_status!(f.app.get(&f.admin, &f.app.e(f.org, "/binding-options?kind=nope")).await, 400);
    let v = expect_status!(f.app.get(&f.admin, &f.app.e(f.org, "/binding-options?kind=rating")).await, 200);
    assert_eq!(v["items"].as_array().unwrap().len(), 2);
}

/// 來源：bindings.test.ts「[workflow] 綁定 → 換版 → 解除，效期軸自己接得起來」
/// 來源：bindings.test.ts「[rating] 綁定 → 換版 → 解除，效期軸自己接得起來」
#[actix_web::test]
async fn bind_switch_end_both_kinds() {
    let f = binding_fixture().await;
    let ctx = f.ctx().await;
    for (kind, v1, v2, prefix) in [(Kind::Workflow, f.def_v1, f.def_v2, "flow"), (Kind::Rating, f.strat_v1, f.strat_v2, "strategy")] {
        let k = kind.as_str();
        in_tx!(f.app, tx => bindings::create(&mut tx, &ctx, f.bare, kind, None, v1, date("2000-01-01")).await).unwrap();
        let mut tx = f.app.core.pool.begin().await.unwrap();
        let out = bindings::list(&mut tx, &ctx, f.bare).await.unwrap();
        drop(tx);
        let segs = out[k].as_array().unwrap();
        assert_eq!(segs.len(), 1);
        assert!(segs[0]["validTo"].is_null());
        assert_eq!((segs[0]["isCurrent"].clone(), segs[0]["editable"].clone()), (json!(true), json!(true)));
        assert!(f.latest(&format!("{prefix}.binding_created")).await.is_some());
        let r = in_tx!(f.app, tx => bindings::create(&mut tx, &ctx, f.bare, kind, None, v2, date("2026-01-01")).await);
        assert_eq!(code_of(r).1, "binding_already_open");

        in_tx!(f.app, tx => bindings::switch_version(&mut tx, &ctx, f.bare, kind, None, v2, date("2026-01-01")).await).unwrap();
        let mut tx = f.app.core.pool.begin().await.unwrap();
        let out = bindings::list(&mut tx, &ctx, f.bare).await.unwrap();
        drop(tx);
        let segs = out[k].as_array().unwrap();
        assert_eq!(segs.len(), 2, "換版後兩段");
        assert_eq!(segs[0]["validTo"], "2026-01-01");
        assert_eq!(segs[1]["validFrom"], "2026-01-01");
        assert!(segs[1]["validTo"].is_null());
        assert_eq!(segs[1]["version"], 2);
        assert_eq!(segs[0]["editable"], false, "已結束的段不可編輯");
        assert_eq!(segs[1]["isCurrent"], true);
        let (summary, payload) = f.latest(&format!("{prefix}.binding_switched")).await.unwrap();
        assert!(summary.contains("換版"));
        assert_eq!((payload["kind"].clone(), payload["fromVersion"].clone(), payload["toVersion"].clone()), (json!(k), json!(1), json!(2)));
        let r = in_tx!(f.app, tx => bindings::switch_version(&mut tx, &ctx, f.bare, kind, None, v1, date("2025-01-01")).await);
        assert_eq!(code_of(r).1, "effective_from_before_current");

        let r = in_tx!(f.app, tx => bindings::end(&mut tx, &ctx, f.bare, kind, None, None, date("2026-06-01"), "   ").await);
        assert_eq!(code_of(r).1, "reason_required");
        in_tx!(f.app, tx => bindings::end(&mut tx, &ctx, f.bare, kind, None, None, date("2026-06-01"), "案場轉手，合約終止").await).unwrap();
        let mut tx = f.app.core.pool.begin().await.unwrap();
        let out = bindings::list(&mut tx, &ctx, f.bare).await.unwrap();
        drop(tx);
        let segs = out[k].as_array().unwrap();
        assert_eq!(segs.len(), 2, "解除不是刪除");
        assert_eq!(segs[1]["validTo"], "2026-06-01");
        let (summary, payload) = f.latest(&format!("{prefix}.binding_ended")).await.unwrap();
        assert!(summary.contains(if kind == Kind::Workflow { "收不了單" } else { "算不出應付" }), "{summary}");
        assert_eq!(payload["reason"], "案場轉手，合約終止");
        let r = in_tx!(f.app, tx => bindings::end(&mut tx, &ctx, f.bare, kind, None, None, date("2026-07-01"), "x").await);
        assert_eq!(code_of(r).1, "no_open_binding");
    }
}

/// 來源：bindings.test.ts「preview 是獨立一支，且**兩個數字都給**（含「不受影響的」那個）」
/// 來源：bindings.test.ts「漏綁的案場列得出來（漏綁不報錯，幾週後才變成一張退回的單）」
/// 來源：bindings.test.ts「跨組織動不了別人的案場（403，不是 404）」
/// 來源：bindings.test.ts「綁不到未發佈的版本（draft 上線＝還沒定稿的流程在跑）」
#[actix_web::test]
async fn preview_unbound_cross_org_and_drafts() {
    let f = binding_fixture().await;
    let ctx = f.ctx().await;
    let lo = f.app.landowner(f.org, "林地主").await;
    let st = f.app.station(f.org, lo, "12-0001", "預覽案場").await;
    let b1 = f.app.bill(f.org, Some(st), "2026-01-05", "2026-03-04", Some("60000.00"), Some("1000")).await;
    f.app.exec_sql(&format!("UPDATE tbl_energy_bill SET review_status = 'pendingReview' WHERE id = {b1}")).await;
    let running = f.app.bill(f.org, Some(st), "2026-01-05", "2026-04-04", Some("60000.00"), Some("1000")).await;
    f.app
        .exec_sql(&format!(
            "INSERT INTO tbl_energy_workflow_execution (organization_id, definition_id, subject_type, subject_id, station_key, started_at, status, current_node_key, business_date)
             VALUES ({}, {}, 'energyBill', '{running}', '12-0001', now(), 'running', 'manager-approve', '2026-04-04')",
            f.org, f.def_v1
        ))
        .await;
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let p = bindings::preview(&mut tx, &ctx, st, Kind::Workflow, date("2026-01-01")).await.unwrap();
    assert_eq!(p["isBackdated"], true, "早於今天＝回溯生效");
    assert_eq!(p["affectedDraftBills"], 2);
    assert_eq!(p["runningExecutions"], 1, "這個數字一定要有");
    let rows = bindings::unbound_stations(&mut tx, &ctx).await.unwrap();
    let bare = rows.iter().find(|r| r["id"] == f.bare).expect("漏綁的案場列得出來");
    assert_eq!((bare["missingWorkflow"].clone(), bare["missingRating"].clone()), (json!(true), json!(true)));
    drop(tx);

    let (other_owner, other_org) = f.app.energy_org("c7e-other").await;
    let other = f.app.ctx(&other_owner, other_org).await;
    let r = in_tx!(f.app, tx => bindings::create(&mut tx, &other, f.bare, Kind::Workflow, None, f.def_v1, date("2027-01-01")).await);
    assert_eq!(code_of(r), (403, "station_not_in_org".into()));
    // 跨組織的版本也綁不到自己的案場。
    let lo2 = f.app.landowner(other_org, "別家地主").await;
    let st2 = f.app.station(other_org, lo2, "99-0001", "別家案場").await;
    let r = in_tx!(f.app, tx => bindings::create(&mut tx, &other, st2, Kind::Workflow, None, f.def_v1, date("2027-01-01")).await);
    assert_eq!(code_of(r), (403, "version_not_in_org".into()));

    let draft: i64 = sqlx::query_scalar(
        "INSERT INTO tbl_energy_workflow_definition (organization_id, key, version, name, subject_type, status) VALUES ($1, 'bill-approval', 99, '草稿版', 'energyBill', 'draft') RETURNING id",
    )
    .bind(f.org)
    .fetch_one(&f.app.core.pool)
    .await
    .unwrap();
    let r = in_tx!(f.app, tx => bindings::create(&mut tx, &ctx, f.bare, Kind::Workflow, None, draft, date("2027-01-01")).await);
    assert_eq!(code_of(r), (409, "version_not_published".into()));
}
