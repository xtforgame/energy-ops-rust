//! 移植自 `tests/rating.test.ts` 的「rating strategies (1-a, pure)」。
//! 碰 DB 的 RatingService 部分（1-c）屬於 eo-energy，不在這裡。

use eo_domain::rating::*;
use serde_json::{json, Value};

fn num(v: f64) -> DecimalInput {
    DecimalInput::Number(v)
}

fn text(v: &str) -> DecimalInput {
    DecimalInput::Text(v.into())
}

fn amount(v: DecimalInput) -> RatingInput {
    RatingInput { total_amount: Some(v), ..Default::default() }
}

fn kwh(v: DecimalInput) -> RatingInput {
    RatingInput { generation_kwh: Some(v), ..Default::default() }
}

fn code_of(r: Result<String, RatingError>) -> String {
    r.expect_err("應該要報錯").code().to_string()
}

/// 來源：rating.test.ts「declares exactly the three v1 types」
#[test]
fn declares_exactly_the_three_v1_types() {
    let mut types = rating_strategy_types();
    types.sort();
    assert_eq!(types, vec!["fixedFee", "generationUnitPrice", "revenueShare"]);
    for t in rating_strategy_types() {
        // 不印 type 字串給使用者看。
        assert!(get_rating_strategy_def(t).unwrap().label_key.starts_with("energy.rating.type."));
    }
    assert!(is_known_rating_type("fixedFee"));
    assert!(!is_known_rating_type("madeUp"));
}

/// 來源：rating.test.ts「decimal helpers survive the values that break floats」
#[test]
fn decimal_helpers_survive_float_breakers() {
    assert_eq!(from_scaled(to_scaled(&text("12.5"), 4).unwrap(), 4), "12.5000");
    let sum = to_scaled(&num(0.1), 2).unwrap() + to_scaled(&num(0.2), 2).unwrap();
    assert_eq!(from_scaled(sum, 2), "0.30", "0.1+0.2");
    // half-up
    assert_eq!(round_to_money(to_scaled(&text("1.005"), 3).unwrap(), 3), "1.01");
    assert_eq!(round_to_money(to_scaled(&text("1.004"), 3).unwrap(), 3), "1.00");
    assert_eq!(round_to_money(to_scaled(&text("-1.005"), 3).unwrap(), 3), "-1.01");
}

/// 來源：rating.test.ts「money math does not inherit float rounding bugs」
#[test]
fn money_math_has_no_float_rounding_bugs() {
    // 1.005 在 binary float 裡其實是 1.00499999…，所以 toFixed(2) 會少一分錢；數字輸入照這個語意走。
    assert_eq!(to_scaled(&num(1.005), 2).unwrap(), 100);
    assert_eq!(round_to_money(to_scaled(&text("1.005"), 3).unwrap(), 3), "1.01");

    // 輸入的小數位跟著 DB 欄位走：totalAmount 是 decimal(14,2)，1.005 會被截到 1.00。
    assert_eq!(evaluate_strategy("revenueShare", &json!({ "sharePercent": 100 }), &amount(text("1.005"))).unwrap(), "1.00");
    assert_eq!(
        evaluate_strategy("revenueShare", &json!({ "sharePercent": 12.5 }), &amount(num(98000.0))).unwrap(),
        "12250.00"
    );
    // 0.1 系列
    assert_eq!(evaluate_strategy("revenueShare", &json!({ "sharePercent": "0.1" }), &amount(text("0.3"))).unwrap(), "0.00");
    assert_eq!(
        evaluate_strategy("revenueShare", &json!({ "sharePercent": "33.3333" }), &amount(text("10000.00"))).unwrap(),
        "3333.33"
    );
}

/// 來源：rating.test.ts「generationUnitPrice multiplies kwh by unit price exactly」
#[test]
fn generation_unit_price_is_exact() {
    // 4321.5 × 1.85 = 7994.775 → half-up → 7994.78
    assert_eq!(
        evaluate_strategy("generationUnitPrice", &json!({ "unitPrice": 1.85 }), &kwh(num(4321.5))).unwrap(),
        "7994.78"
    );
    assert_eq!(evaluate_strategy("generationUnitPrice", &json!({ "unitPrice": "0.0001" }), &kwh(text("1"))).unwrap(), "0.00");
}

/// 來源：rating.test.ts「fixedFee ignores the bill contents (it only cares that a bill exists)」
#[test]
fn fixed_fee_ignores_bill_contents() {
    assert!(get_rating_strategy_def("fixedFee").unwrap().required_bill_fields.is_empty());
    assert_eq!(evaluate_strategy("fixedFee", &json!({ "amount": 25000 }), &RatingInput::default()).unwrap(), "25000.00");
}

/// 來源：rating.test.ts「throws — never returns 0 — when the required bill field is missing」
#[test]
fn missing_required_field_is_an_error_not_zero() {
    let r = evaluate_strategy("revenueShare", &json!({ "sharePercent": 10 }), &RatingInput::default());
    assert_eq!(code_of(r), "missing_bill_fields");
    // 空字串也算沒有。
    let r = evaluate_strategy("revenueShare", &json!({ "sharePercent": 10 }), &amount(text("")));
    let err = r.expect_err("空字串不是金額");
    assert!(matches!(&err, RatingError::Evaluation { code: "missing_bill_fields", details }
        if details["missing"] == json!(["totalAmount"])));
}

/// 來源：rating.test.ts「rejects unknown types and invalid params (whitelist + per-type zod)」
#[test]
fn rejects_unknown_types_and_invalid_params() {
    assert_eq!(code_of(evaluate_strategy("madeUp", &json!({}), &RatingInput::default())), "unknown_strategy_type");
    assert_eq!(
        code_of(evaluate_strategy("revenueShare", &json!({ "sharePercent": 101 }), &amount(num(1.0)))),
        "invalid_strategy_params"
    );
    assert_eq!(
        code_of(evaluate_strategy("fixedFee", &json!({ "amount": -5 }), &RatingInput::default())),
        "invalid_strategy_params"
    );
    // 缺鍵、型別不對、params 不是物件都一樣擋。
    for bad in [json!({}), json!({ "amount": true }), json!([1]), json!({ "amount": "abc" })] {
        assert_eq!(
            code_of(evaluate_strategy("fixedFee", &bad, &RatingInput::default())),
            "invalid_strategy_params",
            "{bad}"
        );
    }
}

/// 來源：rating.test.ts「params keys the schema did not declare are dropped」
#[test]
fn undeclared_param_keys_are_dropped() {
    let def = get_rating_strategy_def("fixedFee").unwrap();
    let parsed = def.parse_params(&json!({ "amount": 100, "secretMultiplier": 999 })).unwrap();
    assert_eq!(parsed.keys().collect::<Vec<_>>(), vec!["amount"]);
    assert_eq!(Value::Object(parsed)["amount"], json!(100));
}
