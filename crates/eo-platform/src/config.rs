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
    /// 一個辨識 job 最多放幾份檔（1–50）。
    pub tpr_batch_size: usize,
    /// 辨識對帳背景任務的間隔秒數；0 ＝ 不啟動。
    pub recognition_tick_secs: u64,
    /// 提醒檢查器背景任務的間隔秒數；0 ＝ 不啟動（卡 E，手動端點／測試照常可跑）。
    pub reminder_check_secs: u64,
    /// cookie 加 `Secure`（正式環境走 https 時開）。
    pub cookie_secure: bool,
    /// 開機時建立的初始超級使用者（帳號、密碼）；兩者都設才建。
    pub bootstrap_admin: Option<(String, String)>,
    /// 灌示範資料。
    pub demo_seed: bool,
    /// 示範資料另外灌四份範本電費單（含它們的流程）；e2e crawler 用。
    pub demo_sample_bills: bool,
    /// 四份範本 PDF 所在的資料夾。
    pub demo_samples_dir: PathBuf,
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
            tpr_batch_size: env("EO_TPR_BATCH_SIZE")
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|n| *n >= 1)
                .map(|n| n.min(50))
                .unwrap_or(10),
            recognition_tick_secs: env("EO_RECOGNITION_TICK_SECS").and_then(|v| v.parse().ok()).unwrap_or(60),
            reminder_check_secs: env("EO_REMINDER_CHECK_SECS").and_then(|v| v.parse().ok()).unwrap_or(0),
            cookie_secure: env("EO_COOKIE_SECURE").map(|v| v == "1" || v == "true").unwrap_or(false),
            bootstrap_admin: match (env("EO_BOOTSTRAP_ADMIN_USER"), env("EO_BOOTSTRAP_ADMIN_PASSWORD")) {
                (Some(u), Some(p)) => Some((u, p)),
                _ => None,
            },
            demo_seed: env("EO_DEMO_SEED").map(|v| v == "1" || v == "true").unwrap_or(false),
            demo_sample_bills: env("EO_DEMO_SAMPLE_BILLS").map(|v| v == "1" || v == "true").unwrap_or(false),
            demo_samples_dir: env("EO_DEMO_SAMPLES_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("tests/fixtures/samples")),
        })
    }
}
