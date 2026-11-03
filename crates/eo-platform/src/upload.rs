//! multipart 上傳：讀進記憶體（單檔有上限），檔名照舊 parser 的 latin1 問題修正。

use crate::error::{ApiError, ApiResult};
use actix_multipart::Multipart;
use bytes::{Bytes, BytesMut};
use futures::StreamExt;
use std::collections::HashMap;

pub struct UploadedFile {
    pub field: String,
    pub file_name: String,
    pub content_type: String,
    pub bytes: Bytes,
}

#[derive(Default)]
pub struct MultipartForm {
    pub files: Vec<UploadedFile>,
    pub fields: HashMap<String, String>,
}

impl MultipartForm {
    /// 第一個檔案（多數端點只收一個 `file`）。
    pub fn take_file(&mut self, field: &str) -> Option<UploadedFile> {
        let idx = self.files.iter().position(|f| f.field == field)?;
        Some(self.files.remove(idx))
    }

    pub fn field(&self, k: &str) -> Option<&str> {
        self.fields.get(k).map(String::as_str).filter(|v| !v.is_empty())
    }
}

/// 讀完整個 multipart。超過 `max_file_bytes` 回 413；文字欄位上限 64KB。
pub async fn read_multipart(mut payload: Multipart, max_file_bytes: u64) -> ApiResult<MultipartForm> {
    let mut form = MultipartForm::default();
    while let Some(field) = payload.next().await {
        let mut field = field.map_err(|e| ApiError::bad_request("invalid_multipart", e.to_string()))?;
        let name = field.name().unwrap_or_default().to_string();
        let file_name = field.content_disposition().and_then(|cd| cd.get_filename().map(String::from));
        let content_type = field.content_type().map(|m| m.essence_str().to_string());
        let limit = if file_name.is_some() { max_file_bytes } else { 64 * 1024 };
        let mut buf = BytesMut::new();
        while let Some(chunk) = field.next().await {
            let chunk = chunk.map_err(|e| ApiError::bad_request("invalid_multipart", e.to_string()))?;
            if (buf.len() + chunk.len()) as u64 > limit {
                return Err(ApiError::new(
                    actix_web::http::StatusCode::PAYLOAD_TOO_LARGE,
                    "file_too_large",
                    format!("file exceeds {limit} bytes"),
                ));
            }
            buf.extend_from_slice(&chunk);
        }
        match file_name {
            Some(raw) => form.files.push(UploadedFile {
                field: name,
                file_name: eo_domain::upload_filename::decode_upload_filename(Some(&raw)),
                content_type: content_type.unwrap_or_else(|| "application/octet-stream".into()),
                bytes: buf.freeze(),
            }),
            None => {
                form.fields.insert(name, String::from_utf8_lossy(&buf).into_owned());
            }
        }
    }
    Ok(form)
}
