//! 台電電費單辨識服務（taipower-recognition）的 HTTP 客戶端，只由後端呼叫。
//!
//! 照 App 5 `recognition/tprClient.ts` 的直連模式（`EO_TPR_URL`，可加 `EO_TPR_API_KEY` 當 Bearer）：
//! `GET /ready`、`POST /api/v1/jobs`（multipart：files、sourceFileIds、passwords）、
//! `GET /api/v1/jobs/{id}`、`GET /api/v1/jobs/{id}/result`、`DELETE /api/v1/jobs/{id}`。
//! 沒設定時 `TprClient::from_config` 回 None，畫面顯示「服務未開」。
//! App 5 的「平台模式」（vaxal 按需容器）是 Supabase 部署專用，單一 binary 不需要，沒搬。

use eo_domain::taipower_canonical::{CanonicalJobResult, TprJobSummary};
use eo_platform::error::ApiError;
use std::time::Duration;

#[derive(Debug, Clone, thiserror::Error)]
#[error("{code}: {message}")]
pub struct TprError {
    pub code: String,
    pub status: u16,
    pub message: String,
}

impl TprError {
    pub fn new(code: &str, status: u16, message: impl Into<String>) -> Self {
        Self { code: code.into(), status, message: message.into() }
    }

    /// 415（格式不收）與 413（太大）是永久錯誤，重送也不會好。
    pub fn is_permanent(&self) -> bool {
        matches!(self.status, 413 | 415) || self.code == "tpr_no_files"
    }
}

impl From<TprError> for ApiError {
    fn from(e: TprError) -> Self {
        let status = actix_web::http::StatusCode::from_u16(e.status).unwrap_or(actix_web::http::StatusCode::BAD_GATEWAY);
        ApiError::new(status, e.code, e.message)
    }
}

pub struct TprUploadFile {
    pub source_file_id: String,
    pub file_name: String,
    pub bytes: bytes::Bytes,
    pub content_type: String,
    pub password: Option<String>,
}

#[derive(Clone)]
pub struct TprClient {
    base: String,
    api_key: Option<String>,
    http: reqwest::Client,
}

impl TprClient {
    pub fn new(base: &str, api_key: Option<String>) -> Self {
        Self {
            base: base.trim().trim_end_matches('/').to_string(),
            api_key: api_key.filter(|k| !k.trim().is_empty()),
            http: reqwest::Client::new(),
        }
    }

    pub fn from_config(cfg: &eo_platform::config::Config) -> Option<Self> {
        cfg.tpr_url.as_deref().filter(|u| !u.trim().is_empty()).map(|u| Self::new(u, cfg.tpr_api_key.clone()))
    }

    fn req(&self, method: reqwest::Method, path: &str, timeout: Duration) -> reqwest::RequestBuilder {
        let mut r = self.http.request(method, format!("{}{path}", self.base)).timeout(timeout);
        if let Some(k) = &self.api_key {
            r = r.bearer_auth(k);
        }
        r
    }

    async fn send(rb: reqwest::RequestBuilder, timeout: Duration) -> Result<reqwest::Response, TprError> {
        rb.send().await.map_err(|e| {
            if e.is_timeout() {
                TprError::new("tpr_timeout", 504, format!("辨識服務逾時（{}ms）", timeout.as_millis()))
            } else {
                TprError::new("tpr_unreachable", 503, format!("連不上辨識服務：{e}"))
            }
        })
    }

    /// 服務的錯誤格式 `{error:{code,message}}`；不是 JSON（例如 502 HTML）就用狀態碼描述。
    async fn read_error(resp: reqwest::Response) -> TprError {
        let status = resp.status().as_u16();
        let body: Option<serde_json::Value> = resp.json().await.ok();
        if let Some(code) = body.as_ref().and_then(|b| b["error"]["code"].as_str()) {
            let msg = body.as_ref().and_then(|b| b["error"]["message"].as_str()).unwrap_or(code);
            return TprError::new(code, status, msg);
        }
        TprError::new(&format!("http_{status}"), status, format!("辨識服務回應 {status}"))
    }

    pub async fn ready(&self) -> bool {
        let t = Duration::from_secs(3);
        matches!(Self::send(self.req(reqwest::Method::GET, "/ready", t), t).await, Ok(r) if r.status().is_success())
    }

    pub async fn create_job(&self, files: Vec<TprUploadFile>) -> Result<TprJobSummary, TprError> {
        if files.is_empty() {
            return Err(TprError::new("tpr_no_files", 400, "至少要一個檔案"));
        }
        let any_password = files.iter().any(|f| f.password.is_some());
        let mut form = reqwest::multipart::Form::new();
        for f in files {
            form = form.text("sourceFileIds", f.source_file_id.clone());
            if any_password {
                form = form.text("passwords", f.password.clone().unwrap_or_default());
            }
            let part = reqwest::multipart::Part::stream(f.bytes)
                .file_name(f.file_name)
                .mime_str(if f.content_type.is_empty() { "application/octet-stream" } else { &f.content_type })
                .map_err(|e| TprError::new("tpr_bad_file", 400, e.to_string()))?;
            form = form.part("files", part);
        }
        let t = Duration::from_secs(120);
        let resp = Self::send(self.req(reqwest::Method::POST, "/api/v1/jobs", t).multipart(form), t).await?;
        if !resp.status().is_success() {
            return Err(Self::read_error(resp).await);
        }
        resp.json().await.map_err(|e| TprError::new("tpr_bad_response", 502, e.to_string()))
    }

    pub async fn get_job(&self, job_id: &str) -> Result<TprJobSummary, TprError> {
        let t = Duration::from_secs(30);
        let path = format!("/api/v1/jobs/{}", urlencode(job_id));
        let resp = Self::send(self.req(reqwest::Method::GET, &path, t), t).await?;
        if !resp.status().is_success() {
            return Err(Self::read_error(resp).await);
        }
        resp.json().await.map_err(|e| TprError::new("tpr_bad_response", 502, e.to_string()))
    }

    pub async fn get_result(&self, job_id: &str) -> Result<CanonicalJobResult, TprError> {
        let t = Duration::from_secs(30);
        let path = format!("/api/v1/jobs/{}/result", urlencode(job_id));
        let resp = Self::send(self.req(reqwest::Method::GET, &path, t), t).await?;
        if !resp.status().is_success() {
            return Err(Self::read_error(resp).await);
        }
        resp.json().await.map_err(|e| TprError::new("tpr_bad_response", 502, e.to_string()))
    }

    /// 結果的原始 JSON（整包存進 `ocrRaw`：形狀不 pin，服務改版不動 schema）。
    pub async fn get_result_raw(&self, job_id: &str) -> Result<serde_json::Value, TprError> {
        let t = Duration::from_secs(30);
        let path = format!("/api/v1/jobs/{}/result", urlencode(job_id));
        let resp = Self::send(self.req(reqwest::Method::GET, &path, t), t).await?;
        if !resp.status().is_success() {
            return Err(Self::read_error(resp).await);
        }
        resp.json().await.map_err(|e| TprError::new("tpr_bad_response", 502, e.to_string()))
    }

    /// 取回結果後清掉服務端的副本；失敗不算錯（服務會自己過期）。
    pub async fn delete_job(&self, job_id: &str) -> bool {
        let t = Duration::from_secs(10);
        let path = format!("/api/v1/jobs/{}", urlencode(job_id));
        matches!(Self::send(self.req(reqwest::Method::DELETE, &path, t), t).await, Ok(r) if r.status().is_success() || r.status().as_u16() == 404)
    }
}

fn urlencode(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}
