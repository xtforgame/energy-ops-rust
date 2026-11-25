//! 收件、電費單三層值、送審去重、跨門檻稽核、單據 PATCH 的模式防線 —— 全程走 HTTP。
//! 外部管線的辨識結果用 `POST ingest-batches {files:[{originalFilename, ocrRaw}]}` 灌進來（App 5 的同一條縫）。

use crate::support::{TestApp, TestUser};
use eo_domain::business_key::{bill_business_key, normalize_customer_number};
use eo_domain::workflow_defs::{default_bill_approval, EXEC_APPROVAL_THRESHOLD};
use serde_json::{json, Value};
use std::cell::Cell;

pub struct Fx {
    pub app: TestApp,
    pub org: i64,
    pub admin: TestUser,
    pub clerk: TestUser,
    pub manager: TestUser,
    pub finance: TestUser,
    pub accountant: TestUser,
    pub viewer: TestUser,
    pub nobody: TestUser,
    pub station: i64,
    pub contract_no: String,
    pub installment: i64,
    seq: Cell<u32>,
}

pub const KNOWN: &str = "03-1111-2222";

impl Fx {
    /// Explicit legacy engine test. Ordinary intake intentionally uses the new single review flow.
    pub async fn start_legacy(&self, bill: i64) -> i64 {
        let v=expect_status!(self.app.post(&self.clerk,&self.u("/executions"),json!({"stationKey":KNOWN,"workflowKey":"bill-approval","subjectType":"energyBill","subjectId":bill.to_string(),"businessDate":"2026-02-28","attributes":{"bill":{"totalAmount":60000}}})).await,200);
        v["id"].as_i64().unwrap()
    }
    pub fn u(&self, p: &str) -> String {
        self.app.e(self.org, p)
    }

    pub async fn def_id(&self, key: &str) -> i64 {
        sqlx::query_scalar("SELECT id FROM tbl_energy_workflow_definition WHERE organization_id = $1 AND key = $2 ORDER BY version DESC LIMIT 1")
            .bind(self.org)
            .bind(key)
            .fetch_one(&self.app.core.pool)
            .await
            .unwrap()
    }

    /// 每呼叫一次就換一個計費期間（不循環）；`ym` 給「故意造重複」的測試指定期間。
    pub fn bill_ocr(&self, cn: &str, amount: f64, ym: Option<&str>) -> Value {
        let n = self.seq.get() + 1;
        self.seq.set(n);
        let p = ym.map(String::from).unwrap_or_else(|| format!("{}-{:02}", 2026 + (n - 1) / 12, (n - 1) % 12 + 1));
        json!({
            "raw": "台灣電力 電費通知 用電度數 計費期間",
            "bills": [{ "customerNumber": cn, "totalAmount": amount, "generationKwh": 1000,
                        "billingPeriodStart": format!("{p}-01"), "billingPeriodEnd": format!("{p}-28") }]
        })
    }

    pub fn deposit_ocr(contract_no: &str, seq: i32) -> Value {
        json!({ "title": "履約保證金請款單", "合約編號": contract_no, "期別": format!("第 {seq} 期"), "應付日": "2026-03-15", "請款金額": "150000" })
    }

    /// 開一批並帶辨識結果，回傳批次詳情。
    pub async fn run(&self, files: Vec<(&str, Value)>) -> Value {
        let files: Vec<Value> = files.into_iter().map(|(n, o)| json!({ "originalFilename": n, "ocrRaw": o })).collect();
        expect_status!(self.app.post(&self.clerk, &self.u("/ingest-batches"), json!({ "files": files })).await, 200)
    }

    pub async fn submit_item(&self, item: &Value, date: Option<&str>) -> (u16, Value) {
        let body = match date {
            Some(d) => json!({ "businessDate": d }),
            None => json!({}),
        };
        let (s, v) = self.app.post(&self.clerk, &self.u(&format!("/ingest-items/{}/submit", item["id"])), body).await;
        (s.as_u16(), v)
    }

    pub async fn bill_row(&self, id: i64) -> Value {
        expect_status!(self.app.get(&self.clerk, &self.u(&format!("/bills/{id}"))).await, 200)
    }

    pub async fn open_task(&self, who: &TestUser, exec: i64) -> Value {
        let v = expect_status!(self.app.get(who, &self.u(&format!("/executions/{exec}"))).await, 200);
        v["tasks"].as_array().unwrap().iter().rev().find(|t| t["state"] == "ready" || t["state"] == "reserved").cloned().unwrap()
    }

    pub async fn step(&self, who: &TestUser, exec: i64, action: Value) {
        let t = self.open_task(who, exec).await;
        let id = t["id"].as_i64().unwrap();
        if t["state"] == "ready" {
            expect_status!(self.app.post(who, &self.u(&format!("/tasks/{id}/claim")), json!({})).await, 200);
        }
        expect_status!(self.app.post(who, &self.u(&format!("/tasks/{id}/actions")), action).await, 200);
    }

    pub async fn audits(&self, target_type: &str, id: i64) -> Vec<(String, Value, String, Option<i64>)> {
        self.app.audit_rows(self.org, target_type, id).await
    }
}

/// 內建定義（安裝端點）、三個部門的成員、一座案場（電號 KNOWN）＋核可與保證金流程的綁定、預設策略綁定、一份契約與期數。
pub async fn fixture() -> Fx {
    fixture_with(TestApp::new().await).await
}

pub async fn fixture_with(app: TestApp) -> Fx {
    let (admin, org) = app.energy_org("intake-org").await;
    let clerk = app.member(org, "clerk", Some("energy:operator")).await;
    let manager = app.member(org, "manager", Some("energy:operator")).await;
    let finance = app.member(org, "finance", Some("energy:finance")).await;
    let accountant = app.member(org, "accountant", Some("energy:finance")).await;
    let viewer = app.member(org, "viewer", Some("energy:viewer")).await;
    let nobody = app.member(org, "nobody", None).await;
    // 七關樣本先佔住 bill-approval：install-defaults 遇同 key 會跳過（definition.rs:404-405）。
    app.install_def(&admin, org, &default_bill_approval(), None).await;
    expect_status!(app.post(&admin, &app.e(org, "/setup/install-defaults"), json!({})).await, 200);
    let dept = |k: &'static str| {
        let pool = app.core.pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>("SELECT id FROM tbl_energy_department WHERE organization_id = $1 AND key = $2")
                .bind(org)
                .bind(k)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    app.dept_member(dept("sales").await, &clerk, "member").await;
    app.dept_member(dept("bill-reviewers").await, &clerk, "member").await;
    app.dept_member(dept("bill-reviewers").await, &manager, "member").await;
    app.dept_member(dept("sales").await, &manager, "manager").await;
    app.dept_member(dept("finance").await, &finance, "manager").await;
    app.dept_member(dept("accounting").await, &accountant, "manager").await;
    let lo = app.landowner(org, "林地主").await;
    let station = app.station(org, lo, KNOWN, "屏東二號案場").await;
    let contract_no = "JV-2026-0001".to_string();
    let contract = app.contract(org, lo, &contract_no, "2026-01-01", None).await;
    app.contract_station(contract, station, json!({}), None, None).await;
    let installment = app.installment(org, contract, 2, "2026-03-15", "150000.00").await;
    let fx = Fx {
        app,
        org,
        admin,
        clerk,
        manager,
        finance,
        accountant,
        viewer,
        nobody,
        station,
        contract_no,
        installment,
        seq: Cell::new(0),
    };
    let bill_def = fx.def_id("bill-approval").await;
    let dep_def = fx.def_id("deposit-request").await;
    let utility_def = fx.def_id("utility-bill").await;
    fx.app.wf_binding(org, Some(KNOWN), None, Some("electricityBill"), bill_def, "2000-01-01", None).await;
    fx.app.wf_binding(org, Some(KNOWN), None, Some("consumptionBill"), utility_def, "2000-01-01", None).await;
    fx.app.wf_binding(org, Some(KNOWN), None, Some("depositRequest"), dep_def, "2000-01-01", None).await;
    let strategy: i64 = sqlx::query_scalar("SELECT id FROM tbl_energy_rating_strategy WHERE organization_id = $1 AND key = 'station-payout'")
        .bind(org)
        .fetch_one(&fx.app.core.pool)
        .await
        .unwrap();
    fx.app.strategy_binding(org, station, strategy, "2000-01-01", None).await;
    fx
}

fn item(batch: &Value, i: usize) -> Value {
    batch["items"][i].clone()
}

/// 來源：ingest.test.ts「一次 4 個檔 → 1 batch ＋ 4 item，四條路徑各走一次」
#[actix_web::test]
async fn four_files_take_four_paths() {
    let fx = fixture().await;
    let b = fx
        .run(vec![
            ("bill.jpg", fx.bill_ocr(KNOWN, 50000.0, None)),
            ("junk.jpg", json!({ "text": "完全看不出來是什麼" })),
            ("deposit-ok.jpg", Fx::deposit_ocr(&fx.contract_no, 2)),
            ("deposit-bad.jpg", Fx::deposit_ocr("JV-9999-0000", 7)),
        ])
        .await;
    let items = b["items"].as_array().unwrap();
    assert_eq!(items.len(), 4);
    assert_eq!((items[0]["docType"].clone(), items[0]["status"].clone()), (json!("electricityBill"), json!("extracted")));
    assert!(items[0]["billId"].is_i64());
    assert_eq!((items[1]["docType"].clone(), items[1]["status"].clone(), items[1]["billId"].clone()), (json!("unclassified"), json!("unclassified"), Value::Null));
    assert_eq!((items[2]["docType"].clone(), items[2]["status"].clone()), (json!("depositRequest"), json!("extracted")));
    assert_eq!(items[2]["billId"], Value::Null, "保證金不可以產生 energyBill");
    assert_eq!(items[2]["installmentId"], fx.installment);
    assert_eq!((items[3]["status"].clone(), items[3]["installmentId"].clone()), (json!("unmatched"), Value::Null));
    assert_eq!(b["status"], "ready");

    // 來源：ingest.test.ts「送單紀錄答得出「上傳 N → 送審／待處理各幾份」」
    let list = expect_status!(fx.app.get(&fx.clerk, &fx.u("/ingest-batches")).await, 200);
    let four = &list["items"][0];
    assert_eq!((four["counts"]["unclassified"].clone(), four["counts"]["unmatched"].clone(), four["counts"]["extracted"].clone()), (json!(1), json!(1), json!(2)));
    assert_eq!(four["submittedByUserId"], fx.clerk.id);
    assert_eq!(four["pendingCount"], 4);
    let audits = fx.audits("energyIngestBatch", b["id"].as_i64().unwrap()).await;
    assert!(audits.iter().any(|a| a.0 == "ingest.batch_classified" && a.1["byStatus"]["extracted"] == 2));
}

/// 來源：ingest.test.ts「一批裡有**兩張同一張紙** → 兩筆都建得起來，但第二筆送審被擋」
/// ＋「同批重複**在 `getBatch` 就標出來**」＋「同批重複：**先送第二筆**回 `duplicate_in_batch`，先送第一筆放行」
#[actix_web::test]
async fn same_paper_twice_in_one_batch() {
    let fx = fixture().await;
    let b = fx.run(vec![("dup-a.jpg", fx.bill_ocr(KNOWN, 60000.0, Some("2099-11"))), ("dup-b.jpg", fx.bill_ocr(KNOWN, 60000.0, Some("2099-11")))]).await;
    let (a, bb) = (item(&b, 0), item(&b, 1));
    assert_eq!(a["businessKey"], bb["businessKey"]);
    assert!(a["businessKey"].is_string());
    assert_eq!(a["duplicateOfItemId"], Value::Null, "第一筆是本尊");
    assert_eq!((bb["duplicateOfItemId"].clone(), bb["duplicateOfSeq"].clone()), (a["id"].clone(), json!(1)));

    let (s, v) = fx.submit_item(&bb, Some("2099-11-28")).await;
    assert_eq!((s, v["code"].clone()), (409, json!("duplicate_in_batch")));
    assert_eq!((v["details"]["itemId"].clone(), v["details"]["seq"].clone(), v["details"]["originalFilename"].clone()), (a["id"].clone(), json!(1), json!("dup-a.jpg")));
    let (s, _) = fx.submit_item(&a, Some("2099-11-28")).await;
    assert_eq!(s, 200);
    // 第一筆在跑了，第二筆再送就是 already_in_review（第一筆還可能被退回，不能說「已經通過」）。
    let (s, v) = fx.submit_item(&bb, Some("2099-11-28")).await;
    assert_eq!((s, v["code"].clone()), (409, json!("already_in_review")));
}

/// 來源：ingest.test.ts「捨棄第一筆之後，第二筆就**變成本尊**送得出去」
#[actix_web::test]
async fn discarding_the_original_promotes_the_twin() {
    let fx = fixture().await;
    let b = fx.run(vec![("k-a.jpg", fx.bill_ocr(KNOWN, 60000.0, Some("2099-07"))), ("k-b.jpg", fx.bill_ocr(KNOWN, 60000.0, Some("2099-07")))]).await;
    let v = expect_status!(fx.app.post(&fx.clerk, &fx.u(&format!("/ingest-items/{}/discard", item(&b, 0)["id"])), json!({ "reason": "duplicate" })).await, 200);
    assert_eq!(v["status"], "discarded");
    let view = expect_status!(fx.app.get(&fx.clerk, &fx.u(&format!("/ingest-batches/{}", b["id"]))).await, 200);
    assert_eq!(view["items"][1]["duplicateOfItemId"], Value::Null);
    let (s, _) = fx.submit_item(&item(&b, 1), Some("2099-07-28")).await;
    assert_eq!(s, 200);
    let view = expect_status!(fx.app.get(&fx.clerk, &fx.u(&format!("/ingest-batches/{}", b["id"]))).await, 200);
    assert_eq!(view["status"], "completed", "全部有了歸宿（送審或捨棄）");
    // 已送審的不能捨棄。
    let (s, v) = fx.app.post(&fx.clerk, &fx.u(&format!("/ingest-items/{}/discard", item(&b, 1)["id"])), json!({})).await;
    assert_eq!((s.as_u16(), v["code"].clone()), (409, json!("item_already_submitted")));
}

/// 來源：ingest.test.ts「清單頁的兩個關鍵：`pendingCount` ＋ **未完成置頂**」
/// ＋「batch 狀態是**推導**的：全部待校對 → ready」＋「送審一筆（共兩筆）→ batch 自動 `partial`，不是前端傳的」
#[actix_web::test]
async fn batch_status_is_derived_and_pending_sorts_first() {
    let fx = fixture().await;
    let done = fx.run(vec![("done.jpg", fx.bill_ocr(KNOWN, 60000.0, Some("2099-06")))]).await;
    assert_eq!(done["status"], "ready");
    fx.submit_item(&item(&done, 0), Some("2099-06-28")).await;
    let pending = fx.run(vec![("pending.jpg", fx.bill_ocr(KNOWN, 60000.0, Some("2099-05")))]).await;
    let two = fx.run(vec![("a.jpg", fx.bill_ocr(KNOWN, 60000.0, None)), ("b.jpg", fx.bill_ocr(KNOWN, 70000.0, None))]).await;
    fx.submit_item(&item(&two, 0), Some("2026-02-28")).await;
    let v = expect_status!(fx.app.get(&fx.clerk, &fx.u(&format!("/ingest-batches/{}", two["id"]))).await, 200);
    assert_eq!(v["status"], "partial");
    fx.submit_item(&item(&two, 1), Some("2026-02-28")).await;
    let v = expect_status!(fx.app.get(&fx.clerk, &fx.u(&format!("/ingest-batches/{}", two["id"]))).await, 200);
    assert_eq!(v["status"], "completed");

    let rows = expect_status!(fx.app.get(&fx.clerk, &fx.u("/ingest-batches")).await, 200)["items"].as_array().unwrap().clone();
    let of = |id: &Value| rows.iter().find(|r| &r["id"] == id).unwrap()["pendingCount"].as_i64().unwrap();
    assert_eq!((of(&done["id"]), of(&pending["id"])), (0, 1));
    let flags: Vec<i64> = rows.iter().map(|r| i64::from(r["pendingCount"].as_i64().unwrap() == 0)).collect();
    assert!(flags.windows(2).all(|w| w[0] <= w[1]), "未完成沒有置頂：{flags:?}");
}

/// 來源：ingest.test.ts「保證金送審 → execution 的 subjectType 是 `energyInstallment`」
#[actix_web::test]
async fn deposit_submits_against_the_installment() {
    let fx = fixture().await;
    let b = fx.run(vec![("d.jpg", Fx::deposit_ocr(&fx.contract_no, 2))]).await;
    let (s, v) = fx.submit_item(&item(&b, 0), Some("2026-03-15")).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!((v["subjectType"].clone(), v["subjectId"].clone()), (json!("energyInstallment"), json!(fx.installment.to_string())));
}

/// 來源：ingest.test.ts「`unclassified` / `unmatched` 不可送審，但**有出口**（手動指定）」
/// ＋「從單反查得到「哪一批進來的」」
#[actix_web::test]
async fn unclassified_has_an_exit() {
    let fx = fixture().await;
    let b = fx.run(vec![("junk2.jpg", json!({ "text": "看不出來" }))]).await;
    let it = item(&b, 0);
    let (s, v) = fx.submit_item(&it, None).await;
    assert_eq!((s, v["code"].clone()), (409, json!("item_needs_doc_type")));
    let p = expect_status!(fx.app.patch(&fx.clerk, &fx.u(&format!("/ingest-items/{}", it["id"])), json!({ "docType": "renewableSurplus" })).await, 200);
    assert_eq!(p["workflowKey"], "bill-approval");
    assert_eq!(p["status"], "unmatched", "有單、但還沒電號 → 例外件");
    let bill_id = p["billId"].as_i64().expect("要建出一張可以填的空白單");
    let blank = fx.bill_row(bill_id).await;
    assert_eq!((blank["stationId"].clone(), blank["customerNumberRaw"].clone(), blank["ocrExtracted"].clone()), (Value::Null, Value::Null, json!({})));
    assert_eq!(blank["docType"], "renewableSurplus");
    assert!(fx.audits("energyBill", bill_id).await.iter().any(|a| a.0 == "bill.created_manually"));
    let (s, v) = fx.submit_item(&it, None).await;
    assert_eq!((s, v["code"].clone()), (409, json!("item_needs_source_match")));
    // 把電號填上去，那一列就要解鎖。
    expect_status!(
        fx.app.patch(&fx.clerk, &fx.u(&format!("/bills/{bill_id}")), json!({ "customerNumberRaw": KNOWN, "billingPeriodStart": "2026-01-01", "billingPeriodEnd": "2026-01-31" })).await,
        200
    );
    let view = expect_status!(fx.app.get(&fx.clerk, &fx.u(&format!("/ingest-batches/{}", b["id"]))).await, 200);
    assert_eq!(view["items"][0]["status"], "extracted");
    let filled = fx.bill_row(bill_id).await;
    assert_eq!(filled["stationId"], fx.station);
    assert_eq!(filled["ingest"]["batchId"], b["id"]);
    assert_eq!(filled["ingest"]["originalFilename"], "junk2.jpg");
    // 值域擋在後端。
    let (s, v) = fx.app.patch(&fx.clerk, &fx.u(&format!("/ingest-items/{}", it["id"])), json!({ "docType": "bogus" })).await;
    assert_eq!((s.as_u16(), v["code"].clone()), (400, json!("invalid_doc_type")));
}

/// 來源：ingest.test.ts「跨組織 403，不是 404」
#[actix_web::test]
async fn cross_org_batch_is_403() {
    let fx = fixture().await;
    let b = fx.run(vec![("x.jpg", fx.bill_ocr(KNOWN, 1.0, None))]).await;
    let (owner2, org2) = fx.app.energy_org("other-org").await;
    let (s, v) = fx.app.get(&owner2, &fx.app.e(org2, &format!("/ingest-batches/{}", b["id"]))).await;
    assert_eq!((s.as_u16(), v["code"].clone()), (403, json!("batch_not_in_org")));
    let (s, v) = fx.app.get(&owner2, &fx.app.e(org2, &format!("/bills/{}", item(&b, 0)["billId"]))).await;
    assert_eq!((s.as_u16(), v["code"].clone()), (403, json!("bill_not_in_org")));
    let (s, _) = fx.app.get(&owner2, &fx.app.e(org2, "/ingest-batches/999999")).await;
    assert_eq!(s.as_u16(), 404);
}

/// 來源：ingest.test.ts「發起人打開自己送出的單 → **零個動作** ＋ `excludedOwner`」
/// ＋「不是候選人 → `notCandidate`（不是「權限不足」）」
#[actix_web::test]
async fn submitter_sees_no_actions_at_approval() {
    let fx = fixture().await;
    let b = fx.run(vec![("blocked.png", fx.bill_ocr(KNOWN, 55000.0, None))]).await;
    let exec = fx.start_legacy(item(&b,0)["billId"].as_i64().unwrap()).await;
    let t = fx.open_task(&fx.clerk, exec).await;
    assert_eq!(t["nodeKey"], "clerk-review");
    fx.step(&fx.clerk, exec, json!({ "actionKey": "submit" })).await;
    let v = expect_status!(fx.app.get(&fx.clerk, &fx.u(&format!("/executions/{exec}/actions"))).await, 200);
    assert!(v["availableActions"].as_array().unwrap().is_empty());
    assert_eq!(v["actionBlocked"]["code"], "excludedOwner");
    let outsider = fx.app.member(fx.org, "outsider", Some("energy:viewer")).await;
    let v = expect_status!(fx.app.get(&outsider, &fx.u(&format!("/executions/{exec}/actions"))).await, 200);
    assert_eq!(v["actionBlocked"]["code"], "notCandidate");
}

// ── 電費單（billIngest.test.ts）──

/// 來源：billIngest.test.ts「a known customer number lands matched, with all three value layers」
/// ＋「an UNKNOWN customer number becomes an EXCEPTION — the bill is still created」
/// ＋「one scan with several customer numbers produces several bills」＋「"nothing extracted at all" IS an error (≠ unmatched)」
#[actix_web::test]
async fn ingest_three_layers_and_exceptions() {
    let fx = fixture().await;
    let ocr = json!({ "engine": "mock-v0", "pages": 3, "bills": [{ "customerNumber": KNOWN, "計費起": "2026/01/01", "計費訖": "2026/01/31",
        "totalAmount": "98,765", "generationKwh": "4321.5", "issuedAt": "2026/02/05", "confidence": 0.93 }] });
    let r = expect_status!(fx.app.post(&fx.clerk, &fx.u("/bills/ingest"), json!({ "ocrJson": ocr })).await, 200);
    assert_eq!((r["matched"].clone(), r["unmatched"].clone()), (json!(1), json!(0)));
    let id = r["billIds"][0].as_i64().unwrap();
    let b = fx.bill_row(id).await;
    assert_eq!((b["stationId"].clone(), b["reviewStatus"].clone()), (json!(fx.station), json!("pendingReview")));
    assert_eq!((b["ocrRaw"]["engine"].clone(), b["ocrRaw"]["pages"].clone()), (json!("mock-v0"), json!(3)));
    assert_eq!((b["ocrExtracted"]["totalAmount"].clone(), b["ocrExtracted"]["billingPeriodEnd"].clone()), (json!(98765.0), json!("2026-01-31")));
    assert_eq!((b["totalAmount"].clone(), b["generationKwh"].clone(), b["billingPeriodEnd"].clone()), (json!("98765.00"), json!("4321.500"), json!("2026-01-31")));
    assert_eq!(b["changedFields"], json!([]), "decimal 帶尾零不算被改過");
    let audits = fx.audits("energyBill", id).await;
    let m = audits.iter().find(|a| a.0 == "bill.matched").unwrap();
    assert_eq!((m.2.as_str(), m.1.get("manual")), ("success", None), "自動配上的不是人工指認");

    let r = expect_status!(fx.app.post(&fx.clerk, &fx.u("/bills/ingest"), json!({ "ocrJson": { "bills": [{ "customerNumber": "99-0000-0000", "totalAmount": 500 }] } })).await, 200);
    assert_eq!(r["unmatched"], 1);
    let un = r["billIds"][0].as_i64().unwrap();
    let b = fx.bill_row(un).await;
    assert_eq!((b["stationId"].clone(), b["reviewStatus"].clone(), b["customerNumberRaw"].clone()), (Value::Null, json!("pendingMatch"), json!("99-0000-0000")));
    assert_eq!(fx.audits("energyBill", un).await.iter().find(|a| a.0 == "bill.unmatched").unwrap().2, "failed");

    let r = expect_status!(
        fx.app.post(&fx.clerk, &fx.u("/bills/ingest"), json!({ "ocrJson": { "bills": [{ "customerNumber": KNOWN, "totalAmount": 1 }, { "customerNumber": "88-x", "totalAmount": 2 }] } })).await,
        200
    );
    assert_eq!((r["billIds"].as_array().unwrap().len(), r["matched"].clone(), r["unmatched"].clone()), (2, json!(1), json!(1)));
    let (s, v) = fx.app.post(&fx.clerk, &fx.u("/bills/ingest"), json!({ "ocrJson": { "garbage": true } })).await;
    assert_eq!((s.as_u16(), v["code"].clone()), (422, json!("nothing_extracted")));

    // 來源：billIngest.test.ts「US-5: the exception list shows it, and manual matching puts it back on track」
    let list = expect_status!(fx.app.get(&fx.clerk, &fx.u("/bills/unmatched")).await, 200);
    assert!(list["items"].as_array().unwrap().iter().any(|b| b["id"] == un));
    let v = expect_status!(fx.app.post(&fx.clerk, &fx.u(&format!("/bills/{un}/match")), json!({ "stationId": fx.station })).await, 200);
    assert_eq!(v["reviewStatus"], "pendingReview");
    let list = expect_status!(fx.app.get(&fx.clerk, &fx.u("/bills/unmatched")).await, 200);
    assert!(!list["items"].as_array().unwrap().iter().any(|b| b["id"] == un));
    let audits = fx.audits("energyBill", un).await;
    assert_eq!(audits.iter().rev().find(|a| a.0 == "bill.matched").unwrap().1["manual"], true);

    // 來源：billIngest.test.ts「cannot match a bill / station from another org」＋「a user without flow:act gets 403 on ingest」
    let (o2, org2) = fx.app.energy_org("b2-other").await;
    let (s, v) = fx.app.post(&o2, &fx.app.e(org2, &format!("/bills/{un}/match")), json!({ "stationId": fx.station })).await;
    assert_eq!((s.as_u16(), v["code"].clone()), (403, json!("bill_not_in_org")));
    let (s, _) = fx.app.post(&fx.nobody, &fx.u("/bills/ingest"), json!({ "ocrJson": { "customerNumber": "x" } })).await;
    assert_eq!(s.as_u16(), 403);
}

/// 來源：billIngest.test.ts describe「correcting touches ONLY the final layer」的六條，
/// ＋「the correct endpoint has no way to touch ocrRaw / ocrExtracted」
/// ＋ domainModel.test.ts「correcting the final value does NOT touch the extracted value」「the diff between ② and ③ is what the review screen shows」
#[actix_web::test]
async fn correcting_touches_only_the_final_layer() {
    let fx = fixture().await;
    let ocr = json!({ "engine": "v0", "bills": [{ "customerNumber": KNOWN, "billingPeriodStart": "2026-01-01", "billingPeriodEnd": "2026-01-31", "totalAmount": 98765, "generationKwh": 4321.5 }] });
    let id = expect_status!(fx.app.post(&fx.clerk, &fx.u("/bills/ingest"), json!({ "ocrJson": ocr })).await, 200)["billIds"][0].as_i64().unwrap();
    let before = fx.bill_row(id).await;
    // 惡意 body：企圖覆蓋辨識值 —— 不認得的欄位被丟掉。
    let after = expect_status!(
        fx.app.patch(&fx.clerk, &fx.u(&format!("/bills/{id}")), json!({ "totalAmount": 98000, "ocrExtracted": { "totalAmount": 1 }, "ocrRaw": { "hacked": true } })).await,
        200
    );
    assert_eq!(after["totalAmount"], "98000.00");
    assert_eq!(after["ocrExtracted"], before["ocrExtracted"], "② 一個字都不能變");
    assert_eq!(after["ocrRaw"], before["ocrRaw"], "① 也不動");
    assert_eq!(after["changedFields"], json!(["totalAmount"]));
    let rows = fx.audits("energyBill", id).await;
    let c = rows.iter().find(|a| a.0 == "bill.corrected").unwrap();
    assert_eq!((c.1["changes"]["totalAmount"]["to"].clone(), c.1["changes"]["totalAmount"]["from"].clone()), (json!(98000.0), json!("98765.00")));
    assert_eq!(c.1["ocrExtracted"]["totalAmount"], json!(98765.0), "稽核列自己答得出 OCR 當時抽到什麼");
    // 值沒變就不該多一筆稽核。
    let n = rows.iter().filter(|a| a.0 == "bill.corrected").count();
    expect_status!(fx.app.patch(&fx.clerk, &fx.u(&format!("/bills/{id}")), json!({ "totalAmount": 98000 })).await, 200);
    assert_eq!(fx.audits("energyBill", id).await.iter().filter(|a| a.0 == "bill.corrected").count(), n);

    // 改電號 → 重新配案場；配不上 → 退回例外件，業務身分跟著重算。
    let v = expect_status!(fx.app.patch(&fx.clerk, &fx.u(&format!("/bills/{id}")), json!({ "customerNumberRaw": "99-99-9999-99-9" })).await, 200);
    assert_eq!((v["stationId"].clone(), v["reviewStatus"].clone()), (Value::Null, json!("pendingMatch")));
    assert!(v["businessKey"].as_str().unwrap().contains("9999999999"));
    let (s, v) = fx.app.patch(&fx.clerk, &fx.u(&format!("/bills/{id}")), json!({ "markReviewed": true })).await;
    assert_eq!((s.as_u16(), v["code"].clone()), (409, json!("station_required_before_review")));
    let v = expect_status!(fx.app.patch(&fx.clerk, &fx.u(&format!("/bills/{id}")), json!({ "customerNumberRaw": KNOWN })).await, 200);
    assert_eq!((v["stationId"].clone(), v["reviewStatus"].clone()), (json!(fx.station), json!("pendingReview")));
    // 同一個請求裡改壞電號＋標記已校對也不行。
    let (s, v) = fx.app.patch(&fx.clerk, &fx.u(&format!("/bills/{id}")), json!({ "customerNumberRaw": "88-88-8888-88-8", "markReviewed": true })).await;
    assert_eq!((s.as_u16(), v["code"].clone()), (409, json!("station_required_before_review")));
    let v = expect_status!(fx.app.patch(&fx.clerk, &fx.u(&format!("/bills/{id}")), json!({ "totalAmount": 4000, "markReviewed": true })).await, 200);
    assert_eq!(v["reviewStatus"], "reviewed");
    // 值域擋在後端。
    let (s, v) = fx.app.patch(&fx.clerk, &fx.u(&format!("/bills/{id}")), json!({ "docType": "somethingElse" })).await;
    assert_eq!((s.as_u16(), v["code"].clone()), (400, json!("invalid_doc_type")));
}

// ── 送審去重（submitDedupe.test.ts）──

async fn mk_bill(fx: &Fx, cn: Option<&str>, to: &str) -> i64 {
    let key = bill_business_key(cn, Some("2026-01-01"), Some(to));
    sqlx::query_scalar(
        "INSERT INTO tbl_energy_bill (organization_id, station_id, customer_number_raw, billing_period_start, billing_period_end, total_amount,
                                      generation_kwh, review_status, business_key, ocr_raw, ocr_extracted)
         VALUES ($1, $2, $3, '2026-01-01', $4::date, 60000.00, 15000.000, 'reviewed', $5, '{}', '{}') RETURNING id",
    )
    .bind(fx.org)
    .bind(fx.station)
    .bind(cn)
    .bind(to)
    .bind(key)
    .fetch_one(&fx.app.core.pool)
    .await
    .unwrap()
}

async fn try_submit(fx: &Fx, bill: i64) -> (u16, Value) {
    let (s, v) = fx.app.post(&fx.clerk, &fx.u(&format!("/bills/{bill}/submit")), json!({})).await;
    (s.as_u16(), v)
}

/// 來源：submitDedupe.test.ts「帶分隔符與純數字的電號算**同一個** businessKey」
/// ＋「改了計費期間會**重算** businessKey」＋「把期間**清掉**也要重算」
#[actix_web::test]
async fn business_key_follows_corrections() {
    let fx = fixture().await;
    let plain = normalize_customer_number(Some(KNOWN)).unwrap();
    assert_eq!(bill_business_key(Some(KNOWN), Some("2026-01-01"), Some("2026-02-28")), bill_business_key(Some(&plain), Some("2026-01-01"), Some("2026-02-28")));
    let id = mk_bill(&fx, Some(KNOWN), "2026-02-28").await;
    let v = expect_status!(fx.app.patch(&fx.clerk, &fx.u(&format!("/bills/{id}")), json!({ "billingPeriodEnd": "2026-03-04" })).await, 200);
    assert_eq!(v["businessKey"], format!("bill|{plain}|2026-01-01|2026-03-04"));
    let v = expect_status!(fx.app.patch(&fx.clerk, &fx.u(&format!("/bills/{id}")), json!({ "billingPeriodStart": null })).await, 200);
    assert_eq!(v["businessKey"], Value::Null, "沒有期間就沒有業務身分");
    let v = expect_status!(fx.app.patch(&fx.clerk, &fx.u(&format!("/bills/{id}")), json!({ "billingPeriodStart": "2026-01-01" })).await, 200);
    assert!(v["businessKey"].is_string());
}

/// 來源：submitDedupe.test.ts「第一筆送得出去；第二筆（同一張紙、電號寫法不同）回 **already_in_review**」
/// ＋「第一筆**被中斷**後，第二筆**送得出去**」＋「走到 `end` 才寫 `approvalState=approved`；之後再送回 **duplicate_document**」
/// ＋「DB partial unique 是最後一道」＋「`businessKey` 為 NULL 的單**送得出去**」
#[actix_web::test]
async fn submit_dedupe_two_layers() {
    let fx = fixture().await;
    let plain = normalize_customer_number(Some(KNOWN)).unwrap();
    let first = mk_bill(&fx, Some(KNOWN), "2026-04-30").await;
    let second = mk_bill(&fx, Some(&plain), "2026-04-30").await;
    let (s, r1) = try_submit(&fx, first).await;
    assert_eq!(s, 200, "{r1}");
    let (s, r2) = try_submit(&fx, second).await;
    assert_eq!((s, r2["code"].clone()), (409, json!("already_in_review")));
    assert_eq!(r2["details"]["executionId"], r1["id"]);
    assert!(r2["details"]["currentNodeName"].is_string(), "印節點名稱不印 key");
    assert_eq!(r2["details"]["submittedByName"], "clerk");
    assert!(r2["details"]["submittedAt"].is_string());
    // 已在跑的單再按一次送審。
    let (s, v) = try_submit(&fx, first).await;
    assert_eq!((s, v["code"].clone()), (409, json!("bill_already_submitted")));

    // 中斷之後同一張紙可以重送。
    fx.app.exec_sql(&format!("UPDATE tbl_energy_workflow_execution SET status = 'aborted', ended_at = now() WHERE id = {}", r1["id"])).await;
    let (s, r3) = try_submit(&fx, second).await;
    assert_eq!(s, 200, "{r3}");

    // 走到 end 才通過；之後同一張紙回 duplicate_document。
    let b = expect_status!(fx.app.get(&fx.clerk, &fx.u(&format!("/bills/{second}"))).await, 200);
    assert_eq!(b["approvalState"], "pending", "送審只代表開始跑");
    expect_status!(fx.app.post(&fx.admin, &fx.u(&format!("/executions/{}/force-advance", r3["id"])), json!({ "toNodeKey": "end", "reason": "測試：直接推到完成" })).await, 200);
    assert_eq!(fx.bill_row(second).await["approvalState"], "approved");
    let third = mk_bill(&fx, Some(KNOWN), "2026-04-30").await;
    let (s, v) = try_submit(&fx, third).await;
    assert_eq!((s, v["code"].clone()), (409, json!("duplicate_document")));
    assert_eq!(v["details"]["executionId"], r3["id"]);
    // DB 的 partial unique：兩張同 businessKey 不能都 approved。
    let e = sqlx::query(&format!("UPDATE tbl_energy_bill SET approval_state = 'approved' WHERE id = {third}")).execute(&fx.app.core.pool).await.unwrap_err();
    assert_eq!(e.as_database_error().unwrap().code().as_deref(), Some("23505"));
    // businessKey 為 NULL 一律放行。
    let nokey = mk_bill(&fx, None, "2026-04-30").await;
    let (s, v) = try_submit(&fx, nokey).await;
    assert_eq!(s, 200, "{v}");
}

// ── 跨門檻稽核（routeGateAudit.test.ts）──

const ABOVE: i64 = EXEC_APPROVAL_THRESHOLD + 38195;
const BELOW: i64 = EXEC_APPROVAL_THRESHOLD - 1;

async fn amount_bill(fx: &Fx, amount: i64) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO tbl_energy_bill (organization_id, station_id, customer_number_raw, billing_period_start, billing_period_end, total_amount,
                                      generation_kwh, review_status, ocr_raw, ocr_extracted)
         VALUES ($1, $2, $3, '2026-01-05', '2026-03-04', $4::numeric, 48250.000, 'pendingReview', '{}', '{}') RETURNING id",
    )
    .bind(fx.org)
    .bind(fx.station)
    .bind(KNOWN)
    .bind(amount.to_string())
    .fetch_one(&fx.app.core.pool)
    .await
    .unwrap()
}

async fn events(fx: &Fx, bill: i64) -> Vec<(i64, String, Value, Option<i64>, String)> {
    sqlx::query_as(
        "SELECT id, type, payload, caused_by_event_id, summary FROM tbl_energy_audit_event
          WHERE organization_id = $1 AND target_type = 'energyBill' AND target_id = $2 ORDER BY id",
    )
    .bind(fx.org)
    .bind(bill.to_string())
    .fetch_all(&fx.app.core.pool)
    .await
    .unwrap()
}

/// 來源：routeGateAudit.test.ts「改小跨門檻 → `bill.route_gate_removed`(critical)，且指得出少了哪一關」
/// ＋「改大跨門檻 → `bill.route_gate_added`(notable)」＋「同一側的金額修正**不**產生路線事件」
/// ＋「改的不是金額（不影響條件）→ 不產生路線事件」＋「稽核算出的路線 ＝ 校對台預覽算出的路線（同一支 `walkRoute`）」
#[actix_web::test]
async fn route_gate_audit() {
    let fx = fixture().await;
    let bill = amount_bill(&fx, ABOVE).await;
    let preview = expect_status!(fx.app.get(&fx.clerk, &fx.u(&format!("/bills/{bill}/branch-preview?totalAmount={BELOW}"))).await, 200);
    assert_eq!(preview["available"], true);
    let preview_keys: Vec<Value> = preview["nodes"].as_array().unwrap().iter().map(|n| n["nodeKey"].clone()).collect();
    assert!(!preview_keys.contains(&json!("exec-approve")));
    assert!(preview["removed"].as_array().unwrap().iter().any(|n| n["nodeKey"] == "exec-approve"));
    let (s, _) = fx.app.get(&fx.clerk, &fx.u(&format!("/bills/{bill}/branch-preview?totalAmount=abc"))).await;
    assert_eq!(s.as_u16(), 400);

    expect_status!(fx.app.patch(&fx.clerk, &fx.u(&format!("/bills/{bill}")), json!({ "totalAmount": BELOW })).await, 200);
    let ev = events(&fx, bill).await;
    let gate = ev.iter().find(|e| e.1 == "bill.route_gate_removed").expect("少一關一定要留下一筆");
    let corrected = ev.iter().find(|e| e.1 == "bill.corrected").unwrap();
    assert_eq!(gate.3, Some(corrected.0), "「做了什麼」與「造成什麼」串起來");
    assert!(gate.2["gatesDelta"]["removed"].as_array().unwrap().contains(&json!("exec-approve")));
    assert!(gate.2["gatesDelta"]["added"].as_array().unwrap().is_empty());
    assert!(!gate.2["gatesAfter"].as_array().unwrap().contains(&json!("exec-approve")));
    assert!(gate.2["gatesAfter"].as_array().unwrap().contains(&json!("manager-approve")));
    assert_eq!(gate.2["gateNames"]["exec-approve"], "高層加簽");
    assert!(gate.4.contains("少了一關") && gate.4.contains("高層加簽") && gate.4.contains("138,195"), "{}", gate.4);
    assert_eq!(eo_domain::audit_events::get_audit_event_def("bill.route_gate_removed").unwrap().severity, eo_domain::audit_events::AuditSeverity::Critical);

    let up = amount_bill(&fx, BELOW).await;
    expect_status!(fx.app.patch(&fx.clerk, &fx.u(&format!("/bills/{up}")), json!({ "totalAmount": ABOVE })).await, 200);
    let g = events(&fx, up).await.into_iter().find(|e| e.1 == "bill.route_gate_added").unwrap();
    assert!(g.4.contains("多了一關"));

    let same = amount_bill(&fx, BELOW).await;
    expect_status!(fx.app.patch(&fx.clerk, &fx.u(&format!("/bills/{same}")), json!({ "totalAmount": BELOW - 5000 })).await, 200);
    let ev = events(&fx, same).await;
    assert!(!ev.iter().any(|e| e.1.starts_with("bill.route_gate")) && ev.iter().any(|e| e.1 == "bill.corrected"));
    let kwh = amount_bill(&fx, ABOVE).await;
    expect_status!(fx.app.patch(&fx.clerk, &fx.u(&format!("/bills/{kwh}")), json!({ "generationKwh": 12345 })).await, 200);
    assert!(!events(&fx, kwh).await.iter().any(|e| e.1.starts_with("bill.route_gate")));
}

/// 來源：mode.test.ts「以 **act** 模式打 `PATCH /bills/:id` → 403 subject_not_editable」
/// ＋「revise 重新送審 → execution **還是同一筆**」的 bill_already_submitted 那一半
#[actix_web::test]
async fn patch_is_guarded_by_mode() {
    let fx = fixture().await;
    let bill = amount_bill(&fx, 60000).await;
    let exec = fx.start_legacy(bill).await;
    expect_status!(fx.app.post(&fx.admin, &fx.u(&format!("/executions/{exec}/force-advance")), json!({ "toNodeKey": "manager-approve", "reason": "測試：推到不可編輯的關卡" })).await, 200);
    let v = fx.bill_row(bill).await;
    let as_manager = expect_status!(fx.app.get(&fx.manager, &fx.u(&format!("/bills/{bill}"))).await, 200);
    assert_eq!((as_manager["mode"].clone(), as_manager["canEditSubject"].clone()), (json!("act"), json!(false)));
    assert_eq!(v["docTypeEditable"], false, "有流程在跑就不能改單別");
    let (s, e) = fx.app.patch(&fx.manager, &fx.u(&format!("/bills/{bill}")), json!({ "totalAmount": 1 })).await;
    assert_eq!((s.as_u16(), e["code"].clone(), e["details"]["mode"].clone()), (403, json!("subject_not_editable"), json!("act")));
    assert_eq!(fx.bill_row(bill).await["totalAmount"], "60000.00", "金額不可以被改掉");
    let (s, e) = try_submit(&fx, bill).await;
    assert_eq!((s, e["code"].clone()), (409, json!("bill_already_submitted")));
    let n = fx.app.scalar_i64(&format!("SELECT COUNT(*) FROM tbl_energy_workflow_execution WHERE subject_id = '{bill}'")).await;
    assert_eq!(n, 1, "被擋之後仍然只有一筆");
}

/// 卡 20261126-0210 S3：流程跑完（readonly＋finished）之後，仍然可以單獨 PATCH `markReviewed:true`；
/// 其他欄位（連同 finished 時一起帶）一律照舊 403 `subject_not_editable`。
#[actix_web::test]
async fn mark_reviewed_after_flow_finished() {
    let fx = fixture().await;
    let bill = amount_bill(&fx, 70000).await;
    // 一般 `/submit` 在這份 fixture 走的是單步自動審核 `bill-data-review`——跑完就直接把
    // `reviewStatus` 設成 `reviewed`（`workflow/engine/run.rs:123-126`），驗不到「流程跑完、
    // 仍待校對」這個 S3 要補的缺口。照 `patch_is_guarded_by_mode` 的做法，直接用舊版引擎走
    // `bill-approval`（七關樣本，完成時不會自動改 reviewStatus）。
    let exec = fx.start_legacy(bill).await;
    expect_status!(fx.app.post(&fx.admin, &fx.u(&format!("/executions/{exec}/force-advance")), json!({ "toNodeKey": "end", "reason": "測試：直接推到完成" })).await, 200);

    let v = expect_status!(fx.app.get(&fx.manager, &fx.u(&format!("/bills/{bill}"))).await, 200);
    assert_eq!((v["mode"].clone(), v["modeReason"]["code"].clone(), v["canEditSubject"].clone()), (json!("readonly"), json!("finished"), json!(false)), "{v}");
    assert_eq!(v["reviewStatus"], "pendingReview", "{v}");

    // 其他欄位：finished 時一樣照舊 403（即使只帶一個鍵）。
    let (s, e) = fx.app.patch(&fx.manager, &fx.u(&format!("/bills/{bill}")), json!({ "totalAmount": 1 })).await;
    assert_eq!((s.as_u16(), e["code"].clone(), e["details"]["mode"].clone()), (403, json!("subject_not_editable"), json!("readonly")));

    // finished 時帶 markReviewed 以外的欄位（即使也帶 markReviewed）一樣 403。
    let (s, e) = fx.app.patch(&fx.manager, &fx.u(&format!("/bills/{bill}")), json!({ "markReviewed": true, "totalAmount": 1 })).await;
    assert_eq!((s.as_u16(), e["code"].clone()), (403, json!("subject_not_editable")));

    // 只有 markReviewed:true 一個鍵：放行。
    let marked = expect_status!(fx.app.patch(&fx.manager, &fx.u(&format!("/bills/{bill}")), json!({ "markReviewed": true })).await, 200);
    assert_eq!(marked["reviewStatus"], "reviewed", "{marked}");
    // audit_rows 只選得到 (type, payload, outcome, actor)，稽核的人話 summary 是另一個欄位
    // （不在這支測試 helper 的查詢裡），用 payload 確認是這一筆「只標記已校對、沒有其他欄位變更」的事件。
    let audited = fx.audits("energyBill", bill).await;
    assert!(
        audited.iter().any(|e| e.0 == "bill.corrected" && e.1["markReviewed"] == true && e.1["changes"] == json!({})),
        "{audited:?}"
    );

    // 已校對之後再 PATCH totalAmount 仍然 403（不是因為標記過，是因為流程還是完成狀態）。
    let (s, e) = fx.app.patch(&fx.manager, &fx.u(&format!("/bills/{bill}")), json!({ "totalAmount": 2 })).await;
    assert_eq!((s.as_u16(), e["code"].clone()), (403, json!("subject_not_editable")));
}

/// 來源：executionList.test.ts「「尚未送審」的家在**電費單**：`listBills` 每列帶 `executionId`（null ＝ 沒送）」
/// ＋ domainModel.test.ts「the exception list is a single indexed query」
#[actix_web::test]
async fn bill_list_carries_one_execution_link() {
    let fx = fixture().await;
    let unsent = amount_bill(&fx, 1000).await;
    let sent = amount_bill(&fx, 2000).await;
    fx.app.exec_sql(&format!("UPDATE tbl_energy_bill SET billing_period_end = '2026-03-05', business_key = NULL WHERE id = {sent}")).await;
    let (_, r) = try_submit(&fx, sent).await;
    let list = expect_status!(fx.app.get(&fx.clerk, &fx.u("/bills")).await, 200)["items"].as_array().unwrap().clone();
    let of = |id: i64| list.iter().find(|b| b["id"] == id).cloned().unwrap();
    assert_eq!(of(unsent)["executionId"], Value::Null);
    assert_eq!(of(sent)["executionId"], r["id"]);
    for k in ["nodeKey", "nodeName", "waitingDays", "workflowKey", "version"] {
        assert!(of(unsent).get(k).is_none(), "電費單上不可以有 {k}");
    }
    let (s, v) = fx.app.get(&fx.clerk, &fx.u("/bills?from=2026-1")).await;
    assert_eq!((s.as_u16(), v["code"].clone()), (400, json!("invalid_date")));
    let filtered = expect_status!(fx.app.get(&fx.clerk, &fx.u("/bills?reviewStatus=pendingMatch")).await, 200);
    assert!(filtered["items"].as_array().unwrap().is_empty());
    // 應付紀錄端點（尚未計算 → 空）。
    let p = expect_status!(fx.app.get(&fx.clerk, &fx.u(&format!("/bills/{sent}/payouts"))).await, 200);
    assert!(p["items"].as_array().unwrap().is_empty());
}

/// 電費單共用資料審核；刪除舊案場綁定不阻擋電費送審。
/// 來源：recognitionFlow.test.ts「該跑的流程**不存在**就擋在送審之前（不是開一半）」＋「改了單別，會開哪些流程**當場跟著變**」
#[actix_web::test]
async fn binding_status_follows_doc_type() {
    let fx = fixture().await;
    let b = fx.run(vec![("x.jpg", fx.bill_ocr(KNOWN, 1000.0, None))]).await;
    let bill = item(&b, 0)["billId"].as_i64().unwrap();
    let v = fx.bill_row(bill).await;
    assert_eq!(v["binding"]["bound"], true);
    assert_eq!(v["binding"]["flows"][0]["workflowKey"], "bill-data-review");
    let v = expect_status!(fx.app.patch(&fx.clerk, &fx.u(&format!("/bills/{bill}")), json!({ "docType": "consumptionDeviceRental" })).await, 200);
    assert_eq!(v["binding"]["flows"][0]["workflowKey"], "bill-data-review");
    assert_eq!(v["binding"]["docType"], "consumptionDeviceRental");
    // 刪掉舊用電流程綁定，新的電費資料審核仍正常送審。
    fx.app.exec_sql(&format!("UPDATE tbl_energy_workflow_binding SET deleted_at = now() WHERE organization_id = {} AND doc_type = 'consumptionBill'", fx.org)).await;
    let v = fx.bill_row(bill).await;
    assert_eq!(v["binding"]["bound"], true, "電費審核不依賴舊案場綁定");
    let (s, e) = fx.submit_item(&item(&b, 0), None).await;
    assert_eq!(s, 200, "{e}");
    assert_eq!(fx.app.scalar_i64(&format!("SELECT COUNT(*) FROM tbl_energy_workflow_execution WHERE subject_id = '{bill}'")).await, 1);
}
