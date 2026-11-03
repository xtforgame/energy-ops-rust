//! 上傳的兩種來源（照 App 5 `platform/stagedUpload.ts`）：
//! · multipart（欄位名 `file`，其餘 form 欄位）；
//! · 直傳：瀏覽器先拿 `uploads/sign` 的預簽網址把檔案 PUT 到暫存位置，再對原本的上傳端點送
//!   JSON `{stagingKey, originalFilename, mimeType, …}`。暫存位置必須屬於「這個人＋這個目標」，
//!   用完就刪（成功失敗都刪）。

use crate::common::{err, err_d};
use actix_web::{web, HttpRequest};
use bytes::Bytes;
use eo_domain::attachment_limits::MAX_FILE_BYTES;
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::HashMap;

/// 暫存位置的前綴：`energy/staging/u<userId>/<target>/`。
pub fn staging_prefix(user_id: i64, target: &str) -> String {
    format!("energy/staging/u{user_id}/{target}/")
}

pub fn new_staging_key(user_id: i64, target: &str) -> String {
    format!("{}{}", staging_prefix(user_id, target), uuid::Uuid::new_v4())
}

pub fn batch_target(batch_id: i64) -> String {
    format!("batch-{batch_id}")
}

pub fn execution_target(execution_id: i64) -> String {
    format!("exec-{execution_id}")
}

/// 讀進來的檔案（不論來源）。
pub struct Incoming {
    pub file_name: String,
    pub content_type: String,
    pub bytes: Bytes,
    pub fields: HashMap<String, String>,
    /// 直傳來的才有：處理完要刪掉。
    pub staging_key: Option<String>,
}

impl Incoming {
    pub fn field(&self, k: &str) -> Option<&str> {
        self.fields.get(k).map(String::as_str).filter(|v| !v.is_empty())
    }

    pub fn field_id(&self, k: &str) -> ApiResult<Option<i64>> {
        match self.field(k) {
            None => Ok(None),
            Some(v) => v
                .trim()
                .parse::<i64>()
                .ok()
                .filter(|n| *n > 0)
                .map(Some)
                .ok_or_else(|| err_d(400, "invalid_body", json!({ "field": k }))),
        }
    }
}

fn is_json(req: &HttpRequest) -> bool {
    req.headers()
        .get(actix_web::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().starts_with("application/json"))
}

fn is_multipart(req: &HttpRequest) -> bool {
    req.headers()
        .get(actix_web::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().starts_with("multipart/form-data"))
}

/// 讀上傳：multipart 或直傳 JSON。`target` 是這個端點的目標（決定暫存位置該長什麼樣）。
pub async fn read_incoming(core: &CoreRef, req: &HttpRequest, mut payload: web::Payload, user_id: i64, target: &str) -> ApiResult<Incoming> {
    if is_multipart(req) {
        let mp = actix_multipart::Multipart::new(req.headers(), payload);
        let mut form = eo_platform::upload::read_multipart(mp, MAX_FILE_BYTES).await?;
        let file = form.take_file("file").ok_or_else(|| err(400, "file_required"))?;
        return Ok(Incoming {
            file_name: file.file_name,
            content_type: file.content_type,
            bytes: file.bytes,
            fields: form.fields,
            staging_key: None,
        });
    }
    if !is_json(req) {
        return Err(err(400, "file_required"));
    }
    let mut buf = bytes::BytesMut::new();
    while let Some(chunk) = payload.next().await {
        let chunk = chunk.map_err(|e| err_d(400, "invalid_body", json!({ "message": e.to_string() })))?;
        if buf.len() + chunk.len() > 64 * 1024 {
            return Err(err(413, "body_too_large"));
        }
        buf.extend_from_slice(&chunk);
    }
    let body: Value = serde_json::from_slice(&buf).map_err(|e| err_d(400, "invalid_body", json!({ "message": e.to_string() })))?;
    let key = body.get("stagingKey").and_then(Value::as_str).unwrap_or("").to_string();
    if key.is_empty() {
        return Err(err(400, "file_required"));
    }
    if !key.starts_with(&staging_prefix(user_id, target)) || key.contains("..") {
        return Err(err(400, "staging_key_invalid"));
    }
    let head = core.storage.head(&key).await?.ok_or_else(|| err(400, "staged_file_missing"))?;
    if head.size > MAX_FILE_BYTES {
        remove_staged(core, &key).await;
        return Err(err_d(413, "file_too_large", json!({ "size": head.size, "max": MAX_FILE_BYTES })));
    }
    let bytes = core.storage.get(&key).await?;
    let mut fields = HashMap::new();
    if let Some(m) = body.as_object() {
        for (k, v) in m {
            if matches!(k.as_str(), "stagingKey" | "originalFilename" | "mimeType") {
                continue;
            }
            let s = match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => continue,
            };
            fields.insert(k.clone(), s);
        }
    }
    Ok(Incoming {
        file_name: body.get("originalFilename").and_then(Value::as_str).unwrap_or("upload").to_string(),
        content_type: body.get("mimeType").and_then(Value::as_str).unwrap_or("").to_string(),
        bytes,
        fields,
        staging_key: Some(key),
    })
}

/// 刪暫存物件。刪不掉不影響這次上傳的結果，但要留下紀錄。
pub async fn remove_staged(core: &CoreRef, key: &str) {
    if let Err(e) = core.storage.delete(key).await {
        log::warn!("[energy] 暫存檔刪除失敗 {key}: {e}");
    }
}

/// 處理完（成功或失敗）都清掉暫存物件。
pub async fn finish(core: &CoreRef, incoming: &Incoming) {
    if let Some(k) = &incoming.staging_key {
        remove_staged(core, k).await;
    }
}
