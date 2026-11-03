//! 所有 plugin 共用的基礎設施：設定、連線池、物件儲存、時鐘。

use crate::clock::Clock;
use crate::config::Config;
use crate::rbac::Rbac;
use crate::storage::Storage;
use sqlx::PgPool;
use std::sync::Arc;

pub struct Core {
    pub config: Config,
    pub pool: PgPool,
    pub storage: Storage,
    pub clock: Arc<dyn Clock>,
    pub rbac: Rbac,
}

pub type CoreRef = Arc<Core>;

impl Core {
    /// 連線池用 lazy：建構不需要 async，第一次用到才連（`database` plugin 開機時會先連一次）。
    pub fn new(config: Config, clock: Arc<dyn Clock>) -> Result<Self, String> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(16)
            .connect_lazy(&config.database_url)
            .map_err(|e| format!("invalid database url: {e}"))?;
        let storage = Storage::new(config.s3.clone());
        let rbac = Rbac::new(pool.clone());
        Ok(Self { config, pool, storage, clock, rbac })
    }
}
