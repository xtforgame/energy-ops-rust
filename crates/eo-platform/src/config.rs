//! 設定：全部來自環境變數（開發時由 `.env` 載入）。

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct S3Config {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
    /// 物件鍵的前綴（測試用來隔開彼此）。
    pub key_prefix: String,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub http_host: String,
    pub http_port: u16,
    pub s3: S3Config,
    pub web_dir: Option<PathBuf>,
    pub tpr_url: Option<String>,
    pub tpr_api_key: Option<String>,
    /// cookie 加 `Secure`（正式環境走 https 時開）。
    pub cookie_secure: bool,
    /// 開機時建立的初始超級使用者（帳號、密碼）；兩者都設才建。
    pub bootstrap_admin: Option<(String, String)>,
    /// 灌示範資料。
    pub demo_seed: bool,
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let req = |k: &str| env(k).ok_or_else(|| format!("missing env {k}"));
        Ok(Self {
            database_url: req("EO_DATABASE_URL")?,
            http_host: env("EO_HTTP_HOST").unwrap_or_else(|| "127.0.0.1".into()),
            http_port: env("EO_HTTP_PORT").and_then(|p| p.parse().ok()).unwrap_or(10950),
            s3: S3Config {
                endpoint: req("EO_S3_ENDPOINT")?,
                region: env("EO_S3_REGION").unwrap_or_else(|| "us-east-1".into()),
                bucket: req("EO_S3_BUCKET")?,
                access_key: req("EO_S3_ACCESS_KEY")?,
                secret_key: req("EO_S3_SECRET_KEY")?,
                key_prefix: env("EO_S3_KEY_PREFIX").unwrap_or_default(),
            },
            web_dir: env("EO_WEB_DIR").map(PathBuf::from),
            tpr_url: env("EO_TPR_URL"),
            tpr_api_key: env("EO_TPR_API_KEY"),
            cookie_secure: env("EO_COOKIE_SECURE").map(|v| v == "1" || v == "true").unwrap_or(false),
            bootstrap_admin: match (env("EO_BOOTSTRAP_ADMIN_USER"), env("EO_BOOTSTRAP_ADMIN_PASSWORD")) {
                (Some(u), Some(p)) => Some((u, p)),
                _ => None,
            },
            demo_seed: env("EO_DEMO_SEED").map(|v| v == "1" || v == "true").unwrap_or(false),
        })
    }
}
