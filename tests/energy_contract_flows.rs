//! 契約決定流程、配對決定參數：契約優先（多份取聯集）、案場保底、兩層效期、參數快照。

use crate::support::energy::{code_of, date};
use crate::support::{TestApp, TestUser};
use eo_domain::document_ingest::EnergyDocType;
use eo_domain::workflow_defs::{default_bill_approval, invoice_issue, utility_bill, DEPT_ACCOUNTING, DEPT_FINANCE, DEPT_SALES};
use eo_energy::workflow::definition::{contract_attributes, effective_contracts, resolve_flow_rows};
use eo_energy::workflow::engine::run::{start_execution, StartExecution};
use serde_json::json;

struct Fx {
    app: TestApp,
    org: i64,
    owner: TestUser,
    clerk: TestUser,
    landowner: i64,
    station: i64,
    station_key: String,
    approval: i64,
    invoice: i64,
    utility: i64,
}

async fn fixture() -> Fx {
    let app = TestApp::new().await;
    let (owner, org) = app.energy_org("ct-org").await;
    let clerk = app.member(org, "clerk", Some("energy:operator")).await;
    for key in [DEPT_SALES, DEPT_ACCOUNTING, DEPT_FINANCE] {
        let d = app.department(org, key, key).await;
        app.dept_member(d, &clerk, "member").await;
    }
    let landowner = app.landowner(org, "地主").await;
    let station_key = "00-60-1234-79-5".to_string();
    let station = app.station(org, landowner, &station_key, "契約測試案場").await;
    let approval = app.install_def(&owner, org, &default_bill_approval(), None).await;
    let invoice = app.install_def(&owner, org, &invoice_issue(), None).await;
    let utility = app.install_def(&owner, org, &utility_bill(), None).await;
    Fx { app, org, owner, clerk, landowner, station, station_key, approval, invoice, utility }
}

impl Fx {
    async fn resolve(&self, on: &str) -> eo_platform::error::ApiResult<Vec<eo_energy::workflow::ResolvedFlow>> {
        let ctx = self.app.ctx(&self.owner, self.org).await;
        let mut tx = self.app.core.pool.begin().await.unwrap();
        resolve_flow_rows(&mut tx, &ctx.access, &self.station_key, Some(self.station), EnergyDocType::RenewableSurplus, date(on)).await
    }

    async fn contract_covering(&self, station: i64, cover_from: Option<&str>, params: serde_json::Value, no: &str) -> i64 {
        let c = self.app.contract(self.org, self.landowner, no, "2000-01-01", None).await;
        self.app.contract_station(c, station, params, cover_from, None).await;
        c
    }
}

fn keys(flows: &[eo_energy::workflow::ResolvedFlow]) -> Vec<String> {
    let mut k: Vec<String> = flows.iter().map(|f| f.workflow_key.clone()).collect();
    k.sort();
    k
}

/// 來源：contractFlows.test.ts「沒有契約也沒有案場綁定 → 解不到（而且說得出是哪一種單）」
/// 來源：contractFlows.test.ts「只有案場層綁定 → 用案場層（既有行為不因為多了契約層而壞掉）」
/// 來源：contractFlows.test.ts「契約層一綁上去就**贏過**案場層（案場只是找到契約的路）」
/// 來源：contractFlows.test.ts「**多份契約取聯集** —— 一份發動核可、另一份發動開票，兩條都開」
/// 來源：contractFlows.test.ts「配對自己的效期會擋掉還沒輪到的那一份（案場是陸續併聯的）」
/// 來源：contractFlows.test.ts「`effectiveContracts` 只回在該日期生效的那幾份（兩層效期都算）」
#[actix_web::test]
async fn contract_layer_wins_and_unions() {
    let f = fixture().await;
    let e = f.resolve("2026-07-31").await;
    let err = e.expect_err("沒有任何綁定");
    assert_eq!(err.code, "no_binding_for_business_date");
    assert_eq!(err.details.unwrap()["docType"], "renewableSurplus", "說得出是哪一種單");

    f.app.wf_binding(f.org, Some(&f.station_key), None, Some("electricityBill"), f.utility, "2000-01-01", None).await;
    let flows = f.resolve("2026-07-31").await.unwrap();
    assert_eq!(keys(&flows), ["utility-bill"]);
    assert!(flows[0].contract.is_none(), "案場層那一段沒有契約");
    assert_eq!(flows[0].matched_doc_type.as_deref(), Some("electricityBill"), "細分回退到粗分");

    let c1 = f.contract_covering(f.station, None, json!({ "sharePercent": 12.5 }), "CT-1").await;
    f.app.wf_binding(f.org, None, Some(c1), Some("electricityBill"), f.approval, "2000-01-01", None).await;
    let flows = f.resolve("2026-07-31").await.unwrap();
    assert_eq!(keys(&flows), ["bill-approval"], "案場層的 utility-bill 不該再出現");
    assert_eq!(flows[0].contract.as_ref().unwrap().contract_id, c1);
    assert_eq!(flows[0].contract.as_ref().unwrap().params, json!({ "sharePercent": 12.5 }));

    let c2 = f.contract_covering(f.station, None, json!({ "rentPerMonth": 3000 }), "CT-2").await;
    f.app.wf_binding(f.org, None, Some(c2), Some("electricityBill"), f.invoice, "2000-01-01", None).await;
    let flows = f.resolve("2026-07-31").await.unwrap();
    assert_eq!(keys(&flows), ["bill-approval", "invoice-issue"]);
    let by = |k: &str| flows.iter().find(|x| x.workflow_key == k).unwrap().contract.clone().unwrap().params;
    assert_eq!(by("invoice-issue"), json!({ "rentPerMonth": 3000 }), "參數不可張冠李戴");
    assert_eq!(by("bill-approval"), json!({ "sharePercent": 12.5 }));

    let c3 = f.contract_covering(f.station, Some("2027-01-01"), json!({ "sharePercent": 99 }), "CT-3").await;
    f.app.wf_binding(f.org, None, Some(c3), Some("electricityBill"), f.utility, "2000-01-01", None).await;
    assert!(!keys(&f.resolve("2026-07-31").await.unwrap()).contains(&"utility-bill".to_string()));
    assert_eq!(keys(&f.resolve("2027-07-31").await.unwrap()), ["bill-approval", "invoice-issue", "utility-bill"]);

    let ctx = f.app.ctx(&f.owner, f.org).await;
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let in2026 = effective_contracts(&mut tx, &ctx.access, f.station, date("2026-07-31")).await.unwrap();
    let in2027 = effective_contracts(&mut tx, &ctx.access, f.station, date("2027-07-31")).await.unwrap();
    assert_eq!((in2026.len(), in2027.len()), (2, 3));
    assert!(in2026.iter().any(|c| c.params["sharePercent"] == 12.5), "參數是這一組配對的");
}

/// 來源：contractFlows.test.ts「送審時把**契約參數快照**帶進 execution，計算就用那一份」
#[actix_web::test]
async fn contract_params_snapshot_drives_the_calculation() {
    let f = fixture().await;
    let other = f.app.station(f.org, f.landowner, "07-42-0003-90-1", "快照測試案場").await;
    // 同一種契約、不同案主參數不同 —— 這一份的算式與比例寫在配對上。
    let c = f.contract_covering(other, None, json!({ "strategyType": "revenueShare", "sharePercent": 20 }), "CT-SNAP").await;
    f.app.wf_binding(f.org, None, Some(c), Some("electricityBill"), f.approval, "2000-01-01", None).await;
    let bill = f.app.bill(f.org, Some(other), "2026-06-01", "2026-06-30", Some("100000.00"), Some("10000.000")).await;

    // 收件送審的那條路：一次解析（契約層）→ 帶著解好的版本與契約快照開流程。
    let ctx = f.app.ctx(&f.clerk, f.org).await;
    let mut tx = f.app.core.pool.begin().await.unwrap();
    let flows = resolve_flow_rows(&mut tx, &ctx.access, "07-42-0003-90-1", Some(other), EnergyDocType::RenewableSurplus, date("2026-06-30"))
        .await
        .unwrap();
    assert_eq!(flows.len(), 1);
    let flow = &flows[0];
    let exec = start_execution(
        &mut tx,
        &ctx,
        StartExecution {
            station_key: "07-42-0003-90-1".into(),
            workflow_key: flow.workflow_key.clone(),
            subject_type: "energyBill".into(),
            subject_id: bill.to_string(),
            business_date: date("2026-06-30"),
            attributes: contract_attributes(json!({ "bill": { "totalAmount": 100000 } }), flow),
            definition: Some(flow.definition.clone()),
            station_id: Some(other),
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let snap = &exec.data["attributes"]["contract"];
    assert_eq!(snap["id"], c);
    assert_eq!(snap["params"], json!({ "strategyType": "revenueShare", "sharePercent": 20 }));

    // 這座案場沒有任何策略綁定 —— 算得出來就證明用的是契約那一份。
    let r = in_tx!(f.app, tx => eo_energy::rating::compute_for_bill(&mut tx, &ctx, bill, "workflowNode", Some(exec.id)).await).unwrap();
    assert_eq!(r.strategy_type, "revenueShare");
    assert_eq!(r.amount_str(), "20000.00", "100,000 的 20%");
    assert_eq!(r.params_snapshot["sharePercent"], 20);
    assert_eq!(r.data["paramsSource"], "contract");

    // 改了參數，已經開好的流程不受影響 —— 快照的全部意義。
    f.app.exec_sql(&format!(
        "UPDATE mn_energy_contract_station SET params = '{{\"strategyType\":\"revenueShare\",\"sharePercent\":5}}' WHERE contract_id = {c}"
    ))
    .await;
    let again = in_tx!(f.app, tx => eo_energy::rating::compute_for_bill(&mut tx, &ctx, bill, "manualRecompute", Some(exec.id)).await).unwrap();
    assert_eq!(again.amount_str(), "20000.00", "仍然照流程開始時的 20% 算");
    // 沒有流程快照的那條（不帶 execution）就回頭找案場策略 → 沒綁就算不了。
    let r = in_tx!(f.app, tx => eo_energy::rating::compute_for_bill(&mut tx, &ctx, bill, "manualRecompute", None).await);
    assert_eq!(code_of(r).1, "no_strategy_binding_for_business_date");
}
