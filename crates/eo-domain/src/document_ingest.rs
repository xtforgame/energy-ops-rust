//! 文件分類 —— 「這份掃描件屬於哪一個工作流？」（來源：`common/documentIngest.ts`）。
//!
//! 判不出來就標 `unclassified`，絕不猜：猜錯（整張單走錯流程、算錯錢）遠比多請人指定一次貴。
//! TS 的分類器是 async；這裡沒有 IO，改成同步函式。

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::js_compat;

/// 認得的文件型別。新增型別時要同時決定它走哪條流程（`doc_type_to_workflow`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EnergyDocType {
    /// 躉購·有餘電。台電付我們，我們要開發票。
    RenewableSurplus,
    /// 躉購·無餘電（只剩電表租費）。我們付台電。
    RenewableNoSurplus,
    /// 逆變器·僅設備發電使用電費。我們付台電。
    ConsumptionDevice,
    /// 逆變器·含電器租費。我們付台電。
    ConsumptionDeviceRental,
    /// 舊的粗分：躉購電費單。新資料不再產生，舊列與外部管線還在用。
    ElectricityBill,
    /// 舊的粗分：用電電費單（我們付台電），走的流程沒有計算節點。
    ConsumptionBill,
    DepositRequest,
    /// 售電業（陽光伏特家／阿波羅／英業達）賣綠電給我們的購售電單——我們要開發票。
    /// 不併進 `RenewableSurplus`：那是台電躉購單，RentCycle 會把它當台電躉購單處理。
    RetailerSale,
    /// 判不出來 —— 合法答案，不是錯誤。
    Unclassified,
}

impl EnergyDocType {
    pub const ALL: [EnergyDocType; 9] = [
        EnergyDocType::RenewableSurplus,
        EnergyDocType::RenewableNoSurplus,
        EnergyDocType::ConsumptionDevice,
        EnergyDocType::ConsumptionDeviceRental,
        EnergyDocType::ElectricityBill,
        EnergyDocType::ConsumptionBill,
        EnergyDocType::DepositRequest,
        EnergyDocType::RetailerSale,
        EnergyDocType::Unclassified,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            EnergyDocType::RenewableSurplus => "renewableSurplus",
            EnergyDocType::RenewableNoSurplus => "renewableNoSurplus",
            EnergyDocType::ConsumptionDevice => "consumptionDevice",
            EnergyDocType::ConsumptionDeviceRental => "consumptionDeviceRental",
            EnergyDocType::ElectricityBill => "electricityBill",
            EnergyDocType::ConsumptionBill => "consumptionBill",
            EnergyDocType::DepositRequest => "depositRequest",
            EnergyDocType::RetailerSale => "retailerSale",
            EnergyDocType::Unclassified => "unclassified",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == s)
    }
}

/// 辨識服務收得下的檔案型別。與附件的白名單不同（沒有 HEIC）——送 HEIC 過去會拿到永久的 415。
pub const RECOGNIZABLE_MIME_TYPES: [&str; 3] = ["application/pdf", "image/png", "image/jpeg"];

pub fn is_recognizable_mime(mime_type: &str) -> bool {
    RECOGNIZABLE_MIME_TYPES.contains(&mime_type.to_lowercase().as_str())
}

/// 人可以在校對台上指定的單別。順序＝下拉的順序。
pub const BILL_DOC_TYPES: [EnergyDocType; 5] = [
    EnergyDocType::RenewableSurplus,
    EnergyDocType::RenewableNoSurplus,
    EnergyDocType::ConsumptionDevice,
    EnergyDocType::ConsumptionDeviceRental,
    EnergyDocType::RetailerSale,
];

/// 細分 → 粗分（錢的方向）。綁定解析要用它回退，否則既有綁定在細分第一天全部失效。
pub fn doc_type_family(t: EnergyDocType) -> Option<EnergyDocType> {
    match t {
        EnergyDocType::RenewableSurplus | EnergyDocType::RetailerSale => Some(EnergyDocType::ElectricityBill),
        EnergyDocType::RenewableNoSurplus
        | EnergyDocType::ConsumptionDevice
        | EnergyDocType::ConsumptionDeviceRental => Some(EnergyDocType::ConsumptionBill),
        _ => None,
    }
}

/// 錢的方向：`Some(true)` ＝ 我們要付出去（不該算分潤）；保證金與未判定沒有這個屬性。
pub fn doc_type_is_payable(t: EnergyDocType) -> Option<bool> {
    match t {
        EnergyDocType::RenewableSurplus | EnergyDocType::ElectricityBill | EnergyDocType::RetailerSale => Some(false),
        EnergyDocType::RenewableNoSurplus
        | EnergyDocType::ConsumptionDevice
        | EnergyDocType::ConsumptionDeviceRental
        | EnergyDocType::ConsumptionBill => Some(true),
        _ => None,
    }
}

/// 這個單別會不會產生一張電費單（保證金請款只是觸發器）。
pub fn is_bill_doc_type(t: &str) -> bool {
    BILL_DOC_TYPES.iter().any(|d| d.as_str() == t) || t == "electricityBill" || t == "consumptionBill"
}

/// 解析綁定時要試的單別，由細到粗。順序不可顛倒：細分綁定代表這座案場對這種單有特別安排。
pub fn doc_type_lookup_chain(t: EnergyDocType) -> Vec<EnergyDocType> {
    match doc_type_family(t) {
        Some(f) => vec![t, f],
        None => vec![t],
    }
}

/// 單別 → 預設流程 key。這是預設值不是答案：送審一律以綁定為準。
pub fn doc_type_to_workflow(t: EnergyDocType) -> Option<&'static str> {
    match t {
        EnergyDocType::RenewableSurplus | EnergyDocType::ElectricityBill | EnergyDocType::RetailerSale => Some("bill-approval"),
        // 無餘電那張是我們付台電 —— 走付款流程，不是躉購核可流程。
        EnergyDocType::RenewableNoSurplus
        | EnergyDocType::ConsumptionDevice
        | EnergyDocType::ConsumptionDeviceRental
        | EnergyDocType::ConsumptionBill => Some("utility-bill"),
        EnergyDocType::DepositRequest => Some("deposit-request"),
        EnergyDocType::Unclassified => None,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassifyResult {
    pub doc_type: EnergyDocType,
    /// `unclassified` 時為 null —— 不回「預設流程」，那就是猜。
    pub workflow_key: Option<String>,
    /// 0–1。僅供顯示與排序，不可拿來當門檻。
    pub confidence: f64,
    /// 為什麼這樣判 —— 給人看的。
    pub reason: String,
    /// 命中的特徵（除錯用），不 pin shape。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signals: Option<Value>,
}

impl ClassifyResult {
    pub(crate) fn unclassified(reason: String, signals: Option<Value>) -> Self {
        Self { doc_type: EnergyDocType::Unclassified, workflow_key: None, confidence: 0.0, reason, signals }
    }
}

/// 保證金請款單抽出來的東西。刻意很窮：期數、金額、到期日的真相都在合約，這裡只是觸發器與佐證。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedDepositRequest {
    /// 合約編號原樣。對不上也要留著 —— 人工指認要靠它。
    pub contract_no_raw: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<i64>,
    /// 只用來交叉驗證，不覆蓋合約的金額。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
}

// ---------------------------------------------------------------------------
// v1 的規則式分類器
// ---------------------------------------------------------------------------

/// 把 OCR 輸出攤平成一段可搜尋的文字。不假設 provider 的 JSON 形狀。
fn flatten(v: &Value, depth: usize) -> String {
    if depth > 6 {
        return String::new();
    }
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Number(_) | Value::Bool(_) => js_compat::to_js_string(v),
        Value::Array(items) => items.iter().map(|x| flatten(x, depth + 1)).collect::<Vec<_>>().join(" "),
        Value::Object(map) => map
            .iter()
            .map(|(k, x)| format!("{} {}", k, flatten(x, depth + 1)))
            .collect::<Vec<_>>()
            .join(" "),
    }
}

struct Rule {
    doc_type: EnergyDocType,
    label: &'static str,
    patterns: Vec<Regex>,
}

/// 規則刻意保守：寧可判不出來，也不要判錯。`\d` 寫成 `[0-9]`：JS 的 `\d` 只認 ASCII。
static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    let re = |s: &str| Regex::new(s).expect("分類規則是常數，必須編得過");
    vec![
        Rule {
            doc_type: EnergyDocType::ElectricityBill,
            label: "電費單",
            patterns: vec![
                re("台灣電力|台電|電費通知|電費單"),
                re("(?i)customerNumber|電號"),
                re("(?i)用電度數|kwh|度數"),
                re("(?i)計費期間|billingPeriod"),
            ],
        },
        Rule {
            doc_type: EnergyDocType::DepositRequest,
            label: "保證金請款單",
            patterns: vec![
                re("保證金|履約保證|請款單"),
                re("(?i)合約編號|contractNo"),
                re(r"(?i)第\s*[0-9]+\s*期|installment|期別"),
                re("(?i)應付日|dueDate"),
            ],
        },
    ]
});

/// 至少要命中幾個特徵才敢說認得。調低之前先想清楚誤判的代價。
pub const CLASSIFY_MIN_HITS: usize = 2;

/// v1 規則式分類器。特徵不足、兩種平手、OCR 空白都回 `unclassified`（不 fallback 成電費單）。
pub fn mock_classifier(ocr_raw: &Value) -> ClassifyResult {
    let text = flatten(ocr_raw, 0);
    if js_compat::trim(&text).is_empty() {
        return ClassifyResult::unclassified("OCR 輸出是空的，無法判定".into(), None);
    }

    let mut scored: Vec<(&Rule, Vec<&Regex>)> = RULES
        .iter()
        .map(|r| (r, r.patterns.iter().filter(|p| p.is_match(&text)).collect()))
        .collect();
    // 穩定排序：同分時保留宣告順序（與 JS 的 Array.sort 相同）。
    scored.sort_by_key(|s| std::cmp::Reverse(s.1.len()));
    let signals_all = || {
        Value::Array(
            scored
                .iter()
                .map(|(r, hits)| json!({ "docType": r.doc_type.as_str(), "hits": hits.len() }))
                .collect(),
        )
    };

    let top = scored.first();
    let runner_up = scored.get(1);
    let top_hits = top.map(|t| t.1.len()).unwrap_or(0);
    let Some((top_rule, top_matched)) = top.filter(|_| top_hits >= CLASSIFY_MIN_HITS) else {
        return ClassifyResult::unclassified(
            format!("可辨識的特徵不足（最多只命中 {top_hits} 項，需要 {CLASSIFY_MIN_HITS} 項）"),
            Some(signals_all()),
        );
    };
    // 平手不猜 —— 兩種都像，代表特徵不足以區分。
    if let Some((ru_rule, ru_hits)) = runner_up {
        if ru_hits.len() == top_hits {
            return ClassifyResult::unclassified(
                format!("「{}」與「{}」的特徵一樣多，無法區分", top_rule.label, ru_rule.label),
                Some(signals_all()),
            );
        }
    }

    let ratio = top_hits as f64 / top_rule.patterns.len() as f64;
    ClassifyResult {
        doc_type: top_rule.doc_type,
        workflow_key: doc_type_to_workflow(top_rule.doc_type).map(str::to_string),
        // 只是「命中幾成特徵」（toFixed(2)），不是機率。
        confidence: (ratio * 100.0).round() / 100.0,
        reason: format!("命中「{}」的 {} 項特徵", top_rule.label, top_hits),
        signals: Some(json!({
            "matched": top_matched.iter().map(|p| js_source(p.as_str())).collect::<Vec<_>>(),
            "runnerUpHits": runner_up.map(|r| r.1.len()).unwrap_or(0),
        })),
    }
}

/// signals 裡印 JS 原本的 `RegExp.source`（不含 Rust 的 `(?i)` 旗標）。
fn js_source(pattern: &str) -> String {
    pattern.trim_start_matches("(?i)").replace("[0-9]", r"\d")
}

static CONTRACT_NO_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Z]{2}-[0-9]{4}-[0-9]{3,5}").expect("常數"));
static DIGITS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[0-9]+").expect("常數"));
static SEQ_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"第\s*([0-9]+)\s*期").expect("常數"));
static DATE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[0-9]{4}-[0-9]{2}-[0-9]{2}").expect("常數"));

/// v1 保證金請款單抽取器。抽不到合約編號就回空字串 —— 對不上是例外件，不是錯誤。
pub fn mock_deposit_request_extractor(ocr_raw: &Value) -> ExtractedDepositRequest {
    let text = flatten(ocr_raw, 0);
    let pick = |keys: &[&str]| -> String {
        for k in keys {
            if let Some(v) = ocr_raw.get(k).filter(|v| !v.is_null()) {
                let s = js_compat::to_js_string(v);
                let t = js_compat::trim(&s);
                if !t.is_empty() {
                    return t.to_string();
                }
            }
        }
        String::new()
    };

    let mut contract_no_raw = pick(&["contractNo", "contractNoRaw", "合約編號"]);
    if contract_no_raw.is_empty() {
        contract_no_raw = CONTRACT_NO_RE.find(&text).map(|m| m.as_str().to_string()).unwrap_or_default();
    }

    let seq_raw = pick(&["seq", "期別", "installmentSeq"]);
    let seq_digits = DIGITS_RE
        .find(&seq_raw)
        .map(|m| m.as_str().to_string())
        .or_else(|| SEQ_RE.captures(&text).and_then(|c| c.get(1)).map(|m| m.as_str().to_string()));
    // 位數超過 i64 的「期數」視同抽不到。
    let seq = seq_digits.and_then(|d| d.parse::<i64>().ok());

    let amount_raw = pick(&["amount", "請款金額", "金額"]);
    // 千分位要吃掉（同 billExtractor）；吃完是空的就當沒有。
    let amount = Some(amount_raw.replace(',', "")).filter(|a| !a.is_empty());

    let mut due_date = pick(&["dueDate", "應付日"]);
    if due_date.is_empty() {
        due_date = DATE_RE.find(&text).map(|m| m.as_str().to_string()).unwrap_or_default();
    }

    ExtractedDepositRequest {
        contract_no_raw,
        seq,
        amount,
        due_date: Some(due_date).filter(|d| !d.is_empty()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 來源：documentIngest.ts 的 mockClassifier（規則保守、平手不判）。
    #[test]
    fn mock_classifier_is_conservative() {
        let bill = json!({ "title": "台電 電費通知", "電號": "01-2233-4455", "用電度數": 100 });
        let r = mock_classifier(&bill);
        assert_eq!(r.doc_type, EnergyDocType::ElectricityBill);
        assert_eq!(r.workflow_key.as_deref(), Some("bill-approval"));
        assert_eq!(r.confidence, 0.75);

        let weak = json!({ "note": "電號 only" });
        assert_eq!(mock_classifier(&weak).doc_type, EnergyDocType::Unclassified);

        let tie = json!({ "a": "台電 電號 保證金 合約編號" });
        let r = mock_classifier(&tie);
        assert_eq!(r.doc_type, EnergyDocType::Unclassified);
        assert!(r.reason.contains("無法區分"));

        assert_eq!(mock_classifier(&Value::Null).doc_type, EnergyDocType::Unclassified);
    }

    /// 來源：documentIngest.ts 的 mockDepositRequestExtractor。
    #[test]
    fn mock_deposit_extractor_picks_fields() {
        let r = mock_deposit_request_extractor(&json!({
            "text": "保證金請款單 合約編號 AB-2026-0012 第 3 期 應付日 2026-05-01",
            "金額": "1,200,000",
        }));
        assert_eq!(r.contract_no_raw, "AB-2026-0012");
        assert_eq!(r.seq, Some(3));
        assert_eq!(r.amount.as_deref(), Some("1200000"));
        assert_eq!(r.due_date.as_deref(), Some("2026-05-01"));

        let empty = mock_deposit_request_extractor(&json!({}));
        assert_eq!(empty, ExtractedDepositRequest::default());
    }

    /// 來源：無對應 TS 測試；補驗 documentIngest.ts 的 docTypeLookupChain／isRecognizableMime／isBillDocType。
    #[test]
    fn lookup_chain_goes_fine_to_coarse() {
        assert_eq!(
            doc_type_lookup_chain(EnergyDocType::RenewableSurplus),
            vec![EnergyDocType::RenewableSurplus, EnergyDocType::ElectricityBill]
        );
        assert_eq!(doc_type_lookup_chain(EnergyDocType::DepositRequest), vec![EnergyDocType::DepositRequest]);
        assert!(is_recognizable_mime("APPLICATION/PDF"));
        assert!(!is_recognizable_mime("image/heic"));
        assert!(is_bill_doc_type("consumptionBill"));
        assert!(!is_bill_doc_type("depositRequest"));
    }
}
