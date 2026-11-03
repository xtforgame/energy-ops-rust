//! 地址 → 座標的邊界（來源：`common/geocoder.ts`）。
//!
//! 查不到就回 None，絕不回退成行政區中心點：看起來正常的錯座標比查不到危險得多。
//! `precision` 是一級欄位，不用 score 當門檻（實測 score 76.4 的座標是區公所）。

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::js_compat;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GeocodePrecision {
    /// 門牌／建物級 —— 可直接當初值。
    Exact,
    /// 路段級 —— 可當初值，建議人工確認。
    Street,
    /// 只到鄉鎮／行政區級 —— 不可當座標用，只能提示。
    Locality,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeocodeResult {
    pub latitude: f64,
    pub longitude: f64,
    pub precision: GeocodePrecision,
    /// provider 名 —— 一定要記，換 provider 時才知道哪些座標該重跑。
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_address: Option<String>,
    /// provider 原始回應，不 pin shape。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
}

/// 這個精度可不可以直接填進座標欄位。
pub fn is_usable_precision(p: GeocodePrecision) -> bool {
    matches!(p, GeocodePrecision::Exact | GeocodePrecision::Street)
}

struct Fixture {
    matcher: Regex,
    latitude: f64,
    longitude: f64,
    precision: GeocodePrecision,
    matched_address: &'static str,
}

/// fixture 對照表 —— 假資料，只為了讓 UI 與流程能被驗證。
static FIXTURES: LazyLock<Vec<Fixture>> = LazyLock::new(|| {
    let re = |s: &str| Regex::new(s).expect("fixture 規則是常數");
    vec![
        Fixture {
            matcher: re("屏東.*枋寮"),
            latitude: 22.3654,
            longitude: 120.5931,
            precision: GeocodePrecision::Exact,
            matched_address: "屏東縣枋寮鄉",
        },
        Fixture {
            matcher: re("嘉義.*六腳"),
            latitude: 23.4921,
            longitude: 120.3078,
            precision: GeocodePrecision::Street,
            matched_address: "嘉義縣六腳鄉",
        },
        // 故意的：地段地號只查得到「區」，precision=locality ＝ 不可填進座標。
        Fixture {
            matcher: re(r"地號|段\s*[0-9]"),
            latitude: 23.1417,
            longitude: 120.1394,
            precision: GeocodePrecision::Locality,
            matched_address: "（僅比對到行政區）",
        },
    ]
});

/// v1 的 mock geocoder。查不到回 None（不是錯誤，也不是給個大概的點）。
pub fn mock_geocoder(address: &str) -> Option<GeocodeResult> {
    let a = js_compat::trim(address);
    if a.is_empty() {
        return None;
    }
    let hit = FIXTURES.iter().find(|f| f.matcher.is_match(a))?;
    Some(GeocodeResult {
        latitude: hit.latitude,
        longitude: hit.longitude,
        precision: hit.precision,
        source: "mock".into(),
        matched_address: Some(hit.matched_address.into()),
        raw: Some(json!({ "fixture": true, "query": a })),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 來源：無對應 TS 測試；補驗 geocoder.ts 的 mockGeocoder／isUsablePrecision。
    #[test]
    fn mock_geocoder_never_guesses() {
        let r = mock_geocoder("屏東縣枋寮鄉中山路1號").expect("fixture");
        assert_eq!(r.precision, GeocodePrecision::Exact);
        assert!(is_usable_precision(r.precision));
        let land = mock_geocoder("台南市七股區大潭段123地號").expect("fixture");
        assert_eq!(land.precision, GeocodePrecision::Locality);
        assert!(!is_usable_precision(land.precision));
        assert_eq!(mock_geocoder("台北市信義區"), None);
        assert_eq!(mock_geocoder("   "), None);
    }
}
