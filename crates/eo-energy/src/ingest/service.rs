//! 送件編排 —— 單子進系統的唯一入口（照 App 5 `bills/IngestService.ts`）。
//!
//! ```text
//! 開批 → 上傳檔案（每份建一筆 uploaded 的 item）→ 辨識（分類 → 依單別分流 → 抽取）
//! → 校對 → 送審（依綁定開流程）→ 批次狀態重算
//! ```
//!
//! 三條不可違反：判不出來不猜（`unclassified` 是合法答案）；confidence 不當門檻；批次狀態是推導的。

use super::bills::{self, BillRow};
use crate::attachments::service as attachments;
use crate::audit::{self, AuditInput};
use crate::common::{err, err_d, user_names, Ctx, Tx};
use crate::workflow::model::{ExecutionRow, EXEC_COLS};
use crate::workflow::{self, definition::contract_attributes, StartExecution};
use chrono::{DateTime, NaiveDate, Utc};
use eo_domain::document_ingest::{
    doc_type_to_workflow, is_bill_doc_type, mock_deposit_request_extractor, ClassifyResult, EnergyDocType, ExtractedDepositRequest,
};
use eo_domain::taipower_extractor::{energy_document_classifier, recognition_summary};
use eo_platform::error::ApiResult;
use eo_platform::rbac::OrgAccess;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde_json::{json, Map, Value};
use sqlx::FromRow;
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Debug, Clone, FromRow)]
pub struct BatchRow {
    pub id: i64,
    pub submitted_at: DateTime<Utc>,
    pub file_count: i32,
    pub status: String,
    pub note: Option<String>,
    pub data: Value,
    pub organization_id: i64,
    pub submitted_by_user_id: Option<i64>,
}

pub const BATCH_COLS: &str = "id, submitted_at, file_count, status, note, data, organization_id, submitted_by_user_id";

#[derive(Debug, Clone, FromRow)]
pub struct ItemRow {
    pub id: i64,
    pub original_filename: Option<String>,
    pub doc_type: Option<String>,
    pub workflow_key: Option<String>,
    pub classify_confidence: Option<Decimal>,
    pub status: String,
    pub error_message: Option<String>,
    pub data: Value,
    pub organization_id: i64,
    pub batch_id: i64,
    pub attachment_id: Option<i64>,
    pub bill_id: Option<i64>,
    pub execution_id: Option<i64>,
    pub installment_id: Option<i64>,
}

pub const ITEM_COLS: &str = "id, original_filename, doc_type, workflow_key, classify_confidence, status, error_message, data,
    organization_id, batch_id, attachment_id, bill_id, execution_id, installment_id";

/// 已經有結論的兩個 item 狀態。「還沒送出去」用補集定義：新狀態會被算成待辦（錯得看得見）。
pub const CLOSED_ITEM_STATUSES: [&str; 2] = ["submitted", "discarded"];

fn is_closed(status: &str) -> bool {
    CLOSED_ITEM_STATUSES.contains(&status)
}

pub async fn batch_in_org(tx: &mut Tx<'_>, org_id: i64, batch_id: i64) -> ApiResult<BatchRow> {
    let row: Option<BatchRow> =
        sqlx::query_as(&format!("SELECT {BATCH_COLS} FROM tbl_energy_ingest_batch WHERE id = $1 AND deleted_at IS NULL"))
            .bind(batch_id)
            .fetch_optional(&mut **tx)
            .await?;
    let row = row.ok_or_else(|| err(404, "batch_not_found"))?;
    if row.organization_id != org_id {
        return Err(err(403, "batch_not_in_org"));
    }
    Ok(row)
}

pub async fn item_in_org(tx: &mut Tx<'_>, org_id: i64, item_id: i64) -> ApiResult<ItemRow> {
    let row: Option<ItemRow> =
        sqlx::query_as(&format!("SELECT {ITEM_COLS} FROM tbl_energy_ingest_item WHERE id = $1 AND deleted_at IS NULL FOR UPDATE"))
            .bind(item_id)
            .fetch_optional(&mut **tx)
            .await?;
    let row = row.ok_or_else(|| err(404, "item_not_found"))?;
    // 跨組織 403（不是 404）。
    if row.organization_id != org_id {
        return Err(err(403, "item_not_in_org"));
    }
    Ok(row)
}

async fn items_of_batch(tx: &mut Tx<'_>, batch_id: i64) -> ApiResult<Vec<ItemRow>> {
    Ok(sqlx::query_as(&format!("SELECT {ITEM_COLS} FROM tbl_energy_ingest_item WHERE batch_id = $1 AND deleted_at IS NULL ORDER BY id"))
        .bind(batch_id)
        .fetch_all(&mut **tx)
        .await?)
}

/// 開一批。送件者一律由已判權的身分推導，不信 client。開批時還沒有任何檔案 → `uploading`。
pub async fn create_batch(tx: &mut Tx<'_>, ctx: &Ctx, file_count: i32, note: Option<&str>) -> ApiResult<BatchRow> {
    let note: Option<String> = note.map(|n| n.chars().take(300).collect());
    Ok(sqlx::query_as(&format!(
        "INSERT INTO tbl_energy_ingest_batch (organization_id, submitted_by_user_id, submitted_at, file_count, status, note, data)
         VALUES ($1, $2, $3, $4, 'uploading', $5, '{{}}'::jsonb) RETURNING {BATCH_COLS}"
    ))
    .bind(ctx.org_id())
    .bind(ctx.user_id())
    .bind(ctx.now())
    .bind(file_count)
    .bind(note)
    .fetch_one(&mut **tx)
    .await?)
}

/// 批次狀態是推導出來的。必須在每次 item 變動的同一個交易內重算，否則會有人看到中間態。
pub async fn recompute_batch_status(tx: &mut Tx<'_>, batch_id: i64) -> ApiResult<String> {
    let statuses: Vec<String> =
        sqlx::query_scalar("SELECT status FROM tbl_energy_ingest_item WHERE batch_id = $1 AND deleted_at IS NULL ORDER BY id")
            .bind(batch_id)
            .fetch_all(&mut **tx)
            .await?;
    let n = statuses.len();
    let count = |st: &[&str]| statuses.iter().filter(|s| st.contains(&s.as_str())).count();
    let submitted = count(&["submitted"]);
    let discarded = count(&["discarded"]);
    let failed = count(&["failed"]);
    let uploaded = count(&["uploaded"]);
    let recognizing = count(&["recognizing"]);
    let status = if n == 0 {
        "uploading"
    } else if recognizing > 0 {
        // 還有一份在辨識，整批就是辨識中（前端靠它決定要不要繼續輪詢）。
        "recognizing"
    } else if submitted + discarded == n {
        "completed"
    } else if submitted > 0 {
        "partial"
    } else if failed == n {
        "failed"
    } else if uploaded == n {
        // 檔案都在但還沒辨識 —— 不可以說「可以校對了」。
        "uploading"
    } else {
        "ready"
    };
    sqlx::query("UPDATE tbl_energy_ingest_batch SET status = $2 WHERE id = $1 AND deleted_at IS NULL")
        .bind(batch_id)
        .bind(status)
        .execute(&mut **tx)
        .await?;
    Ok(status.to_string())
}

pub async fn recompute_batch_status_of_item(tx: &mut Tx<'_>, item_id: i64) -> ApiResult<()> {
    let batch: Option<i64> = sqlx::query_scalar("SELECT batch_id FROM tbl_energy_ingest_item WHERE id = $1")
        .bind(item_id)
        .fetch_optional(&mut **tx)
        .await?;
    if let Some(b) = batch {
        recompute_batch_status(tx, b).await?;
    }
    Ok(())
}

fn merge(base: &Value, extra: Value) -> Value {
    let mut out = base.as_object().cloned().unwrap_or_default();
    if let Value::Object(m) = extra {
        for (k, v) in m {
            out.insert(k, v);
        }
    }
    Value::Object(out)
}

fn confidence(cls: &ClassifyResult) -> Option<Decimal> {
    Decimal::from_str(&super::route::js_num_str(cls.confidence)).ok()
}

/// 抽取的結果：要寫進 item 的欄位。
struct ExtractPatch {
    status: &'static str,
    bill_id: Option<i64>,
    installment_id: Option<i64>,
    data: Value,
}

/// 依單別分流的抽取：電費單產生 `energyBill`；保證金不產新單、對上既有那一期。
async fn extraction_patch(
    tx: &mut Tx<'_>,
    ctx: &Ctx,
    doc_type: EnergyDocType,
    ocr_raw: &Value,
    attachment_id: Option<i64>,
    batch_id: i64,
    data: &Value,
) -> ApiResult<ExtractPatch> {
    if is_bill_doc_type(doc_type.as_str()) {
        let r = bills::ingest(tx, ctx, ocr_raw, attachment_id, Some(batch_id), Some(doc_type.as_str())).await?;
        // 一個 item 至多一張 bill；其餘照樣存在但不掛 item。
        let bill_id = r.bill_ids.first().copied();
        // 電號對不上 → item 也是 unmatched（收件台看的是 item 的狀態）。
        let unmatched = match bill_id {
            Some(id) => bills::bill_in_org(tx, ctx.org_id(), id, false).await?.station_id.is_none(),
            None => false,
        };
        return Ok(ExtractPatch {
            status: if unmatched { "unmatched" } else { "extracted" },
            bill_id,
            installment_id: None,
            data: data.clone(),
        });
    }
    if doc_type == EnergyDocType::DepositRequest {
        // 不產新單：期數、金額、到期日全部來自合約，掃描件只是觸發器。
        let ex = mock_deposit_request_extractor(ocr_raw);
        let inst = find_installment(tx, ctx.org_id(), &ex).await?;
        let amount_mismatch = match (&inst, &ex.amount) {
            // 金額差異只標示，不擋也不覆蓋。
            (Some(i), Some(a)) => {
                let a = super::route::js_number(a);
                let b = i.1.to_f64().unwrap_or(f64::NAN);
                a != b
            }
            _ => false,
        };
        let already_paid = inst.as_ref().is_some_and(|i| i.2 == "paid");
        return Ok(ExtractPatch {
            status: if inst.is_some() { "extracted" } else { "unmatched" },
            bill_id: None,
            installment_id: inst.as_ref().map(|i| i.0),
            data: merge(data, json!({ "extracted": ex, "amountMismatch": amount_mismatch, "alreadyPaid": already_paid })),
        });
    }
    Err(err_d(422, "unsupported_doc_type", json!({ "docType": doc_type.as_str() })))
}

/// 合約編號＋期別 → 唯一一筆期數；對不上回 None（不挑一個最像的）。
async fn find_installment(tx: &mut Tx<'_>, org_id: i64, ex: &ExtractedDepositRequest) -> ApiResult<Option<(i64, Decimal, String)>> {
    let Some(seq) = ex.seq else { return Ok(None) };
    if ex.contract_no_raw.is_empty() {
        return Ok(None);
    }
    let contracts: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM tbl_energy_contract WHERE organization_id = $1 AND contract_no = $2 AND deleted_at IS NULL ORDER BY id",
    )
    .bind(org_id)
    .bind(&ex.contract_no_raw)
    .fetch_all(&mut **tx)
    .await?;
    // 多筆同編號 ＝ 分不出來，不猜。
    if contracts.len() != 1 {
        return Ok(None);
    }
    let Ok(seq) = i32::try_from(seq) else { return Ok(None) };
    let rows: Vec<(i64, Decimal, String)> = sqlx::query_as(
        "SELECT id, amount, status FROM tbl_energy_installment WHERE contract_id = $1 AND seq = $2 AND deleted_at IS NULL ORDER BY id",
    )
    .bind(contracts[0])
    .bind(seq)
    .fetch_all(&mut **tx)
    .await?;
    Ok(if rows.len() == 1 { rows.into_iter().next() } else { None })
}

/// 分類 → 抽取 → 寫回某一筆 item（新建或既有）。抽取失敗時那一份標 `failed`，不讓整批倒。
async fn classify_into_item(tx: &mut Tx<'_>, ctx: &Ctx, item: &ItemRow, ocr_raw: &Value) -> ApiResult<ItemRow> {
    let cls = energy_document_classifier(ocr_raw);
    let workflow_key = cls.workflow_key.clone().or_else(|| doc_type_to_workflow(cls.doc_type).map(String::from));
    let data = merge(
        &item.data,
        json!({ "classifyReason": cls.reason, "classifySignals": cls.signals, "ocrRaw": ocr_raw }),
    );
    let (status, bill_id, installment_id, data, error) = if cls.doc_type == EnergyDocType::Unclassified {
        // 判不出來是合法答案：留著附件與 item，人可以手動指定流程。
        ("unclassified", item.bill_id, item.installment_id, data, None)
    } else {
        let mut sp = sqlx::Connection::begin(&mut **tx).await?;
        match extraction_patch(&mut sp, ctx, cls.doc_type, ocr_raw, item.attachment_id, item.batch_id, &data).await {
            Ok(p) => {
                sp.commit().await?;
                (p.status, p.bill_id.or(item.bill_id), p.installment_id.or(item.installment_id), p.data, None)
            }
            Err(e) => {
                sp.rollback().await?;
                // 給人看的一句話，不塞 stack。
                ("failed", item.bill_id, item.installment_id, data, Some(e.code))
            }
        }
    };
    let row: ItemRow = sqlx::query_as(&format!(
        "UPDATE tbl_energy_ingest_item
            SET doc_type = $2, workflow_key = $3, classify_confidence = $4, status = $5, error_message = COALESCE($6, error_message),
                data = $7, bill_id = $8, installment_id = $9
          WHERE id = $1 RETURNING {ITEM_COLS}"
    ))
    .bind(item.id)
    .bind(cls.doc_type.as_str())
    .bind(workflow_key)
    .bind(confidence(&cls))
    .bind(status)
    .bind(error)
    .bind(&data)
    .bind(bill_id)
    .bind(installment_id)
    .fetch_one(&mut **tx)
    .await?;
    recompute_batch_status(tx, row.batch_id).await?;
    Ok(row)
}

/// 外部管線開批時直接帶辨識結果（`files[].ocrRaw`）。逐檔獨立：一份壞掉不讓整批回滾。
pub struct FileWithOcr {
    pub original_filename: String,
    pub attachment_id: Option<i64>,
    pub ocr_raw: Value,
}

pub async fn classify_batch(ctx: &Ctx, batch_id: i64, files: Vec<FileWithOcr>) -> ApiResult<Vec<ItemRow>> {
    {
        let mut tx = ctx.core.pool.begin().await?;
        batch_in_org(&mut tx, ctx.org_id(), batch_id).await?;
    }
    let mut created = Vec::new();
    for f in &files {
        let mut tx = ctx.core.pool.begin().await?;
        let attachment_id = match f.attachment_id {
            // 只接受本組織的附件 —— 不然 item 會指到別家的原檔。
            Some(a) => attachments::attachment_in_org(&mut tx, ctx.org_id(), a).await.ok().map(|r| r.id),
            None => None,
        };
        let item = insert_item(&mut tx, ctx, batch_id, attachment_id, &f.original_filename, "classified").await?;
        let row = classify_into_item(&mut tx, ctx, &item, &f.ocr_raw).await?;
        tx.commit().await?;
        created.push(row);
    }
    let mut tx = ctx.core.pool.begin().await?;
    recompute_batch_status(&mut tx, batch_id).await?;
    let mut by_status = Map::new();
    for i in &created {
        let n = by_status.get(&i.status).and_then(Value::as_u64).unwrap_or(0) + 1;
        by_status.insert(i.status.clone(), json!(n));
    }
    audit::record(
        &mut tx,
        ctx,
        AuditInput::new("ingest.batch_classified", batch_id, format!("送件 {} 份，分類完成", files.len()))
            .label(format!("批次 #{batch_id}"))
            .payload(json!({ "fileCount": files.len(), "byStatus": by_status })),
    )
    .await?;
    tx.commit().await?;
    Ok(created)
}

async fn insert_item(tx: &mut Tx<'_>, ctx: &Ctx, batch_id: i64, attachment_id: Option<i64>, name: &str, status: &str) -> ApiResult<ItemRow> {
    Ok(sqlx::query_as(&format!(
        "INSERT INTO tbl_energy_ingest_item (organization_id, batch_id, attachment_id, original_filename, doc_type, workflow_key, status, data)
         VALUES ($1, $2, $3, $4, 'unclassified', NULL, $5, '{{}}'::jsonb) RETURNING {ITEM_COLS}"
    ))
    .bind(ctx.org_id())
    .bind(batch_id)
    .bind(attachment_id)
    .bind(name.chars().take(300).collect::<String>())
    .bind(status)
    .fetch_one(&mut **tx)
    .await?)
}

/// 檔案落地才建 item（先上傳、後辨識）：附件與 item 天生一對一。
pub async fn register_uploaded_file(tx: &mut Tx<'_>, ctx: &Ctx, batch_id: i64, attachment_id: i64, original_filename: &str) -> ApiResult<ItemRow> {
    batch_in_org(tx, ctx.org_id(), batch_id).await?;
    let item = insert_item(tx, ctx, batch_id, Some(attachment_id), original_filename, "uploaded").await?;
    recompute_batch_status(tx, batch_id).await?;
    Ok(item)
}

/// 送去辨識之前翻成 `recognizing`，讓畫面知道要繼續輪詢。
pub async fn mark_items_recognizing(tx: &mut Tx<'_>, ids: &[i64]) -> ApiResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    sqlx::query("UPDATE tbl_energy_ingest_item SET status = 'recognizing', error_message = NULL WHERE id = ANY($1) AND deleted_at IS NULL")
        .bind(ids)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// 辨識結果回來了 → 分類、抽取、寫回既有的 item（與開批帶辨識結果走同一份抽取）。
pub async fn apply_recognition_to_item(tx: &mut Tx<'_>, ctx: &Ctx, item_id: i64, ocr_raw: &Value) -> ApiResult<ItemRow> {
    let item = item_in_org(tx, ctx.org_id(), item_id).await?;
    classify_into_item(tx, ctx, &item, ocr_raw).await
}

/// 辨識失敗（服務層級或單一檔案）→ 標在 item 上，訊息要是人看得懂的一句話。
pub async fn mark_item_failed(tx: &mut Tx<'_>, ctx: &Ctx, item_id: i64, message: &str, code: Option<&str>) -> ApiResult<ItemRow> {
    let item = item_in_org(tx, ctx.org_id(), item_id).await?;
    let data = merge(&item.data, json!({ "recognitionErrorCode": code }));
    let row: ItemRow = sqlx::query_as(&format!(
        "UPDATE tbl_energy_ingest_item SET status = 'failed', error_message = $2, data = $3 WHERE id = $1 RETURNING {ITEM_COLS}"
    ))
    .bind(item_id)
    .bind(message)
    .bind(&data)
    .fetch_one(&mut **tx)
    .await?;
    recompute_batch_status(tx, row.batch_id).await?;
    Ok(row)
}

#[derive(Debug, Default)]
pub struct PatchItem {
    pub doc_type: Option<String>,
    pub workflow_key: Option<String>,
    pub installment_id: Option<i64>,
}

/// 收件台改 item：手動指定單別／流程（`unclassified` 的出口）、人工指認期數。
pub async fn patch_item(tx: &mut Tx<'_>, ctx: &Ctx, item_id: i64, input: &PatchItem) -> ApiResult<ItemRow> {
    let item = item_in_org(tx, ctx.org_id(), item_id).await?;
    let mut doc_type = item.doc_type.clone();
    let mut workflow_key = item.workflow_key.clone();
    let mut status = item.status.clone();
    let mut bill_id = item.bill_id;
    let mut installment_id = item.installment_id;
    let mut data = item.data.clone();
    let mut changed = false;

    if let Some(dt) = input.doc_type.as_deref().filter(|s| !s.is_empty()) {
        let t = EnergyDocType::parse(dt).ok_or_else(|| err_d(400, "invalid_doc_type", json!({ "docType": dt })))?;
        changed = true;
        doc_type = Some(t.as_str().into());
        workflow_key = input.workflow_key.clone().or_else(|| doc_type_to_workflow(t).map(String::from));
        if item.status == "unclassified" {
            status = "classified".into();
        }
        // 手動指定單別之後要有一張單可以填：空白但真實（辨識值誠實地空著），人逐格把電號、期間、金額填進去。
        if is_bill_doc_type(t.as_str()) && item.bill_id.is_none() {
            let blank: i64 = sqlx::query_scalar(
                "INSERT INTO tbl_energy_bill (organization_id, doc_type, ocr_raw, ocr_extracted, source_attachment_id, review_status, data)
                 VALUES ($1, $2, '{}'::jsonb, '{}'::jsonb, $3, 'pendingMatch', '{\"createdBy\":\"manualDocType\"}'::jsonb) RETURNING id",
            )
            .bind(ctx.org_id())
            .bind(t.as_str())
            .bind(item.attachment_id)
            .fetch_one(&mut **tx)
            .await?;
            bill_id = Some(blank);
            // 有單但沒案場 ＝ unmatched（與辨識出來卻對不上同一個狀態）。
            status = "unmatched".into();
            audit::record(
                tx,
                ctx,
                AuditInput::new("bill.created_manually", blank, format!("辨識判不出來，改由人工建立電費單（{}）", t.as_str()))
                    .label(item.original_filename.clone().unwrap_or_else(|| format!("#{item_id}")))
                    .payload(json!({ "itemId": item_id, "docType": t.as_str() })),
            )
            .await?;
        }
    }
    if let Some(wk) = input.workflow_key.as_deref().filter(|s| !s.is_empty()) {
        changed = true;
        workflow_key = Some(wk.into());
        // 「人指定過」要留記號：這一欄收件時就有分類器填的預設值，拿「有值」當「人指定過」會讓綁定的第二條流程永遠不開。
        data = merge(&data, json!({ "workflowPinned": true }));
    }
    if let Some(inst) = input.installment_id {
        // 只能指到本組織的期數。
        let org: Option<i64> =
            sqlx::query_scalar("SELECT organization_id FROM tbl_energy_installment WHERE id = $1 AND deleted_at IS NULL")
                .bind(inst)
                .fetch_optional(&mut **tx)
                .await?;
        match org {
            None => return Err(err(404, "installment_not_found")),
            Some(o) if o != ctx.org_id() => return Err(err(403, "installment_not_in_org")),
            _ => {}
        }
        changed = true;
        installment_id = Some(inst);
        if status == "unmatched" {
            status = "extracted".into();
        }
    }
    if !changed {
        return Ok(item);
    }
    let row: ItemRow = sqlx::query_as(&format!(
        "UPDATE tbl_energy_ingest_item SET doc_type = $2, workflow_key = $3, status = $4, bill_id = $5, installment_id = $6, data = $7
          WHERE id = $1 RETURNING {ITEM_COLS}"
    ))
    .bind(item_id)
    .bind(doc_type)
    .bind(workflow_key)
    .bind(status)
    .bind(bill_id)
    .bind(installment_id)
    .bind(&data)
    .fetch_one(&mut **tx)
    .await?;
    recompute_batch_status(tx, row.batch_id).await?;
    Ok(row)
}

/// 捨棄（掃錯、重複）。不刪除 —— 稽核要看得到「這個檔進來過但被丟掉」。
pub async fn discard_item(tx: &mut Tx<'_>, ctx: &Ctx, item_id: i64, reason: Option<&str>) -> ApiResult<ItemRow> {
    let item = item_in_org(tx, ctx.org_id(), item_id).await?;
    if item.status == "submitted" {
        return Err(err(409, "item_already_submitted"));
    }
    let row: ItemRow = sqlx::query_as(&format!(
        "UPDATE tbl_energy_ingest_item SET status = 'discarded', error_message = $2 WHERE id = $1 RETURNING {ITEM_COLS}"
    ))
    .bind(item_id)
    .bind(reason)
    .fetch_one(&mut **tx)
    .await?;
    recompute_batch_status(tx, row.batch_id).await?;
    Ok(row)
}

// ── 送審去重 ──

async fn latest_execution_of(tx: &mut Tx<'_>, org_id: i64, subject_type: &str, subject_id: i64) -> ApiResult<Option<ExecutionRow>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {EXEC_COLS} FROM tbl_energy_workflow_execution
          WHERE organization_id = $1 AND subject_type = $2 AND subject_id = $3 AND deleted_at IS NULL ORDER BY id DESC LIMIT 1"
    ))
    .bind(org_id)
    .bind(subject_type)
    .bind(subject_id.to_string())
    .fetch_optional(&mut **tx)
    .await?)
}

/// 撞單的可讀資訊 —— 指得出「是哪一筆、誰送的、現在在哪一關」，前端靠 `executionId` 畫連結。
async fn clash_info(tx: &mut Tx<'_>, org_id: i64, subject_type: &str, subject_id: i64, known: Option<ExecutionRow>) -> ApiResult<Map<String, Value>> {
    let exec = match known {
        Some(e) => Some(e),
        None => latest_execution_of(tx, org_id, subject_type, subject_id).await?,
    };
    let Some(exec) = exec else { return Ok(Map::new()) };
    let node = match &exec.current_node_key {
        Some(k) => workflow::model::node_by_key_opt(tx, exec.definition_id, k).await?.map(|n| n.name),
        None => None,
    };
    let starter = match exec.started_by_user_id {
        Some(u) => crate::common::user_name(tx, u).await?,
        None => None,
    };
    let mut m = Map::new();
    m.insert("executionId".into(), json!(exec.id));
    m.insert("status".into(), json!(exec.status));
    // 印節點名稱不印 key。
    m.insert("currentNodeName".into(), json!(node));
    m.insert("submittedByName".into(), json!(starter));
    m.insert("submittedAt".into(), json!(exec.started_at));
    Ok(m)
}

/// 兩層兩碼：已通過的同業務身分 → `duplicate_document`；正在跑的 → `already_in_review`。
/// 中斷／終止的、審核未通過的、`businessKey` 為 null 的都放行。DB 的 partial unique 是併發下的最後一道。
async fn assert_not_duplicate(
    tx: &mut Tx<'_>,
    org_id: i64,
    subject_type: &str,
    business_key: Option<&str>,
    self_id: i64,
) -> ApiResult<()> {
    let Some(key) = business_key else { return Ok(()) };
    let table = if subject_type == "energyBill" { "tbl_energy_bill" } else { "tbl_energy_installment" };
    // FOR UPDATE：兩個並行的送審在這裡排隊，而不是各自通過。
    let siblings: Vec<(i64, String, Option<String>)> = sqlx::query_as(&format!(
        "SELECT id, approval_state, document_no FROM {table}
          WHERE organization_id = $1 AND business_key = $2 AND deleted_at IS NULL ORDER BY id FOR UPDATE"
    ))
    .bind(org_id)
    .bind(key)
    .fetch_all(&mut **tx)
    .await?;
    if siblings.is_empty() {
        return Ok(());
    }
    if let Some(a) = siblings.iter().find(|s| s.1 == "approved") {
        let mut d = clash_info(tx, org_id, subject_type, a.0, None).await?;
        d.insert("subjectId".into(), json!(a.0));
        d.insert("documentNo".into(), json!(a.2));
        return Err(err_d(409, "duplicate_document", Value::Object(d)));
    }
    let ids: Vec<String> = siblings.iter().map(|s| s.0.to_string()).collect();
    let live: Option<ExecutionRow> = sqlx::query_as(&format!(
        "SELECT {EXEC_COLS} FROM tbl_energy_workflow_execution
          WHERE organization_id = $1 AND subject_type = $2 AND subject_id = ANY($3) AND status = ANY($4) AND deleted_at IS NULL
          ORDER BY id DESC LIMIT 1"
    ))
    .bind(org_id)
    .bind(subject_type)
    .bind(&ids)
    .bind(&["running", "abortPending"][..])
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(live) = live {
        // 自己這一列已經在跑的情況由別的碼處理。
        if live.subject_id == self_id.to_string() {
            return Ok(());
        }
        let sid: i64 = live.subject_id.parse().unwrap_or(0);
        let mut d = clash_info(tx, org_id, subject_type, sid, Some(live)).await?;
        d.insert("subjectId".into(), json!(sid));
        return Err(err_d(409, "already_in_review", Value::Object(d)));
    }
    Ok(())
}

/// 同一批裡的兩張同一張紙：只比更早的那幾筆（第一筆是本尊），已捨棄／已送審的不算。
async fn assert_no_batch_duplicate(tx: &mut Tx<'_>, batch_id: i64, self_item_id: i64, business_key: Option<&str>) -> ApiResult<()> {
    let Some(key) = business_key else { return Ok(()) };
    let siblings = items_of_batch(tx, batch_id).await?;
    for (idx, s) in siblings.iter().enumerate() {
        if s.id >= self_item_id || is_closed(&s.status) {
            continue;
        }
        let other_key: Option<String> = match (s.bill_id, s.installment_id) {
            (Some(b), _) => {
                sqlx::query_scalar("SELECT business_key FROM tbl_energy_bill WHERE id = $1 AND deleted_at IS NULL")
                    .bind(b)
                    .fetch_optional(&mut **tx)
                    .await?
            }
            (None, Some(i)) => {
                sqlx::query_scalar("SELECT business_key FROM tbl_energy_installment WHERE id = $1 AND deleted_at IS NULL")
                    .bind(i)
                    .fetch_optional(&mut **tx)
                    .await?
            }
            _ => continue,
        }
        .flatten();
        if other_key.as_deref() == Some(key) {
            // 「本批第幾筆」用未過濾的順序算 —— 跟畫面上的序號一致。
            return Err(err_d(
                409,
                "duplicate_in_batch",
                json!({ "itemId": s.id, "seq": idx + 1, "originalFilename": s.original_filename }),
            ));
        }
    }
    Ok(())
}

/// 要開的一條流程（已解好的版本，或人手動指定的流程 key）。
struct FlowToStart {
    workflow_key: String,
    definition: Option<workflow::model::DefinitionRow>,
    attributes: Value,
}

async fn resolve_flows(
    tx: &mut Tx<'_>,
    ctx: &Ctx,
    station_key: &str,
    station_id: Option<i64>,
    doc_type: EnergyDocType,
    business_date: NaiveDate,
    base: &Value,
) -> ApiResult<Vec<FlowToStart>> {
    let flows = workflow::resolve_flow_rows(tx, &ctx.access, station_key, station_id, doc_type, business_date).await?;
    Ok(flows
        .into_iter()
        .map(|f| FlowToStart {
            // 每條流程帶自己那份契約的參數 —— 兩條流程可能來自不同契約，共用一份快照會張冠李戴。
            attributes: contract_attributes(base.clone(), &f),
            workflow_key: f.workflow_key.clone(),
            definition: Some(f.definition),
        })
        .collect())
}

/// 一張單、多筆 execution，全部在同一個交易裡；每一條都接上原檔（核准的本質是拿螢幕上的數字對紙上的數字）。
async fn start_flows(
    tx: &mut Tx<'_>,
    ctx: &Ctx,
    flows: Vec<FlowToStart>,
    station_key: &str,
    station_id: Option<i64>,
    subject_type: &str,
    subject_id: i64,
    business_date: NaiveDate,
    attachment_id: Option<i64>,
) -> ApiResult<Vec<ExecutionRow>> {
    let mut out = Vec::new();
    for f in flows {
        let exec = workflow::start_execution(
            tx,
            ctx,
            StartExecution {
                station_key: station_key.to_string(),
                workflow_key: f.workflow_key,
                subject_type: subject_type.to_string(),
                subject_id: subject_id.to_string(),
                business_date,
                attributes: f.attributes,
                definition: f.definition,
                station_id,
            },
        )
        .await?;
        if let Some(a) = attachment_id {
            attachments::link_to_execution(tx, ctx, a, exec.id, "source").await?;
        }
        out.push(exec);
    }
    Ok(out)
}

async fn mark_item_submitted(tx: &mut Tx<'_>, item: &ItemRow, executions: &[ExecutionRow]) -> ApiResult<()> {
    let ids: Vec<i64> = executions.iter().map(|e| e.id).collect();
    // `execution_id` 是主要那一筆（第一條）；全部的 id 記在 `data.executionIds`。
    sqlx::query("UPDATE tbl_energy_ingest_item SET execution_id = $2, data = $3, status = 'submitted' WHERE id = $1")
        .bind(item.id)
        .bind(ids.first().copied())
        .bind(merge(&item.data, json!({ "executionIds": ids })))
        .execute(&mut **tx)
        .await?;
    recompute_batch_status(tx, item.batch_id).await?;
    Ok(())
}

/// 送審：依綁定開流程、接上原檔、item 標 submitted。去重與開流程同一個交易。
pub async fn submit_item(tx: &mut Tx<'_>, ctx: &Ctx, item_id: i64, business_date: Option<NaiveDate>) -> ApiResult<Vec<ExecutionRow>> {
    let item = item_in_org(tx, ctx.org_id(), item_id).await?;
    match item.status.as_str() {
        "unclassified" => return Err(err(409, "item_needs_doc_type")),
        "unmatched" => return Err(err(409, "item_needs_source_match")),
        "submitted" => return Err(err(409, "item_already_submitted")),
        "discarded" => return Err(err(409, "item_discarded")),
        _ => {}
    }
    let item_doc_type = item.doc_type.clone().unwrap_or_default();
    let (subject_type, subject_id, station_key, station_id, doc_type, base, business_key) = if is_bill_doc_type(&item_doc_type) {
        let bill_id = item.bill_id.ok_or_else(|| err(409, "item_has_no_bill"))?;
        let bill: BillRow = bills::bill_in_org(tx, ctx.org_id(), bill_id, true).await?;
        // 單別的真相在單上（人校對過的那一個），不在 item 上。
        let doc_type = bill.effective_doc_type();
        let station_id = bill.station_id.ok_or_else(|| err(409, "bill_needs_station"))?;
        // S2：找不到案場擋在上面那一行；找不到生效合約是另一種例外，各自的 409。
        if bill.review_status == "pendingContract" {
            return Err(err(409, "bill_needs_contract"));
        }
        let station = bills::station_by_id(tx, ctx.org_id(), station_id).await?.ok_or_else(|| err(409, "bill_needs_station"))?;
        ("energyBill", bill_id, station.customer_number, Some(station.id), doc_type, bill.attributes(), bill.business_key.clone())
    } else if item_doc_type == "depositRequest" {
        let inst_id = item.installment_id.ok_or_else(|| err(409, "item_needs_source_match"))?;
        let inst: Option<(i64, Decimal, Option<String>)> = sqlx::query_as(
            "SELECT contract_id, amount, business_key FROM tbl_energy_installment WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL",
        )
        .bind(inst_id)
        .bind(ctx.org_id())
        .fetch_optional(&mut **tx)
        .await?;
        let (contract_id, amount, key) = inst.ok_or_else(|| err(409, "item_needs_source_match"))?;
        // 保證金綁在契約涵蓋的案場上：取第一座（流程本身由契約決定，案場只是 execution 上要記的位置）。
        let station: Option<(i64, String)> = sqlx::query_as(
            "SELECT s.id, s.customer_number FROM mn_energy_contract_station cs
               JOIN tbl_energy_station s ON s.id = cs.station_id AND s.deleted_at IS NULL
              WHERE cs.contract_id = $1 AND cs.deleted_at IS NULL ORDER BY cs.id ASC LIMIT 1",
        )
        .bind(contract_id)
        .fetch_optional(&mut **tx)
        .await?;
        let (sid, skey) = station.map(|s| (Some(s.0), s.1)).unwrap_or((None, String::new()));
        // 金額取合約的，不取掃描件抽出來的。
        let base = json!({ "installment": { "amount": amount.to_f64().unwrap_or(0.0) } });
        ("energyInstallment", inst_id, skey, sid, EnergyDocType::DepositRequest, base, key)
    } else {
        return Err(err_d(422, "unsupported_doc_type", json!({ "docType": item_doc_type })));
    };

    assert_not_duplicate(tx, ctx.org_id(), subject_type, business_key.as_deref(), subject_id).await?;
    // 排在後面：那一份若已在跑或已通過，前兩種碼講得更精確（它們指得出流程單）。
    assert_no_batch_duplicate(tx, item.batch_id, item.id, business_key.as_deref()).await?;

    let business_date = business_date.unwrap_or_else(|| ctx.today());
    // 人在收件台手動指定過流程，就以他指定的為準（明確的人為決定不可被自動解析蓋掉）。
    let pinned = item.data.get("workflowPinned").and_then(Value::as_bool).unwrap_or(false);
    let flows = match item.workflow_key.as_deref().filter(|k| pinned && !k.is_empty()) {
        Some(k) => vec![FlowToStart { workflow_key: k.to_string(), definition: None, attributes: base.clone() }],
        None => resolve_flows(tx, ctx, &station_key, station_id, doc_type, business_date, &base).await?,
    };
    if flows.is_empty() {
        return Err(err(409, "item_has_no_workflow"));
    }
    let executions =
        start_flows(tx, ctx, flows, &station_key, station_id, subject_type, subject_id, business_date, item.attachment_id).await?;
    mark_item_submitted(tx, &item, &executions).await?;
    Ok(executions)
}

/// 從電費單直接送審 —— 與 `submit_item` 同一套去重與流程解析（兩條送審路徑不可以有兩種規則）。
pub async fn submit_bill(tx: &mut Tx<'_>, ctx: &Ctx, bill_id: i64, business_date: Option<NaiveDate>) -> ApiResult<Vec<ExecutionRow>> {
    let bill = bills::bill_in_org(tx, ctx.org_id(), bill_id, true).await?;
    let station_id = bill.station_id.ok_or_else(|| err(409, "bill_needs_station"))?;
    if bill.review_status == "pendingContract" {
        return Err(err(409, "bill_needs_contract"));
    }
    // 同一張單同時只能有一個在跑。
    if let Some(e) = bills::live_execution(tx, ctx.org_id(), bill_id).await? {
        return Err(err_d(409, "bill_already_submitted", json!({ "executionId": e })));
    }
    let station = bills::station_by_id(tx, ctx.org_id(), station_id).await?.ok_or_else(|| err(409, "bill_needs_station"))?;
    // 業務日期優先取計費期間結束日：回頭補送的舊單要綁到它自己那個時點的流程版本。
    let business_date = business_date.unwrap_or_else(|| bill.business_date(ctx.today()));
    assert_not_duplicate(tx, ctx.org_id(), "energyBill", bill.business_key.as_deref(), bill_id).await?;
    let flows =
        resolve_flows(tx, ctx, &station.customer_number, Some(station.id), bill.effective_doc_type(), business_date, &bill.attributes()).await?;
    let item: Option<ItemRow> = sqlx::query_as(&format!(
        "SELECT {ITEM_COLS} FROM tbl_energy_ingest_item WHERE organization_id = $1 AND bill_id = $2 AND deleted_at IS NULL ORDER BY id LIMIT 1"
    ))
    .bind(ctx.org_id())
    .bind(bill_id)
    .fetch_optional(&mut **tx)
    .await?;
    // 沒有送件項目（直接建檔的單）就沒有原檔可接，那是合法狀態。
    let attachment_id = match &item {
        Some(i) => i.attachment_id,
        None => bill.source_attachment_id,
    };
    let executions = start_flows(
        tx,
        ctx,
        flows,
        &station.customer_number,
        Some(station.id),
        "energyBill",
        bill_id,
        business_date,
        attachment_id,
    )
    .await?;
    // 這張單有送件項目的話一併標成 submitted，否則送單紀錄會永遠顯示「待處理」。
    if let Some(i) = item.filter(|i| i.status != "submitted") {
        mark_item_submitted(tx, &i, &executions).await?;
    }
    Ok(executions)
}

// ── 查詢 ──

pub fn item_view(i: &ItemRow) -> Value {
    let d = &i.data;
    json!({
        "id": i.id,
        "batchId": i.batch_id,
        "originalFilename": i.original_filename,
        "docType": i.doc_type,
        "workflowKey": i.workflow_key,
        // 只為了顯示，前端也不可以拿它當門檻。
        "classifyConfidence": i.classify_confidence.map(|c| c.to_string()),
        "classifyReason": d.get("classifyReason").cloned().unwrap_or(Value::Null),
        "attachmentId": i.attachment_id,
        "billId": i.bill_id,
        "installmentId": i.installment_id,
        "executionId": i.execution_id,
        "executionIds": d.get("executionIds").cloned().unwrap_or_else(|| json!([])),
        "status": i.status,
        "errorMessage": i.error_message,
        "amountMismatch": d.get("amountMismatch").cloned().unwrap_or(json!(false)),
        "alreadyPaid": d.get("alreadyPaid").cloned().unwrap_or(json!(false)),
        "extracted": d.get("extracted").cloned().unwrap_or(Value::Null),
        // 辨識這件事本身的結果（issues、resolution）；整包 ocrRaw 不放進列表。
        "recognition": d.get("ocrRaw").and_then(recognition_summary),
        "recognitionErrorCode": d.get("recognitionErrorCode").cloned().unwrap_or(Value::Null),
    })
}

fn counts_of(items: &[&ItemRow]) -> Value {
    let by = |st: &str| items.iter().filter(|i| i.status == st).count();
    json!({
        "submitted": by("submitted"),
        "classified": by("classified"),
        "extracted": by("extracted"),
        "unclassified": by("unclassified"),
        "unmatched": by("unmatched"),
        "discarded": by("discarded"),
        "failed": by("failed"),
    })
}

/// 送單紀錄。未完成的置頂（這頁是承辦的待辦清單，不是時間軸），同組之內新到舊。
pub async fn list_batches(tx: &mut Tx<'_>, access: &OrgAccess, limit: Option<i64>) -> ApiResult<Vec<Value>> {
    let limit = limit.filter(|n| *n > 0).unwrap_or(50).min(200);
    let rows: Vec<BatchRow> = sqlx::query_as(&format!(
        "SELECT {BATCH_COLS} FROM tbl_energy_ingest_batch WHERE organization_id = $1 AND deleted_at IS NULL
          ORDER BY submitted_at DESC, id DESC LIMIT $2"
    ))
    .bind(access.org_id())
    .bind(limit)
    .fetch_all(&mut **tx)
    .await?;
    if rows.is_empty() {
        return Ok(vec![]);
    }
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let items: Vec<ItemRow> =
        sqlx::query_as(&format!("SELECT {ITEM_COLS} FROM tbl_energy_ingest_item WHERE batch_id = ANY($1) AND deleted_at IS NULL ORDER BY id"))
            .bind(&ids)
            .fetch_all(&mut **tx)
            .await?;
    let user_ids: Vec<i64> = crate::common::uniq(rows.iter().filter_map(|r| r.submitted_by_user_id));
    let names = user_names(tx, &user_ids).await?;
    let mut views: Vec<(usize, DateTime<Utc>, i64, Value)> = rows
        .iter()
        .map(|r| {
            let mine: Vec<&ItemRow> = items.iter().filter(|i| i.batch_id == r.id).collect();
            let pending = mine.iter().filter(|i| !is_closed(&i.status)).count();
            let mut wf = Map::new();
            for i in &mine {
                let k = i.workflow_key.clone().filter(|k| !k.is_empty()).unwrap_or_else(|| "unclassified".into());
                let n = wf.get(&k).and_then(Value::as_u64).unwrap_or(0) + 1;
                wf.insert(k, json!(n));
            }
            let v = json!({
                "id": r.id,
                "submittedAt": r.submitted_at,
                "submittedByUserId": r.submitted_by_user_id,
                "submittedByName": r.submitted_by_user_id.and_then(|u| names.get(&u).cloned()),
                "fileCount": r.file_count,
                "status": r.status,
                "note": r.note,
                "counts": counts_of(&mine),
                // 角標的數字由後端算：前端加總 counts 的版本會在多一個狀態時默默漏掉那一類。
                "pendingCount": pending,
                "billIds": mine.iter().filter_map(|i| i.bill_id).collect::<Vec<_>>(),
                "workflowCounts": wf,
            });
            (usize::from(pending == 0), r.submitted_at, r.id, v)
        })
        .collect();
    views.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(b.2.cmp(&a.2)));
    Ok(views.into_iter().map(|v| v.3).collect())
}

/// 批次詳情：item 加上 subject 的身分與同批重複標記（兩件事都要把整批放在一起看才算得出來）。
pub async fn get_batch(tx: &mut Tx<'_>, access: &OrgAccess, batch_id: i64) -> ApiResult<Value> {
    let row = batch_in_org(tx, access.org_id(), batch_id).await?;
    let items = items_of_batch(tx, batch_id).await?;
    let submitter = match row.submitted_by_user_id {
        Some(u) => crate::common::user_name(tx, u).await?,
        None => None,
    };
    let pending = items.iter().filter(|i| !is_closed(&i.status)).count();
    Ok(json!({
        "id": row.id,
        "submittedAt": row.submitted_at,
        "submittedByUserId": row.submitted_by_user_id,
        "submittedByName": submitter,
        "fileCount": row.file_count,
        "status": row.status,
        "note": row.note,
        "pendingCount": pending,
        "items": decorate_items(tx, &items).await?,
    }))
}

async fn decorate_items(tx: &mut Tx<'_>, items: &[ItemRow]) -> ApiResult<Vec<Value>> {
    let bill_ids: Vec<i64> = crate::common::uniq(items.iter().filter_map(|i| i.bill_id));
    let inst_ids: Vec<i64> = crate::common::uniq(items.iter().filter_map(|i| i.installment_id));
    let bills: Vec<(i64, Option<String>, Option<String>, String, Option<i64>, String)> = if bill_ids.is_empty() {
        vec![]
    } else {
        sqlx::query_as(
            "SELECT id, document_no, business_key, approval_state, station_id, review_status FROM tbl_energy_bill
              WHERE id = ANY($1) AND deleted_at IS NULL ORDER BY id",
        )
        .bind(&bill_ids)
        .fetch_all(&mut **tx)
        .await?
    };
    let insts: Vec<(i64, Option<String>, Option<String>, String)> = if inst_ids.is_empty() {
        vec![]
    } else {
        sqlx::query_as(
            "SELECT id, document_no, business_key, approval_state FROM tbl_energy_installment WHERE id = ANY($1) AND deleted_at IS NULL ORDER BY id",
        )
        .bind(&inst_ids)
        .fetch_all(&mut **tx)
        .await?
    };
    let bill_by: HashMap<i64, _> = bills.iter().map(|b| (b.0, b)).collect();
    let inst_by: HashMap<i64, _> = insts.iter().map(|b| (b.0, b)).collect();
    // 同批重複：以 businessKey 分組，第一筆是本尊；已捨棄的不參與（那正是使用者處理重複的方式）。
    let mut first_of: HashMap<String, (i64, usize)> = HashMap::new();
    let mut out = Vec::new();
    for (idx, i) in items.iter().enumerate() {
        let mut v = item_view(i);
        let (subject_type, doc_no, key, approval, station, review) = if let Some(b) = i.bill_id.and_then(|id| bill_by.get(&id)) {
            (json!("energyBill"), json!(b.1), b.2.clone(), json!(b.3), json!(b.4), json!(b.5))
        } else if i.bill_id.is_some() {
            (json!("energyBill"), Value::Null, None, Value::Null, Value::Null, Value::Null)
        } else if let Some(s) = i.installment_id.and_then(|id| inst_by.get(&id)) {
            (json!("energyInstallment"), json!(s.1), s.2.clone(), json!(s.3), Value::Null, Value::Null)
        } else if i.installment_id.is_some() {
            (json!("energyInstallment"), Value::Null, None, Value::Null, Value::Null, Value::Null)
        } else {
            (Value::Null, Value::Null, None, Value::Null, Value::Null, Value::Null)
        };
        v["subjectType"] = subject_type;
        v["subjectId"] = json!(i.bill_id.or(i.installment_id));
        v["documentNo"] = doc_no;
        v["businessKey"] = json!(key);
        v["approvalState"] = approval;
        v["stationId"] = station;
        v["reviewStatus"] = review;
        let (dup_id, dup_seq) = match key.filter(|_| i.status != "discarded") {
            None => (Value::Null, Value::Null),
            Some(k) => match first_of.get(&k) {
                // seq 是畫面上的第幾筆（1-based），不是 DB id。
                Some((id, seq)) => (json!(id), json!(seq)),
                None => {
                    first_of.insert(k, (i.id, idx + 1));
                    (Value::Null, Value::Null)
                }
            },
        };
        v["duplicateOfItemId"] = dup_id;
        v["duplicateOfSeq"] = dup_seq;
        out.push(v);
    }
    Ok(out)
}
