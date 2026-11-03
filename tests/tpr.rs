//! 辨識服務客戶端對假服務：建立、輪詢、取結果、錯誤分類。

use eo_energy::fake_tpr;
use eo_energy::tpr_client::{TprClient, TprUploadFile};

fn file(id: &str, name: &str, ct: &str) -> TprUploadFile {
    TprUploadFile {
        source_file_id: id.into(),
        file_name: name.into(),
        bytes: bytes::Bytes::from_static(b"%PDF-1.4 fake"),
        content_type: ct.into(),
        password: None,
    }
}

/// 來源：App 5 tprClient.test.ts「createJob → getJob → getResult」
#[actix_web::test]
async fn job_lifecycle_against_fake() {
    let (url, state, server) = fake_tpr::spawn(0, 1).unwrap();
    actix_web::rt::spawn(server);
    let c = TprClient::new(&url, None);
    assert!(c.ready().await);
    let job = c
        .create_job(vec![file("11", "電費單(有餘電需開發票).pdf", "application/pdf"), file("12", "whatever.pdf", "application/pdf")])
        .await
        .unwrap();
    assert_eq!(job.status.as_str(), "processing");
    assert_eq!(state.created.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(c.get_job(&job.job_id).await.unwrap().status.as_str(), "processing");
    let done = c.get_job(&job.job_id).await.unwrap();
    assert_eq!(done.status.as_str(), "completed_with_issues");
    let result = c.get_result(&job.job_id).await.unwrap();
    assert_eq!(result.documents.len(), 2);
    let v = serde_json::to_value(&result).unwrap();
    assert_eq!(v["documents"][0]["sourceFileId"], "11");
    assert_eq!(v["documents"][0]["status"], "succeeded");
    assert_eq!(v["documents"][1]["error"]["code"], "unsupported_template");
    assert!(c.delete_job(&job.job_id).await);
    assert_eq!(c.get_job(&job.job_id).await.unwrap_err().status, 404);
}

/// 415 是永久錯誤；連不上是暫時錯誤。
#[actix_web::test]
async fn error_classification() {
    let (url, _state, server) = fake_tpr::spawn(0, 0).unwrap();
    actix_web::rt::spawn(server);
    let c = TprClient::new(&url, None);
    let e = c.create_job(vec![file("1", "a.heic", "image/heic")]).await.unwrap_err();
    assert_eq!(e.status, 415);
    assert!(e.is_permanent());
    let dead = TprClient::new("http://127.0.0.1:9", None);
    let e = dead.create_job(vec![file("1", "a.pdf", "application/pdf")]).await.unwrap_err();
    assert_eq!(e.code, "tpr_unreachable");
    assert!(!e.is_permanent());
    assert!(!dead.ready().await);
}
