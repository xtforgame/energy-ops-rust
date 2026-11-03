//! 物件儲存：S3 相容（MinIO）的最小客戶端，SigV4 簽章、path-style 網址。
//! 只做產品用得到的幾個動作；presign 也在這裡（簽章是同一套）。

use crate::config::S3Config;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use sha2::{Digest, Sha256};
use std::time::Duration;

type HmacSha256 = Hmac<Sha256>;

/// RFC 3986 unreserved 以外都編碼（SigV4 的要求）。
const S3_ENCODE: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("storage request failed: {0}")]
    Transport(String),
    #[error("storage returned {status}: {body}")]
    Status { status: u16, body: String },
}

#[derive(Clone)]
pub struct Storage {
    cfg: S3Config,
    http: reqwest::Client,
}

pub struct ObjectHead {
    pub size: u64,
    pub content_type: Option<String>,
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let mut m = HmacSha256::new_from_slice(key).expect("hmac key");
    m.update(data.as_bytes());
    m.finalize().into_bytes().to_vec()
}

fn encode_path_segment(s: &str) -> String {
    utf8_percent_encode(s, S3_ENCODE).to_string()
}

fn encode_query(s: &str) -> String {
    utf8_percent_encode(s, S3_ENCODE).to_string()
}

impl Storage {
    pub fn new(cfg: S3Config) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("reqwest client");
        Self { cfg, http }
    }

    pub fn bucket(&self) -> &str {
        &self.cfg.bucket
    }

    /// 產品碼給的鍵加上設定的前綴。
    pub fn full_key(&self, key: &str) -> String {
        if self.cfg.key_prefix.is_empty() {
            key.to_string()
        } else {
            format!("{}/{}", self.cfg.key_prefix.trim_end_matches('/'), key)
        }
    }

    fn host(&self) -> String {
        let e = self.cfg.endpoint.trim_end_matches('/');
        e.split("://").nth(1).unwrap_or(e).to_string()
    }

    fn canonical_uri(&self, key: Option<&str>) -> String {
        let mut uri = format!("/{}", encode_path_segment(&self.cfg.bucket));
        if let Some(k) = key {
            uri.push('/');
            let full = self.full_key(k);
            let encoded: Vec<String> = full.split('/').map(encode_path_segment).collect();
            uri.push_str(&encoded.join("/"));
        }
        uri
    }

    fn signing_key(&self, date: &str) -> Vec<u8> {
        let k_date = hmac(format!("AWS4{}", self.cfg.secret_key).as_bytes(), date);
        let k_region = hmac(&k_date, &self.cfg.region);
        let k_service = hmac(&k_region, "s3");
        hmac(&k_service, "aws4_request")
    }

    fn canonical_query(params: &[(String, String)]) -> String {
        let mut p: Vec<(String, String)> =
            params.iter().map(|(k, v)| (encode_query(k), encode_query(v))).collect();
        p.sort();
        p.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&")
    }

    /// 帶 Authorization 標頭的請求。
    async fn send(
        &self,
        method: reqwest::Method,
        key: Option<&str>,
        query: &[(String, String)],
        body: Bytes,
        content_type: Option<&str>,
    ) -> Result<reqwest::Response, StorageError> {
        let now: DateTime<Utc> = Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();
        let payload_hash = sha256_hex(&body);
        let host = self.host();
        let uri = self.canonical_uri(key);
        let cq = Self::canonical_query(query);

        let canonical_headers = format!("host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n");
        let signed_headers = "host;x-amz-content-sha256;x-amz-date";
        let canonical_request =
            format!("{}\n{uri}\n{cq}\n{canonical_headers}\n{signed_headers}\n{payload_hash}", method.as_str());
        let scope = format!("{date}/{}/s3/aws4_request", self.cfg.region);
        let string_to_sign =
            format!("AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}", sha256_hex(canonical_request.as_bytes()));
        let signature = hex::encode(hmac(&self.signing_key(&date), &string_to_sign));
        let auth = format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.cfg.access_key
        );

        let mut url = format!("{}{uri}", self.cfg.endpoint.trim_end_matches('/'));
        if !cq.is_empty() {
            url.push('?');
            url.push_str(&cq);
        }
        let mut req = self
            .http
            .request(method, url)
            .header("x-amz-date", amz_date)
            .header("x-amz-content-sha256", payload_hash)
            .header("authorization", auth);
        if let Some(ct) = content_type {
            req = req.header("content-type", ct);
        }
        req.body(body).send().await.map_err(|e| StorageError::Transport(e.to_string()))
    }

    async fn expect_ok(resp: reqwest::Response) -> Result<reqwest::Response, StorageError> {
        if resp.status().is_success() {
            Ok(resp)
        } else {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Err(StorageError::Status { status, body })
        }
    }

    /// bucket 不在就建（冪等）。
    pub async fn ensure_bucket(&self) -> Result<(), StorageError> {
        let head = self.send(reqwest::Method::HEAD, None, &[], Bytes::new(), None).await?;
        if head.status().is_success() {
            return Ok(());
        }
        let resp = self.send(reqwest::Method::PUT, None, &[], Bytes::new(), None).await?;
        let status = resp.status().as_u16();
        if resp.status().is_success() || status == 409 {
            return Ok(());
        }
        Err(StorageError::Status { status, body: resp.text().await.unwrap_or_default() })
    }

    pub async fn put(&self, key: &str, body: Bytes, content_type: &str) -> Result<(), StorageError> {
        let resp = self.send(reqwest::Method::PUT, Some(key), &[], body, Some(content_type)).await?;
        Self::expect_ok(resp).await.map(|_| ())
    }

    pub async fn get(&self, key: &str) -> Result<Bytes, StorageError> {
        let resp = self.send(reqwest::Method::GET, Some(key), &[], Bytes::new(), None).await?;
        let resp = Self::expect_ok(resp).await?;
        resp.bytes().await.map_err(|e| StorageError::Transport(e.to_string()))
    }

    /// 物件不在回 `None`。
    pub async fn head(&self, key: &str) -> Result<Option<ObjectHead>, StorageError> {
        let resp = self.send(reqwest::Method::HEAD, Some(key), &[], Bytes::new(), None).await?;
        if resp.status().as_u16() == 404 {
            return Ok(None);
        }
        let resp = Self::expect_ok(resp).await?;
        let size = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let content_type = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).map(String::from);
        Ok(Some(ObjectHead { size, content_type }))
    }

    pub async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let resp = self.send(reqwest::Method::DELETE, Some(key), &[], Bytes::new(), None).await?;
        if resp.status().as_u16() == 404 {
            return Ok(());
        }
        Self::expect_ok(resp).await.map(|_| ())
    }

    /// 預簽網址（query 簽章）。`extra` 例如 `response-content-disposition`。
    pub fn presign(&self, method: &str, key: &str, expires: Duration, extra: &[(String, String)]) -> String {
        let now = Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();
        let scope = format!("{date}/{}/s3/aws4_request", self.cfg.region);
        let mut query: Vec<(String, String)> = vec![
            ("X-Amz-Algorithm".into(), "AWS4-HMAC-SHA256".into()),
            ("X-Amz-Credential".into(), format!("{}/{scope}", self.cfg.access_key)),
            ("X-Amz-Date".into(), amz_date.clone()),
            ("X-Amz-Expires".into(), expires.as_secs().min(604_800).to_string()),
            ("X-Amz-SignedHeaders".into(), "host".into()),
        ];
        query.extend_from_slice(extra);
        let uri = self.canonical_uri(Some(key));
        let cq = Self::canonical_query(&query);
        let canonical_request = format!("{method}\n{uri}\n{cq}\nhost:{}\n\nhost\nUNSIGNED-PAYLOAD", self.host());
        let string_to_sign =
            format!("AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}", sha256_hex(canonical_request.as_bytes()));
        let signature = hex::encode(hmac(&self.signing_key(&date), &string_to_sign));
        format!("{}{uri}?{cq}&X-Amz-Signature={signature}", self.cfg.endpoint.trim_end_matches('/'))
    }
}
