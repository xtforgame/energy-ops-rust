//! 流程：定義與取版、引擎、可視度、模式判定、清單、流程圖與總覽。
//!
//! 給其他模組（收件、計算）用的入口：`start_execution`、`resolve_flow_rows`、
//! `authorize_execution`、`mode_for`。

pub mod access;
pub mod definition;
pub mod engine;
pub mod graph;
pub mod handlers;
pub mod list;
pub mod mode;
pub mod model;
pub mod subjects;
pub mod views;

pub use access::{authorize_execution, ExecutionAccess};
pub use definition::{install_builtin_definitions, resolve_definition, resolve_flow_rows, ResolvedFlow};
pub use engine::run::{start_execution, StartExecution};
pub use mode::{mode_for, ModeOut};

use crate::path;
use actix_web::web;
use eo_platform::plugin::{AppPlugin, RouteRegistrar};
use handlers as h;
use std::sync::Arc;

pub struct WorkflowPlugin;

crate::energy_plugin!(WorkflowPlugin, "energy_workflow");

impl AppPlugin for WorkflowPlugin {
    fn configure_routes(&self, cfg: &mut RouteRegistrar<'_>, _self_arc: &Arc<dyn AppPlugin>) {
        cfg.get(&path("/executions"), web::get().to(h::list_executions));
        cfg.post(&path("/executions"), web::post().to(h::start_execution));
        cfg.get(&path("/executions/{executionIdOrNo}"), web::get().to(h::get_execution));
        cfg.get(&path("/executions/{executionId}/actions"), web::get().to(h::node_actions));
        cfg.get(&path("/executions/{executionId}/graph"), web::get().to(h::execution_graph));
        cfg.get(&path("/executions/{executionId}/revertable"), web::get().to(h::revertable));
        cfg.get(&path("/executions/{executionId}/lineage"), web::get().to(h::lineage_trace));
        cfg.post(&path("/executions/{executionId}/abort"), web::post().to(h::request_abort));
        cfg.post(&path("/executions/{executionId}/force-advance"), web::post().to(h::force_advance));
        cfg.post(&path("/executions/{executionId}/terminate"), web::post().to(h::terminate));
        cfg.post(&path("/tasks/{taskId}/claim"), web::post().to(h::claim));
        cfg.post(&path("/tasks/{taskId}/release"), web::post().to(h::release));
        cfg.post(&path("/tasks/{taskId}/actions"), web::post().to(h::perform_action));
        cfg.post(&path("/tasks/{taskId}/abort-decision"), web::post().to(h::decide_abort));
        cfg.post(&path("/tasks/{taskId}/reassign"), web::post().to(h::reassign));
        cfg.post(&path("/side-effects/{sideEffectId}/release"), web::post().to(h::release_side_effect));
        cfg.get(&path("/delegations"), web::get().to(h::list_delegations));
        cfg.post(&path("/delegations"), web::post().to(h::create_delegation));
        cfg.post(&path("/delegations/{delegationId}/revoke"), web::post().to(h::revoke_delegation));
        cfg.post(&path("/lineage"), web::post().to(h::link_lineage));
        cfg.get(&path("/workflows"), web::get().to(h::workflows));
        cfg.get(&path("/workflows/{definitionId}/graph"), web::get().to(h::workflow_graph));
        cfg.get(&path("/workflows/{definitionId}/nodes/{nodeKey}/tasks"), web::get().to(h::workflow_node_tasks));
        cfg.post(&path("/setup/install-defaults"), web::post().to(h::install_defaults));
    }
}
