//! 物件儲存：開機時確認 bucket 存在（不在就建）。

use crate::azldi::Plugin;
use crate::core::CoreRef;
use crate::plugin::{AppPlugin, StartContext, StartError};
use async_trait::async_trait;

pub struct StoragePlugin {
    core: CoreRef,
}

impl StoragePlugin {
    pub fn new(core: CoreRef) -> Self {
        Self { core }
    }
}

impl Plugin for StoragePlugin {
    fn name(&self) -> &str {
        "storage"
    }
    crate::plugin_boilerplate!();
}

#[async_trait]
impl AppPlugin for StoragePlugin {
    async fn on_start(&self, _ctx: &StartContext) -> Result<(), StartError> {
        self.core
            .storage
            .ensure_bucket()
            .await
            .map_err(|e| StartError::new(format!("storage not reachable: {e}")))?;
        Ok(())
    }
}
