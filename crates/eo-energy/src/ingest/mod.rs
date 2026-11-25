//! 收件：批次、項目、電費單（三層值）、送審去重與開流程、辨識驅動（背景對帳＋租約）。
//! 開機時另外負責示範資料（`EO_DEMO_SEED=1`）。

pub mod bills;
pub mod handlers;
pub mod recognition;
pub mod rent_payment;
pub mod rent_rules;
pub mod route;
pub mod service;

use crate::path;
use actix_web::web;
use eo_platform::core::CoreRef;
use eo_platform::plugin::{AppPlugin, RouteRegistrar, StartContext, StartError};
use handlers as h;
use std::sync::{Arc, Mutex};

pub struct IngestPlugin {
    core: CoreRef,
    background: Mutex<Option<actix_web::rt::task::JoinHandle<()>>>,
}

impl IngestPlugin {
    pub fn new(core: CoreRef) -> Self {
        Self { core, background: Mutex::new(None) }
    }
}

impl eo_platform::azldi::Plugin for IngestPlugin {
    fn name(&self) -> &str {
        "energy_ingest"
    }
    eo_platform::plugin_boilerplate!();

    fn deps(&self, method: &str) -> Vec<&str> {
        match method {
            // 示範資料要用到 migration、RBAC seed 與 bucket。
            "start" => vec!["database", "rbac", "storage"],
            _ => vec![],
        }
    }
}

#[async_trait::async_trait]
impl AppPlugin for IngestPlugin {
    fn configure_routes(&self, cfg: &mut RouteRegistrar<'_>, _self_arc: &Arc<dyn AppPlugin>) {
        cfg.post(&path("/bills/ingest"), web::post().to(h::ingest_bill));
        cfg.get(&path("/stations/{stationId}/rent-payment"), web::get().to(rent_payment::get_station));
        cfg.get(&path("/stations/{stationId}/rent-rules"), web::get().to(rent_rules::list));
        cfg.post(&path("/stations/{stationId}/rent-rules"), web::post().to(rent_rules::save));
        cfg.post(&path("/stations/{stationId}/rent-payment/voucher"), web::post().to(rent_payment::save_voucher));
        cfg.get(&path("/bills"), web::get().to(h::list_bills));
        // 要排在 /bills/{billId} 之前，否則 'unmatched' 會被當成 id。
        cfg.get(&path("/bills/unmatched"), web::get().to(h::list_unmatched));
        cfg.get(&path("/bills/{billId}"), web::get().to(h::get_bill));
        cfg.patch(&path("/bills/{billId}"), web::patch().to(h::patch_bill));
        cfg.get(&path("/bills/{billId}/branch-preview"), web::get().to(h::branch_preview));
        cfg.get(&path("/bills/{billId}/payouts"), web::get().to(h::bill_payouts));
        cfg.get(&path("/bills/{billId}/easyflow-application"), web::get().to(h::easyflow_application));
        cfg.post(&path("/bills/{billId}/easyflow-application"), web::post().to(h::save_easyflow_application));
        cfg.get(&path("/bills/{billId}/rent-payment"), web::get().to(rent_payment::get));
        cfg.post(&path("/bills/{billId}/rent-payment"), web::post().to(rent_payment::save));
        cfg.post(&path("/bills/{billId}/submit"), web::post().to(h::submit_bill));
        cfg.post(&path("/bills/{billId}/match"), web::post().to(h::match_bill));
        cfg.get(&path("/bills/{billId}/match-candidates"), web::get().to(h::match_candidates));
        cfg.post(&path("/bills/{billId}/rematch"), web::post().to(h::rematch_bill));
        cfg.post(&path("/ingest-batches"), web::post().to(h::create_batch));
        cfg.get(&path("/ingest-batches"), web::get().to(h::list_batches));
        cfg.get(&path("/ingest-batches/{batchId}"), web::get().to(h::get_batch));
        cfg.post(&path("/ingest-batches/{batchId}/attachments"), web::post().to(h::upload_batch_attachment));
        cfg.post(&path("/ingest-batches/{batchId}/recognize"), web::post().to(h::recognize));
        cfg.patch(&path("/ingest-items/{itemId}"), web::patch().to(h::patch_item));
        cfg.post(&path("/ingest-items/{itemId}/recognition"), web::post().to(h::supply_recognition));
        cfg.post(&path("/ingest-items/{itemId}/submit"), web::post().to(h::submit_item));
        cfg.post(&path("/ingest-items/{itemId}/discard"), web::post().to(h::discard_item));
        cfg.get(&path("/recognition/status"), web::get().to(h::recognition_status));
    }

    async fn on_start(&self, _ctx: &StartContext) -> Result<(), StartError> {
        if self.core.config.demo_seed {
            let report = crate::demo_seed::run(&self.core).await.map_err(|e| StartError::new(format!("demo seed: {e}")))?;
            log::info!("[energy] demo seed: org #{} ({})", report.org_id, report.org_unique_name);
            if self.core.config.demo_sample_bills {
                let dir = self.core.config.demo_samples_dir.clone();
                let s = crate::demo_seed::run_sample_bills(&self.core, &report, &dir)
                    .await
                    .map_err(|e| StartError::new(format!("demo sample bills: {e}")))?;
                log::info!("[energy] demo sample bills: {s:?}");
            }
        }
        let secs = self.core.config.recognition_tick_secs;
        if secs > 0 && recognition::client(&self.core).is_some() {
            *self.background.lock().unwrap() = Some(recognition::spawn_background(Arc::clone(&self.core), secs));
            log::info!("[energy] recognition reconcile every {secs}s");
        }
        Ok(())
    }

    async fn on_destroy(&self) {
        if let Some(h) = self.background.lock().unwrap().take() {
            h.abort();
        }
    }
}
