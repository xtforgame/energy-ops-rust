//! 假的台電辨識服務：講真服務的 HTTP 協定（見 `tpr_client`），結果取自四份真範本的辨識輸出
//! （`tests/fixtures/canonical-samples.json`）。給整合測試與本機開發用（`energy-ops fake-tpr`），
//! 不必跑 4.2GB 的真服務。
//!
//! 依上傳的檔名決定結果：
//! · 檔名（去掉路徑）等於某份範本的檔名，或檔名含範本代號（`high-voltage-device`、`high-voltage-rental`、
//!   `renewable-surplus`、`renewable-no-surplus`）→ 回那份範本的 record／issues；
//! · 檔名含 `locked` → 該份失敗，錯誤碼 `password_required`（有給密碼就當成功，回第一份範本）；
//! · content-type 是 HEIC → 整個請求 415；
//! · 其他 → 該份失敗，錯誤碼 `unsupported_template`。
//! 建立後前 `polls_before_done` 次查詢回 processing，之後 completed。

use actix_web::{web, App, HttpResponse, HttpServer};
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

const SAMPLES: &str = include_str!("../../../tests/fixtures/canonical-samples.json");

const ALIASES: &[(&str, &str)] = &[
    ("high-voltage-device", "逆變器電費單(僅設備發電使用電費).pdf"),
    ("high-voltage-rental", "逆變器電費單(含電表租費&設備發電使用電費).pdf"),
    ("renewable-surplus", "電費單(有餘電需開發票).pdf"),
    ("renewable-no-surplus", "電費單(無餘電需付款).pdf"),
];

/// 某份範本的辨識結果（canonical document），以範本檔名查。
pub fn canonical_sample(file_name: &str) -> Option<Value> {
    let all: Value = serde_json::from_str(SAMPLES).ok()?;
    all["documents"].get(file_name).cloned()
}

struct Job {
    created_at: String,
    files: Vec<(String, String)>,
    documents: Vec<Value>,
    polls: u32,
    deleted: bool,
}

pub struct FakeTprState {
    samples: Value,
    jobs: Mutex<HashMap<String, Job>>,
    seq: AtomicU64,
    polls_before_done: u32,
    /// 收到的建立請求數（測試用來確認「有送／沒送」）。
    pub created: AtomicU64,
}

impl FakeTprState {
    pub fn new(polls_before_done: u32) -> Self {
        Self {
            samples: serde_json::from_str(SAMPLES).expect("canonical samples"),
            jobs: Mutex::new(HashMap::new()),
            seq: AtomicU64::new(1),
            polls_before_done,
            created: AtomicU64::new(0),
        }
    }

    fn sample_for(&self, file_name: &str) -> Option<Value> {
        let docs = self.samples["documents"].as_object()?;
        let base = file_name.rsplit('/').next().unwrap_or(file_name);
        if let Some(d) = docs.get(base) {
            return Some(d.clone());
        }
        ALIASES.iter().find(|(alias, _)| base.contains(alias)).and_then(|(_, name)| docs.get(*name).cloned())
    }

    fn document_for(&self, source_file_id: &str, file_name: &str, password: Option<&str>) -> Value {
        let failed = |code: &str, msg: &str| {
            json!({
                "sourceFileId": source_file_id, "sourceFileName": file_name, "status": "failed",
                "error": { "code": code, "message": msg }
            })
        };
        let sample = if file_name.contains("locked") {
            if password.map(|p| p.is_empty()).unwrap_or(true) {
                return failed("password_required", "檔案需要密碼");
            }
            self.samples["documents"].as_object().and_then(|d| d.values().next().cloned())
        } else {
            self.sample_for(file_name)
        };
        match sample {
            Some(mut d) => {
                d["sourceFileId"] = json!(source_file_id);
                d["sourceFileName"] = json!(file_name);
                // 卡 B 的假資料：上傳檔名帶 `cn-<假電號>` 時覆寫 accountNumber，
                // 讓 smoke 與使用者操作都用假電號，不會帶出範本裡的真電號。
                if let Some(cn) = cn_override(file_name) {
                    if let Some(f) = d.get_mut("record").and_then(|r| r.get_mut("fields")).and_then(|f| f.get_mut("accountNumber")) {
                        f["value"] = json!(cn);
                        f["rawValue"] = json!(cn);
                    }
                }
                d
            }
            None => failed("unsupported_template", "無法辨識的版型"),
        }
    }
}

/// 檔名裡 `cn-` 之後的那段（去掉副檔名）就是假電號。例：
/// `renewable-surplus-cn-00-0000-0001.pdf` → `00-0000-0001`。
fn cn_override(file_name: &str) -> Option<String> {
    let base = file_name.rsplit('/').next().unwrap_or(file_name);
    let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(base);
    let idx = stem.find("cn-")?;
    let v = &stem[idx + 3..];
    (!v.is_empty()).then(|| v.to_string())
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn err(status: u16, code: &str, message: &str) -> HttpResponse {
    HttpResponse::build(actix_web::http::StatusCode::from_u16(status).unwrap())
        .json(json!({ "error": { "code": code, "message": message } }))
}

fn job_status(job: &Job, done: bool) -> &'static str {
    if !done {
        return "processing";
    }
    let any_failed = job.documents.iter().any(|d| d["status"] == "failed");
    let all_failed = job.documents.iter().all(|d| d["status"] == "failed");
    if all_failed {
        "failed"
    } else if any_failed || job.documents.iter().any(|d| d["issues"].as_array().map(|a| !a.is_empty()).unwrap_or(false)) {
        "completed_with_issues"
    } else {
        "completed"
    }
}

fn summary(id: &str, job: &Job, done: bool) -> Value {
    let files: Vec<Value> = job
        .files
        .iter()
        .zip(&job.documents)
        .map(|((sid, name), d)| {
            let status = if !done { "processing" } else if d["status"] == "failed" { "failed" } else { "succeeded" };
            let mut f = json!({ "sourceFileId": sid, "sourceFileName": name, "status": status });
            if done && d["status"] == "failed" {
                f["error"] = d["error"].clone();
            }
            f
        })
        .collect();
    let mut v = json!({ "jobId": id, "status": job_status(job, done), "createdAt": job.created_at, "updatedAt": now(), "files": files });
    if done {
        v["completedAt"] = json!(now());
    }
    v
}

async fn create(state: web::Data<FakeTprState>, mut payload: actix_multipart::Multipart) -> HttpResponse {
    let mut ids: Vec<String> = Vec::new();
    let mut passwords: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    while let Some(Ok(mut field)) = payload.next().await {
        let name = field.name().unwrap_or_default().to_string();
        let file_name = field.content_disposition().and_then(|cd| cd.get_filename().map(String::from));
        let ct = field.content_type().map(|m| m.to_string()).unwrap_or_default();
        let mut buf = Vec::new();
        while let Some(Ok(chunk)) = field.next().await {
            buf.extend_from_slice(&chunk);
        }
        match name.as_str() {
            "sourceFileIds" => ids.push(String::from_utf8_lossy(&buf).into_owned()),
            "passwords" => passwords.push(String::from_utf8_lossy(&buf).into_owned()),
            "files" => {
                if ct.contains("heic") || ct.contains("heif") {
                    return err(415, "unsupported_media_type", "HEIC 不支援");
                }
                files.push(file_name.unwrap_or_else(|| "file".into()));
            }
            _ => {}
        }
    }
    if files.is_empty() {
        return err(400, "no_files", "至少要一個檔案");
    }
    state.created.fetch_add(1, Ordering::SeqCst);
    let id = format!("job-{}", state.seq.fetch_add(1, Ordering::SeqCst));
    let pairs: Vec<(String, String)> =
        files.iter().enumerate().map(|(i, f)| (ids.get(i).cloned().unwrap_or_else(|| format!("{i}")), f.clone())).collect();
    let documents = pairs
        .iter()
        .enumerate()
        .map(|(i, (sid, name))| state.document_for(sid, name, passwords.get(i).map(String::as_str)))
        .collect();
    let job = Job { created_at: now(), files: pairs, documents, polls: 0, deleted: false };
    let body = summary(&id, &job, false);
    state.jobs.lock().unwrap().insert(id, job);
    HttpResponse::Ok().json(body)
}

async fn get_job(state: web::Data<FakeTprState>, path: web::Path<String>) -> HttpResponse {
    let id = path.into_inner();
    let mut jobs = state.jobs.lock().unwrap();
    let Some(job) = jobs.get_mut(&id).filter(|j| !j.deleted) else { return err(404, "job_not_found", "no such job") };
    job.polls += 1;
    let done = job.polls > state.polls_before_done;
    HttpResponse::Ok().json(summary(&id, job, done))
}

async fn get_result(state: web::Data<FakeTprState>, path: web::Path<String>) -> HttpResponse {
    let id = path.into_inner();
    let jobs = state.jobs.lock().unwrap();
    let Some(job) = jobs.get(&id).filter(|j| !j.deleted) else { return err(404, "job_not_found", "no such job") };
    if job.polls <= state.polls_before_done {
        return err(409, "job_not_finished", "job is still processing");
    }
    let status = match job_status(job, true) {
        "failed" => "failed",
        "completed" => "completed",
        _ => "completed_with_issues",
    };
    HttpResponse::Ok().json(json!({
        "schemaVersion": "1.0", "jobId": id, "status": status,
        "createdAt": job.created_at, "completedAt": now(), "documents": job.documents,
    }))
}

async fn delete_job(state: web::Data<FakeTprState>, path: web::Path<String>) -> HttpResponse {
    match state.jobs.lock().unwrap().get_mut(&path.into_inner()) {
        Some(j) => {
            j.deleted = true;
            HttpResponse::NoContent().finish()
        }
        None => err(404, "job_not_found", "no such job"),
    }
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/health", web::get().to(|| async { HttpResponse::Ok().json(json!({"status": "ok"})) }))
        .route("/ready", web::get().to(|| async { HttpResponse::Ok().json(json!({"ready": true})) }))
        .route("/api/v1/jobs", web::post().to(create))
        .route("/api/v1/jobs/{id}", web::get().to(get_job))
        .route("/api/v1/jobs/{id}", web::delete().to(delete_job))
        .route("/api/v1/jobs/{id}/result", web::get().to(get_result));
}

/// 起一個假服務，回傳 (base url, state, server handle)。
pub fn spawn(port: u16, polls_before_done: u32) -> std::io::Result<(String, web::Data<FakeTprState>, actix_web::dev::Server)> {
    let state = web::Data::new(FakeTprState::new(polls_before_done));
    let s = state.clone();
    let server = HttpServer::new(move || App::new().app_data(s.clone()).configure(configure))
        .workers(1)
        .bind(("127.0.0.1", port))?;
    let port = server.addrs()[0].port();
    Ok((format!("http://127.0.0.1:{port}"), state, server.run()))
}
