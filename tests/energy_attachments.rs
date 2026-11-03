//! 附件：真的 MinIO（不 mock）。先存物件驗過才寫 DB、可視度＝execution 可視度、只增不刪。

use crate::energy_intake::{fixture, Fx, KNOWN};
use crate::energy_recognition::upload_as;
use crate::support::TestApp;
use async_trait::async_trait;
use bytes::Bytes;
use eo_energy::attachments::service::{self as att, AttachmentStore, Owner, UploadInput};
use eo_platform::storage::StorageError;
use reqwest::Method;
use serde_json::{json, Value};

const CHINESE_NAME: &str = "台電電費單_2026年1月.pdf";
const PDF: &[u8] = b"%PDF-1.4\n% attachment spec\n";

/// 一張送審中的單（停在承辦確認），回傳 (execution, 第一關 task)。
async fn running(fx: &Fx) -> (i64, i64) {
    let b = fx.run(vec![("scan.jpg", fx.bill_ocr(KNOWN, 5000.0, None))]).await;
    let (s, v) = fx.submit_item(&b["items"][0], None).await;
    assert_eq!(s, 200, "{v}");
    let exec = v["executionId"].as_i64().unwrap();
    let task = fx.open_task(&fx.clerk, exec).await["id"].as_i64().unwrap();
    (exec, task)
}

async fn upload_exec(fx: &Fx, user: &crate::support::TestUser, exec: i64, name: &str, mime: &str, bytes: &[u8], fields: &[(&str, String)]) -> (u16, Value) {
    let mut form = reqwest::multipart::Form::new();
    for (k, v) in fields {
        form = form.text(k.to_string(), v.clone());
    }
    let part = reqwest::multipart::Part::bytes(bytes.to_vec()).file_name(name.to_string()).mime_str(mime).unwrap();
    form = form.part("file", part);
    let (s, v) = TestApp::send(fx.app.req(Some(user), Method::POST, &fx.u(&format!("/executions/{exec}/attachments"))).multipart(form)).await;
    (s.as_u16(), v)
}

async fn storage_key(fx: &Fx, id: i64) -> String {
    sqlx::query_scalar("SELECT storage_key FROM tbl_energy_attachment WHERE id = $1").bind(id).fetch_one(&fx.app.core.pool).await.unwrap()
}

/// 來源：attachments.test.ts「builds an execution-scoped key, not a flat hash pool」＋「收件的原始檔走 batch 前綴（C6）」
#[test]
fn storage_keys_are_owner_scoped() {
    let k = att::build_storage_key(Owner::Execution(42), "application/pdf");
    assert!(k.starts_with("energy/executions/42/") && k.ends_with(".pdf") && k.len() == "energy/executions/42/".len() + 36 + 4, "{k}");
    let k = att::build_storage_key(Owner::Batch(77), "image/jpeg");
    assert!(k.starts_with("energy/batches/77/") && k.ends_with(".jpg"), "{k}");
}

/// 來源：attachments.test.ts「uploads: object lands in MinIO, DB row is written, audit row follows」
/// ＋「rejects a mime type outside the whitelist (HEIC is IN the whitelist)」＋「rejects an oversized file and an empty file」
/// ＋「downloads the exact bytes back and records the (only) read event」
#[actix_web::test]
async fn upload_and_download_round_trip() {
    let fx = fixture().await;
    let (exec, task) = running(&fx).await;
    let (s, v) = upload_exec(&fx, &fx.clerk, exec, CHINESE_NAME, "application/pdf", PDF, &[("taskId", task.to_string()), ("role", "billScan".into())]).await;
    assert_eq!(s, 200, "{v}");
    let id = v["id"].as_i64().unwrap();
    assert_eq!(v["originalFilename"], CHINESE_NAME);
    assert_eq!(v["hash"], att::sha256_hex(PDF));
    assert_eq!(v["size"], PDF.len());
    assert_eq!(v["status"], "active");
    let key = storage_key(&fx, id).await;
    assert!(key.starts_with(&format!("energy/executions/{exec}/")));
    assert!(fx.app.core.storage.head(&key).await.unwrap().is_some(), "物件必須真的落地");
    let list = expect_status!(fx.app.get(&fx.clerk, &fx.u(&format!("/executions/{exec}/attachments"))).await, 200);
    let row = list["items"].as_array().unwrap().iter().find(|a| a["id"] == id).unwrap().clone();
    assert_eq!((row["links"][0]["role"].clone(), row["links"][0]["taskId"].clone()), (json!("billScan"), json!(task)));
    assert_eq!(row["uploadedByUserId"], fx.clerk.id);
    let audits = fx.audits("energyAttachment", id).await;
    let up = audits.iter().find(|a| a.0 == "attachment.uploaded").expect("沒留痕就不該上傳成功");
    assert_eq!((up.1["originalFilename"].clone(), up.1["hash"].clone()), (json!(CHINESE_NAME), v["hash"].clone()));

    let (s, e) = upload_exec(&fx, &fx.clerk, exec, "x.exe", "application/x-msdownload", PDF, &[]).await;
    assert_eq!((s, e["code"].clone()), (415, json!("unsupported_mime_type")));
    assert!(e["details"]["allowed"].as_array().unwrap().contains(&json!("image/heic")));
    let (s, e) = upload_exec(&fx, &fx.clerk, exec, "x.pdf", "application/pdf", b"", &[]).await;
    assert_eq!((s, e["code"].clone()), (400, json!("empty_file")));
    let e = att::validate_file(eo_domain::attachment_limits::MAX_FILE_BYTES + 1, "application/pdf", "big.pdf").unwrap_err();
    assert_eq!((e.status.as_u16(), e.code.as_str()), (413, "file_too_large"));
    let (s, e) = upload_exec(&fx, &fx.clerk, exec, "x.pdf", "application/pdf", PDF, &[("taskId", "999999".into())]).await;
    assert_eq!((s, e["code"].clone()), (400, json!("task_not_in_execution")));

    let dl = fx.app.req(Some(&fx.clerk), Method::GET, &fx.u(&format!("/attachments/{id}/download"))).send().await.unwrap();
    assert_eq!(dl.status().as_u16(), 200);
    assert_eq!(dl.headers()["content-type"], "application/pdf");
    let cd = dl.headers()["content-disposition"].to_str().unwrap().to_string();
    assert!(cd.contains("filename=\"") && cd.contains("filename*=UTF-8''") && !cd.contains("台電"), "{cd}");
    assert_eq!(dl.bytes().await.unwrap().as_ref(), PDF, "拿回來的位元組要一模一樣");
    let read = fx.audits("energyAttachment", id).await;
    let d = read.iter().find(|a| a.0 == "attachment.downloaded").expect("下載要記");
    assert_eq!(d.3, Some(fx.clerk.id));

    // ?redirect=1 → 302 到預簽網址，網址本身拿得到同一份檔案。
    let no_follow = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    let r = no_follow.get(format!("{}{}", fx.app.base, fx.u(&format!("/attachments/{id}/download?redirect=1")))).bearer_auth(&fx.clerk.token).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 302);
    let loc = r.headers()["location"].to_str().unwrap().to_string();
    assert!(loc.contains("X-Amz-Signature=") && loc.contains("response-content-disposition="), "{loc}");
    let signed = fx.app.http.get(&loc).send().await.unwrap();
    assert_eq!(signed.status().as_u16(), 200);
    assert_eq!(signed.bytes().await.unwrap().as_ref(), PDF);
}

/// 物件存了卻驗不到：整個上傳失敗、不留孤兒 DB 列。
struct GhostStore;

#[async_trait]
impl AttachmentStore for GhostStore {
    async fn put(&self, _key: &str, _body: Bytes, _ct: &str) -> Result<(), StorageError> {
        Ok(())
    }
    async fn exists(&self, _key: &str) -> Result<bool, StorageError> {
        Ok(false)
    }
}

/// 來源：attachments.test.ts「post-verify failure leaves NO orphan DB row」
#[actix_web::test]
async fn post_verify_failure_leaves_no_row() {
    let fx = fixture().await;
    let (exec, _) = running(&fx).await;
    let ctx = fx.app.ctx(&fx.clerk, fx.org).await;
    let r = att::upload(
        &ctx,
        &GhostStore,
        UploadInput {
            owner: Owner::Execution(exec),
            bytes: Bytes::from_static(PDF),
            original_filename: "ghost.pdf".into(),
            mime_type: "application/pdf".into(),
            task_id: None,
            role: None,
            note: None,
            supersedes_attachment_id: None,
        },
    )
    .await;
    let e = r.unwrap_err();
    assert_eq!((e.status.as_u16(), e.code.as_str()), (502, "storage_verify_failed"));
    let n = fx.app.scalar_i64("SELECT COUNT(*) FROM tbl_energy_attachment WHERE original_filename = 'ghost.pdf'").await;
    assert_eq!(n, 0, "寧可整個失敗，也不要孤兒 DB row");
}

/// 來源：attachments.test.ts describe「visibility = execution visibility」的七條（透過端點）
#[actix_web::test]
async fn visibility_is_execution_visibility() {
    let fx = fixture().await;
    let (exec, task) = running(&fx).await;
    let (_, v) = upload_exec(&fx, &fx.clerk, exec, "a.pdf", "application/pdf", PDF, &[]).await;
    let id = v["id"].as_i64().unwrap();
    let path = fx.u(&format!("/executions/{exec}/attachments"));
    let list = |u: &crate::support::TestUser| {
        let rb = fx.app.req(Some(u), Method::GET, &path);
        TestApp::send(rb)
    };
    // 發起人與候選池裡的人（還沒動手）看得到。
    assert_eq!(list(&fx.clerk).await.0.as_u16(), 200);
    assert_eq!(list(&fx.manager).await.0.as_u16(), 200);
    // 唯讀角色靠 view-all。
    assert_eq!(list(&fx.viewer).await.0.as_u16(), 200);
    // 有流程權限但不參與的人：清單、下載、上傳都不行。
    let stranger = fx.app.member(fx.org, "stranger", Some("energy:operator")).await;
    let (s, v) = list(&stranger).await;
    assert_eq!((s.as_u16(), v["code"].clone()), (403, json!("execution_not_visible")));
    assert_eq!(fx.app.get(&stranger, &fx.u(&format!("/attachments/{id}/download"))).await.0.as_u16(), 403);
    assert_eq!(upload_exec(&fx, &stranger, exec, "b.pdf", "application/pdf", PDF, &[]).await.0, 403);
    // 沒有附件權限的人連閘都過不了。
    assert_eq!(list(&fx.nobody).await.0.as_u16(), 403);
    // 參與者的代理人繼承可視度。
    let stand_in = fx.app.member(fx.org, "stand-in", Some("energy:operator")).await;
    expect_status!(
        fx.app.post(&fx.manager, &fx.u("/delegations"), json!({ "delegatorUserId": fx.manager.id, "delegateUserId": stand_in.id, "validFrom": "2000-01-01" })).await,
        200
    );
    assert_eq!(list(&stand_in).await.0.as_u16(), 200);
    // 經手過就永遠看得到。
    expect_status!(fx.app.post(&fx.clerk, &fx.u(&format!("/tasks/{task}/claim")), json!({})).await, 200);
    expect_status!(fx.app.post(&fx.clerk, &fx.u(&format!("/tasks/{task}/actions")), json!({ "actionKey": "submit" })).await, 200);
    assert_eq!(list(&fx.clerk).await.0.as_u16(), 200);
    // 別的組織：403。
    let (o2, org2) = fx.app.energy_org("att-other").await;
    assert_eq!(fx.app.get(&o2, &fx.app.e(org2, &format!("/attachments/{id}/download"))).await.0.as_u16(), 403);
}

/// 來源：attachments.test.ts describe「append-only: nothing is ever deleted」的三條
#[actix_web::test]
async fn append_only_supersede_and_unlink() {
    let fx = fixture().await;
    let (exec, task) = running(&fx).await;
    let (_, v1) = upload_exec(&fx, &fx.clerk, exec, CHINESE_NAME, "application/pdf", PDF, &[("taskId", task.to_string()), ("role", "billScan".into())]).await;
    let v1 = v1["id"].as_i64().unwrap();
    let k1 = storage_key(&fx, v1).await;
    let mut v2_bytes = PDF.to_vec();
    v2_bytes.extend_from_slice(b"v2");
    let (s, v2) = upload_exec(
        &fx,
        &fx.clerk,
        exec,
        "台電電費單_2026年1月_更正.pdf",
        "application/pdf",
        &v2_bytes,
        &[("taskId", task.to_string()), ("role", "billScan".into()), ("supersedesAttachmentId", v1.to_string())],
    )
    .await;
    assert_eq!(s, 200, "{v2}");
    assert_eq!(v2["supersedesAttachmentId"], v1);
    let v2 = v2["id"].as_i64().unwrap();
    let list = expect_status!(fx.app.get(&fx.clerk, &fx.u(&format!("/executions/{exec}/attachments"))).await, 200)["items"].as_array().unwrap().clone();
    let of = |id: i64| list.iter().find(|a| a["id"] == id).cloned().unwrap();
    assert_eq!(of(v1)["status"], "superseded", "舊列不可被刪除");
    assert!(fx.app.core.storage.head(&k1).await.unwrap().is_some(), "物件永不 delete");
    assert!(fx.audits("energyAttachment", v1).await.iter().any(|a| a.0 == "attachment.superseded" && a.1["replacedByAttachmentId"] == v2));

    let link = of(v2)["links"][0]["id"].as_i64().unwrap();
    let (s, e) = fx.app.post(&fx.admin, &fx.u(&format!("/attachment-links/{link}/remove")), json!({ "reason": "  " })).await;
    assert_eq!((s.as_u16(), e["code"].clone()), (400, json!("reason_required")));
    // 解除連結要 unlink 權限（承辦沒有）。
    assert_eq!(fx.app.post(&fx.clerk, &fx.u(&format!("/attachment-links/{link}/remove")), json!({ "reason": "x" })).await.0.as_u16(), 403);
    expect_status!(fx.app.post(&fx.admin, &fx.u(&format!("/attachment-links/{link}/remove")), json!({ "reason": "掛錯關卡" })).await, 200);
    let list = expect_status!(fx.app.get(&fx.clerk, &fx.u(&format!("/executions/{exec}/attachments"))).await, 200)["items"].as_array().unwrap().clone();
    let of = |id: i64| list.iter().find(|a| a["id"] == id).cloned().unwrap();
    assert!(of(v2)["links"].as_array().unwrap().is_empty(), "link 被 soft-delete");
    assert!(!of(v1)["links"].as_array().unwrap().is_empty(), "第一份的 link 沒被動過");
    assert!(fx.app.core.storage.head(&storage_key(&fx, v2).await).await.unwrap().is_some(), "檔案留著");
    let ev = fx.app.audit_rows(fx.org, "energyAttachmentLink", link).await;
    assert_eq!(ev[0].1["reason"], "掛錯關卡");

    // 補件另外記一筆。
    let (_, added) = upload_exec(&fx, &fx.clerk, exec, "補件.pdf", "application/pdf", PDF, &[("role", "added".into())]).await;
    assert!(fx.audits("energyAttachment", added["id"].as_i64().unwrap()).await.iter().any(|a| a.0 == "flow.attachment_added"));
    // 被取代的那一份必須在同一個 execution 底下。
    let (s, e) = upload_exec(&fx, &fx.clerk, exec, "x.pdf", "application/pdf", PDF, &[("supersedesAttachmentId", "999999".into())]).await;
    assert_eq!((s, e["code"].clone()), (400, json!("supersedes_not_in_execution")));
}

/// 收件批次的原檔：沒有 execution，下載走組織層的下載權。
#[actix_web::test]
async fn batch_attachment_download_is_org_level() {
    let fx = fixture().await;
    let b = expect_status!(fx.app.post(&fx.clerk, &fx.u("/ingest-batches"), json!({ "fileCount": 1 })).await, 200);
    let (s, v) = upload_as(&fx.app, &fx.clerk, &fx.u(&format!("/ingest-batches/{}/attachments", b["id"])), "raw.pdf", "application/pdf", PDF.to_vec()).await;
    assert_eq!(s, 200, "{v}");
    let id = v["id"].as_i64().unwrap();
    assert!(storage_key(&fx, id).await.starts_with(&format!("energy/batches/{}/", b["id"])));
    assert_eq!(fx.app.get(&fx.manager, &fx.u(&format!("/attachments/{id}/download"))).await.0.as_u16(), 200);
    assert_eq!(fx.app.get(&fx.nobody, &fx.u(&format!("/attachments/{id}/download"))).await.0.as_u16(), 403);
    assert_eq!(fx.app.get(&fx.clerk, &fx.u("/attachments/999999/download")).await.0.as_u16(), 404);
    // 沒有上傳權的人不能往批次丟檔。
    let (s, _) = upload_as(&fx.app, &fx.nobody, &fx.u(&format!("/ingest-batches/{}/attachments", b["id"])), "raw.pdf", "application/pdf", PDF.to_vec()).await;
    assert_eq!(s, 403);
}
