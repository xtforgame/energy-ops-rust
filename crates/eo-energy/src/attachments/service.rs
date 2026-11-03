//! 附件的上傳、查詢、換版、解除連結（照 App 5 `attachments/AttachmentService.ts`）。
//!
//! 硬約束：
//! · key 依歸屬分資料夾（`energy/executions/<id>/…`、`energy/batches/<id>/…`），不是扁平 hash 池；
//! · 先存物件、驗過真的落地、才寫 DB —— 寧可整個失敗，也不要「DB 說有、物件沒有」；
//! · 附件永不刪除：換版是新列＋supersedes，移除的是連結；
//! · 原檔名 DB 那份是真值，物件 metadata 只放 ASCII 編碼版（這裡的 put 不帶 metadata，DB 即真值）。

use crate::audit::{self, AuditInput};
use crate::common::{err, err_d, Ctx, Tx};
use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use eo_domain::attachment_limits::{
    ext_for_mime, is_allowed_mime, ALLOWED_MIME_TYPES, MAX_BYTES_PER_EXECUTION, MAX_FILES_PER_EXECUTION, MAX_FILE_BYTES,
};
use eo_platform::error::{ApiError, ApiResult};
use eo_platform::storage::{Storage, StorageError};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::FromRow;

/// 上傳用到的物件儲存動作。抽成 trait 只為了測得到「物件沒落地」那條路。
#[async_trait]
pub trait AttachmentStore: Send + Sync {
    async fn put(&self, key: &str, body: Bytes, content_type: &str) -> Result<(), StorageError>;
    /// 物件真的在嗎（post-verify）。
    async fn exists(&self, key: &str) -> Result<bool, StorageError>;
}

#[async_trait]
impl AttachmentStore for Storage {
    async fn put(&self, key: &str, body: Bytes, content_type: &str) -> Result<(), StorageError> {
        Storage::put(self, key, body, content_type).await
    }
    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        Ok(self.head(key).await?.is_some())
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct AttachmentRow {
    pub id: i64,
    pub storage_key: String,
    pub original_filename: String,
    pub mime_type: String,
    pub size: i64,
    pub hash: String,
    pub hash_algo: String,
    pub uploaded_at: DateTime<Utc>,
    pub status: String,
    pub organization_id: i64,
    pub execution_id: Option<i64>,
    pub ingest_batch_id: Option<i64>,
    pub uploaded_by_user_id: Option<i64>,
    pub supersedes_attachment_id: Option<i64>,
}

pub const ATTACHMENT_COLS: &str = "id, storage_key, original_filename, mime_type, size, hash, hash_algo, uploaded_at, status,
    organization_id, execution_id, ingest_batch_id, uploaded_by_user_id, supersedes_attachment_id";

#[derive(Debug, Clone, FromRow)]
pub struct LinkRow {
    pub id: i64,
    pub role: String,
    pub seq: i32,
    pub note: Option<String>,
    pub attachment_id: i64,
    pub task_id: Option<i64>,
}

const LINK_COLS: &str = "id, role, seq, note, attachment_id, task_id";

/// 附件的歸屬：execution 或收件批次（DB 有 CHECK 擋兩個都空）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Execution(i64),
    Batch(i64),
}

/// 兩種前綴：流程進行中掛上去的、收件時的原始掃描檔（那時 execution 還不存在）。
pub fn build_storage_key(owner: Owner, mime_type: &str) -> String {
    let ext = ext_for_mime(mime_type);
    let id = uuid::Uuid::new_v4();
    match owner {
        Owner::Execution(e) => format!("energy/executions/{e}/{id}.{ext}"),
        Owner::Batch(b) => format!("energy/batches/{b}/{id}.{ext}"),
    }
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

pub struct UploadInput {
    pub owner: Owner,
    pub bytes: Bytes,
    pub original_filename: String,
    pub mime_type: String,
    /// 掛在哪一關。None ＝ 掛在 execution 層級。
    pub task_id: Option<i64>,
    pub role: Option<String>,
    pub note: Option<String>,
    /// 換版：這一份取代哪一份。
    pub supersedes_attachment_id: Option<i64>,
}

/// 進 DB 之前的檢查（大小、型別、檔名）。直傳與 multipart 共用。
pub fn validate_file(size: u64, mime_type: &str, original_filename: &str) -> ApiResult<()> {
    if !is_allowed_mime(mime_type) {
        return Err(err_d(415, "unsupported_mime_type", json!({ "mimeType": mime_type, "allowed": ALLOWED_MIME_TYPES })));
    }
    if size == 0 {
        return Err(err(400, "empty_file"));
    }
    if size > MAX_FILE_BYTES {
        return Err(err_d(413, "file_too_large", json!({ "size": size, "max": MAX_FILE_BYTES })));
    }
    if original_filename.trim().is_empty() {
        return Err(err(400, "filename_required"));
    }
    Ok(())
}

/// 上傳。順序是「先存物件、驗過、才寫 DB」；DB 那一段（含稽核）在自己的交易裡。
pub async fn upload(ctx: &Ctx, store: &dyn AttachmentStore, input: UploadInput) -> ApiResult<AttachmentRow> {
    validate_file(input.bytes.len() as u64, &input.mime_type, &input.original_filename)?;
    let size = input.bytes.len() as i64;

    // 單一歸屬的總量防呆（純防呆，不是業務規則）。
    let (owner_col, owner_id) = match input.owner {
        Owner::Execution(e) => ("execution_id", e),
        Owner::Batch(b) => ("ingest_batch_id", b),
    };
    let (count, total): (i64, Option<i64>) = sqlx::query_as(&format!(
        "SELECT COUNT(*), SUM(size)::bigint FROM tbl_energy_attachment WHERE {owner_col} = $1 AND deleted_at IS NULL"
    ))
    .bind(owner_id)
    .fetch_one(&ctx.core.pool)
    .await?;
    if count as u64 >= MAX_FILES_PER_EXECUTION {
        return Err(err_d(409, "too_many_files", json!({ "max": MAX_FILES_PER_EXECUTION })));
    }
    if (total.unwrap_or(0) + size) as u64 > MAX_BYTES_PER_EXECUTION {
        return Err(err_d(409, "execution_quota_exceeded", json!({ "max": MAX_BYTES_PER_EXECUTION })));
    }

    let hash = sha256_hex(&input.bytes);
    let storage_key = build_storage_key(input.owner, &input.mime_type);
    store.put(&storage_key, input.bytes.clone(), &input.mime_type).await?;
    // post-verify：put 回報成功但物件沒落地是真的會發生的。
    if !store.exists(&storage_key).await? {
        return Err(err_d(502, "storage_verify_failed", json!({ "storageKey": storage_key })));
    }

    let result = async {
        let mut tx = ctx.core.pool.begin().await?;
        let row = insert_rows(&mut tx, ctx, &input, &storage_key, &hash, size).await?;
        tx.commit().await?;
        Ok::<AttachmentRow, ApiError>(row)
    }
    .await;
    if result.is_err() {
        // 物件已落地但 DB 失敗：不刪物件（附件永不 delete）。沒有 DB 列的物件只是浪費空間。
        log::warn!("[energy][attachment] DB 寫入失敗，物件 {storage_key} 保留不刪");
    }
    result
}

async fn insert_rows(tx: &mut Tx<'_>, ctx: &Ctx, input: &UploadInput, storage_key: &str, hash: &str, size: i64) -> ApiResult<AttachmentRow> {
    let (execution_id, batch_id) = match input.owner {
        Owner::Execution(e) => (Some(e), None),
        Owner::Batch(b) => (None, Some(b)),
    };
    let role = input.role.clone().filter(|r| !r.is_empty()).unwrap_or_else(|| "submitted".into());
    let attachment: AttachmentRow = sqlx::query_as(&format!(
        "INSERT INTO tbl_energy_attachment
           (organization_id, execution_id, ingest_batch_id, storage_key, original_filename, mime_type, size, hash, hash_algo,
            uploaded_by_user_id, uploaded_at, status, supersedes_attachment_id, data)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'sha256', $9, $10, 'active', $11, $12) RETURNING {ATTACHMENT_COLS}"
    ))
    .bind(ctx.org_id())
    .bind(execution_id)
    .bind(batch_id)
    .bind(storage_key)
    .bind(&input.original_filename)
    .bind(&input.mime_type)
    .bind(size)
    .bind(hash)
    .bind(ctx.user_id())
    .bind(ctx.now())
    .bind(input.supersedes_attachment_id)
    .bind(json!({}))
    .fetch_one(&mut **tx)
    .await?;

    // 換版：舊筆標 superseded，不刪除。
    if let Some(old) = input.supersedes_attachment_id {
        sqlx::query("UPDATE tbl_energy_attachment SET status = 'superseded' WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL")
            .bind(old)
            .bind(ctx.org_id())
            .execute(&mut **tx)
            .await?;
        audit::record(
            tx,
            ctx,
            AuditInput::new("attachment.superseded", old, format!("附件被新版取代（#{}）", attachment.id))
                .payload(json!({ "replacedByAttachmentId": attachment.id })),
        )
        .await?;
    }

    let siblings: i64 = match input.task_id {
        Some(t) => sqlx::query_scalar(
            "SELECT COUNT(*) FROM tbl_energy_attachment_link WHERE task_id = $1 AND role = $2 AND deleted_at IS NULL",
        )
        .bind(t)
        .bind(&role)
        .fetch_one(&mut **tx)
        .await?,
        None => sqlx::query_scalar(
            "SELECT COUNT(*) FROM tbl_energy_attachment_link WHERE task_id IS NULL AND role = $1 AND deleted_at IS NULL",
        )
        .bind(&role)
        .fetch_one(&mut **tx)
        .await?,
    };
    sqlx::query(
        "INSERT INTO tbl_energy_attachment_link (attachment_id, task_id, role, seq, note, linked_by_user_id, data)
         VALUES ($1, $2, $3, $4, $5, $6, '{}'::jsonb)",
    )
    .bind(attachment.id)
    .bind(input.task_id)
    .bind(&role)
    .bind(siblings as i32)
    .bind(input.note.as_deref())
    .bind(ctx.user_id())
    .execute(&mut **tx)
    .await?;

    // fail-closed：上傳成功但沒留痕，等於檔案來路不明。
    audit::record(
        tx,
        ctx,
        AuditInput::new("attachment.uploaded", attachment.id, format!("上傳附件「{}」", input.original_filename))
            .label(input.original_filename.clone())
            .payload(json!({
                "executionId": execution_id,
                "ingestBatchId": batch_id,
                "taskId": input.task_id,
                "role": role,
                "originalFilename": input.original_filename,
                "size": size,
                "hash": hash,
                "mimeType": input.mime_type,
            })),
    )
    .await?;

    // 流程中才補上的另外記一筆：稽核頁是按 type 篩的。
    if role == "added" {
        if let Some(e) = execution_id {
            audit::record(
                tx,
                ctx,
                AuditInput::new("flow.attachment_added", attachment.id, format!("補件「{}」", input.original_filename))
                    .label(input.original_filename.clone())
                    .payload(json!({
                        "executionId": e,
                        "taskId": input.task_id,
                        "role": "added",
                        "originalFilename": input.original_filename,
                        "size": size,
                    })),
            )
            .await?;
        }
    }
    Ok(attachment)
}

/// 本組織的某份附件（查無 404、別家 403）。
pub async fn attachment_in_org(tx: &mut Tx<'_>, org_id: i64, id: i64) -> ApiResult<AttachmentRow> {
    let row: Option<AttachmentRow> =
        sqlx::query_as(&format!("SELECT {ATTACHMENT_COLS} FROM tbl_energy_attachment WHERE id = $1 AND deleted_at IS NULL"))
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?;
    let row = row.ok_or_else(|| err(404, "attachment_not_found"))?;
    if row.organization_id != org_id {
        return Err(err(403, "attachment_not_in_org"));
    }
    Ok(row)
}

/// 系統內部讀取（辨識要原始 bytes）—— 不寫 `attachment.downloaded`：那一欄記的是「人看了什麼」。
pub async fn read_bytes(ctx: &Ctx, attachment_id: i64) -> ApiResult<(AttachmentRow, Bytes)> {
    let mut tx = ctx.core.pool.begin().await?;
    let row = attachment_in_org(&mut tx, ctx.org_id(), attachment_id).await?;
    drop(tx);
    let bytes = ctx.core.storage.get(&row.storage_key).await?;
    Ok((row, bytes))
}

/// 人的下載：留一筆讀取事件（best-effort —— 下載不該因為稽核寫失敗而失敗）。
pub async fn record_download(ctx: &Ctx, row: &AttachmentRow) {
    let _ = audit::record_best_effort(
        &ctx.core.pool,
        ctx,
        AuditInput::new("attachment.downloaded", row.id, format!("下載附件「{}」", row.original_filename))
            .label(row.original_filename.clone())
            .payload(json!({ "executionId": row.execution_id.unwrap_or(0) })),
    )
    .await;
}

/// 把收件時的原檔接到 execution 上（同一個交易）。`ingest_batch_id` 保留：那是血緣。
/// 沒有附件、或附件不屬於本組織，都不是錯 —— 回 None。
pub async fn link_to_execution(tx: &mut Tx<'_>, ctx: &Ctx, attachment_id: i64, execution_id: i64, role: &str) -> ApiResult<Option<i64>> {
    let owner: Option<i64> =
        sqlx::query_scalar("SELECT organization_id FROM tbl_energy_attachment WHERE id = $1 AND deleted_at IS NULL")
            .bind(attachment_id)
            .fetch_optional(&mut **tx)
            .await?;
    if owner != Some(ctx.org_id()) {
        return Ok(None);
    }
    sqlx::query("UPDATE tbl_energy_attachment SET execution_id = $2 WHERE id = $1 AND deleted_at IS NULL")
        .bind(attachment_id)
        .bind(execution_id)
        .execute(&mut **tx)
        .await?;
    // 冪等：同一份檔案同一個 role 不重複 link。
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM tbl_energy_attachment_link WHERE attachment_id = $1 AND role = $2 AND deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .bind(attachment_id)
    .bind(role)
    .fetch_optional(&mut **tx)
    .await?;
    if existing.is_some() {
        return Ok(existing);
    }
    let siblings: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tbl_energy_attachment_link WHERE attachment_id = $1 AND deleted_at IS NULL")
            .bind(attachment_id)
            .fetch_one(&mut **tx)
            .await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO tbl_energy_attachment_link (attachment_id, task_id, role, seq, linked_by_user_id, data)
         VALUES ($1, NULL, $2, $3, $4, '{}'::jsonb) RETURNING id",
    )
    .bind(attachment_id)
    .bind(role)
    .bind(siblings as i32)
    .bind(ctx.user_id())
    .fetch_one(&mut **tx)
    .await?;
    Ok(Some(id))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkView {
    pub id: i64,
    pub task_id: Option<i64>,
    pub role: String,
    pub seq: i32,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentView {
    pub id: i64,
    pub original_filename: String,
    pub mime_type: String,
    pub size: i64,
    pub hash: String,
    pub status: String,
    pub uploaded_by_user_id: Option<i64>,
    pub uploaded_at: DateTime<Utc>,
    pub supersedes_attachment_id: Option<i64>,
    pub links: Vec<LinkView>,
}

/// 某 execution 的附件（新到舊，含它們掛在哪些關卡）。
pub async fn list_for_execution(tx: &mut Tx<'_>, execution_id: i64) -> ApiResult<Vec<AttachmentView>> {
    let rows: Vec<AttachmentRow> = sqlx::query_as(&format!(
        "SELECT {ATTACHMENT_COLS} FROM tbl_energy_attachment WHERE execution_id = $1 AND deleted_at IS NULL
          ORDER BY uploaded_at DESC, id DESC"
    ))
    .bind(execution_id)
    .fetch_all(&mut **tx)
    .await?;
    if rows.is_empty() {
        return Ok(vec![]);
    }
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let links: Vec<LinkRow> = sqlx::query_as(&format!(
        "SELECT {LINK_COLS} FROM tbl_energy_attachment_link WHERE attachment_id = ANY($1) AND deleted_at IS NULL ORDER BY seq ASC, id ASC"
    ))
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| AttachmentView {
            links: links
                .iter()
                .filter(|l| l.attachment_id == r.id)
                .map(|l| LinkView { id: l.id, task_id: l.task_id, role: l.role.clone(), seq: l.seq, note: l.note.clone() })
                .collect(),
            id: r.id,
            original_filename: r.original_filename,
            mime_type: r.mime_type,
            size: r.size,
            hash: r.hash,
            status: r.status,
            uploaded_by_user_id: r.uploaded_by_user_id,
            uploaded_at: r.uploaded_at,
            supersedes_attachment_id: r.supersedes_attachment_id,
        })
        .collect())
}

/// 連結與它的附件（解除連結前判權用）。
pub async fn link_in_org(tx: &mut Tx<'_>, org_id: i64, link_id: i64) -> ApiResult<(LinkRow, AttachmentRow)> {
    let link: Option<LinkRow> =
        sqlx::query_as(&format!("SELECT {LINK_COLS} FROM tbl_energy_attachment_link WHERE id = $1 AND deleted_at IS NULL"))
            .bind(link_id)
            .fetch_optional(&mut **tx)
            .await?;
    let link = link.ok_or_else(|| err(404, "attachment_link_not_found"))?;
    let att = attachment_in_org(tx, org_id, link.attachment_id).await?;
    Ok((link, att))
}

/// 移除連結 —— 不是刪檔，檔案與附件列都留著。理由必填。
pub async fn remove_link(tx: &mut Tx<'_>, ctx: &Ctx, link: &LinkRow, attachment: &AttachmentRow, reason: &str) -> ApiResult<()> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err(err(400, "reason_required"));
    }
    sqlx::query("UPDATE tbl_energy_attachment_link SET deleted_at = $2 WHERE id = $1 AND deleted_at IS NULL")
        .bind(link.id)
        .bind(ctx.now())
        .execute(&mut **tx)
        .await?;
    audit::record(
        tx,
        ctx,
        AuditInput::new(
            "attachment.link_removed",
            link.id,
            format!("移除附件連結「{}」：{reason}", attachment.original_filename),
        )
        .label(attachment.original_filename.clone())
        .payload(json!({ "attachmentId": attachment.id, "taskId": link.task_id, "role": link.role, "reason": reason })),
    )
    .await?;
    Ok(())
}

/// 給回應用的附件摘要。
pub fn summary(row: &AttachmentRow) -> Value {
    json!({
        "id": row.id,
        "originalFilename": row.original_filename,
        "mimeType": row.mime_type,
        "size": row.size,
        "hash": row.hash,
        "status": row.status,
        "executionId": row.execution_id,
        "ingestBatchId": row.ingest_batch_id,
        "supersedesAttachmentId": row.supersedes_attachment_id,
    })
}
