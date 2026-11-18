//! 附件的 HTTP 端點。

use super::service::{self, AttachmentStore, Owner, UploadInput};
use super::staged;
use crate::common::{err, err_d, Ctx, RequestMeta};
use crate::gates;
use crate::tpr_client::{TprClient, TprUploadFile};
use crate::workflow::authorize_execution;
use actix_web::{web, HttpRequest, HttpResponse};
use eo_domain::attachment_limits::{is_allowed_mime, AttachmentCategory, MAX_FILE_BYTES};
use eo_domain::content_disposition::content_disposition;
use eo_domain::document_ingest::EnergyDocType;
use eo_domain::taipower_canonical::is_tpr_terminal;
use eo_domain::taipower_extractor::energy_document_classifier;
use eo_platform::auth::CurrentUser;
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use serde_json::{json, Value};
use std::time::Duration;

type Core = web::Data<CoreRef>;

pub(crate) async fn gate(core: &Core, user: &CurrentUser, org: i64, perms: &[&str], meta: &RequestMeta) -> ApiResult<Ctx> {
    let access = core.rbac.authorize_org(user, org, perms).await?;
    Ok(Ctx::new(core, access, meta))
}

/// 預簽網址的效期（上傳與下載轉址共用）。
const PRESIGN_TTL: Duration = Duration::from_secs(15 * 60);

pub async fn list_for_execution(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, execution_id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::ATTACHMENT_DOWNLOAD, &meta).await?;
    let mut tx = core.pool.begin().await?;
    authorize_execution(&core, &mut tx, &ctx.access, execution_id).await?;
    let items = service::list_for_execution(&mut tx, execution_id).await?;
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

pub async fn upload_to_execution(
    core: Core,
    user: CurrentUser,
    meta: RequestMeta,
    path: web::Path<(i64, i64)>,
    req: HttpRequest,
    payload: web::Payload,
) -> ApiResult<HttpResponse> {
    let (org, execution_id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::ATTACHMENT_UPLOAD, &meta).await?;
    {
        let mut tx = core.pool.begin().await?;
        authorize_execution(&core, &mut tx, &ctx.access, execution_id).await?;
    }
    let incoming = staged::read_incoming(&core, &req, payload, user.id, &staged::execution_target(execution_id)).await?;
    let result = upload_execution_file(&ctx, &incoming, execution_id).await;
    staged::finish(&core, &incoming).await;
    let row = result?;
    // 補件（role=added）才會判定；收件原檔走 service::upload 一律標 taipowerNotice，不重複送辨識——
    // 這是唯一接自動判定的地方（DESIGN.md「三件一定要守」第二條）。
    if service::classify_state(&row.data, chrono::Utc::now()).as_deref() == Some("recognizing") {
        spawn_classify(core.get_ref().clone(), ctx.org_id(), row.id);
    }
    Ok(HttpResponse::Ok().json(service::summary(&row)))
}

/// 補件的背景分類：建 job → 每 2 秒輪詢到終態（180 秒逾時）→ 取結果 → 過判讀器。
/// 任何一步失敗、逾時、沒設定服務都落 `needsCategory`（fail-closed：寧可請人選，不可亂猜）。
fn spawn_classify(core: CoreRef, org_id: i64, attachment_id: i64) {
    actix_web::rt::spawn(async move {
        if let Err(e) = classify_added_attachment(&core, org_id, attachment_id).await {
            log::warn!("[energy][attachment] 補件辨識 attachment={attachment_id} 失敗：{e}");
            let ctx = Ctx::system(&core, org_id);
            let _ = service::apply_classify_result(&ctx, attachment_id, None, "needsCategory").await;
        }
    });
}

async fn classify_added_attachment(core: &CoreRef, org_id: i64, attachment_id: i64) -> ApiResult<()> {
    let ctx = Ctx::system(core, org_id);
    let Some(tpr) = TprClient::from_config(&core.config) else {
        return service::apply_classify_result(&ctx, attachment_id, None, "needsCategory").await;
    };
    let (row, bytes) = service::read_bytes(&ctx, attachment_id).await?;
    let file = TprUploadFile {
        source_file_id: attachment_id.to_string(),
        file_name: row.original_filename.clone(),
        bytes,
        content_type: row.mime_type.clone(),
        password: None,
    };
    let job = match tpr.create_job(vec![file]).await {
        Ok(j) => j,
        Err(_) => return service::apply_classify_result(&ctx, attachment_id, None, "needsCategory").await,
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(180);
    loop {
        if std::time::Instant::now() >= deadline {
            let _ = tpr.delete_job(&job.job_id).await;
            return service::apply_classify_result(&ctx, attachment_id, None, "needsCategory").await;
        }
        match tpr.get_job(&job.job_id).await {
            Ok(s) if is_tpr_terminal(s.status.as_str()) => break,
            Ok(_) => {}
            Err(_) => {
                let _ = tpr.delete_job(&job.job_id).await;
                return service::apply_classify_result(&ctx, attachment_id, None, "needsCategory").await;
            }
        }
        actix_web::rt::time::sleep(Duration::from_secs(2)).await;
    }
    let result = tpr.get_result_raw(&job.job_id).await;
    let _ = tpr.delete_job(&job.job_id).await;
    let Ok(result) = result else {
        return service::apply_classify_result(&ctx, attachment_id, None, "needsCategory").await;
    };
    let doc = result.get("documents").and_then(Value::as_array).and_then(|a| a.first()).cloned().unwrap_or(json!({}));
    let classify = energy_document_classifier(&doc);
    if classify.doc_type == EnergyDocType::Unclassified {
        service::apply_classify_result(&ctx, attachment_id, None, "needsCategory").await
    } else {
        service::apply_classify_result(&ctx, attachment_id, Some(AttachmentCategory::TaipowerNotice.as_str()), "auto").await
    }
}

async fn upload_execution_file(ctx: &Ctx, incoming: &staged::Incoming, execution_id: i64) -> ApiResult<service::AttachmentRow> {
    let task_id = incoming.field_id("taskId")?;
    let supersedes = incoming.field_id("supersedesAttachmentId")?;
    // 掛的關卡與被取代的那份都必須在同一個 execution 底下 —— 否則一個參與者能把檔掛到別人的流程上。
    if let Some(t) = task_id {
        let ok: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM tbl_energy_workflow_task WHERE id = $1 AND execution_id = $2 AND deleted_at IS NULL)",
        )
        .bind(t)
        .bind(execution_id)
        .fetch_one(&ctx.core.pool)
        .await?;
        if !ok {
            return Err(err_d(400, "task_not_in_execution", json!({ "taskId": t })));
        }
    }
    if let Some(s) = supersedes {
        let ok: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM tbl_energy_attachment WHERE id = $1 AND execution_id = $2 AND deleted_at IS NULL)",
        )
        .bind(s)
        .bind(execution_id)
        .fetch_one(&ctx.core.pool)
        .await?;
        if !ok {
            return Err(err_d(400, "supersedes_not_in_execution", json!({ "supersedesAttachmentId": s })));
        }
    }
    let store: &dyn AttachmentStore = &ctx.core.storage;
    service::upload(
        ctx,
        store,
        UploadInput {
            owner: Owner::Execution(execution_id),
            bytes: incoming.bytes.clone(),
            original_filename: incoming.file_name.clone(),
            mime_type: incoming.content_type.clone(),
            task_id,
            role: incoming.field("role").map(String::from),
            note: incoming.field("note").map(String::from),
            supersedes_attachment_id: supersedes,
        },
    )
    .await
}

#[derive(serde::Deserialize)]
pub struct DownloadQuery {
    redirect: Option<String>,
}

pub async fn download(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, q: web::Query<DownloadQuery>) -> ApiResult<HttpResponse> {
    let (org, attachment_id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::ATTACHMENT_DOWNLOAD, &meta).await?;
    let row = {
        let mut tx = core.pool.begin().await?;
        let row = service::attachment_in_org(&mut tx, org, attachment_id).await?;
        // 兩種歸屬兩種授權來源：execution 的走可視度；收件批次的只有組織層的下載權（上面已過）。
        if let Some(e) = row.execution_id {
            authorize_execution(&core, &mut tx, &ctx.access, e).await?;
        }
        row
    };
    let disposition = content_disposition(&row.original_filename);
    if q.redirect.as_deref().is_some_and(|v| v == "1" || v == "true") {
        let url = core.storage.presign(
            "GET",
            &row.storage_key,
            PRESIGN_TTL,
            &[
                ("response-content-disposition".into(), disposition),
                ("response-content-type".into(), row.mime_type.clone()),
            ],
        );
        service::record_download(&ctx, &row).await;
        return Ok(HttpResponse::Found().insert_header(("Location", url)).finish());
    }
    let bytes = core.storage.get(&row.storage_key).await?;
    service::record_download(&ctx, &row).await;
    Ok(HttpResponse::Ok()
        .insert_header(("Content-Type", row.mime_type.clone()))
        .insert_header(("Content-Disposition", disposition))
        .body(bytes))
}

pub async fn remove_link(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: Option<web::Json<Value>>) -> ApiResult<HttpResponse> {
    let (org, link_id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::ATTACHMENT_UNLINK, &meta).await?;
    let reason = body.as_ref().and_then(|b| b.get("reason")).and_then(Value::as_str).unwrap_or("").to_string();
    let mut tx = core.pool.begin().await?;
    let (link, attachment) = service::link_in_org(&mut tx, org, link_id).await?;
    let execution_id = attachment.execution_id.ok_or_else(|| err(404, "execution_not_found"))?;
    authorize_execution(&core, &mut tx, &ctx.access, execution_id).await?;
    service::remove_link(&mut tx, &ctx, &link, &attachment, &reason).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

/// 人選類別（判不出或要改類別都走這裡）：`PATCH /organizations/:org/attachments/:id/category`。
pub async fn set_category(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let (org, attachment_id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::ATTACHMENT_UPLOAD, &meta).await?;
    let category = body.get("category").and_then(Value::as_str).unwrap_or("").to_string();
    let mut tx = core.pool.begin().await?;
    let attachment = service::attachment_in_org(&mut tx, org, attachment_id).await?;
    if let Some(e) = attachment.execution_id {
        authorize_execution(&core, &mut tx, &ctx.access, e).await?;
    }
    service::set_category_manual(&mut tx, &ctx, &attachment, &category).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true, "category": category })))
}

/// 直傳第一步：與真正的上傳端點同一道閘，回暫存位置與預簽 PUT 網址。
pub async fn sign_upload(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let org = org.into_inner();
    let b = body.into_inner();
    let mime = b.get("mimeType").and_then(Value::as_str).unwrap_or("").to_string();
    let size = b.get("size").and_then(Value::as_f64).unwrap_or(0.0);
    if !is_allowed_mime(&mime) {
        return Err(err(415, "unsupported_mime_type"));
    }
    if !size.is_finite() || size <= 0.0 {
        return Err(err(400, "empty_file"));
    }
    if size > MAX_FILE_BYTES as f64 {
        return Err(err_d(413, "file_too_large", json!({ "max": MAX_FILE_BYTES })));
    }
    let ctx = gate(&core, &user, org, gates::ATTACHMENT_UPLOAD, &meta).await?;
    let id_of = |k: &str| b.get(k).and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())));
    let target = match b.get("target").and_then(Value::as_str) {
        Some("batch") => {
            let batch_id = id_of("batchId").ok_or_else(|| err_d(400, "invalid_body", json!({ "field": "batchId" })))?;
            let mut tx = core.pool.begin().await?;
            crate::ingest::service::batch_in_org(&mut tx, org, batch_id).await?;
            staged::batch_target(batch_id)
        }
        Some("execution") => {
            let execution_id = id_of("executionId").ok_or_else(|| err_d(400, "invalid_body", json!({ "field": "executionId" })))?;
            let mut tx = core.pool.begin().await?;
            authorize_execution(&core, &mut tx, &ctx.access, execution_id).await?;
            staged::execution_target(execution_id)
        }
        _ => return Err(err(400, "target_invalid")),
    };
    let key = staged::new_staging_key(user.id, &target);
    let url = core.storage.presign("PUT", &key, PRESIGN_TTL, &[]);
    // 簽章只簽 host：瀏覽器帶 content-type 不影響簽章，物件會帶著它落地。
    Ok(HttpResponse::Ok().json(json!({
        "stagingKey": key,
        "signedUrl": url,
        "method": "PUT",
        "headers": { "content-type": mime },
        "expiresInSeconds": PRESIGN_TTL.as_secs(),
    })))
}
