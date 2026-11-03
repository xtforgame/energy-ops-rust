//! 組裝根：決定有哪些 plugin、怎麼起服務。服務與測試共用同一個組裝。

use actix_web::web;
use eo_platform::core::CoreRef;
use eo_platform::plugin::AppPluginSystem;
use eo_platform::plugins::{
    authn::AuthnPlugin, database::DatabasePlugin, health::HealthPlugin, multitenancy::MultitenancyPlugin, rbac::RbacPlugin,
    static_web::StaticWebPlugin, storage::StoragePlugin, user::UserPlugin,
};
use sqlx::migrate::Migrator;
use std::sync::Arc;

/// 所有 migration 編進 binary，開機時由 database plugin 套用。
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// 註冊全部 plugin。
pub fn build_system(core: &CoreRef) -> AppPluginSystem {
    let mut system = AppPluginSystem::new();
    system.register_new(DatabasePlugin::new(Arc::clone(core), &MIGRATOR));
    system.register_new(StoragePlugin::new(Arc::clone(core)));
    system.register_new(RbacPlugin::new(Arc::clone(core)));
    system.register_new(AuthnPlugin::new(Arc::clone(core)));
    system.register_new(UserPlugin);
    system.register_new(MultitenancyPlugin);
    eo_energy::register(&mut system, core);
    system.register_new(HealthPlugin::new(system.route_table()));
    system.register_new(StaticWebPlugin::new(core.config.web_dir.clone()));
    system
}

/// 一個 actix App 的設定：共用資料、extractor 錯誤格式、所有 plugin 的路由。
pub fn configure_app(cfg: &mut web::ServiceConfig, system: &AppPluginSystem, core: &CoreRef) {
    cfg.app_data(web::Data::new(Arc::clone(core)))
        .app_data(eo_platform::error::json_config())
        .app_data(eo_platform::error::query_config())
        .app_data(eo_platform::error::path_config());
    system.configure_routes(cfg);
}

/// digest＋開機（migration、seed、bucket……）。
pub async fn start(core: &CoreRef) -> Result<Arc<AppPluginSystem>, String> {
    let system = build_system(core);
    system.digest().map_err(|e| e.to_string())?;
    let system = Arc::new(system);
    system.start().await.map_err(|e| e.to_string())?;
    Ok(system)
}
