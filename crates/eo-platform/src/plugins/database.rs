//! 連線池與 migration：開機時先連一次、套用所有 migration（編進 binary）。

use crate::azldi::Plugin;
use crate::core::CoreRef;
use crate::plugin::{AppPlugin, StartContext, StartError};
use async_trait::async_trait;
use sqlx::migrate::Migrator;

pub struct DatabasePlugin {
    core: CoreRef,
    migrator: &'static Migrator,
}

impl DatabasePlugin {
    pub fn new(core: CoreRef, migrator: &'static Migrator) -> Self {
        Self { core, migrator }
    }
}

impl Plugin for DatabasePlugin {
    fn name(&self) -> &str {
        "database"
    }
    crate::plugin_boilerplate!();
}

#[async_trait]
impl AppPlugin for DatabasePlugin {
    async fn on_start(&self, _ctx: &StartContext) -> Result<(), StartError> {
        self.migrator
            .run(&self.core.pool)
            .await
            .map_err(|e| StartError::new(format!("migration failed: {e}")))?;
        log::info!("[database] migrations applied ({} total)", self.migrator.iter().count());
        Ok(())
    }

    async fn on_destroy(&self) {
        self.core.pool.close().await;
    }
}
