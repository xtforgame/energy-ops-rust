//! 收件與電費單的 HTTP 端點：閘 → 解析 → 服務（業務與稽核同一個交易）→ 回寫。

use super::bills::{self, BillFilter};
use super::{recognition, service};
use crate::attachments::service::{self as attachments, AttachmentStore, Owner, UploadInput};
use crate::attachments::staged;
use crate::common::{err, err_d, parse_body, query_day, Ctx, RequestMeta, Tx};
use crate::gates;
use crate::workflow::mode_for;
use crate::workflow::model::ExecutionRow;
use actix_web::{web, HttpRequest, HttpResponse};
use eo_domain::schemas::{CorrectBillInput, IngestBillInput, MatchStationInput};
use eo_platform::auth::CurrentUser;
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use serde_json::{json, Value};

type Core = web::Data<CoreRef>;

async fn gate(core: &Core, user: &CurrentUser, org: i64, perms: &[&str], meta: &RequestMeta) -> ApiResult<Ctx> {
    let access = core.rbac.authorize_org(user, org, perms).await?;
    Ok(Ctx::new(core, access, meta))
}

fn body_or_empty(body: Option<web::Json<Value>>) -> Value {
    body.map(|b| b.into_inner()).unwrap_or_else(|| json!({}))
}

fn q_str(q: &Value, k: &str) -> Option<String> {
    q.get(k).and_then(Value::as_str).map(String::from).filter(|s| !s.is_empty())
}

fn q_num(q: &Value, k: &str) -> ApiResult<Option<i64>> {
    match q_str(q, k) {
        None => Ok(None),
        Some(s) => s.trim().parse::<i64>().map(Some).map_err(|_| err_d(400, "invalid_number", json!({ "field": k }))),
    }
}

fn body_id(b: &Value, k: &str) -> ApiResult<Option<i64>> {
    match b.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_i64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            .filter(|n| *n > 0)
            .map(Some)
            .ok_or_else(|| err_d(400, "invalid_body", json!({ "field": k }))),
    }
}

fn body_day(b: &Value, k: &str) -> ApiResult<Option<chrono::NaiveDate>> {
    query_day(k, b.get(k).and_then(Value::as_str))
}

fn executions_out(execs: &[ExecutionRow]) -> Vec<i64> {
    execs.iter().map(|e| e.id).collect()
}

// ── 電費單 ──

pub async fn ingest_bill(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let ctx = gate(&core, &user, org.into_inner(), gates::FLOW_ACT, &meta).await?;
    let input: IngestBillInput = parse_body(&body)?;
    let mut tx = core.pool.begin().await?;
    let source = input.source_attachment_id.flatten();
    if let Some(a) = source {
        attachments::attachment_in_org(&mut tx, ctx.org_id(), a).await?;
    }
    let r = bills::ingest(&mut tx, &ctx, &input.ocr_json, source, None, None).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(r))
}

pub async fn list_bills(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, q: web::Query<Value>) -> ApiResult<HttpResponse> {
    let ctx = gate(&core, &user, org.into_inner(), gates::FLOW_VIEW, &meta).await?;
    let q = q.into_inner();
    // 壞掉的日期一律 400：前端 month 欄位編輯到一半會送出半截字串。
    let filter = BillFilter {
        review_status: q_str(&q, "reviewStatus"),
        station_id: q_num(&q, "stationId")?,
        limit: q_num(&q, "limit")?,
        from: query_day("from", q.get("from").and_then(Value::as_str))?,
        to: query_day("to", q.get("to").and_then(Value::as_str))?,
    };
    let mut tx = core.pool.begin().await?;
    let items = bills::list_bills(&mut tx, &ctx.access, &filter).await?;
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

pub async fn list_unmatched(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>) -> ApiResult<HttpResponse> {
    let ctx = gate(&core, &user, org.into_inner(), gates::FLOW_VIEW, &meta).await?;
    let mut tx = core.pool.begin().await?;
    let items = bills::list_unmatched(&mut tx, &ctx.access).await?;
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

/// 校對台要的整包：三層值、案場、流程、來源送件、綁定狀態、單別能不能改、模式判定。
async fn bill_detail(tx: &mut Tx<'_>, ctx: &Ctx, bill_id: i64) -> ApiResult<Value> {
    let bill = bills::bill_in_org(tx, ctx.org_id(), bill_id, false).await?;
    let mut v = bills::to_view(&bill);
    v["ocrRaw"] = bill.ocr_raw.clone();
    let station = match bill.station_id {
        Some(s) => bills::station_by_id(tx, ctx.org_id(), s).await?,
        None => None,
    };
    v["station"] = station.map(|s| json!({ "id": s.id, "name": s.name, "customerNumber": s.customer_number })).unwrap_or(Value::Null);
    v["executions"] = json!(bills::executions_of_bill(tx, ctx.org_id(), bill_id).await?);
    v["ingest"] = bills::ingest_of_bill(tx, ctx.org_id(), bill_id).await?;
    v["binding"] = bills::binding_status(tx, &ctx.access, &bill, ctx.today()).await?;
    v["docTypeEditable"] = json!(bills::doc_type_editable(tx, ctx.org_id(), bill_id).await?);
    // 送審前的單也要回 mode：「我能不能編輯這張單」一樣由後端說了算。
    let mode = serde_json::to_value(mode_for(tx, ctx, "energyBill", bill_id).await?).unwrap_or_else(|_| json!({}));
    if let (Some(m), Some(out)) = (mode.as_object(), v.as_object_mut()) {
        for (k, x) in m {
            out.insert(k.clone(), x.clone());
        }
    }
    Ok(v)
}

pub async fn get_bill(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, bill_id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_VIEW, &meta).await?;
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(bill_detail(&mut tx, &ctx, bill_id).await?))
}

pub async fn patch_bill(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let (org, bill_id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_ACT, &meta).await?;
    let raw = body.into_inner();
    if let Some(dt) = raw.get("docType") {
        bills::check_doc_type(dt)?;
    }
    let input: CorrectBillInput = parse_body(&raw)?;
    let mut tx = core.pool.begin().await?;
    // 重新判定模式，只有 intake／revise 放行：前端的唯讀只是體驗，不是防線。
    let verdict = mode_for(&mut tx, &ctx, "energyBill", bill_id).await?;
    if !verdict.verdict.can_edit_subject {
        return Err(err_d(
            403,
            "subject_not_editable",
            json!({ "mode": verdict.verdict.mode, "modeReason": verdict.verdict.mode_reason }),
        ));
    }
    bills::correct(&mut tx, &ctx, bill_id, &input).await?;
    tx.commit().await?;
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(bill_detail(&mut tx, &ctx, bill_id).await?))
}

pub async fn branch_preview(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, q: web::Query<Value>) -> ApiResult<HttpResponse> {
    let (org, bill_id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_VIEW, &meta).await?;
    let amount = q
        .get("totalAmount")
        .and_then(Value::as_str)
        .map(super::route::js_number)
        .filter(|n| n.is_finite())
        .ok_or_else(|| err(400, "invalid_amount"))?;
    let mut tx = core.pool.begin().await?;
    let bill = bills::bill_in_org(&mut tx, org, bill_id, false).await?;
    Ok(HttpResponse::Ok().json(bills::branch_preview(&mut tx, &ctx.access, &bill, amount, ctx.today()).await?))
}

pub async fn bill_payouts(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, bill_id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_VIEW, &meta).await?;
    let mut tx = core.pool.begin().await?;
    bills::bill_in_org(&mut tx, org, bill_id, false).await?;
    let items = bills::payouts(&mut tx, &ctx.access, bill_id).await?;
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

pub async fn submit_bill(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: Option<web::Json<Value>>) -> ApiResult<HttpResponse> {
    let (org, bill_id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_ACT, &meta).await?;
    let b = body_or_empty(body);
    let business_date = body_day(&b, "businessDate")?;
    let mut tx = core.pool.begin().await?;
    let execs = service::submit_bill(&mut tx, &ctx, bill_id, business_date).await?;
    tx.commit().await?;
    let first = &execs[0];
    Ok(HttpResponse::Ok().json(json!({
        "id": first.id,
        "status": first.status,
        "currentNodeKey": first.current_node_key,
        "executionIds": executions_out(&execs),
    })))
}

pub async fn match_bill(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let (org, bill_id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_ACT, &meta).await?;
    let input: MatchStationInput = parse_body(&body)?;
    let mut tx = core.pool.begin().await?;
    let row = bills::match_station(&mut tx, &ctx, bill_id, input.station_id).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "id": row.id, "reviewStatus": row.review_status, "stationId": row.station_id })))
}

// ── 收件批次 ──

pub async fn create_batch(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, body: Option<web::Json<Value>>) -> ApiResult<HttpResponse> {
    let ctx = gate(&core, &user, org.into_inner(), gates::FLOW_ACT, &meta).await?;
    let b = body_or_empty(body);
    let files = b.get("files").and_then(Value::as_array).cloned();
    let file_count = b
        .get("fileCount")
        .and_then(Value::as_i64)
        .or_else(|| files.as_ref().map(|f| f.len() as i64))
        .unwrap_or(0);
    if file_count < 1 {
        return Err(err(400, "no_files"));
    }
    if file_count > 50 {
        return Err(err_d(400, "too_many_files", json!({ "max": 50 })));
    }
    let note = b.get("note").and_then(Value::as_str);
    let mut tx = core.pool.begin().await?;
    let batch = service::create_batch(&mut tx, &ctx, file_count as i32, note).await?;
    tx.commit().await?;
    // 外部管線可以直接帶 `files[].ocrRaw`；沒有 ocrRaw 時不在這裡發明一份（那是用檔名猜辨識結果的老路）。
    let with_ocr: Vec<service::FileWithOcr> = files
        .unwrap_or_default()
        .into_iter()
        .filter(|f| f.get("ocrRaw").is_some())
        .map(|f| service::FileWithOcr {
            original_filename: f
                .get("originalFilename")
                .or_else(|| f.get("filename"))
                .and_then(Value::as_str)
                .unwrap_or("(未命名)")
                .to_string(),
            attachment_id: f.get("attachmentId").and_then(Value::as_i64),
            ocr_raw: f.get("ocrRaw").cloned().unwrap_or(Value::Null),
        })
        .collect();
    if !with_ocr.is_empty() {
        service::classify_batch(&ctx, batch.id, with_ocr).await?;
    }
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(service::get_batch(&mut tx, &ctx.access, batch.id).await?))
}

pub async fn list_batches(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>, q: web::Query<Value>) -> ApiResult<HttpResponse> {
    let ctx = gate(&core, &user, org.into_inner(), gates::FLOW_VIEW, &meta).await?;
    let limit = q_num(&q, "limit")?;
    let mut tx = core.pool.begin().await?;
    let items = service::list_batches(&mut tx, &ctx.access, limit).await?;
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

pub async fn get_batch(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, batch_id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_VIEW, &meta).await?;
    {
        let mut tx = core.pool.begin().await?;
        service::batch_in_org(&mut tx, org, batch_id).await?;
    }
    // 正在辨識的話先問服務一次：結果到了就寫回，畫面不用等背景任務。
    if let Err(e) = recognition::tick_batch(&core, batch_id).await {
        log::error!("[energy][recognition] tick on read batch={batch_id}: {e}");
    }
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(service::get_batch(&mut tx, &ctx.access, batch_id).await?))
}

/// 收件時的原始掃描檔（multipart 或直傳）。授權只能走組織層：execution 還不存在。檔案落地才建 item。
pub async fn upload_batch_attachment(
    core: Core,
    user: CurrentUser,
    meta: RequestMeta,
    path: web::Path<(i64, i64)>,
    req: HttpRequest,
    payload: web::Payload,
) -> ApiResult<HttpResponse> {
    let (org, batch_id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::ATTACHMENT_UPLOAD, &meta).await?;
    {
        let mut tx = core.pool.begin().await?;
        service::batch_in_org(&mut tx, org, batch_id).await?;
    }
    let incoming = staged::read_incoming(&core, &req, payload, user.id, &staged::batch_target(batch_id)).await?;
    let result = async {
        let store: &dyn AttachmentStore = &core.storage;
        let row = attachments::upload(
            &ctx,
            store,
            UploadInput {
                owner: Owner::Batch(batch_id),
                bytes: incoming.bytes.clone(),
                original_filename: incoming.file_name.clone(),
                mime_type: incoming.content_type.clone(),
                task_id: None,
                role: Some("source".into()),
                note: None,
                supersedes_attachment_id: None,
            },
        )
        .await?;
        let mut tx = core.pool.begin().await?;
        let item = service::register_uploaded_file(&mut tx, &ctx, batch_id, row.id, &row.original_filename).await?;
        tx.commit().await?;
        Ok::<_, eo_platform::error::ApiError>((row, item))
    }
    .await;
    staged::finish(&core, &incoming).await;
    let (row, item) = result?;
    Ok(HttpResponse::Ok().json(json!({
        "id": row.id,
        "originalFilename": row.original_filename,
        "size": row.size,
        "mimeType": row.mime_type,
        "itemId": item.id,
    })))
}

pub async fn recognize(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: Option<web::Json<Value>>) -> ApiResult<HttpResponse> {
    let (org, batch_id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_ACT, &meta).await?;
    let passwords = recognition::parse_passwords(&body_or_empty(body))?;
    let outcome = recognition::recognize_batch(&ctx, batch_id, &passwords).await?;
    let mut v = serde_json::to_value(&outcome).unwrap_or_else(|_| json!({}));
    // 讓畫面可以直接說「這個環境沒有接辨識服務」。
    v["configured"] = json!(recognition::client(&core).is_some());
    let mut tx = core.pool.begin().await?;
    v["batch"] = service::get_batch(&mut tx, &ctx.access, batch_id).await?;
    Ok(HttpResponse::Ok().json(v))
}

pub async fn patch_item(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let (org, item_id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_ACT, &meta).await?;
    let b = body.into_inner();
    let input = service::PatchItem {
        doc_type: b.get("docType").and_then(Value::as_str).map(String::from),
        workflow_key: b.get("workflowKey").and_then(Value::as_str).map(String::from),
        installment_id: body_id(&b, "installmentId")?,
    };
    let mut tx = core.pool.begin().await?;
    let row = service::patch_item(&mut tx, &ctx, item_id, &input).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({
        "id": row.id,
        "status": row.status,
        "docType": row.doc_type,
        "workflowKey": row.workflow_key,
        "billId": row.bill_id,
        "installmentId": row.installment_id,
    })))
}

/// 外部管線把辨識結果灌回已上傳的那一份：與服務回來時同一條路，但信任來源不同，另記稽核。
pub async fn supply_recognition(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: web::Json<Value>) -> ApiResult<HttpResponse> {
    let (org, item_id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_ACT, &meta).await?;
    let b = body.into_inner();
    let Some(ocr_raw) = b.get("ocrRaw") else { return Err(err(400, "ocr_raw_required")) };
    let mut tx = core.pool.begin().await?;
    let item = service::apply_recognition_to_item(&mut tx, &ctx, item_id, ocr_raw).await?;
    crate::audit::record(
        &mut tx,
        &ctx,
        crate::audit::AuditInput::new(
            "ingest.recognition_supplied",
            item.id,
            format!("外部管線提供了「{}」的辨識結果", item.original_filename.clone().unwrap_or_default()),
        )
        .label(item.original_filename.clone().unwrap_or_else(|| format!("#{}", item.id)))
        .payload(json!({
            "itemId": item.id,
            "batchId": item.batch_id,
            "docType": item.doc_type.clone().unwrap_or_else(|| "unclassified".into()),
            "billId": item.bill_id,
            "status": item.status,
        })),
    )
    .await?;
    tx.commit().await?;
    let mut tx = core.pool.begin().await?;
    Ok(HttpResponse::Ok().json(service::get_batch(&mut tx, &ctx.access, item.batch_id).await?))
}

pub async fn submit_item(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: Option<web::Json<Value>>) -> ApiResult<HttpResponse> {
    let (org, item_id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_ACT, &meta).await?;
    let b = body_or_empty(body);
    let business_date = body_day(&b, "businessDate")?;
    let mut tx = core.pool.begin().await?;
    let execs = service::submit_item(&mut tx, &ctx, item_id, business_date).await?;
    tx.commit().await?;
    let first = &execs[0];
    Ok(HttpResponse::Ok().json(json!({
        "executionId": first.id,
        "subjectType": first.subject_type,
        "subjectId": first.subject_id,
        "executionIds": executions_out(&execs),
    })))
}

pub async fn discard_item(core: Core, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>, body: Option<web::Json<Value>>) -> ApiResult<HttpResponse> {
    let (org, item_id) = path.into_inner();
    let ctx = gate(&core, &user, org, gates::FLOW_ACT, &meta).await?;
    let b = body_or_empty(body);
    let mut tx = core.pool.begin().await?;
    let row = service::discard_item(&mut tx, &ctx, item_id, b.get("reason").and_then(Value::as_str)).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "id": row.id, "status": row.status })))
}

pub async fn recognition_status(core: Core, user: CurrentUser, meta: RequestMeta, org: web::Path<i64>) -> ApiResult<HttpResponse> {
    gate(&core, &user, org.into_inner(), gates::FLOW_VIEW, &meta).await?;
    Ok(HttpResponse::Ok().json(recognition::status(&core).await))
}
