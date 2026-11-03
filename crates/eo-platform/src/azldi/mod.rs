//! # azldi — 輕量 Plugin 依賴管理框架
//!
//! 靈感來自 TypeScript 的 [azldi](https://github.com/xtforgame/azldi)，
//! 以 Rust 的 trait + Arc 重新設計。
//!
//! ## 職責
//! - 管理 plugin 的註冊與查詢
//! - 依據 plugin 宣告的依賴關係進行拓撲排序
//! - 提供 `digest()`：按依賴順序初始化所有 plugin
//! - 提供 `resolve_order(method)`：讓衍生層按正確順序呼叫業務方法
//!
//! ## 不負責
//! - 不定義任何業務方法（route、start 等）
//! - 不做方法分派（沒有 `run("methodName")`）
//!
//! 業務方法由衍生的 trait（如 `AppPlugin`）和系統（如 `AppPluginSystem`）處理。

// 原樣搬自 ryxis-core；不為 clippy 改動它的寫法。
#![allow(clippy::new_without_default, clippy::type_complexity)]

mod plugin;

pub use plugin::{Plugin, PluginRegistry};

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::Arc;

// ── Error ──

/// azldi 的錯誤類型。
#[derive(Debug)]
pub enum AzldiError {
    /// 偵測到循環依賴，附帶參與循環的 plugin 名稱。
    CircularDependency(Vec<String>),
    /// plugin 宣告的依賴不存在（未註冊）。
    DependencyNotFound { plugin: String, dep: String },
}

impl fmt::Display for AzldiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AzldiError::CircularDependency(names) => {
                write!(f, "Circular dependency among: {}", names.join(", "))
            }
            AzldiError::DependencyNotFound { plugin, dep } => {
                write!(f, "Plugin '{plugin}' depends on '{dep}', which is not registered")
            }
        }
    }
}

impl std::error::Error for AzldiError {}

// ── Azldi ──

/// Plugin 容器，負責註冊、排序、初始化。
///
/// 典型用法：
/// ```ignore
/// let mut azldi = Azldi::new();
/// azldi.register(Arc::new(HealthPlugin::new()));
/// azldi.register(Arc::new(UserPlugin::new(db)));
/// azldi.digest()?;  // 按 init_deps 順序呼叫每個 plugin 的 init()
/// ```
///
/// 衍生層（如 `AppPluginSystem`）包裝 Azldi，並透過 `resolve_order(method)`
/// 取得拓撲排序後的索引，再依序呼叫各 plugin 的具體方法。
pub struct Azldi {
    /// 所有已註冊的 plugin，以 Arc 共享（跨 thread 安全）。
    plugins: Vec<Arc<dyn Plugin>>,
}

impl Azldi {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// 註冊單一 plugin（傳入 Arc）。
    pub fn register(&mut self, plugin: Arc<dyn Plugin>) {
        self.plugins.push(plugin);
    }

    /// 註冊單一 plugin（自動包 Arc）。
    ///
    /// 便利方法，適合不需要保留 Arc reference 的場景：
    /// ```ignore
    /// azldi.register_new(MyPlugin::new());
    /// ```
    pub fn register_new<P: Plugin + 'static>(&mut self, plugin: P) {
        self.plugins.push(Arc::new(plugin));
    }

    /// 批次註冊多個 plugin。
    pub fn register_many(&mut self, plugins: Vec<Arc<dyn Plugin>>) {
        self.plugins.extend(plugins);
    }

    /// 取得所有已註冊 plugin 的 slice（唯讀）。
    pub fn plugins(&self) -> &[Arc<dyn Plugin>] {
        &self.plugins
    }

    /// 依 name 查詢 plugin（回傳 `Arc<dyn Plugin>`）。
    pub fn get(&self, name: &str) -> Option<Arc<dyn Plugin>> {
        self.plugins.iter().find(|p| p.name() == name).cloned()
    }

    /// 依 name 查詢並 downcast 到具體類型。
    pub fn get_as<T: Plugin + 'static>(&self, name: &str) -> Option<Arc<T>> {
        self.get(name)
            .and_then(|p| p.as_any_arc().downcast::<T>().ok())
    }

    /// 拓撲排序的核心實作（Kahn's algorithm）。
    ///
    /// 接受兩個 closure 來提取每個 plugin 的依賴資訊：
    /// - `get_deps`：plugin 依賴誰（它們要先跑）
    /// - `get_run_before`：plugin 要在誰之前跑
    ///
    /// 回傳排序後的 plugin 索引（對應 `self.plugins` 的位置）。
    fn topo_sort(
        &self,
        get_deps: impl Fn(&dyn Plugin) -> Vec<&str>,
        get_run_before: impl Fn(&dyn Plugin) -> Vec<&str>,
    ) -> Result<Vec<usize>, AzldiError> {
        let n = self.plugins.len();

        // 建立 name → index 對照表
        let name_to_idx: HashMap<&str, usize> = self
            .plugins
            .iter()
            .enumerate()
            .map(|(i, p)| (p.name(), i))
            .collect();

        // adj[j] 包含 j 的所有後繼節點
        let mut adj: Vec<Vec<usize>> = vec![vec![]; n];
        // 每個節點的入度（被多少人依賴）
        let mut in_degree = vec![0usize; n];

        for (i, plugin) in self.plugins.iter().enumerate() {
            // deps：plugin i 依賴 dep → dep 必須在 i 之前
            // 建邊 dep → i
            for dep in get_deps(plugin.as_ref()) {
                match name_to_idx.get(dep) {
                    Some(&j) => {
                        adj[j].push(i);
                        in_degree[i] += 1;
                    }
                    None => {
                        return Err(AzldiError::DependencyNotFound {
                            plugin: plugin.name().to_string(),
                            dep: dep.to_string(),
                        });
                    }
                }
            }

            // run_before：plugin i 要在 target 之前跑
            // 建邊 i → target
            // 如果 target 未註冊則靜默忽略（它可能不參與此方法）
            for target in get_run_before(plugin.as_ref()) {
                if let Some(&j) = name_to_idx.get(target) {
                    adj[i].push(j);
                    in_degree[j] += 1;
                }
            }
        }

        // BFS：從入度為 0 的節點開始
        let mut queue: VecDeque<usize> = in_degree
            .iter()
            .enumerate()
            .filter(|(_, &d)| d == 0)
            .map(|(i, _)| i)
            .collect();

        let mut order = Vec::with_capacity(n);
        while let Some(node) = queue.pop_front() {
            order.push(node);
            for &next in &adj[node] {
                in_degree[next] -= 1;
                if in_degree[next] == 0 {
                    queue.push_back(next);
                }
            }
        }

        // 如果排序結果數量不等於 plugin 數量，代表有循環依賴
        if order.len() != n {
            let remaining: Vec<String> = (0..n)
                .filter(|i| in_degree[*i] > 0)
                .map(|i| self.plugins[i].name().to_string())
                .collect();
            return Err(AzldiError::CircularDependency(remaining));
        }

        Ok(order)
    }

    /// 取得某個方法的執行順序。
    ///
    /// 衍生層在呼叫業務方法前，先用這個取得排序後的索引，
    /// 再依序呼叫對應 plugin 的具體方法。
    ///
    /// `method` 是衍生層自訂的字串 key（如 "start"、"configure_routes"），
    /// 對應每個 plugin 在 `deps(method)` / `run_before(method)` 中的宣告。
    pub fn resolve_order(&self, method: &str) -> Result<Vec<usize>, AzldiError> {
        let m = method.to_string();
        self.topo_sort(
            |p| p.deps(&m),
            |p| p.run_before(&m),
        )
    }

    /// 按依賴順序迭代所有 plugin，對每個呼叫 closure。
    ///
    /// 這是衍生層最常用的 helper：取得排序 → 逐一呼叫。
    /// closure 參數為 `(index, plugin_name)`，index 對應 `plugins()` 的位置，
    /// 衍生層用同一個 index 存取自己的 `Vec<Arc<dyn DerivedTrait>>`。
    ///
    /// ```ignore
    /// // 衍生層的 start() 只需：
    /// self.azldi.for_each_in_order("start", |idx, name| {
    ///     let plugin = &self.plugins[idx];
    ///     plugin.on_start(&results);
    /// })?;
    /// ```
    pub fn for_each_in_order(
        &self,
        method: &str,
        mut f: impl FnMut(usize, &str),
    ) -> Result<(), AzldiError> {
        let order = self.resolve_order(method)?;
        for &idx in &order {
            f(idx, self.plugins[idx].name());
        }
        Ok(())
    }

    /// 按依賴順序初始化所有 plugin（等同 TS 的 `digest()`）。
    ///
    /// 使用 `init_deps()`（而非 `deps("init")`）來決定順序，
    /// 確保每個 plugin 的 `init()` 被呼叫時，它宣告的依賴已經 init 完畢。
    ///
    /// `init()` 會收到一個 `PluginRegistry`，只包含已初始化的 plugin，
    /// plugin 可以透過 `registry.get_as::<T>()` 取得依賴的具體 reference。
    pub fn digest(&self) -> Result<(), AzldiError> {
        self.digest_with(|_, _| {})
    }

    /// 帶 callback 的 digest（等同 TS 的 `digest({ onCreate })`）。
    ///
    /// `on_init` 在每個 plugin 的 `init()` 完成後被呼叫，
    /// 參數為 (plugin name, 在排序中的順序 index)。
    /// 可用於追蹤初始化順序、log、或測試驗證。
    pub fn digest_with(
        &self,
        mut on_init: impl FnMut(&str, usize),
    ) -> Result<(), AzldiError> {
        let order = self.topo_sort(|p| p.init_deps(), |_| vec![])?;
        let mut initialized: Vec<Arc<dyn Plugin>> = Vec::with_capacity(self.plugins.len());

        for (seq, idx) in order.iter().enumerate() {
            let plugin = &self.plugins[*idx];
            let registry = PluginRegistry::new(initialized.clone());
            plugin.init(&registry);
            on_init(plugin.name(), seq);
            initialized.push(Arc::clone(plugin));
        }

        Ok(())
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use std::any::Any;
    use std::sync::Mutex;

    // ════════════════════════════════════════════════════════
    // 測試用 mock plugin
    // ════════════════════════════════════════════════════════

    /// 基礎 mock plugin，可自訂 name、init_deps、deps、run_before。
    struct MockPlugin {
        plugin_name: &'static str,
        init_deps: Vec<&'static str>,
        method_deps: Vec<(&'static str, Vec<&'static str>)>,
        method_run_before: Vec<(&'static str, Vec<&'static str>)>,
        /// 記錄 init 被呼叫時，registry 中有哪些 plugin（用於驗證順序）。
        init_log: Mutex<Vec<String>>,
    }

    impl MockPlugin {
        fn new(name: &'static str) -> Self {
            Self {
                plugin_name: name,
                init_deps: vec![],
                method_deps: vec![],
                method_run_before: vec![],
                init_log: Mutex::new(vec![]),
            }
        }

        fn with_init_deps(mut self, deps: Vec<&'static str>) -> Self {
            self.init_deps = deps;
            self
        }

        fn with_deps(mut self, method: &'static str, deps: Vec<&'static str>) -> Self {
            self.method_deps.push((method, deps));
            self
        }

        fn with_run_before(mut self, method: &'static str, targets: Vec<&'static str>) -> Self {
            self.method_run_before.push((method, targets));
            self
        }

        /// 取得 init 時 registry 中的 plugin 名稱清單。
        fn get_init_log(&self) -> Vec<String> {
            self.init_log.lock().unwrap().clone()
        }
    }

    impl Plugin for MockPlugin {
        fn name(&self) -> &str {
            self.plugin_name
        }

        fn as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
            self
        }

        fn init_deps(&self) -> Vec<&str> {
            self.init_deps.clone()
        }

        fn init(&self, registry: &PluginRegistry) {
            // 記錄此時 registry 中已有的 plugin
            let mut log = self.init_log.lock().unwrap();
            for name in registry.names() {
                log.push(name.to_string());
            }
        }

        fn deps(&self, method: &str) -> Vec<&str> {
            for (m, deps) in &self.method_deps {
                if *m == method {
                    return deps.clone();
                }
            }
            vec![]
        }

        fn run_before(&self, method: &str) -> Vec<&str> {
            for (m, targets) in &self.method_run_before {
                if *m == method {
                    return targets.clone();
                }
            }
            vec![]
        }
    }

    // ════════════════════════════════════════════════════════
    // 測試 helper
    // ════════════════════════════════════════════════════════

    /// 從排序結果取得 plugin 名稱列表。
    fn ordered_names<'a>(azldi: &'a Azldi, order: &[usize]) -> Vec<&'a str> {
        order.iter().map(|&i| azldi.plugins()[i].name()).collect()
    }

    // ════════════════════════════════════════════════════════
    // 註冊與查詢
    // ════════════════════════════════════════════════════════

    #[test]
    fn test_register_and_get() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("alpha"));
        azldi.register_new(MockPlugin::new("beta"));

        assert_eq!(azldi.plugins().len(), 2);
        assert!(azldi.get("alpha").is_some());
        assert!(azldi.get("beta").is_some());
        assert!(azldi.get("gamma").is_none());
    }

    #[test]
    fn test_register_many() {
        let mut azldi = Azldi::new();
        azldi.register_many(vec![
            Arc::new(MockPlugin::new("a")) as Arc<dyn Plugin>,
            Arc::new(MockPlugin::new("b")) as Arc<dyn Plugin>,
            Arc::new(MockPlugin::new("c")) as Arc<dyn Plugin>,
        ]);
        assert_eq!(azldi.plugins().len(), 3);
    }

    #[test]
    fn test_get_as_downcast() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("alpha"));

        let plugin = azldi.get_as::<MockPlugin>("alpha");
        assert!(plugin.is_some());
        assert_eq!(plugin.unwrap().name(), "alpha");
    }

    // ════════════════════════════════════════════════════════
    // resolve_order — 方法級依賴排序
    // ════════════════════════════════════════════════════════

    /// 沒有依賴時，保持註冊順序。
    #[test]
    fn test_resolve_order_no_deps() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("a"));
        azldi.register_new(MockPlugin::new("b"));
        azldi.register_new(MockPlugin::new("c"));

        let order = azldi.resolve_order("start").unwrap();
        assert_eq!(ordered_names(&azldi, &order), vec!["a", "b", "c"]);
    }

    /// deps：B 依賴 A → A 先跑。
    #[test]
    fn test_resolve_order_with_deps() {
        let mut azldi = Azldi::new();
        // 故意把 B 先註冊
        azldi.register_new(MockPlugin::new("b").with_deps("start", vec!["a"]));
        azldi.register_new(MockPlugin::new("a"));

        let order = azldi.resolve_order("start").unwrap();
        assert_eq!(ordered_names(&azldi, &order), vec!["a", "b"]);
    }

    /// run_before：A 宣告要在 B 之前跑。
    #[test]
    fn test_resolve_order_with_run_before() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("b"));
        azldi.register_new(MockPlugin::new("a").with_run_before("start", vec!["b"]));

        let order = azldi.resolve_order("start").unwrap();
        assert_eq!(ordered_names(&azldi, &order), vec!["a", "b"]);
    }

    /// 比照 TS 測試：4 個 service 的複雜依賴鏈。
    ///
    /// s00: 無依賴
    /// s01: deps(start) = [s02]
    /// s02: deps(start) = [s00]
    /// s03: run_before(start) = [s00]
    ///
    /// 預期順序：s03 → s00 → s02 → s01
    #[test]
    fn test_resolve_order_complex_chain() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("s00"));
        azldi.register_new(MockPlugin::new("s01").with_deps("start", vec!["s02"]));
        azldi.register_new(MockPlugin::new("s02").with_deps("start", vec!["s00"]));
        azldi.register_new(MockPlugin::new("s03").with_run_before("start", vec!["s00"]));

        let order = azldi.resolve_order("start").unwrap();
        assert_eq!(ordered_names(&azldi, &order), vec!["s03", "s00", "s02", "s01"]);
    }

    /// 混合 deps + run_before。
    ///
    /// A: run_before(x) = [C]    → A 在 C 之前
    /// B: deps(x) = [A]          → A 在 B 之前
    /// C: deps(x) = [B]          → B 在 C 之前
    ///
    /// 預期順序：A → B → C
    #[test]
    fn test_resolve_order_mixed_deps_and_run_before() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("a").with_run_before("x", vec!["c"]));
        azldi.register_new(MockPlugin::new("b").with_deps("x", vec!["a"]));
        azldi.register_new(MockPlugin::new("c").with_deps("x", vec!["b"]));

        let order = azldi.resolve_order("x").unwrap();
        assert_eq!(ordered_names(&azldi, &order), vec!["a", "b", "c"]);
    }

    /// 不同方法有不同的排序。
    #[test]
    fn test_resolve_order_per_method() {
        let mut azldi = Azldi::new();
        azldi.register_new(
            MockPlugin::new("a")
                .with_deps("start", vec!["b"])       // start: A 在 B 之後
                .with_run_before("stop", vec!["b"]), // stop:  A 在 B 之前
        );
        azldi.register_new(MockPlugin::new("b"));

        let start = azldi.resolve_order("start").unwrap();
        assert_eq!(ordered_names(&azldi, &start), vec!["b", "a"]);

        let stop = azldi.resolve_order("stop").unwrap();
        assert_eq!(ordered_names(&azldi, &stop), vec!["a", "b"]);
    }

    /// 未宣告的 method 不影響排序（回傳註冊順序）。
    #[test]
    fn test_resolve_order_unknown_method() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("a").with_deps("start", vec!["b"]));
        azldi.register_new(MockPlugin::new("b"));

        let order = azldi.resolve_order("unknown_method").unwrap();
        // 沒有人對 "unknown_method" 宣告依賴，保持註冊順序
        assert_eq!(ordered_names(&azldi, &order), vec!["a", "b"]);
    }

    // ════════════════════════════════════════════════════════
    // for_each_in_order — 衍生層 helper
    // ════════════════════════════════════════════════════════

    /// for_each_in_order 按依賴順序呼叫 closure。
    #[test]
    fn test_for_each_in_order() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("c").with_deps("start", vec!["b"]));
        azldi.register_new(MockPlugin::new("b").with_deps("start", vec!["a"]));
        azldi.register_new(MockPlugin::new("a"));

        let mut visited = Vec::new();
        azldi.for_each_in_order("start", |_idx, name| {
            visited.push(name.to_string());
        }).unwrap();

        assert_eq!(visited, vec!["a", "b", "c"]);
    }

    /// for_each_in_order 遇到循環依賴時回傳 Err。
    #[test]
    fn test_for_each_in_order_error_propagation() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("a").with_deps("x", vec!["b"]));
        azldi.register_new(MockPlugin::new("b").with_deps("x", vec!["a"]));

        let result = azldi.for_each_in_order("x", |_, _| {});
        assert!(result.is_err());
    }

    // ════════════════════════════════════════════════════════
    // 錯誤偵測
    // ════════════════════════════════════════════════════════

    /// 循環依賴偵測：A → B → A
    #[test]
    fn test_circular_dependency() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("a").with_deps("start", vec!["b"]));
        azldi.register_new(MockPlugin::new("b").with_deps("start", vec!["a"]));

        match azldi.resolve_order("start").unwrap_err() {
            AzldiError::CircularDependency(names) => {
                assert!(names.contains(&"a".to_string()));
                assert!(names.contains(&"b".to_string()));
            }
            e => panic!("Expected CircularDependency, got: {e}"),
        }
    }

    /// 三角循環偵測：A → B → C → A
    #[test]
    fn test_circular_dependency_triangle() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("a").with_deps("x", vec!["c"]));
        azldi.register_new(MockPlugin::new("b").with_deps("x", vec!["a"]));
        azldi.register_new(MockPlugin::new("c").with_deps("x", vec!["b"]));

        match azldi.resolve_order("x").unwrap_err() {
            AzldiError::CircularDependency(names) => assert_eq!(names.len(), 3),
            e => panic!("Expected CircularDependency, got: {e}"),
        }
    }

    /// 依賴不存在偵測。
    #[test]
    fn test_dependency_not_found() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("a").with_deps("start", vec!["nonexistent"]));

        match azldi.resolve_order("start").unwrap_err() {
            AzldiError::DependencyNotFound { plugin, dep } => {
                assert_eq!(plugin, "a");
                assert_eq!(dep, "nonexistent");
            }
            e => panic!("Expected DependencyNotFound, got: {e}"),
        }
    }

    /// run_before 的 target 不存在時，靜默忽略（不報錯）。
    #[test]
    fn test_run_before_missing_target_is_ok() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("a").with_run_before("start", vec!["nonexistent"]));
        assert!(azldi.resolve_order("start").is_ok());
    }

    // ════════════════════════════════════════════════════════
    // digest — 初始化排序
    // ════════════════════════════════════════════════════════

    /// digest 按 init_deps 順序初始化。
    ///
    /// 比照 TS 測試：
    /// s00: 無依賴
    /// s01: init_deps = [s00]
    /// s02: init_deps = [s00]
    /// s03: init_deps = [s00]
    ///
    /// 故意以亂序註冊，驗證 digest 順序正確。
    #[test]
    fn test_digest_order() {
        let mut azldi = Azldi::new();

        // 需要保留 Arc reference 來讀 init_log，這裡用 register + Arc
        let s03 = Arc::new(MockPlugin::new("s03").with_init_deps(vec!["s00"]));
        let s02 = Arc::new(MockPlugin::new("s02").with_init_deps(vec!["s00"]));
        let s01 = Arc::new(MockPlugin::new("s01").with_init_deps(vec!["s00"]));
        let s00 = Arc::new(MockPlugin::new("s00"));

        // 亂序註冊
        azldi.register(s03.clone() as Arc<dyn Plugin>);
        azldi.register(s02.clone() as Arc<dyn Plugin>);
        azldi.register(s01.clone() as Arc<dyn Plugin>);
        azldi.register(s00.clone() as Arc<dyn Plugin>);

        azldi.digest().unwrap();

        // s00 先 init，所以它的 init_log 應該是空的
        assert_eq!(s00.get_init_log(), Vec::<String>::new());

        // s01、s02、s03 init 時，s00 應該已經在 registry 中
        assert!(s01.get_init_log().contains(&"s00".to_string()));
        assert!(s02.get_init_log().contains(&"s00".to_string()));
        assert!(s03.get_init_log().contains(&"s00".to_string()));
    }

    /// digest_with callback 追蹤初始化鏈（等同 TS 的 onCreate）。
    #[test]
    fn test_digest_with_callback() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("s03").with_init_deps(vec!["s00"]));
        azldi.register_new(MockPlugin::new("s02").with_init_deps(vec!["s00"]));
        azldi.register_new(MockPlugin::new("s01").with_init_deps(vec!["s00"]));
        azldi.register_new(MockPlugin::new("s00"));

        let mut init_order: Vec<String> = Vec::new();
        azldi
            .digest_with(|name, _seq| init_order.push(name.to_string()))
            .unwrap();

        // s00 一定是第一個（其他三個都依賴它）
        assert_eq!(init_order[0], "s00");
        // s01、s02、s03 都在 s00 之後
        assert_eq!(init_order.len(), 4);
        assert!(init_order[1..].contains(&"s01".to_string()));
        assert!(init_order[1..].contains(&"s02".to_string()));
        assert!(init_order[1..].contains(&"s03".to_string()));
    }

    /// 比照 TS 測試的完整 digest 順序：
    ///
    /// s00: 無依賴
    /// s01: init_deps = [s00]，deps(start) = [s02]
    /// s02: init_deps = [s00]
    /// s03: init_deps = [s00]，run_before(start) = [s00]
    ///
    /// init 順序：s00 → s03 → s02 → s01（或 s00 → 任意順序的 s01/s02/s03）
    /// start 順序：s03 → s00 → s02 → s01
    #[test]
    fn test_digest_and_resolve_order_combined() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("s00"));
        azldi.register_new(
            MockPlugin::new("s01")
                .with_init_deps(vec!["s00"])
                .with_deps("start", vec!["s02"]),
        );
        azldi.register_new(
            MockPlugin::new("s02")
                .with_init_deps(vec!["s00"])
                .with_deps("start", vec!["s00"]),
        );
        azldi.register_new(
            MockPlugin::new("s03")
                .with_init_deps(vec!["s00"])
                .with_run_before("start", vec!["s00"]),
        );

        // 追蹤 digest 順序
        let mut init_order = Vec::new();
        azldi
            .digest_with(|name, _| init_order.push(name.to_string()))
            .unwrap();
        assert_eq!(init_order[0], "s00");

        // 驗證 start 的執行順序
        let order = azldi.resolve_order("start").unwrap();
        assert_eq!(ordered_names(&azldi, &order), vec!["s03", "s00", "s02", "s01"]);
    }

    /// digest 的循環依賴偵測。
    #[test]
    fn test_digest_circular_dependency() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("a").with_init_deps(vec!["b"]));
        azldi.register_new(MockPlugin::new("b").with_init_deps(vec!["a"]));

        match azldi.digest().unwrap_err() {
            AzldiError::CircularDependency(names) => assert_eq!(names.len(), 2),
            e => panic!("Expected CircularDependency, got: {e}"),
        }
    }

    /// digest 缺少依賴偵測。
    #[test]
    fn test_digest_missing_dependency() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("a").with_init_deps(vec!["missing"]));

        match azldi.digest().unwrap_err() {
            AzldiError::DependencyNotFound { plugin, dep } => {
                assert_eq!(plugin, "a");
                assert_eq!(dep, "missing");
            }
            e => panic!("Expected DependencyNotFound, got: {e}"),
        }
    }

    // ════════════════════════════════════════════════════════
    // PluginRegistry
    // ════════════════════════════════════════════════════════

    #[test]
    fn test_registry_get_and_downcast() {
        let p = Arc::new(MockPlugin::new("test"));
        let registry = PluginRegistry::new(vec![p.clone() as Arc<dyn Plugin>]);

        // get by name
        assert!(registry.get("test").is_some());
        assert!(registry.get("other").is_none());

        // downcast
        let concrete = registry.get_as::<MockPlugin>("test");
        assert!(concrete.is_some());
        assert_eq!(concrete.unwrap().name(), "test");
    }

    /// registry 在 digest 階段只包含已初始化的 plugin。
    #[test]
    fn test_registry_incremental_during_digest() {
        let mut azldi = Azldi::new();

        // 需要保留 Arc reference 來讀 init_log
        let first = Arc::new(MockPlugin::new("first"));
        let second = Arc::new(MockPlugin::new("second").with_init_deps(vec!["first"]));
        let third = Arc::new(MockPlugin::new("third").with_init_deps(vec!["first", "second"]));

        azldi.register(first.clone() as Arc<dyn Plugin>);
        azldi.register(second.clone() as Arc<dyn Plugin>);
        azldi.register(third.clone() as Arc<dyn Plugin>);

        azldi.digest().unwrap();

        // first init 時 registry 是空的
        assert_eq!(first.get_init_log(), Vec::<String>::new());
        // second init 時 registry 有 first
        assert_eq!(second.get_init_log(), vec!["first"]);
        // third init 時 registry 有 first 和 second
        assert!(third.get_init_log().contains(&"first".to_string()));
        assert!(third.get_init_log().contains(&"second".to_string()));
    }

    // ════════════════════════════════════════════════════════
    // 邊界情況
    // ════════════════════════════════════════════════════════

    /// 空的 azldi。
    #[test]
    fn test_empty_azldi() {
        let azldi = Azldi::new();
        assert_eq!(azldi.plugins().len(), 0);
        assert!(azldi.resolve_order("anything").unwrap().is_empty());
        assert!(azldi.digest().is_ok());
    }

    /// 單一 plugin。
    #[test]
    fn test_single_plugin() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("solo"));

        let order = azldi.resolve_order("start").unwrap();
        assert_eq!(order, vec![0]);
        assert!(azldi.digest().is_ok());
    }

    /// 長鏈依賴：A → B → C → D → E。
    #[test]
    fn test_long_chain() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("e"));
        azldi.register_new(MockPlugin::new("d").with_deps("x", vec!["e"]));
        azldi.register_new(MockPlugin::new("c").with_deps("x", vec!["d"]));
        azldi.register_new(MockPlugin::new("b").with_deps("x", vec!["c"]));
        azldi.register_new(MockPlugin::new("a").with_deps("x", vec!["b"]));

        let order = azldi.resolve_order("x").unwrap();
        assert_eq!(ordered_names(&azldi, &order), vec!["e", "d", "c", "b", "a"]);
    }

    /// 鑽石依賴（diamond dependency）：
    ///     A
    ///    / \
    ///   B   C
    ///    \ /
    ///     D
    #[test]
    fn test_diamond_dependency() {
        let mut azldi = Azldi::new();
        azldi.register_new(MockPlugin::new("a"));
        azldi.register_new(MockPlugin::new("b").with_deps("x", vec!["a"]));
        azldi.register_new(MockPlugin::new("c").with_deps("x", vec!["a"]));
        azldi.register_new(MockPlugin::new("d").with_deps("x", vec!["b", "c"]));

        let order = azldi.resolve_order("x").unwrap();
        let names = ordered_names(&azldi, &order);

        // A 一定第一，D 一定最後，B/C 順序不定
        assert_eq!(names[0], "a");
        assert_eq!(names[3], "d");
        assert!(names[1..3].contains(&"b"));
        assert!(names[1..3].contains(&"c"));
    }

    // ════════════════════════════════════════════════════════
    // 衍生案例：context map + onResult merge
    // ════════════════════════════════════════════════════════
    //
    // 展示如何用 azldi 的 resolve_order 建構「共享 context map」的執行鏈：
    // - 外部有一個 HashMap<String, String>（context）
    // - 每個 plugin 收到 &context，回傳自己要貢獻的 KV
    // - 衍生系統在 on_result 時 merge 回 context
    // - 下一個 plugin 馬上看到更新後的 context

    use std::collections::HashMap;

    /// 衍生 trait：每個 plugin 可以讀取當前 context，回傳要 merge 的 KV pairs。
    trait ConfigPlugin: Plugin {
        fn execute(&self, context: &HashMap<String, String>) -> HashMap<String, String>;
    }

    /// 衍生系統：用 azldi 排序，自己跑 context 傳遞迴圈。
    struct ConfigSystem {
        azldi: Azldi,
        plugins: Vec<Arc<dyn ConfigPlugin>>,
    }

    impl ConfigSystem {
        fn new() -> Self {
            Self {
                azldi: Azldi::new(),
                plugins: Vec::new(),
            }
        }

        fn register<P: ConfigPlugin + 'static>(&mut self, plugin: Arc<P>) {
            self.azldi.register(Arc::clone(&plugin) as Arc<dyn Plugin>);
            self.plugins.push(plugin);
        }

        /// 帶 context + on_result 的執行方法。
        ///
        /// 1. 用 for_each_in_order 按依賴順序迭代
        /// 2. 每個 plugin 收到 &context，回傳 KV
        /// 3. 回傳的 KV merge 進 context
        /// 4. 呼叫 on_result callback
        /// 5. 下一個 plugin 看到的 context 已包含前面所有 merge
        fn start_with(
            &self,
            context: &mut HashMap<String, String>,
            mut on_result: impl FnMut(&str, &HashMap<String, String>, &HashMap<String, String>),
        ) {
            self.azldi.for_each_in_order("start", |idx, name| {
                let plugin = &self.plugins[idx];
                let result = plugin.execute(context);
                // merge plugin 回傳的 KV 進 context
                for (k, v) in &result {
                    context.insert(k.clone(), v.clone());
                }
                // on_result: (plugin name, 此 plugin 回傳的 KV, merge 後的完整 context)
                on_result(name, &result, context);
            }).unwrap();
        }
    }

    // ── 測試用的 ConfigPlugin 實作 ──

    /// DbPlugin：無依賴，貢獻 db 連線資訊。
    struct DbPlugin;
    impl Plugin for DbPlugin {
        fn name(&self) -> &str { "db" }
        fn as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> { self }
    }
    impl ConfigPlugin for DbPlugin {
        fn execute(&self, _ctx: &HashMap<String, String>) -> HashMap<String, String> {
            HashMap::from([
                ("db_host".into(), "localhost".into()),
                ("db_port".into(), "5432".into()),
            ])
        }
    }

    /// CachePlugin：依賴 db，讀取 db_host 組成 cache_url。
    struct CachePlugin;
    impl Plugin for CachePlugin {
        fn name(&self) -> &str { "cache" }
        fn as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> { self }
        fn deps(&self, method: &str) -> Vec<&str> {
            if method == "start" { vec!["db"] } else { vec![] }
        }
    }
    impl ConfigPlugin for CachePlugin {
        fn execute(&self, ctx: &HashMap<String, String>) -> HashMap<String, String> {
            // 讀取前一個 plugin merge 進來的 db_host
            let db_host = ctx.get("db_host").expect("db_host should exist from db plugin");
            HashMap::from([
                ("cache_url".into(), format!("redis://{db_host}:6379")),
            ])
        }
    }

    /// AppPlugin：依賴 cache，讀取 cache_url 確認就緒。
    struct AppReadyPlugin;
    impl Plugin for AppReadyPlugin {
        fn name(&self) -> &str { "app" }
        fn as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> { self }
        fn deps(&self, method: &str) -> Vec<&str> {
            if method == "start" { vec!["cache"] } else { vec![] }
        }
    }
    impl ConfigPlugin for AppReadyPlugin {
        fn execute(&self, ctx: &HashMap<String, String>) -> HashMap<String, String> {
            // 讀取 cache plugin merge 進來的 cache_url
            let cache_url = ctx.get("cache_url").expect("cache_url should exist from cache plugin");
            HashMap::from([
                ("app_ready".into(), "true".into()),
                ("app_cache".into(), cache_url.clone()),
            ])
        }
    }

    /// 經典場景：context map 在執行鏈中逐步累積。
    ///
    /// 初始 context: { "env": "test" }
    ///
    /// 執行鏈（依賴序）：db → cache → app
    ///
    /// db 拿到 { env }              → 回傳 { db_host, db_port }
    /// cache 拿到 { env, db_host, db_port } → 讀 db_host，回傳 { cache_url }
    /// app 拿到 { env, db_host, db_port, cache_url } → 讀 cache_url，回傳 { app_ready, app_cache }
    #[test]
    fn test_context_map_flows_through_execution_chain() {
        let mut system = ConfigSystem::new();
        system.register(Arc::new(DbPlugin));
        system.register(Arc::new(CachePlugin));
        system.register(Arc::new(AppReadyPlugin));

        system.azldi.digest().unwrap();

        // 外部 context，帶初始值
        let mut context: HashMap<String, String> = HashMap::new();
        context.insert("env".into(), "test".into());

        // 記錄每一步 on_result 的快照
        let mut snapshots: Vec<(
            String,                      // plugin name
            HashMap<String, String>,     // 此 plugin 回傳的 KV
            HashMap<String, String>,     // merge 後的完整 context
        )> = Vec::new();

        system.start_with(&mut context, |name, result, ctx| {
            snapshots.push((name.to_string(), result.clone(), ctx.clone()));
        });

        // ── 第 1 步：db ──
        assert_eq!(snapshots[0].0, "db");
        // db 回傳了 2 個 KV
        assert_eq!(snapshots[0].1.len(), 2);
        assert_eq!(snapshots[0].1["db_host"], "localhost");
        // merge 後 context = env + db_host + db_port
        assert_eq!(snapshots[0].2.len(), 3);
        assert_eq!(snapshots[0].2["env"], "test");

        // ── 第 2 步：cache（能看到 db 的結果）──
        assert_eq!(snapshots[1].0, "cache");
        // cache 讀到了 db_host，組出 cache_url
        assert_eq!(snapshots[1].1["cache_url"], "redis://localhost:6379");
        // merge 後 context = 3 + cache_url = 4
        assert_eq!(snapshots[1].2.len(), 4);

        // ── 第 3 步：app（能看到 db + cache 的結果）──
        assert_eq!(snapshots[2].0, "app");
        // app 讀到了 cache_url
        assert_eq!(snapshots[2].1["app_cache"], "redis://localhost:6379");
        assert_eq!(snapshots[2].1["app_ready"], "true");
        // merge 後 context = 4 + app_ready + app_cache = 6
        assert_eq!(snapshots[2].2.len(), 6);

        // ── 最終 context 包含所有累積值 ──
        assert_eq!(context.len(), 6);
        assert_eq!(context["env"], "test");
        assert_eq!(context["db_host"], "localhost");
        assert_eq!(context["db_port"], "5432");
        assert_eq!(context["cache_url"], "redis://localhost:6379");
        assert_eq!(context["app_ready"], "true");
        assert_eq!(context["app_cache"], "redis://localhost:6379");
    }
}
