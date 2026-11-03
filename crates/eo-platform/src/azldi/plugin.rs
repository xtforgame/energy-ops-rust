use std::any::Any;
use std::sync::Arc;

// ────────────────────────────────────────────────────────────
// PluginRegistry — 在 digest (init) 階段，提供已初始化 plugin 的查詢
// ────────────────────────────────────────────────────────────

/// 在 `init()` 階段，只包含「排在自己前面、已經 init 完畢」的 plugin。
/// 透過 `get_as::<T>()` 取得具體類型的 Arc，可存進 plugin 自身的 OnceLock 供後續使用。
pub struct PluginRegistry {
    plugins: Vec<Arc<dyn Plugin>>,
}

impl PluginRegistry {
    pub fn new(plugins: Vec<Arc<dyn Plugin>>) -> Self {
        Self { plugins }
    }

    /// 取得 registry 中所有 plugin 的名稱。
    pub fn names(&self) -> Vec<&str> {
        self.plugins.iter().map(|p| p.name()).collect()
    }

    /// 依 plugin name 取得 `Arc<dyn Plugin>`。
    pub fn get(&self, name: &str) -> Option<Arc<dyn Plugin>> {
        self.plugins.iter().find(|p| p.name() == name).cloned()
    }

    /// 依 plugin name 取得具體類型 `Arc<T>`。
    /// 內部透過 `as_any_arc()` + `downcast` 實現。
    ///
    /// 用法：
    /// ```ignore
    /// let health = registry.get_as::<HealthPlugin>("health").unwrap();
    /// self.health.set(health).ok();
    /// ```
    pub fn get_as<T: Plugin + 'static>(&self, name: &str) -> Option<Arc<T>> {
        self.get(name)
            .and_then(|p| p.as_any_arc().downcast::<T>().ok())
    }
}

// ────────────────────────────────────────────────────────────
// Plugin trait — azldi 的基底 trait
// ────────────────────────────────────────────────────────────

/// azldi 的基底 trait，只負責：
/// - 身份識別（name）
/// - 依賴宣告（init_deps / deps / run_before）
/// - 初始化（init）
///
/// 不包含任何業務方法（route、start 等）。
/// 業務方法由衍生 trait（如 `AppPlugin`）定義，
/// 讓衍生層決定具體的方法簽名和型別。
///
/// # 設計原則
/// - `Send + Sync + 'static`：plugin 會透過 `Arc` 跨 thread 共享
/// - 可變狀態使用 interior mutability（OnceLock、AtomicU64、RwLock 等）
pub trait Plugin: Send + Sync + 'static {
    /// Plugin 的唯一名稱，用於依賴宣告和查詢。
    fn name(&self) -> &str;

    /// 讓 `Arc<dyn Plugin>` 能 downcast 回具體類型。
    /// 每個 plugin 只需寫 `self` 即可：
    /// ```ignore
    /// fn as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> { self }
    /// ```
    fn as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync>;

    /// 初始化依賴（等同 TS 的 `$inject`）。
    /// 宣告在這裡的 plugin 會在 `digest()` 時先完成 init，
    /// 確保 `init()` 被呼叫時可以透過 registry 取得它們。
    fn init_deps(&self) -> Vec<&str> {
        vec![]
    }

    /// 在 `digest()` 階段被呼叫，所有 `init_deps` 中的 plugin 已初始化。
    /// 用 `registry.get_as::<T>()` 取得依賴的 plugin reference 並存起來。
    fn init(&self, _registry: &PluginRegistry) {}

    /// 方法級依賴（等同 TS 的 `$funcDeps`）。
    /// 宣告「當執行 `method` 時，我依賴這些 plugin 先跑完」。
    /// `method` 是由衍生層定義的字串 key（如 "start"、"configure_routes"）。
    fn deps(&self, _method: &str) -> Vec<&str> {
        vec![]
    }

    /// 方法級 run-before（等同 TS 的 `$runBefore`）。
    /// 宣告「當執行 `method` 時，我要在這些 plugin 之前跑」。
    fn run_before(&self, _method: &str) -> Vec<&str> {
        vec![]
    }
}
