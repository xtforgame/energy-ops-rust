//! transition 的分支條件求值（來源：`energy/workflow/conditions.ts`）。
//!
//! 條件是資料不是運算式字串：要能畫成人看得懂的標籤，也不在 DB 裡放程式碼。
//!
//! ```jsonc
//! {}                                                  // 無條件（預設邊）
//! { "attribute": "bill.totalAmount", "op": "gte", "value": 100000 }
//! { "all": [ {...}, {...} ] }                          // all / any 可巢狀；另有 not
//! ```
//!
//! 型別以原始 JSON 為底（`serde(from/into = Value)`）：存回去的形狀與讀進來的完全相同，
//! 認不得的形狀保留成 `Invalid` 並一律求值為 false（fail-closed）。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::js_compat;

string_enum! {
    pub enum ConditionOp {
        Eq => "eq",
        Ne => "ne",
        Gt => "gt",
        Gte => "gte",
        Lt => "lt",
        Lte => "lte",
        In => "in",
        Nin => "nin",
        Exists => "exists",
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(from = "Value", into = "Value")]
pub enum Condition {
    /// 無條件成立（`{}`，或 TS 視為「沒有條件」的值）。
    #[default]
    Always,
    Leaf { attribute: String, op: ConditionOp, value: Option<Value> },
    All(Vec<Condition>),
    Any(Vec<Condition>),
    Not(Box<Condition>),
    /// 認不得的形狀 —— 求值一律 false，原樣保留以便存回去與除錯。
    Invalid(Value),
}

impl Condition {
    pub fn leaf(attribute: &str, op: ConditionOp, value: Value) -> Self {
        Condition::Leaf { attribute: attribute.into(), op, value: Some(value) }
    }

    pub fn is_always(&self) -> bool {
        matches!(self, Condition::Always)
    }
}

impl From<Value> for Condition {
    /// 判讀順序照 TS：falsy 或沒有任何鍵 → 無條件；all → any → not（truthy）→ leaf → 認不得。
    fn from(v: Value) -> Self {
        if !js_compat::truthy(&v) {
            return Condition::Always;
        }
        match &v {
            // `Object.keys` 對數字／布林是空的 → 無條件成立。
            Value::Number(_) | Value::Bool(_) => Condition::Always,
            Value::Array(a) if a.is_empty() => Condition::Always,
            Value::Object(m) if m.is_empty() => Condition::Always,
            Value::Object(m) => {
                if let Some(Value::Array(items)) = m.get("all") {
                    return Condition::All(items.iter().cloned().map(Condition::from).collect());
                }
                if let Some(Value::Array(items)) = m.get("any") {
                    return Condition::Any(items.iter().cloned().map(Condition::from).collect());
                }
                if let Some(not) = m.get("not").filter(|n| js_compat::truthy(n)) {
                    return Condition::Not(Box::new(Condition::from(not.clone())));
                }
                if let (Some(Value::String(attribute)), Some(Value::String(op))) = (m.get("attribute"), m.get("op")) {
                    return Condition::Leaf {
                        attribute: attribute.clone(),
                        op: ConditionOp::from(op.clone()),
                        value: m.get("value").cloned(),
                    };
                }
                Condition::Invalid(v)
            }
            _ => Condition::Invalid(v),
        }
    }
}

impl From<Condition> for Value {
    fn from(c: Condition) -> Value {
        match c {
            Condition::Always => Value::Object(Map::new()),
            Condition::Leaf { attribute, op, value } => {
                let mut m = Map::new();
                m.insert("attribute".into(), Value::String(attribute));
                m.insert("op".into(), Value::String(op.as_str().into()));
                if let Some(v) = value {
                    m.insert("value".into(), v);
                }
                Value::Object(m)
            }
            Condition::All(items) => single("all", Value::Array(items.into_iter().map(Value::from).collect())),
            Condition::Any(items) => single("any", Value::Array(items.into_iter().map(Value::from).collect())),
            Condition::Not(inner) => single("not", Value::from(*inner)),
            Condition::Invalid(v) => v,
        }
    }
}

fn single(k: &str, v: Value) -> Value {
    let mut m = Map::new();
    m.insert(k.into(), v);
    Value::Object(m)
}

/// 屬性取值：支援 `bill.totalAmount` 這種點路徑。找不到回 None（JS 的 undefined）。
///
/// 照 JS 的屬性存取：陣列吃數字索引與 `length`，字串吃 `length` 與索引。
pub fn read_attribute(attributes: &Value, path: &str) -> Option<Value> {
    let mut acc = Some(attributes.clone());
    for part in path.split('.') {
        acc = match acc {
            None | Some(Value::Null) => None,
            Some(Value::Object(m)) => m.get(part).cloned(),
            Some(Value::Array(a)) => {
                if part == "length" {
                    Some(Value::from(a.len()))
                } else {
                    array_index(part).and_then(|i| a.get(i).cloned())
                }
            }
            Some(Value::String(s)) => {
                let units: Vec<u16> = s.encode_utf16().collect();
                if part == "length" {
                    Some(Value::from(units.len()))
                } else {
                    array_index(part)
                        .and_then(|i| units.get(i).copied())
                        .map(|u| Value::String(String::from_utf16_lossy(&[u])))
                }
            }
            Some(_) => None,
        };
    }
    acc
}

/// 正規的陣列索引字串（`"0"`、`"12"`；不收 `"01"`）。
fn array_index(part: &str) -> Option<usize> {
    if part.is_empty() || (part.len() > 1 && part.starts_with('0')) || !part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

/// JS 的 `===`：物件與陣列只有同一個實體才相等（從 JSON 來的永遠不是同一個）。
fn strict_eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(Value::Null), Some(Value::Null)) => true,
        (Some(Value::Bool(x)), Some(Value::Bool(y))) => x == y,
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        (Some(Value::String(x)), Some(Value::String(y))) => x == y,
        _ => false,
    }
}

fn compare(op: &ConditionOp, actual: Option<&Value>, expected: Option<&Value>) -> bool {
    match op {
        ConditionOp::Exists => actual.is_some_and(|v| !v.is_null()),
        ConditionOp::Eq => strict_eq(actual, expected),
        ConditionOp::Ne => !strict_eq(actual, expected),
        ConditionOp::In => match expected {
            Some(Value::Array(list)) => list.iter().any(|x| strict_eq(actual, Some(x))),
            _ => false,
        },
        ConditionOp::Nin => match expected {
            Some(Value::Array(list)) => !list.iter().any(|x| strict_eq(actual, Some(x))),
            _ => false,
        },
        // 大小比較只對數字有意義；不做 JS 的隱式轉型（`'10' > 9` 在流程判斷裡是災難）。
        ConditionOp::Gt | ConditionOp::Gte | ConditionOp::Lt | ConditionOp::Lte => {
            let (Some(a), Some(b)) = (actual.and_then(Value::as_f64), expected.and_then(Value::as_f64)) else {
                return false;
            };
            match op {
                ConditionOp::Gt => a > b,
                ConditionOp::Gte => a >= b,
                ConditionOp::Lt => a < b,
                _ => a <= b,
            }
        }
        ConditionOp::Other(_) => false,
    }
}

/// 求值一個條件。空物件 ＝ 無條件成立：定義側靠「末尾放一條無條件邊」保證走得下去，而不是靠引擎隱式 fallback。
pub fn evaluate_condition(condition: Option<&Condition>, attributes: &Value) -> bool {
    let Some(c) = condition else {
        return true;
    };
    match c {
        Condition::Always => true,
        Condition::All(items) => items.iter().all(|x| evaluate_condition(Some(x), attributes)),
        Condition::Any(items) => items.iter().any(|x| evaluate_condition(Some(x), attributes)),
        Condition::Not(inner) => !evaluate_condition(Some(inner), attributes),
        Condition::Leaf { attribute, op, value } => {
            compare(op, read_attribute(attributes, attribute).as_ref(), value.as_ref())
        }
        // 認不得的形狀一律 false：靜默當成 true 會讓打錯字的條件變成永遠成立的預設邊。
        Condition::Invalid(_) => false,
    }
}

/// 有條件與排序值的邊（定義資料、DB 列都可以實作）。
pub trait ConditionalEdge {
    fn condition(&self) -> Option<&Condition>;
    fn sort_order(&self) -> i64;
}

/// 從一組候選邊中挑出要走的。
///
/// - `parallel=false`：依 `sort_order` 取第一個條件成立者（穩定排序，同值保留原順序）。
/// - `parallel=true`：所有成立的邊全開。
///
/// 一條都不成立時回空 —— 呼叫端必須報 `no_matching_transition`，不是靜默結束流程。
pub fn select_transitions<'a, T: ConditionalEdge>(transitions: &'a [T], attributes: &Value, parallel: bool) -> Vec<&'a T> {
    let mut ordered: Vec<&T> = transitions.iter().collect();
    ordered.sort_by_key(|t| t.sort_order());
    let matched = ordered.into_iter().filter(|t| evaluate_condition(t.condition(), attributes));
    if parallel {
        matched.collect()
    } else {
        matched.take(1).collect()
    }
}
