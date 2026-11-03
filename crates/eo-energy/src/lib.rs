//! 能源營運的 plugin。每個領域一個 plugin，各自登記路由（前綴 `/api/v1/orgs/{orgId}/energy`）。
//!
//! 寫法約定：
//! · handler 先用 `core.rbac.authorize_org(&user, org_id, gates::XXX)` 拿 `OrgAccess`，
//!   之後的資料存取函式一律收 `&OrgAccess`，org 條件從它取，不收裸的 org id。
//! · 掛在 execution 底下的資源再過 `ExecutionAccess`（參與者或 view-all）。
//! · 業務變更與稽核寫在同一個交易；授權變更用 `Rbac::commit` 提交。

pub mod fake_tpr;
pub mod gates;
pub mod tpr_client;

use eo_platform::core::CoreRef;
use eo_platform::plugin::AppPluginSystem;

/// 能源 API 的前綴。
pub const PREFIX: &str = "/api/v1/orgs/{orgId}/energy";

/// 組出完整路徑：`path("/departments")` → `/api/v1/orgs/{orgId}/energy/departments`。
pub fn path(p: &str) -> String {
    format!("{PREFIX}{p}")
}

/// 把能源的 plugin 註冊進 system。
pub fn register(_system: &mut AppPluginSystem, _core: &CoreRef) {}
