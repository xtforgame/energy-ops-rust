//! 先上傳、後辨識：上傳原檔 → 送假辨識服務（回四份真範本的結果）→ tick 寫回 → 校對 → 送審 → 走完流程。
//! 以及辨識驅動的每條岔路：租約、分批、逾時、連不上、415、需要密碼、HEIC、對帳重送、背景任務、直傳。

use crate::energy_intake::{fixture_with, Fx};
use crate::support::{TestApp, TestUser};
use actix_web::{web, App, HttpResponse, HttpServer};
use eo_energy::fake_tpr;
use eo_energy::ingest::recognition;
use reqwest::Method;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

const SURPLUS: &str = "電費單(有餘電需開發票).pdf";
const NO_SURPLUS: &str = "電費單(無餘電需付款).pdf";
const DEVICE: &str = "逆變器電費單(僅設備發電使用電費).pdf";
const RENTAL: &str = "逆變器電費單(含電表租費&設備發電使用電費).pdf";

struct Rx {
    fx: Fx,
    fake: web::Data<fake_tpr::FakeTprState>,
    surplus_station: i64,
}

async fn app_with(tweak: impl FnOnce(&mut eo_platform::config::Config)) -> TestApp {
    TestApp::with_config(tweak).await
}

/// 假服務：前 `polls` 次查詢回 processing。案場只建三座（無餘電那張的電號故意不建 → 例外件）。
async fn rx(polls: u32, tweak: impl FnOnce(&mut eo_platform::config::Config)) -> Rx {
    let (url, fake, server) = fake_tpr::spawn(0, polls).unwrap();
    actix_web::rt::spawn(server);
    let app = app_with(|c| {
        c.tpr_url = Some(url);
        tweak(c)
    })
    .await;
    let fx = fixture_with(app).await;
    let lo = fx.app.landowner(fx.org, "範本地主").await;
    let bill_def = fx.def_id("bill-approval").await;
    let utility_def = fx.def_id("utility-bill").await;
    let strategy: i64 = sqlx::query_scalar("SELECT id FROM tbl_energy_rating_strategy WHERE organization_id = $1 AND key = 'station-payout'")
        .bind(fx.org)
        .fetch_one(&fx.app.core.pool)
        .await
        .unwrap();
    let mut surplus_station = 0;
    for (cn, name) in [("00-60-1600-79-5", "躉購案場"), ("11-72-0328-95-9", "高壓案場一"), ("07-42-8560-90-1", "高壓案場二")] {
        // 主檔裡的寫法不帶分隔符：配案場比的是正規化後的電號。
        let id = fx.app.station(fx.org, lo, &cn.replace('-', ""), name).await;
        if surplus_station == 0 {
            surplus_station = id;
            fx.app.strategy_binding(fx.org, id, strategy, "2000-01-01", None).await;
        }
        fx.app.wf_binding(fx.org, Some(&cn.replace('-', "")), None, Some("electricityBill"), bill_def, "2000-01-01", None).await;
        fx.app.wf_binding(fx.org, Some(&cn.replace('-', "")), None, Some("consumptionBill"), utility_def, "2000-01-01", None).await;
        // 卡 B：S1 比對到合約——這幾座案場也要有一份生效合約，不然範本單會變成 pendingContract。
        let contract = fx.app.contract(fx.org, lo, &format!("RX-{id}"), "2026-01-01", None).await;
        fx.app.contract_station(contract, id, json!({}), None, None).await;
    }
    Rx { fx, fake, surplus_station }
}

impl Rx {
    fn u(&self, p: &str) -> String {
        self.fx.u(p)
    }

    async fn new_batch(&self, n: usize) -> i64 {
        let v = expect_status!(self.fx.app.post(&self.fx.clerk, &self.u("/ingest-batches"), json!({ "fileCount": n })).await, 200);
        assert_eq!(v["status"], "uploading");
        assert!(v["items"].as_array().unwrap().is_empty(), "開批之後還沒有任何 item");
        v["id"].as_i64().unwrap()
    }

    async fn upload(&self, batch: i64, name: &str, mime: &str) -> (u16, Value) {
        upload_as(&self.fx.app, &self.fx.clerk, &self.u(&format!("/ingest-batches/{batch}/attachments")), name, mime, b"%PDF-1.4 fake".to_vec()).await
    }

    async fn batch(&self, batch: i64) -> Value {
        expect_status!(self.fx.app.get(&self.fx.clerk, &self.u(&format!("/ingest-batches/{batch}"))).await, 200)
    }

    async fn recognize(&self, batch: i64, body: Value) -> Value {
        expect_status!(self.fx.app.post(&self.fx.clerk, &self.u(&format!("/ingest-batches/{batch}/recognize")), body).await, 200)
    }

    async fn statuses(&self, batch: i64) -> Vec<String> {
        sqlx::query_scalar("SELECT status FROM tbl_energy_ingest_item WHERE batch_id = $1 ORDER BY id")
            .bind(batch)
            .fetch_all(&self.fx.app.core.pool)
            .await
            .unwrap()
    }

    async fn batch_audits(&self, batch: i64) -> Vec<(String, Value, String, Option<i64>)> {
        self.fx.audits("energyIngestBatch", batch).await
    }
}

pub async fn upload_as(app: &TestApp, user: &TestUser, url: &str, name: &str, mime: &str, bytes: Vec<u8>) -> (u16, Value) {
    let part = reqwest::multipart::Part::bytes(bytes).file_name(name.to_string()).mime_str(mime).unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);
    let (s, v) = TestApp::send(app.req(Some(user), Method::POST, url).multipart(form)).await;
    (s.as_u16(), v)
}

/// 端到端：上傳四份真範本 → 辨識分成四種單別 → 校對 → 送審 → 走完流程 → 單據通過、算出應付；同一張紙再送被擋。
/// 來源：recognitionFlow.test.ts「躉購單 → energyBill ＋ 電號對得上案場（正規化比對）」「高壓用電單 → `consumptionBill` ＋ `utility-bill`，且帶著繳費期限」
/// 「一批混著兩種單 → 各走各的流程（分類器有在做事）」「消化完會刪掉服務上的第二份原始檔，而且**留痕**」「全程留下 requested ＋ completed 兩筆稽核」
/// 「上傳一個檔 → 一筆 `uploaded` 的 item，附件天生一對一」「開批之後還沒有任何 item」
#[actix_web::test]
async fn end_to_end_four_samples() {
    let r = rx(1, |_| {}).await;
    let fx = &r.fx;
    let st = expect_status!(fx.app.get(&fx.clerk, &r.u("/recognition/status")).await, 200);
    assert_eq!((st["configured"].clone(), st["available"].clone()), (json!(true), json!(true)));

    let batch = r.new_batch(4).await;
    for name in [SURPLUS, NO_SURPLUS, DEVICE, RENTAL] {
        let (s, v) = r.upload(batch, name, "application/pdf").await;
        assert_eq!(s, 200, "{v}");
        assert!(v["itemId"].is_i64());
        assert_eq!(v["originalFilename"], name);
    }
    let b = r.batch(batch).await;
    assert_eq!(b["status"], "uploading", "檔案都在但還沒辨識 —— 不可以說可以校對了");
    assert!(b["items"].as_array().unwrap().iter().all(|i| i["status"] == "uploaded" && i["attachmentId"].is_i64()));

    let out = r.recognize(batch, json!({})).await;
    assert_eq!((out["itemCount"].clone(), out["chunks"].clone(), out["configured"].clone()), (json!(4), json!(1), json!(true)));
    let job = out["jobId"].as_str().unwrap().to_string();
    assert_eq!(out["batch"]["status"], "recognizing");
    // 第一次讀：服務還在跑，tick 只問一次不動。
    assert_eq!(r.batch(batch).await["status"], "recognizing");
    // 第二次讀：服務完成，tick 寫回。
    let b = r.batch(batch).await;
    assert_eq!(b["status"], "ready", "{b}");
    let items = b["items"].as_array().unwrap().clone();
    let by_name = |n: &str| items.iter().find(|i| i["originalFilename"] == n).cloned().unwrap();
    let docs: Vec<(String, String, String)> = [SURPLUS, NO_SURPLUS, DEVICE, RENTAL]
        .iter()
        .map(|n| {
            let i = by_name(n);
            (i["docType"].as_str().unwrap().into(), i["workflowKey"].as_str().unwrap().into(), i["status"].as_str().unwrap().into())
        })
        .collect();
    assert_eq!(
        docs,
        vec![
            ("renewableSurplus".into(), "bill-data-review".into(), "extracted".into()),
            ("renewableNoSurplus".into(), "bill-data-review".into(), "unmatched".into()),
            ("consumptionDevice".into(), "bill-data-review".into(), "extracted".into()),
            ("consumptionDeviceRental".into(), "bill-data-review".into(), "extracted".into()),
        ]
    );
    assert!(by_name(SURPLUS)["recognition"]["templateId"] == "renewable_purchase");

    let surplus = by_name(SURPLUS);
    let bill_id = surplus["billId"].as_i64().unwrap();
    let bill = fx.bill_row(bill_id).await;
    assert_eq!(bill["stationId"], r.surplus_station, "電號正規化比對：主檔不帶分隔符也配得上");
    assert_eq!((bill["totalAmount"].clone(), bill["generationKwh"].clone()), (json!("127848.00"), json!("21072.000")));
    assert_eq!(bill["templateId"], "renewable_purchase");
    assert_eq!(bill["businessKey"], "bill|00601600795|2026-06-09|2026-08-10");
    assert_eq!(bill["ocrRaw"]["record"]["fields"]["payableTotal"]["value"], json!(133744));
    assert_eq!(bill["sourceAttachmentId"], surplus["attachmentId"]);
    let hv = fx.bill_row(by_name(DEVICE)["billId"].as_i64().unwrap()).await;
    assert_eq!((hv["totalAmount"].clone(), hv["dueDate"].clone(), hv["docType"].clone()), (json!("5035.00"), json!("2026-08-20"), json!("consumptionDevice")));

    let audits = r.batch_audits(batch).await;
    let types: Vec<&str> = audits.iter().map(|a| a.0.as_str()).collect();
    for t in ["ingest.recognition_requested", "ingest.recognition_completed", "ingest.recognition_purged"] {
        assert!(types.contains(&t), "{t} 沒留痕：{types:?}");
    }
    assert_eq!(audits.iter().find(|a| a.0 == "ingest.recognition_purged").unwrap().1, json!({ "jobId": job, "ok": true }));
    let (s, _) = TestApp::send(fx.app.http.get(format!("{}/api/v1/jobs/{job}", fake_base(&r)))).await;
    assert_eq!(s.as_u16(), 404, "服務上的第二份原始檔被刪了");

    // 校對：金額改到門檻以下（少一關高層加簽，稽核要說得出來）。
    let v = expect_status!(fx.app.patch(&fx.clerk, &r.u(&format!("/bills/{bill_id}")), json!({ "totalAmount": 99999, "markReviewed": true })).await, 200);
    assert_eq!((v["reviewStatus"].clone(), v["changedFields"].clone()), (json!("reviewed"), json!(["totalAmount"])));
    assert!(fx.audits("energyBill", bill_id).await.iter().any(|a| a.0 == "bill.route_gate_removed"));

    // 送審：開流程、原檔接到 execution 上。
    let (s, sub) = fx.submit_item(&surplus, None).await;
    assert_eq!(s, 200, "{sub}");
    let exec = sub["executionId"].as_i64().unwrap();
    let att = expect_status!(fx.app.get(&fx.manager, &r.u(&format!("/executions/{exec}/attachments"))).await, 200);
    assert_eq!(att["items"][0]["originalFilename"], SURPLUS);
    assert_eq!(att["items"][0]["links"][0]["role"], "source");
    let dl = fx.app.req(Some(&fx.manager), Method::GET, &r.u(&format!("/attachments/{}/download", surplus["attachmentId"]))).send().await.unwrap();
    assert_eq!(dl.status().as_u16(), 200);
    assert!(dl.headers()["content-disposition"].to_str().unwrap().contains("filename*=UTF-8''"));
    assert_eq!(dl.bytes().await.unwrap().as_ref(), b"%PDF-1.4 fake");

    // 平台只完成資料審核；不代替 EF 簽核，也不計算地主租金。
    fx.step(&fx.clerk, exec, json!({ "actionKey": "pass" })).await;
    assert_eq!(fx.app.exec_state(exec).await.0, "completed");
    let bill = fx.bill_row(bill_id).await;
    assert_eq!((bill["approvalState"].clone(), bill["mode"].clone()), (json!("approved"), json!("readonly")));
    let payouts = expect_status!(fx.app.get(&fx.finance, &r.u(&format!("/bills/{bill_id}/payouts"))).await, 200);
    assert!(payouts["items"].as_array().unwrap().is_empty());
    let b = r.batch(batch).await;
    assert_eq!(b["status"], "partial");

    // 同一張紙再送一次：辨識照樣建得起來，送審被 duplicate_document 擋下。
    let again = r.new_batch(1).await;
    r.upload(again, SURPLUS, "application/pdf").await;
    r.recognize(again, json!({})).await;
    r.batch(again).await;
    let b2 = r.batch(again).await;
    let dup = b2["items"][0].clone();
    assert_eq!(dup["status"], "extracted");
    let (s, v) = fx.submit_item(&dup, None).await;
    assert_eq!((s, v["code"].clone()), (409, json!("duplicate_document")));
    assert_eq!(v["details"]["executionId"], exec);
}

fn fake_base(r: &Rx) -> String {
    r.fx.app.core.config.tpr_url.clone().unwrap()
}

/// 例外件：電號對不上的單 → 例外件清單 → 人工指認 → 那一列解鎖 → 送審。
/// 來源：billIngest.test.ts「US-5: the exception list shows it, and manual matching puts it back on track」（辨識那條路）
#[actix_web::test]
async fn unmatched_station_flow() {
    let r = rx(0, |_| {}).await;
    let fx = &r.fx;
    let batch = r.new_batch(1).await;
    r.upload(batch, NO_SURPLUS, "application/pdf").await;
    r.recognize(batch, json!({})).await;
    let b = r.batch(batch).await;
    let it = b["items"][0].clone();
    assert_eq!((it["status"].clone(), it["stationId"].clone(), it["reviewStatus"].clone()), (json!("unmatched"), Value::Null, json!("pendingMatch")));
    let bill = it["billId"].as_i64().unwrap();
    let list = expect_status!(fx.app.get(&fx.clerk, &r.u("/bills/unmatched")).await, 200);
    assert!(list["items"].as_array().unwrap().iter().any(|b| b["id"] == bill));
    let (s, v) = fx.submit_item(&it, None).await;
    assert_eq!((s, v["code"].clone()), (409, json!("item_needs_source_match")));
    let search = expect_status!(fx.app.get(&fx.clerk, &r.u("/stations?q=躉購")).await, 200);
    let station = search["items"][0]["id"].as_i64().unwrap();
    expect_status!(fx.app.post(&fx.clerk, &r.u(&format!("/bills/{bill}/match")), json!({ "stationId": station })).await, 200);
    let b = r.batch(batch).await;
    assert_eq!(b["items"][0]["status"], "extracted", "人工指認後那一列解鎖");
    let (s, v) = fx.submit_item(&b["items"][0], None).await;
    assert_eq!(s, 200, "{v}");
}

/// 來源：recognitionFlow.test.ts「服務說某一份要密碼 → 那一份 `failed` 且訊息說得出下一步」
/// ＋ recognitionPlatform.test.ts「送出：…sourceFileIds＝item id、密碼放…」（直連模式：密碼隨檔案送）
#[actix_web::test]
async fn password_required_is_its_own_failure() {
    let r = rx(0, |_| {}).await;
    let batch = r.new_batch(2).await;
    r.upload(batch, "locked.pdf", "application/pdf").await;
    r.upload(batch, "locked-too.pdf", "application/pdf").await;
    let items: Vec<i64> = r.batch(batch).await["items"].as_array().unwrap().iter().map(|i| i["id"].as_i64().unwrap()).collect();
    // 第二份給了密碼：服務當成功。
    r.recognize(batch, json!({ "passwords": { items[1].to_string(): "pw-77" } })).await;
    let b = r.batch(batch).await;
    let locked = &b["items"][0];
    assert_eq!(locked["status"], "failed");
    let msg = locked["errorMessage"].as_str().unwrap();
    assert!(msg.contains("密碼") && msg.contains("重新辨識"), "{msg}");
    assert_eq!(locked["recognitionErrorCode"], "password_required");
    assert_eq!(b["items"][1]["status"], "extracted");
    // 密碼不進稽核。
    let audits = r.batch_audits(batch).await;
    let req = audits.iter().find(|a| a.0 == "ingest.recognition_requested").unwrap();
    assert_eq!(req.1["withPassword"], 1);
    assert!(!req.1.to_string().contains("pw-77"));
    assert_eq!(audits.iter().find(|a| a.0 == "ingest.recognition_completed").unwrap().2, "failed");
}

/// 一律回 415 的辨識服務。
async fn always_415() -> String {
    let server = HttpServer::new(|| {
        App::new().route(
            "/api/v1/jobs",
            web::post().to(|| async { HttpResponse::UnsupportedMediaType().json(json!({ "error": { "code": "unsupported_file", "message": "不支援的檔案" } })) }),
        )
    })
    .workers(1)
    .bind(("127.0.0.1", 0))
    .unwrap();
    let port = server.addrs()[0].port();
    actix_web::rt::spawn(server.run());
    format!("http://127.0.0.1:{port}")
}

/// 來源：recognitionFlow.test.ts「服務回 415 時標成失敗，**不退回 `uploaded`**（退回＝無限重試）」
#[actix_web::test]
async fn permanent_415_marks_failed() {
    let url = always_415().await;
    let fx = fixture_with(TestApp::with_config(|c| c.tpr_url = Some(url)).await).await;
    let b = expect_status!(fx.app.post(&fx.clerk, &fx.u("/ingest-batches"), json!({ "fileCount": 1 })).await, 200);
    let batch = b["id"].as_i64().unwrap();
    upload_as(&fx.app, &fx.clerk, &fx.u(&format!("/ingest-batches/{batch}/attachments")), "weird.pdf", "application/pdf", b"%PDF".to_vec()).await;
    let out = expect_status!(fx.app.post(&fx.clerk, &fx.u(&format!("/ingest-batches/{batch}/recognize")), json!({})).await, 200);
    assert_eq!((out["jobId"].clone(), out["skipped"].clone()), (Value::Null, json!("unsupported_file")));
    let it = &out["batch"]["items"][0];
    assert_eq!((it["status"].clone(), it["recognitionErrorCode"].clone()), (json!("failed"), json!("unsupported_file")));
    assert_eq!(out["batch"]["status"], "failed");
    assert!(fx.audits("energyIngestBatch", batch).await.iter().any(|a| a.0 == "ingest.recognition_failed" && a.1["code"] == "unsupported_file"));
}

/// 來源：recognitionFlow.test.ts「檔案留著、item 退回 `uploaded`，不 fallback 成假辨識」
#[actix_web::test]
async fn unreachable_service_keeps_files() {
    let fx = fixture_with(TestApp::with_config(|c| c.tpr_url = Some("http://127.0.0.1:9".into())).await).await;
    let b = expect_status!(fx.app.post(&fx.clerk, &fx.u("/ingest-batches"), json!({ "fileCount": 1 })).await, 200);
    let batch = b["id"].as_i64().unwrap();
    upload_as(&fx.app, &fx.clerk, &fx.u(&format!("/ingest-batches/{batch}/attachments")), "offline.pdf", "application/pdf", b"%PDF".to_vec()).await;
    let out = expect_status!(fx.app.post(&fx.clerk, &fx.u(&format!("/ingest-batches/{batch}/recognize")), json!({})).await, 200);
    assert_eq!((out["jobId"].clone(), out["skipped"].clone()), (Value::Null, json!("tpr_unreachable")));
    let it = &out["batch"]["items"][0];
    assert_eq!((it["status"].clone(), it["billId"].clone()), (json!("uploaded"), Value::Null));
    assert!(fx.audits("energyIngestBatch", batch).await.iter().any(|a| a.0 == "ingest.recognition_failed"));
    let st = expect_status!(fx.app.get(&fx.clerk, &fx.u("/recognition/status")).await, 200);
    assert_eq!((st["configured"].clone(), st["available"].clone(), st["reason"].clone()), (json!(true), json!(false), json!("tpr_not_ready")));
}

/// 沒設定辨識服務：status 說得出來；送辨識留一筆失敗、檔案不動。
#[actix_web::test]
async fn not_configured() {
    let fx = crate::energy_intake::fixture().await;
    let st = expect_status!(fx.app.get(&fx.clerk, &fx.u("/recognition/status")).await, 200);
    assert_eq!((st["configured"].clone(), st["available"].clone()), (json!(false), json!(false)));
    let b = expect_status!(fx.app.post(&fx.clerk, &fx.u("/ingest-batches"), json!({ "fileCount": 1 })).await, 200);
    let batch = b["id"].as_i64().unwrap();
    upload_as(&fx.app, &fx.clerk, &fx.u(&format!("/ingest-batches/{batch}/attachments")), "a.pdf", "application/pdf", b"%PDF".to_vec()).await;
    let out = expect_status!(fx.app.post(&fx.clerk, &fx.u(&format!("/ingest-batches/{batch}/recognize")), json!({})).await, 200);
    assert_eq!((out["configured"].clone(), out["skipped"].clone()), (json!(false), json!("tpr_not_configured")));
    assert_eq!(out["batch"]["items"][0]["status"], "uploaded");
    assert_eq!(recognition::tick_batch(&fx.app.core, batch).await.unwrap(), "not_configured");
}

/// 來源：recognitionPlatform.test.ts「驅動：執行中只問一次不動；終態才寫回、留稽核」＋「同一批同時兩個呼叫：只有一個拿到租約」
#[actix_web::test]
async fn driver_ticks_once_and_leases() {
    let r = rx(1, |_| {}).await;
    let core = r.fx.app.core.clone();
    let batch = r.new_batch(1).await;
    r.upload(batch, SURPLUS, "application/pdf").await;
    let job = r.recognize(batch, json!({})).await["jobId"].as_str().unwrap().to_string();
    // 別人持有租約（還沒過期）時不動它。不用兩個並行 tick 去撞：它們不保證重疊，先到的放掉租約後
    // 後到的會接著做，結果正確但不確定。
    let until = (core.clock.now() + chrono::Duration::minutes(3)).to_rfc3339();
    sqlx::query("UPDATE tbl_energy_ingest_batch SET data = jsonb_set(data, '{driverLeaseUntil}', to_jsonb($2::text)) WHERE id = $1")
        .bind(batch)
        .bind(&until)
        .execute(&core.pool)
        .await
        .unwrap();
    assert_eq!(recognition::tick_batch(&core, batch).await.unwrap(), "leased");
    // 租約過期就能接手；執行中只問一次、不寫回。（不快轉時鐘：會超過辨識的時間預算。）
    let expired = (core.clock.now() - chrono::Duration::seconds(1)).to_rfc3339();
    sqlx::query("UPDATE tbl_energy_ingest_batch SET data = jsonb_set(data, '{driverLeaseUntil}', to_jsonb($2::text)) WHERE id = $1")
        .bind(batch)
        .bind(&expired)
        .execute(&core.pool)
        .await
        .unwrap();
    assert_eq!(recognition::tick_batch(&core, batch).await.unwrap(), "running");
    assert_eq!(r.statuses(batch).await, vec!["recognizing"]);
    assert_eq!(recognition::tick_batch(&core, batch).await.unwrap(), "consumed");
    assert_eq!(r.statuses(batch).await, vec!["extracted"]);
    let ev = r.batch_audits(batch).await;
    assert_eq!(ev.iter().find(|a| a.0 == "ingest.recognition_completed").unwrap().1["jobId"], job);
    assert_eq!(recognition::tick_batch(&core, batch).await.unwrap(), "not_recognizing", "寫回後不再處理");
}

/// 來源：recognitionFlow.test.ts「5 份、一批 2 個 → 送 3 次，而且**送完一批才送下一批**」
/// ＋ recognitionPlatform.test.ts「超過一批的量：第一批寫回後接著送下一批」
#[actix_web::test]
async fn chunks_are_sent_one_after_another() {
    let r = rx(0, |c| c.tpr_batch_size = 2).await;
    let core = r.fx.app.core.clone();
    let batch = r.new_batch(5).await;
    for n in ["a-renewable-surplus.pdf", "b-renewable-surplus.pdf", "c-high-voltage-device.pdf", "d-high-voltage-rental.pdf", "e-renewable-surplus.pdf"] {
        r.upload(batch, n, "application/pdf").await;
    }
    let out = r.recognize(batch, json!({})).await;
    assert_eq!((out["chunks"].clone(), out["itemCount"].clone()), (json!(3), json!(5)));
    assert_eq!(r.fake.created.load(Ordering::SeqCst), 1, "第一批送出、其餘排隊");
    assert_eq!(recognition::tick_batch(&core, batch).await.unwrap(), "consumed_next_chunk");
    assert_eq!(r.fake.created.load(Ordering::SeqCst), 2);
    let s = r.statuses(batch).await;
    assert_eq!(&s[..2], ["extracted", "extracted"]);
    assert_eq!(&s[2..], ["recognizing", "recognizing", "recognizing"]);
    assert_eq!(recognition::tick_batch(&core, batch).await.unwrap(), "consumed_next_chunk");
    assert_eq!(recognition::tick_batch(&core, batch).await.unwrap(), "consumed");
    assert_eq!(r.fake.created.load(Ordering::SeqCst), 3);
    assert!(r.statuses(batch).await.iter().all(|s| s == "extracted"));
    let data: Value = sqlx::query_scalar("SELECT data FROM tbl_energy_ingest_batch WHERE id = $1").bind(batch).fetch_one(&core.pool).await.unwrap();
    assert_eq!(data["recognitionJobIds"].as_array().unwrap().len(), 3, "查得到分成幾次送");
    assert!(data.get("driverLeaseUntil").is_none(), "租約有釋放");
}

/// 時間預算用完：退回 `uploaded` 並留下失敗原因（不會永遠停在辨識中）。
/// 來源：RecognitionService.ts pollJob／recognitionDriver.ts tickBatch 的逾時分支（無對應 TS 測試）。
#[actix_web::test]
async fn timeout_fails_back_to_uploaded() {
    let r = rx(1000, |_| {}).await;
    let core = r.fx.app.core.clone();
    let batch = r.new_batch(1).await;
    r.upload(batch, SURPLUS, "application/pdf").await;
    r.recognize(batch, json!({})).await;
    assert_eq!(recognition::tick_batch(&core, batch).await.unwrap(), "running");
    r.fx.app.clock.advance(chrono::Duration::minutes(3));
    assert_eq!(recognition::tick_batch(&core, batch).await.unwrap(), "failed_timeout");
    assert_eq!(r.statuses(batch).await, vec!["uploaded"]);
    let ev = r.batch_audits(batch).await;
    assert!(ev.iter().any(|a| a.0 == "ingest.recognition_failed" && a.1["code"] == "tpr_timeout"));
}

/// 來源：recognitionFlow.test.ts「對帳撿得回停在 `uploading` 的批次」
#[actix_web::test]
async fn reconcile_resends_stuck_uploading_batches() {
    let r = rx(0, |_| {}).await;
    let core = r.fx.app.core.clone();
    let batch = r.new_batch(1).await;
    r.upload(batch, SURPLUS, "application/pdf").await;
    // 剛上傳的不重送（寬限期內）。
    let out = recognition::tick_all(&core).await.unwrap();
    assert!(!out.contains_key("resent"), "{out:?}");
    r.fx.app.clock.advance(chrono::Duration::minutes(11));
    let out = recognition::tick_all(&core).await.unwrap();
    assert_eq!(out.get("resent"), Some(&1), "{out:?}");
    let out = recognition::tick_all(&core).await.unwrap();
    assert_eq!(out.get("consumed"), Some(&1), "{out:?}");
    assert_eq!(r.statuses(batch).await, vec!["extracted"]);
}

/// 背景任務：開機時啟動，自己把辨識中的批次寫回（不必有人去讀批次詳情）。
#[actix_web::test]
async fn background_task_reconciles() {
    let r = rx(0, |c| c.recognition_tick_secs = 1).await;
    let batch = r.new_batch(1).await;
    r.upload(batch, DEVICE, "application/pdf").await;
    r.recognize(batch, json!({})).await;
    let mut st = String::new();
    for _ in 0..50 {
        st = r.statuses(batch).await[0].clone();
        if st != "recognizing" {
            break;
        }
        actix_web::rt::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(st, "extracted");
}

/// 來源：recognitionFlow.test.ts「HEIC 在**送出去之前**就被擋下來，訊息說得出下一步」
/// ＋「失敗的那一份仍然有**出口**：指定單別 → 建一張空白單自己填」
#[actix_web::test]
async fn heic_is_blocked_before_sending() {
    let r = rx(0, |_| {}).await;
    let fx = &r.fx;
    let batch = r.new_batch(1).await;
    let (s, _) = r.upload(batch, "iphone.heic", "image/heic").await;
    assert_eq!(s, 200, "HEIC 是附件白名單裡的");
    let out = r.recognize(batch, json!({})).await;
    assert_eq!(out["skipped"], "no_readable_files");
    assert_eq!(r.fake.created.load(Ordering::SeqCst), 0, "不送出去");
    let it = out["batch"]["items"][0].clone();
    assert_eq!((it["status"].clone(), it["recognitionErrorCode"].clone()), (json!("failed"), json!("unsupported_for_recognition")));
    assert!(it["errorMessage"].as_str().unwrap().contains("PDF"));
    assert_ne!(out["batch"]["status"], "recognizing");
    let p = expect_status!(fx.app.patch(&fx.clerk, &r.u(&format!("/ingest-items/{}", it["id"])), json!({ "docType": "renewableSurplus" })).await, 200);
    let blank = fx.bill_row(p["billId"].as_i64().unwrap()).await;
    assert_eq!(blank["ocrExtracted"], json!({}), "辨識值要誠實地空著");
    assert_eq!(blank["sourceAttachmentId"], it["attachmentId"]);
}

/// 外部管線把辨識結果灌回已上傳的那一份（信任來源不同，另記稽核）。
/// 來源：AcEnergyOpsController.ts ingestItemRecognition（無對應 TS 測試）。
#[actix_web::test]
async fn external_pipeline_supplies_recognition() {
    let r = rx(0, |_| {}).await;
    let fx = &r.fx;
    let batch = r.new_batch(1).await;
    let (_, up) = r.upload(batch, "scan.pdf", "application/pdf").await;
    let item = up["itemId"].as_i64().unwrap();
    let samples: Value = serde_json::from_str(include_str!("fixtures/canonical-samples.json")).unwrap();
    let doc = samples["documents"][DEVICE].clone();
    let (s, v) = fx.app.post(&fx.clerk, &r.u(&format!("/ingest-items/{item}/recognition")), json!({})).await;
    assert_eq!((s.as_u16(), v["code"].clone()), (400, json!("ocr_raw_required")));
    let v = expect_status!(fx.app.post(&fx.clerk, &r.u(&format!("/ingest-items/{item}/recognition")), json!({ "ocrRaw": doc })).await, 200);
    assert_eq!((v["items"][0]["status"].clone(), v["items"][0]["docType"].clone()), (json!("extracted"), json!("consumptionDevice")));
    let ev = fx.audits("energyIngestItem", item).await;
    assert!(ev.iter().any(|a| a.0 == "ingest.recognition_supplied" && a.1["status"] == "extracted"));
}

/// 直傳：簽名 → 瀏覽器 PUT 到預簽網址 → 對原上傳端點送 JSON `{stagingKey,…}` → item 建起來、暫存物件刪掉。
/// 來源：platform/stagedUpload.ts、AcStagedUploadController.ts（無對應 TS 測試）。
#[actix_web::test]
async fn staged_upload_round_trip() {
    let r = rx(0, |_| {}).await;
    let fx = &r.fx;
    let batch = r.new_batch(1).await;
    let (s, v) = fx.app.post(&fx.clerk, &r.u("/uploads/sign"), json!({ "target": "batch", "batchId": batch, "mimeType": "video/mp4", "size": 3 })).await;
    assert_eq!((s.as_u16(), v["code"].clone()), (415, json!("unsupported_mime_type")));
    let (s, _) = fx.app.post(&fx.clerk, &r.u("/uploads/sign"), json!({ "target": "batch", "batchId": batch, "mimeType": "application/pdf", "size": 0 })).await;
    assert_eq!(s.as_u16(), 400);
    let (s, _) = fx.app.post(&fx.clerk, &r.u("/uploads/sign"), json!({ "target": "batch", "batchId": 999999, "mimeType": "application/pdf", "size": 3 })).await;
    assert_eq!(s.as_u16(), 404);
    let (s, _) = fx.app.post(&fx.nobody, &r.u("/uploads/sign"), json!({ "target": "batch", "batchId": batch, "mimeType": "application/pdf", "size": 3 })).await;
    assert_eq!(s.as_u16(), 403);
    let sign = expect_status!(
        fx.app.post(&fx.clerk, &r.u("/uploads/sign"), json!({ "target": "batch", "batchId": batch, "mimeType": "application/pdf", "size": 13, "originalFilename": SURPLUS })).await,
        200
    );
    let key = sign["stagingKey"].as_str().unwrap().to_string();
    assert!(key.starts_with(&format!("energy/staging/u{}/batch-{batch}/", fx.clerk.id)));
    assert_eq!((sign["method"].clone(), sign["headers"]["content-type"].clone()), (json!("PUT"), json!("application/pdf")));
    let put = fx.app.http.put(sign["signedUrl"].as_str().unwrap()).header("content-type", "application/pdf").body(b"%PDF-1.4 fake".to_vec()).send().await.unwrap();
    assert!(put.status().is_success(), "PUT 到預簽網址：{}", put.status());

    let url = r.u(&format!("/ingest-batches/{batch}/attachments"));
    // 別人的暫存位置／別的目標不收。
    let (s, v) = fx.app.post(&fx.clerk, &url, json!({ "stagingKey": format!("energy/staging/u{}/batch-{batch}/x", fx.manager.id), "originalFilename": "a.pdf", "mimeType": "application/pdf" })).await;
    assert_eq!((s.as_u16(), v["code"].clone()), (400, json!("staging_key_invalid")));
    let (s, v) = fx.app.post(&fx.clerk, &url, json!({ "stagingKey": format!("energy/staging/u{}/batch-{batch}/missing", fx.clerk.id), "originalFilename": "a.pdf", "mimeType": "application/pdf" })).await;
    assert_eq!((s.as_u16(), v["code"].clone()), (400, json!("staged_file_missing")));
    let v = expect_status!(fx.app.post(&fx.clerk, &url, json!({ "stagingKey": key, "originalFilename": SURPLUS, "mimeType": "application/pdf" })).await, 200);
    assert!(v["itemId"].is_i64());
    assert_eq!((v["size"].clone(), v["originalFilename"].clone()), (json!(13), json!(SURPLUS)));
    assert!(fx.app.core.storage.head(&key).await.unwrap().is_none(), "暫存物件用完就刪");
    // 走得完辨識。
    r.recognize(batch, json!({})).await;
    assert_eq!(r.batch(batch).await["items"][0]["status"], "extracted");
}
