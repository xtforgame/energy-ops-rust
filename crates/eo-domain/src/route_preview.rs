//! 「這張單會走哪些關卡」—— 唯一一份路線推導（來源：`energy/workflow/routePreview.ts` 的純函式部分）。
//!
//! 不要再寫第二份：預覽跟真的走起來不一樣，比沒有這個功能還糟。
//! 核心是重用引擎自己的 `select_transitions`，這裡一行條件判斷都不寫。
//! TS 版從 DB 讀節點與邊；這裡吃定義資料（DB 列換成 `WorkflowDefinition` 的順序：節點依 id、邊依 sortOrder）。

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::conditions::select_transitions;
use crate::workflow_defs::{NodeType, TransitionKind, WorkflowDefinition};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteResult {
    /// 依序經過的 nodeKey（含 start/end）。
    pub path: Vec<String>,
    /// nodeKey → 顯示名（畫面上不印 nodeKey）。
    pub name_by_key: BTreeMap<String, String>,
    /// 需要人動作的節點（`humanTask`）。
    #[serde(skip)]
    pub human_keys: BTreeSet<String>,
}

/// 依定義走一次流程。
///
/// `attributes` 的形狀要跟送審時組的一模一樣（例：`{ "bill": { "totalAmount": 138195 } }`），
/// 否則條件讀不到值，會靜默走成預設邊。
pub fn walk_route(def: &WorkflowDefinition, attributes: &Value) -> RouteResult {
    let mut name_by_key = BTreeMap::new();
    let mut human_keys = BTreeSet::new();
    let mut start_key = "start".to_string();
    for n in &def.nodes {
        name_by_key.insert(n.node_key.clone(), n.name.clone());
        if n.node_type == NodeType::HumanTask {
            human_keys.insert(n.node_key.clone());
        }
        // 起點用 nodeType 找，不寫死 `start`：保證金流程的起點叫 `submit`。
        if n.node_type == NodeType::Start {
            start_key = n.node_key.clone();
        }
    }

    let mut path = Vec::new();
    let mut cursor = start_key;
    // 上限＝節點數 + 2 —— 定義裡有環時不可以無窮迴圈。
    for _ in 0..def.nodes.len() + 2 {
        path.push(cursor.clone());
        if cursor == "end" {
            break;
        }
        let outgoing: Vec<_> = def
            .transitions
            .iter()
            .filter(|t| t.from_node_key == cursor && t.kind != TransitionKind::SendBack)
            .cloned()
            .collect();
        let chosen = select_transitions(&outgoing, attributes, false);
        let Some(next) = chosen.first() else {
            break;
        };
        cursor = next.to_node_key.clone();
    }

    RouteResult { path, name_by_key, human_keys }
}

/// 「關卡」＝需要人動作的節點。少一個 `calc` 沒有控管意義，算進去只會讓稽核變雜訊。
pub fn gates_of(route: &RouteResult) -> Vec<String> {
    route.path.iter().filter(|k| route.human_keys.contains(*k)).cloned().collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateDelta {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: bool,
}

/// 兩條路線的關卡差異（保留原順序）。
pub fn gate_delta(before: &[String], after: &[String]) -> GateDelta {
    let b: BTreeSet<&String> = before.iter().collect();
    let a: BTreeSet<&String> = after.iter().collect();
    let added: Vec<String> = after.iter().filter(|k| !b.contains(k)).cloned().collect();
    let removed: Vec<String> = before.iter().filter(|k| !a.contains(k)).cloned().collect();
    let changed = !added.is_empty() || !removed.is_empty();
    GateDelta { added, removed, changed }
}
