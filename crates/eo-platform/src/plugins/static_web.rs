//! 供應前端建置產物（`web/dist`）；找不到的非 API 路徑一律回 index.html（SPA）。

use crate::azldi::Plugin;
use crate::plugin::{AppPlugin, RouteRegistrar};
use actix_web::{web, HttpRequest, HttpResponse};
use std::path::PathBuf;
use std::sync::Arc;

pub struct StaticWebPlugin {
    dir: Option<PathBuf>,
}

impl StaticWebPlugin {
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self { dir }
    }
}

struct WebDir(PathBuf);

fn content_type(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "wasm" => "application/wasm",
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}

async fn serve(req: HttpRequest, dir: web::Data<WebDir>) -> HttpResponse {
    let path = req.path();
    if path.starts_with("/api/") {
        return HttpResponse::NotFound().json(serde_json::json!({ "code": "route_not_found", "message": "no such API route" }));
    }
    let rel = path.trim_start_matches('/');
    // 不允許跳出資料夾。
    if rel.split('/').any(|seg| seg == "..") {
        return HttpResponse::BadRequest().finish();
    }
    let mut file = dir.0.join(rel);
    if rel.is_empty() || !file.is_file() {
        file = dir.0.join("index.html");
    }
    match tokio::fs::read(&file).await {
        Ok(bytes) => {
            let cache = if file.ends_with("index.html") { "no-cache" } else { "public, max-age=3600" };
            HttpResponse::Ok().content_type(content_type(&file)).insert_header(("cache-control", cache)).body(bytes)
        }
        Err(_) => HttpResponse::NotFound().body("web assets not built"),
    }
}

impl Plugin for StaticWebPlugin {
    fn name(&self) -> &str {
        "static_web"
    }
    crate::plugin_boilerplate!();
}

impl AppPlugin for StaticWebPlugin {
    fn configure_routes(&self, cfg: &mut RouteRegistrar<'_>, _self_arc: &Arc<dyn AppPlugin>) {
        let Some(dir) = &self.dir else { return };
        cfg.app_data(web::Data::new(WebDir(dir.clone())));
        cfg.raw().default_service(web::to(serve));
    }
}
