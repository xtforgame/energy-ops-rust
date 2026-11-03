//! 舊 TS 依賴的幾個 JS 內建語意（`String(x)`、`Number(s)`、`trim()`、UTF-16 長度）。
//! 集中在這裡，各模組才不會各自「差不多」地重寫一份而悄悄走鐘。

use serde_json::Value;

/// JS `String.prototype.trim` 認的空白（WhiteSpace ＋ LineTerminator）。
/// 與 Rust 的 `char::is_whitespace` 不同：JS 認 U+FEFF、不認 U+0085。
pub fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{B}' | '\u{C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

pub fn trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

/// zod 的 `.min()`／`.max()` 與 JS 的 `length` 數的是 UTF-16 code unit。
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// 取前 `n` 個 UTF-16 code unit（JS 的 `slice(0, n)`）。切到代理對中間時捨掉那半個字。
pub fn utf16_prefix(s: &str, n: usize) -> String {
    let mut used = 0;
    let mut out = String::new();
    for c in s.chars() {
        let w = c.len_utf16();
        if used + w > n {
            break;
        }
        used += w;
        out.push(c);
    }
    out
}

/// JS 的 truthiness（`if (!x)`）。
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0 && !f.is_nan()).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// JS 的 `Number.prototype.toString()`（最短往返表示）。
pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if n == 0.0 {
        return "0".into();
    }
    let neg = n < 0.0;
    // `{:e}` 給最短往返的有效位數，例如 `1.2345e3`；再照 ECMA-262 Number::toString 排版。
    let sci = format!("{:e}", n.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let k = digits.len() as i32;
    let point = exp + 1; // ECMA 的 n
    let body = if k <= point && point <= 21 {
        format!("{}{}", digits, "0".repeat((point - k) as usize))
    } else if 0 < point && point <= 21 {
        format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
    } else if -6 < point && point <= 0 {
        format!("0.{}{}", "0".repeat((-point) as usize), digits)
    } else {
        let e = point - 1;
        let sign = if e >= 0 { "+" } else { "-" };
        if k == 1 {
            format!("{}e{}{}", digits, sign, e.abs())
        } else {
            format!("{}.{}e{}{}", &digits[..1], &digits[1..], sign, e.abs())
        }
    };
    if neg {
        format!("-{body}")
    } else {
        body
    }
}

/// JS 的 `String(x)`（模板字串插值也是這個）。
pub fn to_js_string(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.as_f64().map(number_to_string).unwrap_or_else(|| n.to_string()),
        Value::String(s) => s.clone(),
        // `Array.prototype.join`：null/undefined 元素印成空字串。
        Value::Array(items) => items
            .iter()
            .map(|x| if x.is_null() { String::new() } else { to_js_string(x) })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// JS 的 `Number(string)`。不是數字回 NaN（呼叫端再用 `is_finite` 判）。
pub fn string_to_number(s: &str) -> f64 {
    let t = trim(s);
    if t.is_empty() {
        return 0.0;
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    let radix = |prefix: &[&str], base: u32| -> Option<f64> {
        for p in prefix {
            if let Some(rest) = t.strip_prefix(p) {
                if rest.is_empty() || !rest.chars().all(|c| c.is_digit(base)) {
                    return Some(f64::NAN);
                }
                return Some(rest.chars().fold(0.0, |acc, c| {
                    acc * base as f64 + c.to_digit(base).unwrap_or(0) as f64
                }));
            }
        }
        None
    };
    if let Some(v) = radix(&["0x", "0X"], 16)
        .or_else(|| radix(&["0o", "0O"], 8))
        .or_else(|| radix(&["0b", "0B"], 2))
    {
        return v;
    }
    if is_decimal_literal(t) {
        t.parse::<f64>().unwrap_or(f64::NAN)
    } else {
        f64::NAN
    }
}

/// `[+-]? (digits [. digits?] | . digits) ([eE][+-]? digits)?` —— 擋掉 Rust 會收而 JS 不收的 `inf`／`nan`。
fn is_decimal_literal(t: &str) -> bool {
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i - int_start;
    let mut frac_digits = 0;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let s = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        frac_digits = i - s;
    }
    if int_digits == 0 && frac_digits == 0 {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let s = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == s {
            return false;
        }
    }
    i == b.len()
}

/// `Number.isInteger`。
pub fn is_integer(n: f64) -> bool {
    n.is_finite() && n.fract() == 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_formatting_matches_js() {
        assert_eq!(number_to_string(127848.0), "127848");
        assert_eq!(number_to_string(-132.0), "-132");
        assert_eq!(number_to_string(0.1), "0.1");
        assert_eq!(number_to_string(1549.5), "1549.5");
        assert_eq!(number_to_string(1e21), "1e+21");
        assert_eq!(number_to_string(1.5e-7), "1.5e-7");
        assert_eq!(number_to_string(0.000001), "0.000001");
        assert_eq!(number_to_string(-0.0), "0");
    }

    #[test]
    fn string_to_number_matches_js() {
        assert_eq!(string_to_number(""), 0.0);
        assert_eq!(string_to_number(" 12 "), 12.0);
        assert_eq!(string_to_number("0x10"), 16.0);
        assert_eq!(string_to_number(".5"), 0.5);
        assert_eq!(string_to_number("5."), 5.0);
        assert!(string_to_number("inf").is_nan());
        assert!(string_to_number("1,234").is_nan());
        assert!(string_to_number("Infinity").is_infinite());
    }
}
