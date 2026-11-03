//! 能源營運的 plugin。每個領域一個 plugin，各自登記路由（前綴 `/api/v1/orgs/{orgId}/energy`）。
//!
//! 寫法約定：
//! · handler 先用 `core.rbac.authorize_org(&user, org_id, gates::XXX)` 拿 `OrgAccess`，
//!   之後的資料存取函式一律收 `&OrgAccess`（或包著它的 `Ctx`），org 條件從它取，不收裸的 org id。
//! · 掛在 execution 底下的資源再過 `ExecutionAccess`（參與者或 view-all）。
//! · 業務變更與稽核寫在同一個交易；授權變更用 `Rbac::commit` 提交。

// 查詢結果多半就地解成 tuple 用一次，不另外宣告列型別；交易＋情境＋業務參數常超過七個。
#![allow(clippy::type_complexity, clippy::too_many_arguments)]

pub mod audit;
pub mod bindings;
pub mod common;
pub mod demo_seed;
pub mod fake_tpr;
pub mod gates;
pub mod master_data;
pub mod org;
pub mod rating;
pub mod tpr_client;
pub mod workbench;
pub mod workflow;

use eo_platform::core::CoreRef;
use eo_platform::plugin::AppPluginSystem;

/// 能源 API 的前綴。
pub const PREFIX: &str = "/api/v1/orgs/{orgId}/energy";

/// 組出完整路徑：`path("/departments")` → `/api/v1/orgs/{orgId}/energy/departments`。
pub fn path(p: &str) -> String {
    format!("{PREFIX}{p}")
}

/// 每個能源 plugin 的 `Plugin` 實作都一樣：名字＋downcast。
#[macro_export]
macro_rules! energy_plugin {
    ($ty:ty, $name:literal) => {
        impl ::eo_platform::azldi::Plugin for $ty {
            fn name(&self) -> &str {
                $name
            }
            ::eo_platform::plugin_boilerplate!();
        }
    };
}

/// 把能源的 plugin 註冊進 system。
pub fn register(system: &mut AppPluginSystem, _core: &CoreRef) {
    system.register_new(org::OrgPlugin);
    system.register_new(audit::AuditPlugin);
    system.register_new(master_data::MasterDataPlugin);
    system.register_new(bindings::BindingsPlugin);
    system.register_new(workflow::WorkflowPlugin);
    system.register_new(workbench::WorkbenchPlugin);
}
