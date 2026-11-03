//! `GET /health`：路由清單、DB 與物件儲存是否可用。

use crate::azldi::Plugin;
use crate::core::CoreRef;
use crate::plugin::{AppPlugin, RouteRegistrar, RouteTable};
use actix_web::{web, HttpResponse};
use std::sync::Arc;

pub struct HealthPlugin {
    routes: Arc<RouteTable>,
}

impl HealthPlugin {
    pub fn new(routes: Arc<RouteTable>) -> Self {
        Self { routes }
    }
}

struct Routes(Arc<RouteTable>);

async fn health(core: web::Data<CoreRef>, routes: web::Data<Routes>) -> HttpResponse {
    let db = sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&core.pool).await.is_ok();
    let storage = core.storage.ensure_bucket().await.is_ok();
    let ok = db && storage;
    let body = serde_json::json!({
        "status": if ok { "ok" } else { "degraded" },
        "database": if db { "ok" } else { "down" },
        "storage": if storage { "ok" } else { "down" },
        "routes": routes.0.paths(),
    });
    if ok {
        HttpResponse::Ok().json(body)
    } else {
        HttpResponse::ServiceUnavailable().json(body)
    }
}

impl Plugin for HealthPlugin {
    fn name(&self) -> &str {
        "health"
    }
    crate::plugin_boilerplate!();
}

impl AppPlugin for HealthPlugin {
    fn configure_routes(&self, cfg: &mut RouteRegistrar<'_>, _self_arc: &Arc<dyn AppPlugin>) {
        cfg.app_data(web::Data::new(Routes(Arc::clone(&self.routes))));
        cfg.get("/health", web::get().to(health));
    }
}
