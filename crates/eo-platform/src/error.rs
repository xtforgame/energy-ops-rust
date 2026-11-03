//! API 錯誤：一律回 `{code, message, details?}`，`code` 是穩定的 snake_case。

use actix_web::http::StatusCode;
use actix_web::{HttpResponse, ResponseError};
use serde_json::Value;
use std::fmt;

#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: String,
    pub message: String,
    pub details: Option<Value>,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(status: StatusCode, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { status, code: code.into(), message: message.into(), details: None }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn bad_request(code: &str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }
    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthenticated", "login required")
    }
    pub fn forbidden(code: &str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, code, message)
    }
    /// 權限不足的標準錯誤；`details.required` 列出要的 permission（任一即可）。
    pub fn permission_denied(required: &[&str]) -> Self {
        Self::forbidden("permission_denied", "permission denied")
            .with_details(serde_json::json!({ "required": required }))
    }
    pub fn not_found(code: &str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, code, message)
    }
    pub fn conflict(code: &str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }
    pub fn unprocessable(code: &str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, code, message)
    }
    pub fn not_implemented(code: &str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_IMPLEMENTED, code, message)
    }
    pub fn service_unavailable(code: &str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, code, message)
    }
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", message)
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.status.as_u16(), self.code, self.message)
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        self.status
    }

    fn error_response(&self) -> HttpResponse {
        // 500 不外露內部訊息。
        let message = if self.status.is_server_error() && self.code == "internal" {
            "internal server error".to_string()
        } else {
            self.message.clone()
        };
        let mut body = serde_json::json!({ "code": self.code, "message": message });
        if let Some(d) = &self.details {
            body["details"] = d.clone();
        }
        HttpResponse::build(self.status).json(body)
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::RowNotFound = e {
            return ApiError::not_found("not_found", "not found");
        }
        if let Some(db) = e.as_database_error() {
            match db.code().as_deref() {
                Some("23505") => {
                    return ApiError::conflict("conflict", "unique constraint violated")
                        .with_details(serde_json::json!({ "constraint": db.constraint() }))
                }
                Some("23503") => {
                    return ApiError::conflict("foreign_key_violation", "referenced row missing or still referenced")
                        .with_details(serde_json::json!({ "constraint": db.constraint() }))
                }
                _ => {}
            }
        }
        log::error!("database error: {e}");
        ApiError::internal(e.to_string())
    }
}

impl From<crate::storage::StorageError> for ApiError {
    fn from(e: crate::storage::StorageError) -> Self {
        log::error!("storage error: {e}");
        ApiError::new(StatusCode::BAD_GATEWAY, "storage_error", e.to_string())
    }
}

/// actix 內建 extractor（Json/Query/Path）的錯誤也轉成同一個形狀。
pub fn json_config() -> actix_web::web::JsonConfig {
    actix_web::web::JsonConfig::default()
        .limit(4 * 1024 * 1024)
        .error_handler(|err, _| ApiError::bad_request("invalid_body", err.to_string()).into())
}

pub fn query_config() -> actix_web::web::QueryConfig {
    actix_web::web::QueryConfig::default()
        .error_handler(|err, _| ApiError::bad_request("invalid_query", err.to_string()).into())
}

pub fn path_config() -> actix_web::web::PathConfig {
    actix_web::web::PathConfig::default()
        .error_handler(|err, _| ApiError::bad_request("invalid_path", err.to_string()).into())
}
