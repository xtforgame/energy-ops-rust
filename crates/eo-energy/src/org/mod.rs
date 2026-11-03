//! 組織治理：部門與部門成員、能源角色指派、成員清單與移除。宣告能源 RBAC seed。

pub mod departments;
pub mod members;
pub mod roles;

use crate::path;
use actix_web::web;
use eo_domain::rbac::RbacSeeds;
use eo_platform::plugin::{AppPlugin, RouteRegistrar};
use std::sync::Arc;

pub struct OrgPlugin;

crate::energy_plugin!(OrgPlugin, "energy_org");

impl AppPlugin for OrgPlugin {
    fn configure_routes(&self, cfg: &mut RouteRegistrar<'_>, _self_arc: &Arc<dyn AppPlugin>) {
        cfg.get(&path("/departments"), web::get().to(departments::list));
        cfg.post(&path("/departments"), web::post().to(departments::create));
        cfg.patch(&path("/departments/{departmentId}"), web::patch().to(departments::update));
        cfg.delete(&path("/departments/{departmentId}"), web::delete().to(departments::delete));
        cfg.put(&path("/departments/{departmentId}/members/{userId}"), web::put().to(departments::set_member));
        cfg.delete(&path("/departments/{departmentId}/members/{userId}"), web::delete().to(departments::remove_member));

        cfg.get(&path("/members"), web::get().to(members::list));
        cfg.delete(&path("/members/{userId}"), web::delete().to(members::remove));
        cfg.get(&path("/assignable-roles"), web::get().to(roles::assignable));
        cfg.post(&path("/role-assignments"), web::post().to(roles::assign));
        cfg.post(&path("/role-assignments/revoke"), web::post().to(roles::revoke));
    }

    /// 能源的 permission／complex／角色；rbac plugin 開機時冪等寫入。
    fn rbac_seeds(&self) -> Option<RbacSeeds> {
        Some(eo_domain::energy_rbac::energy_rbac_seeds())
    }
}
