//! 「這張單會走哪些關卡」—— 讀 DB 的定義列，交給 eo-domain 那一份路線推導的同一套規則
//! （校對台預覽與跨門檻稽核共用；兩邊算的不一樣，兩邊都不能信）。

use crate::common::Tx;
use crate::workflow::model;
use eo_domain::conditions::select_transitions;
use eo_domain::route_preview::RouteResult;
use eo_platform::error::ApiResult;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// 從起點走一次（不是從目前節點）—— 要看的是整條路線，「多一關」才看得出來。
pub async fn walk_route(tx: &mut Tx<'_>, definition_id: i64, attributes: &Value) -> ApiResult<RouteResult> {
    let nodes = model::nodes(tx, definition_id).await?;
    let edges = model::transitions(tx, definition_id).await?;
    let mut name_by_key = BTreeMap::new();
    let mut human_keys = BTreeSet::new();
    let mut start_key = "start".to_string();
    for n in &nodes {
        name_by_key.insert(n.node_key.clone(), n.name.clone());
        if n.node_type == "humanTask" {
            human_keys.insert(n.node_key.clone());
        }
        // 起點用 nodeType 找：保證金流程的起點叫 `submit`。
        if n.node_type == "start" {
            start_key = n.node_key.clone();
        }
    }
    let mut path = Vec::new();
    let mut cursor = start_key;
    // 上限＝節點數 + 2：定義裡有環時不可以無窮迴圈。
    for _ in 0..nodes.len() + 2 {
        path.push(cursor.clone());
        if cursor == "end" {
            break;
        }
        let outgoing: Vec<model::TransitionRow> =
            edges.iter().filter(|t| t.from_node_key == cursor && t.kind != "sendBack").cloned().collect();
        let chosen = select_transitions(&outgoing, attributes, false);
        let Some(next) = chosen.first() else { break };
        cursor = next.to_node_key.clone();
    }
    Ok(RouteResult { path, name_by_key, human_keys })
}

/// JS 的 `Number(string)`：空白算 0、不是數字回 NaN。
pub fn js_number(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// JS 的 `String(number)`（整數不帶小數點）。
pub fn js_num_str(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i128)
    } else {
        format!("{n}")
    }
}

/// JS 的 `String(x)`（只處理純量）。
pub fn js_str(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.as_f64().map(js_num_str).unwrap_or_else(|| n.to_string()),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// JS 的 `Number(v).toLocaleString('en-US')`：千分位、最多三位小數。
pub fn money(v: &Value) -> String {
    if v.is_null() {
        return "—".into();
    }
    let n = match v {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(s) => js_number(s),
        Value::Bool(b) => f64::from(u8::from(*b)),
        _ => f64::NAN,
    };
    if !n.is_finite() {
        return "NaN".into();
    }
    let neg = n < 0.0;
    let scaled = (n.abs() * 1000.0).round() as u128;
    let int = scaled / 1000;
    let frac = scaled % 1000;
    let digits = int.to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let mut out = if neg && scaled != 0 { format!("-{grouped}") } else { grouped };
    if frac != 0 {
        let f = format!("{frac:03}");
        out.push('.');
        out.push_str(f.trim_end_matches('0'));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::money;
    use serde_json::json;

    #[test]
    fn money_matches_en_us_locale() {
        assert_eq!(money(&json!(138195)), "138,195");
        assert_eq!(money(&json!("99999.00")), "99,999");
        assert_eq!(money(&json!(1234.5678)), "1,234.568");
        assert_eq!(money(&json!(-132)), "-132");
        assert_eq!(money(&json!(null)), "—");
    }
}
