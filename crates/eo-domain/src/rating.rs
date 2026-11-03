//! 計算策略 registry —— 策略即資料（來源：`common/ratingStrategies.ts`）。
//!
//! 三個約束：type 白名單（未註冊 → 拒絕）、per-type 參數驗證（未宣告的鍵丟棄）、
//! 算不出來就報錯，絕不回 0（金額 0 的紀錄看起來完全合法，但它是錯帳）。
//!
//! 錢不能有浮點誤差：全部走 `rust_decimal` 定點。TS 的 BigInt 沒有上限，
//! `Decimal` 約 28 位有效數字，超出時報 `NotADecimal` 而不是溢位。

use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use thiserror::Error;

use crate::js_compat;

/// 金額／比例／度數的輸入：DB 讀出來可能是字串（decimal）或數字。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DecimalInput {
    Number(f64),
    Text(String),
}

impl From<f64> for DecimalInput {
    fn from(v: f64) -> Self {
        DecimalInput::Number(v)
    }
}

impl From<&str> for DecimalInput {
    fn from(v: &str) -> Self {
        DecimalInput::Text(v.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum RatingError {
    /// 與 TS 的 `RatingEvaluationError` 對應：`unknown_strategy_type`／`invalid_strategy_params`／`missing_bill_fields`。
    #[error("{code}")]
    Evaluation { code: &'static str, details: Value },
    /// 輸入不是十進位數（TS 是一般 `Error('not a decimal: …')`）。
    #[error("not a decimal: {0}")]
    NotADecimal(String),
}

impl RatingError {
    pub fn code(&self) -> &str {
        match self {
            RatingError::Evaluation { code, .. } => code,
            RatingError::NotADecimal(_) => "not_a_decimal",
        }
    }
}

/// JS `Number.prototype.toFixed(digits)`：對 double 的精確值四捨五入（同距取大的絕對值）。
fn js_to_fixed(v: f64, digits: u32) -> Result<String, RatingError> {
    if !v.is_finite() || v.abs() >= 1e21 {
        // toFixed 對這些值回 `String(v)`（`Infinity`／`1e+21`），進不了十進位格式。
        return Err(RatingError::NotADecimal(js_compat::number_to_string(v)));
    }
    let exact = Decimal::from_f64_retain(v).ok_or_else(|| RatingError::NotADecimal(js_compat::number_to_string(v)))?;
    let mut d = exact.round_dp_with_strategy(digits, RoundingStrategy::MidpointAwayFromZero);
    d.rescale(digits);
    let s = d.to_string();
    // `(-0.001).toFixed(2)` 是 `-0.00`；之後的解析會把符號吃掉，結果仍是 0。
    Ok(if v < 0.0 && !s.starts_with('-') { format!("-{s}") } else { s })
}

/// 把十進位字串／數字轉成「放大 `scale` 倍的整數」。`12.5` @4 → `125000`。
///
/// 多出來的小數位**截斷**（不是四捨五入）：計算精度跟著 DB 欄位走，不製造只存在於記憶體裡的錢。
pub fn to_scaled(value: &DecimalInput, scale: u32) -> Result<i128, RatingError> {
    let s = match value {
        DecimalInput::Number(n) => js_to_fixed(*n, scale)?,
        DecimalInput::Text(t) => js_compat::trim(t).to_string(),
    };
    let bad = || RatingError::NotADecimal(s.clone());
    let (neg, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.as_str()),
    };
    let (int_part, frac_raw) = body.split_once('.').unwrap_or((body, ""));
    let digits_ok = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    if !digits_ok(int_part) || (body.contains('.') && !digits_ok(frac_raw)) {
        return Err(bad());
    }
    let mut frac: String = frac_raw.chars().take(scale as usize).collect();
    while frac.len() < scale as usize {
        frac.push('0');
    }
    let n: i128 = format!("{int_part}{frac}").parse().map_err(|_| bad())?;
    // 確認放得進 Decimal（之後的運算都在 Decimal 上做）。
    Decimal::try_from_i128_with_scale(n, scale).map_err(|_| bad())?;
    Ok(if neg { -n } else { n })
}

fn decimal_of(scaled: i128, scale: u32) -> Result<Decimal, RatingError> {
    Decimal::try_from_i128_with_scale(scaled, scale).map_err(|_| RatingError::NotADecimal(scaled.to_string()))
}

fn format_fixed(mut d: Decimal, scale: u32) -> String {
    d.rescale(scale);
    if d.is_zero() {
        d.set_sign_positive(true);
    }
    d.to_string()
}

/// 放大整數 → 十進位字串（固定 `scale` 位小數）。
pub fn from_scaled(value: i128, scale: u32) -> String {
    decimal_of(value, scale).map(|d| format_fixed(d, scale)).unwrap_or_else(|_| value.to_string())
}

/// 從 `scale` 位縮到 2 位（金額），四捨五入（half-up，負數對稱）。
///
/// 「四捨五入」是業務決定：銀行家捨入與無條件捨去在對帳時會差錢；要改就改這一個函式。
pub fn round_to_money(scaled: i128, scale: u32) -> String {
    match decimal_of(scaled, scale) {
        Ok(d) => round_money(d),
        Err(_) => scaled.to_string(),
    }
}

fn round_money(d: Decimal) -> String {
    format_fixed(d.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero), 2)
}

/// 算式吃的輸入 —— 電費單的最終值（校對後的那一層）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RatingInput {
    #[serde(default)]
    pub total_amount: Option<DecimalInput>,
    #[serde(default)]
    pub generation_kwh: Option<DecimalInput>,
    #[serde(default)]
    pub billing_period_start: Option<String>,
    #[serde(default)]
    pub billing_period_end: Option<String>,
}

impl RatingInput {
    fn field(&self, name: &str) -> Option<&DecimalInput> {
        match name {
            "totalAmount" => self.total_amount.as_ref(),
            "generationKwh" => self.generation_kwh.as_ref(),
            _ => None,
        }
    }
}

/// 參數的規則。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamRule {
    /// 在給定小數位下必須 > 0。
    Positive,
    /// 在給定小數位下必須介於 0–100（含）。
    Percent,
}

#[derive(Debug, Clone, Copy)]
pub struct ParamSpec {
    pub key: &'static str,
    /// 驗證時用的小數位（與計算時相同）。
    pub scale: u32,
    pub rule: ParamRule,
    pub message: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct RatingStrategyDef {
    pub strategy_type: &'static str,
    /// i18n key。不印 type 字串給使用者看。
    pub label_key: &'static str,
    pub params: &'static [ParamSpec],
    /// 需要電費單提供哪些欄位；缺了要早期報錯，不可算成 0。
    pub required_bill_fields: &'static [&'static str],
    evaluate: fn(&Map<String, Value>, &RatingInput) -> Result<String, RatingError>,
}

fn param(params: &Map<String, Value>, key: &str) -> Result<DecimalInput, RatingError> {
    match params.get(key) {
        Some(Value::Number(n)) => Ok(DecimalInput::Number(n.as_f64().unwrap_or(f64::NAN))),
        Some(Value::String(s)) => Ok(DecimalInput::Text(s.clone())),
        _ => Err(RatingError::NotADecimal(key.into())),
    }
}

fn bill_field(input: &RatingInput, key: &str) -> Result<DecimalInput, RatingError> {
    input.field(key).cloned().ok_or_else(|| RatingError::NotADecimal(key.into()))
}

/// 每期固定給付金額。「每期」＝這張電費單的計費期間；不做雙月拆月（proration 已拍板不做）。
fn eval_fixed_fee(params: &Map<String, Value>, _input: &RatingInput) -> Result<String, RatingError> {
    let amount = to_scaled(&param(params, "amount")?, 2)?;
    Ok(from_scaled(amount, 2))
}

/// 電費單金額 × %。先乘再除，不先除（先除會提早損失精度）。
fn eval_revenue_share(params: &Map<String, Value>, input: &RatingInput) -> Result<String, RatingError> {
    let amount = decimal_of(to_scaled(&bill_field(input, "totalAmount")?, 2)?, 2)?;
    let percent = decimal_of(to_scaled(&param(params, "sharePercent")?, 4)?, 4)?;
    // amount(2) × percent(4) → 6 位；除以 100 後照 BigInt 整數除法截到 6 位。
    let product = amount.checked_mul(percent).ok_or_else(|| RatingError::NotADecimal("overflow".into()))?;
    let scaled6 = (product / Decimal::ONE_HUNDRED).trunc_with_scale(6);
    Ok(round_money(scaled6))
}

/// 度數 × 元/度：kwh(3) × price(4) → 7 位，再四捨五入到分。
fn eval_generation_unit_price(params: &Map<String, Value>, input: &RatingInput) -> Result<String, RatingError> {
    let kwh = decimal_of(to_scaled(&bill_field(input, "generationKwh")?, 3)?, 3)?;
    let price = decimal_of(to_scaled(&param(params, "unitPrice")?, 4)?, 4)?;
    let product = kwh.checked_mul(price).ok_or_else(|| RatingError::NotADecimal("overflow".into()))?;
    Ok(round_money(product))
}

pub static RATING_STRATEGY_DEFS: [RatingStrategyDef; 3] = [
    RatingStrategyDef {
        strategy_type: "fixedFee",
        label_key: "energy.rating.type.fixedFee",
        params: &[ParamSpec { key: "amount", scale: 2, rule: ParamRule::Positive, message: "金額必須是正數" }],
        required_bill_fields: &[],
        evaluate: eval_fixed_fee,
    },
    RatingStrategyDef {
        strategy_type: "revenueShare",
        label_key: "energy.rating.type.revenueShare",
        params: &[ParamSpec { key: "sharePercent", scale: 4, rule: ParamRule::Percent, message: "比例必須介於 0–100" }],
        required_bill_fields: &["totalAmount"],
        evaluate: eval_revenue_share,
    },
    RatingStrategyDef {
        strategy_type: "generationUnitPrice",
        label_key: "energy.rating.type.generationUnitPrice",
        params: &[ParamSpec { key: "unitPrice", scale: 4, rule: ParamRule::Positive, message: "單價必須是正數" }],
        required_bill_fields: &["generationKwh"],
        evaluate: eval_generation_unit_price,
    },
];

pub fn get_rating_strategy_def(strategy_type: &str) -> Option<&'static RatingStrategyDef> {
    RATING_STRATEGY_DEFS.iter().find(|d| d.strategy_type == strategy_type)
}

pub fn is_known_rating_type(strategy_type: &str) -> bool {
    get_rating_strategy_def(strategy_type).is_some()
}

pub fn rating_strategy_types() -> Vec<&'static str> {
    RATING_STRATEGY_DEFS.iter().map(|d| d.strategy_type).collect()
}

impl RatingStrategyDef {
    /// 參數驗證（zod 的 `safeParse`）：通過時回傳只含宣告鍵的物件，未宣告的鍵丟棄。
    /// `null` 視同 `{}`；不是物件、缺鍵、型別不對、超出規則都回 `invalid_strategy_params`。
    pub fn parse_params(&self, params: &Value) -> Result<Map<String, Value>, RatingError> {
        let empty = Map::new();
        let obj = match params {
            Value::Null => &empty,
            Value::Object(m) => m,
            _ => return Err(invalid_params(vec!["params 必須是物件".into()])),
        };
        let mut out = Map::new();
        let mut issues = Vec::new();
        for spec in self.params {
            let v = obj.get(spec.key);
            let ok = match v {
                Some(Value::Number(_)) | Some(Value::String(_)) => {
                    let parsed = param(obj, spec.key).and_then(|d| to_scaled(&d, spec.scale));
                    match (parsed, spec.rule) {
                        (Ok(n), ParamRule::Positive) => n > 0,
                        (Ok(n), ParamRule::Percent) => n >= 0 && n <= 100 * 10i128.pow(spec.scale),
                        (Err(_), _) => false,
                    }
                }
                _ => false,
            };
            if ok {
                out.insert(spec.key.into(), v.cloned().unwrap_or(Value::Null));
            } else {
                issues.push(format!("{}: {}", spec.key, spec.message));
            }
        }
        if issues.is_empty() {
            Ok(out)
        } else {
            Err(invalid_params(issues))
        }
    }
}

fn invalid_params(issues: Vec<String>) -> RatingError {
    RatingError::Evaluation { code: "invalid_strategy_params", details: json!(issues) }
}

/// 純函式的計算入口。回傳 2 位小數的金額字串；算不出來就報錯，絕不回 0。
pub fn evaluate_strategy(strategy_type: &str, params: &Value, input: &RatingInput) -> Result<String, RatingError> {
    let def = get_rating_strategy_def(strategy_type).ok_or_else(|| RatingError::Evaluation {
        code: "unknown_strategy_type",
        details: json!({ "type": strategy_type }),
    })?;
    let parsed = def.parse_params(params)?;

    let missing: Vec<&str> = def
        .required_bill_fields
        .iter()
        .copied()
        .filter(|f| match input.field(f) {
            None => true,
            Some(DecimalInput::Text(s)) => s.is_empty(),
            Some(DecimalInput::Number(_)) => false,
        })
        .collect();
    if !missing.is_empty() {
        return Err(RatingError::Evaluation {
            code: "missing_bill_fields",
            details: json!({ "missing": missing, "type": strategy_type }),
        });
    }

    (def.evaluate)(&parsed, input)
}
