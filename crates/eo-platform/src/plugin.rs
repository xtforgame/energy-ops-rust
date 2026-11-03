//! 建在 azldi 上的 AppPlugin：每個 plugin 管自己的路由、開機工作、RBAC seed 與背景任務。
//! 形狀照 ryxis-core `src/plugins/mod.rs`，多了 `rbac_seeds` 一個 hook。

use crate::azldi::{Azldi, AzldiError, Plugin};
use actix_web::web;
use async_trait::async_trait;
use eo_domain::rbac::RbacSeeds;
use std::fmt;
use std::sync::{Arc, RwLock};

/// 真的註冊上去的路徑（集合）。health 列出它，少註冊一條這裡就少一條。
#[derive(Default)]
pub struct RouteTable {
    paths: RwLock<Vec<String>>,
}

impl RouteTable {
    pub fn new() -> Self {
        Self::default()
    }

    fn record(&self, path: &str) {
        let mut v = self.paths.write().unwrap_or_else(|e| e.into_inner());
        // configure_routes 每個 actix worker 各跑一次，同一路徑會來很多次。
        if !v.iter().any(|p| p == path) {
            v.push(path.to_string());
        }
    }

    pub fn paths(&self) -> Vec<String> {
        let mut v = self.paths.read().unwrap_or_else(|e| e.into_inner()).clone();
        v.sort();
        v
    }
}

/// 包一個 `ServiceConfig`，只開放「登記路由」與「放 app_data」。
pub struct RouteRegistrar<'a> {
    cfg: &'a mut web::ServiceConfig,
    table: &'a RouteTable,
}

impl<'a> RouteRegistrar<'a> {
    pub fn new(cfg: &'a mut web::ServiceConfig, table: &'a RouteTable) -> Self {
        Self { cfg, table }
    }

    /// 登記一條路由。`path` 是完整路徑（含 `/api/v1`）；登記簿記成 `METHOD path`。
    pub fn route(&mut self, method: &str, path: &str, route: actix_web::Route) {
        self.table.record(&format!("{method} {path}"));
        self.cfg.route(path, route);
    }

    pub fn get(&mut self, path: &str, route: actix_web::Route) {
        self.route("GET", path, route)
    }
    pub fn post(&mut self, path: &str, route: actix_web::Route) {
        self.route("POST", path, route)
    }
    pub fn patch(&mut self, path: &str, route: actix_web::Route) {
        self.route("PATCH", path, route)
    }
    pub fn put(&mut self, path: &str, route: actix_web::Route) {
        self.route("PUT", path, route)
    }
    pub fn delete(&mut self, path: &str, route: actix_web::Route) {
        self.route("DELETE", path, route)
    }

    pub fn app_data<U: 'static>(&mut self, data: U) {
        self.cfg.app_data(data);
    }

    /// 給 static_web 掛 fallback 用。
    pub fn raw(&mut self) -> &mut web::ServiceConfig {
        self.cfg
    }
}

#[derive(Debug, Clone)]
pub struct StartError(String);

impl StartError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for StartError {}

/// `on_start` 拿到的東西：整個 system（例如 rbac 要收集所有 plugin 的 seed）。
pub struct StartContext {
    pub system: Arc<AppPluginSystem>,
}

#[async_trait]
pub trait AppPlugin: Plugin {
    /// 登記 HTTP 路由。每個 actix worker 呼叫一次，不要在這裡做有副作用的事。
    fn configure_routes(&self, _cfg: &mut RouteRegistrar<'_>, _self_arc: &Arc<dyn AppPlugin>) {}

    /// 開機（listen 之前），依 `deps("start")` 排序。回 Err 就起不來。
    async fn on_start(&self, _ctx: &StartContext) -> Result<(), StartError> {
        Ok(())
    }

    /// 關機（server 關掉之後），依 `deps("destroy")` 排序。
    async fn on_destroy(&self) {}

    /// 本 plugin 宣告的 permission 與角色；rbac plugin 開機時收集。
    fn rbac_seeds(&self) -> Option<RbacSeeds> {
        None
    }
}

pub struct AppPluginSystem {
    azldi: Azldi,
    plugins: Vec<Arc<dyn AppPlugin>>,
    routes: Arc<RouteTable>,
}

impl Default for AppPluginSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl AppPluginSystem {
    pub fn new() -> Self {
        Self { azldi: Azldi::new(), plugins: Vec::new(), routes: Arc::new(RouteTable::new()) }
    }

    pub fn route_table(&self) -> Arc<RouteTable> {
        Arc::clone(&self.routes)
    }

    pub fn register<P: AppPlugin + 'static>(&mut self, plugin: Arc<P>) {
        self.azldi.register(Arc::clone(&plugin) as Arc<dyn Plugin>);
        self.plugins.push(plugin);
    }

    pub fn register_new<P: AppPlugin + 'static>(&mut self, plugin: P) {
        self.register(Arc::new(plugin));
    }

    pub fn digest(&self) -> Result<(), AzldiError> {
        self.azldi.digest()
    }

    pub fn get_as<T: Plugin + 'static>(&self, name: &str) -> Option<Arc<T>> {
        self.azldi.get_as::<T>(name)
    }

    pub fn plugin_names(&self) -> Vec<String> {
        self.plugins.iter().map(|p| p.name().to_string()).collect()
    }

    /// 所有 plugin 宣告的 seed（註冊順序）。
    pub fn collect_rbac_seeds(&self) -> Vec<RbacSeeds> {
        self.plugins.iter().filter_map(|p| p.rbac_seeds()).collect()
    }

    pub fn configure_routes(&self, cfg: &mut web::ServiceConfig) {
        let table = Arc::clone(&self.routes);
        self.azldi
            .for_each_in_order("configure_routes", |idx, _| {
                let plugin = &self.plugins[idx];
                let mut reg = RouteRegistrar { cfg, table: &table };
                plugin.configure_routes(&mut reg, plugin);
            })
            .expect("configure_routes 的依賴有環");
    }

    /// 依序呼叫 `on_start`；第一個失敗就停，後面的不呼叫。
    pub async fn start(self: &Arc<Self>) -> Result<(), StartError> {
        let ctx = StartContext { system: Arc::clone(self) };
        let order = self.azldi.resolve_order("start").map_err(|e| StartError::new(e.to_string()))?;
        for &idx in &order {
            let plugin = &self.plugins[idx];
            let name = plugin.name();
            log::info!("[{name}] starting");
            plugin
                .on_start(&ctx)
                .await
                .map_err(|e| StartError::new(format!("plugin '{name}' failed to start: {e}")))?;
        }
        Ok(())
    }

    pub async fn destroy(&self) {
        let Ok(order) = self.azldi.resolve_order("destroy") else { return };
        for &idx in &order {
            self.plugins[idx].on_destroy().await;
        }
    }
}

/// 每個 plugin 都要寫的 `as_any_arc`。
#[macro_export]
macro_rules! plugin_boilerplate {
    () => {
        fn as_any_arc(self: ::std::sync::Arc<Self>) -> ::std::sync::Arc<dyn ::std::any::Any + Send + Sync> {
            self
        }
    };
}
