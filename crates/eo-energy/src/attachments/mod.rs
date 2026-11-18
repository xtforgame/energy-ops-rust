//! 流程附件：上傳（multipart 或直傳後的 staging key）、清單、下載、解除連結、直傳簽名。
//!
//! 可視度一律等於 execution 可視度（`authorize_execution`）；收件批次的原檔只能走組織層的下載權
//! （那時 execution 還不存在）。

pub mod handlers;
pub mod service;
pub mod staged;

use crate::path;
use actix_web::web;
use eo_platform::plugin::{AppPlugin, RouteRegistrar};
use std::sync::Arc;

pub struct AttachmentsPlugin;

crate::energy_plugin!(AttachmentsPlugin, "energy_attachments");

impl AppPlugin for AttachmentsPlugin {
    fn configure_routes(&self, cfg: &mut RouteRegistrar<'_>, _self_arc: &Arc<dyn AppPlugin>) {
        cfg.get(&path("/executions/{executionId}/attachments"), web::get().to(handlers::list_for_execution));
        cfg.post(&path("/executions/{executionId}/attachments"), web::post().to(handlers::upload_to_execution));
        cfg.get(&path("/attachments/{attachmentId}/download"), web::get().to(handlers::download));
        cfg.post(&path("/attachment-links/{linkId}/remove"), web::post().to(handlers::remove_link));
        cfg.patch(&path("/attachments/{attachmentId}/category"), web::patch().to(handlers::set_category));
        cfg.post(&path("/uploads/sign"), web::post().to(handlers::sign_upload));
    }
}
