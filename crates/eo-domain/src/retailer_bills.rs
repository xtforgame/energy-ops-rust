//! 三家售電業（陽光伏特家、阿波羅、英業達）的購售電單：本機抽 PDF 文字層（含座標）、
//! 認版型、組出跟外部辨識服務一樣格式的 canonical document。
//!
//! 設計依據：DESIGN.md「設計（乙）」1、「三份單的版面」。命中這裡的版型，平台就不送外部
//! taipower-recognition 服務（見 `eo-energy::ingest::recognition::send_chunk`）；認不得的（台電單、
//! 其他單）回 `None`，照舊送服務，一行都不改。
//!
//! 只讀主單頁：三家的主單頁範圍（陽光伏特家 1–3、阿波羅 1–2、英業達 1）直接來自 DESIGN.md
//! 的「三份單的版面」表，用頁碼範圍過濾即可做到「遇到費用清單／繳款通知單頁就停」，不用再從
//! 內容判斷停在哪一頁。

use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

use crate::taipower_canonical::{
    CanonicalDocument, CanonicalDocumentSuccess, CanonicalField, CanonicalFieldStatus, CanonicalRecord,
    CanonicalValue, ExtractionMethod,
};

pub const TEMPLATE_RETAILER_SUNSHINE_VOLT: &str = "retailer_sunshine_volt";
pub const TEMPLATE_RETAILER_APOLLO: &str = "retailer_apollo";
pub const TEMPLATE_RETAILER_INVENTEC: &str = "retailer_inventec";

// ---------------------------------------------------------------------------
// 抽字：OutputDev 收每個字的座標，重建「視覺列」
// ---------------------------------------------------------------------------

/// 一個字連同它在頁面上的座標（已經做過上下翻轉：數值越大越往頁面下方）。
#[derive(Debug, Clone)]
struct Cell {
    page: u32,
    x: f64,
    y: f64,
    text: String,
}

struct Collector {
    cur_page: u32,
    flip_ctm: pdf_extract::Transform,
    cells: Vec<Cell>,
}

impl pdf_extract::OutputDev for Collector {
    fn begin_page(
        &mut self,
        page_num: u32,
        media_box: &pdf_extract::MediaBox,
        _art_box: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), pdf_extract::OutputError> {
        self.cur_page = page_num;
        self.flip_ctm = pdf_extract::Transform::row_major(1., 0., 0., -1., 0., media_box.ury - media_box.lly);
        Ok(())
    }
    fn end_page(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }
    fn output_character(
        &mut self,
        trm: &pdf_extract::Transform,
        _width: f64,
        _spacing: f64,
        _font_size: f64,
        char: &str,
    ) -> Result<(), pdf_extract::OutputError> {
        if !char.trim().is_empty() {
            let p = trm.post_transform(&self.flip_ctm);
            // NFKC：陽光伏特家的字型吐出康熙部首字形（⾴、⽉…），這裡先正規化掉。
            self.cells.push(Cell { page: self.cur_page, x: p.m31, y: p.m32, text: char.nfkc().collect() });
        }
        Ok(())
    }
    fn begin_word(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }
    fn end_word(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }
    fn end_line(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }
}

/// 一頁裡的一條「視覺列」：同一列的字已經依 x 排好序串成一串文字；數字之間的空白已合併。
#[derive(Debug, Clone)]
pub struct PageRow {
    pub page: u32,
    pub y: f64,
    pub text: String,
}

pub type PageTokens = Vec<PageRow>;

/// 同一列的容許 y 誤差（pt）。小於這個差值視為同一條視覺列。
/// 實測阿波羅真實 PDF：同一欄的標籤與值常常相差 2.5～2.6pt（不同基線），
/// 不同欄位之間的間距都在 17pt 以上，6pt 兩邊都安全。
const ROW_TOLERANCE: f64 = 6.0;

/// 合併數字之間的空白：pdf-extract 有時把一個數字拆成一格一格的字，中間夾著空白。
/// 只合併「數字—空白—數字」這種情形，標籤與值之間的空白（非數字接非數字）不動。
fn merge_digit_gaps(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            let prev_digit = out.chars().last().is_some_and(|d| d.is_ascii_digit());
            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            let next_digit = chars.get(j).is_some_and(|d| d.is_ascii_digit());
            if prev_digit && next_digit {
                i = j;
                continue;
            }
            out.push(' ');
            i = j;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn push_row(page: u32, current: &mut Vec<Cell>, rows: &mut Vec<PageRow>) {
    if current.is_empty() {
        return;
    }
    current.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
    let y = current[0].y;
    let joined: String = current.iter().map(|c| c.text.as_str()).collect();
    rows.push(PageRow { page, y, text: merge_digit_gaps(&joined) });
    current.clear();
}

/// 抽出整份 PDF 的視覺列：用座標把同一列的字重新排好序，不依賴 PDF 文字流原本的順序
/// （英業達的文字流是「標籤一串、數值一串」，順序靠不住）。
pub fn extract_pages(bytes: &[u8]) -> Result<PageTokens, String> {
    let doc = pdf_extract::Document::load_mem(bytes).map_err(|e| e.to_string())?;
    let mut collector = Collector { cur_page: 0, flip_ctm: pdf_extract::Transform::identity(), cells: Vec::new() };
    pdf_extract::output_doc(&doc, &mut collector).map_err(|e| e.to_string())?;

    let mut by_page: BTreeMap<u32, Vec<Cell>> = BTreeMap::new();
    for c in collector.cells {
        by_page.entry(c.page).or_default().push(c);
    }

    let mut rows = Vec::new();
    for (page, mut cells) in by_page {
        cells.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal));
        let mut current: Vec<Cell> = Vec::new();
        for c in cells {
            if let Some(last) = current.last() {
                if (c.y - last.y).abs() > ROW_TOLERANCE {
                    push_row(page, &mut current, &mut rows);
                }
            }
            current.push(c);
        }
        push_row(page, &mut current, &mut rows);
    }
    Ok(rows)
}

// ---------------------------------------------------------------------------
// 版型路由：三組錨點都要全中才算命中
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetailerTemplate {
    SunshineVolt,
    Apollo,
    Inventec,
}

impl RetailerTemplate {
    pub fn template_id(self) -> &'static str {
        match self {
            RetailerTemplate::SunshineVolt => TEMPLATE_RETAILER_SUNSHINE_VOLT,
            RetailerTemplate::Apollo => TEMPLATE_RETAILER_APOLLO,
            RetailerTemplate::Inventec => TEMPLATE_RETAILER_INVENTEC,
        }
    }
}

fn joined_text(pages: &PageTokens, page_range: RangeInclusive<u32>) -> String {
    pages.iter().filter(|r| page_range.contains(&r.page)).map(|r| r.text.as_str()).collect::<Vec<_>>().join("\n")
}

fn all_hit(haystack: &str, anchors: &[&str]) -> bool {
    anchors.iter().all(|a| haystack.contains(a))
}

/// 只看主單頁的標題與固定標籤；三組錨點都要全中才算命中，不全中就回 None，交給服務處理。
/// 錨點字樣直接取自 DESIGN.md「三份單的版面」表（標籤，不是真實數字）。
pub fn route_retailer(pages: &PageTokens) -> Option<RetailerTemplate> {
    let head = joined_text(pages, 1..=3);
    if all_hit(&head, &["電廠電費單", "場所電號", "企業統編"]) {
        return Some(RetailerTemplate::SunshineVolt);
    }
    // 「發票申請單」只是 DESIGN.md 表裡給這張單的名字，紙上實際印的是公司全名
    // （實測：`阿波羅電力股份有限公司`，沒有「發票申請單」四個字）。
    if all_hit(&head, &["阿波羅電力股份有限公司", "本期轉供度數", "供應商資訊"]) {
        return Some(RetailerTemplate::Apollo);
    }
    if all_hit(&head, &["電能使用電費通知單", "本期發電度數", "用電戶名稱"]) {
        return Some(RetailerTemplate::Inventec);
    }
    None
}

// ---------------------------------------------------------------------------
// 小工具：在視覺列裡找「標籤右邊的值」
// ---------------------------------------------------------------------------

/// 同一個標籤常常出現不只一次（例如表格先印一行欄位標題，後面才是真的那一列有值的）。
/// 標籤右邊沒有任何數字的那次，當作「只是標題」跳過，繼續找下一次出現；全部都沒有數字時，
/// 回第一次那個（空的）給呼叫端自己判斷。
fn value_after<'a>(pages: &'a PageTokens, page_range: RangeInclusive<u32>, label: &str) -> Option<&'a str> {
    let mut first_match: Option<&'a str> = None;
    for row in pages.iter().filter(|r| page_range.contains(&r.page)) {
        if let Some(idx) = row.text.find(label) {
            let after = &row.text[idx + label.len()..];
            if after.chars().any(|c| c.is_ascii_digit()) {
                return Some(after);
            }
            first_match.get_or_insert(after);
        }
    }
    first_match
}

/// 跟 `value_after`一樣先看標籤右邊；右邊沒有看起來像數字或日期的東西（常見於表單先印一列
/// 全是標籤、值在下一列才出現，例如阿波羅「本期轉供度數／購電費率」那一列），就退回去看
/// 同一頁範圍內緊接著的下一條視覺列。
fn value_after_or_next_row(pages: &PageTokens, page_range: RangeInclusive<u32>, label: &str) -> Option<String> {
    let rows: Vec<&PageRow> = pages.iter().filter(|r| page_range.contains(&r.page)).collect();
    for (i, row) in rows.iter().enumerate() {
        let Some(idx) = row.text.find(label) else { continue };
        let after = row.text[idx + label.len()..].trim();
        if NUMBER_RE.is_match(after) || DATE_TOKEN_RE.is_match(after) {
            return Some(after.to_string());
        }
        if let Some(next) = rows.get(i + 1) {
            return Some(next.text.clone());
        }
        return None;
    }
    None
}

/// 含標籤的那一列文字，和緊接著下一列的文字（如果有）。
fn row_and_next_containing(pages: &PageTokens, page_range: RangeInclusive<u32>, label: &str) -> Option<(String, Option<String>)> {
    let rows: Vec<&PageRow> = pages.iter().filter(|r| page_range.contains(&r.page)).collect();
    let i = rows.iter().position(|r| r.text.contains(label))?;
    Some((rows[i].text.clone(), rows.get(i + 1).map(|r| r.text.clone())))
}

fn numbers_in(s: &str) -> Vec<f64> {
    NUMBER_RE.find_iter(s).filter_map(|m| m.as_str().replace(',', "").parse().ok()).collect()
}

static NUMBER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-?[0-9][0-9,]*(?:\.[0-9]+)?").expect("常數"));
static TAX_ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d{8}").expect("常數"));
static ACCOUNT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d{2}-\d{4}-\d{4}").expect("常數"));
/// 電號沒有加破折號的版型（實測阿波羅：連續 11 位數字，沒有破折號）。
static ACCOUNT_PLAIN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d{8,12}").expect("常數"));
static DATE_TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d{2,4}[-/年]\d{1,2}[-/月]\d{1,2}日?").expect("常數"));
static DATE_PARTS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d{2,4})[-/年](\d{1,2})[-/月](\d{1,2})日?").expect("常數"));

fn first_number(s: &str) -> Option<f64> {
    let m = NUMBER_RE.find(s)?;
    m.as_str().replace(',', "").parse::<f64>().ok()
}

fn first_tax_id(s: &str) -> Option<String> {
    TAX_ID_RE.find(s).map(|m| m.as_str().to_string())
}

fn first_account_number(s: &str) -> Option<String> {
    ACCOUNT_RE.find(s).map(|m| m.as_str().to_string()).or_else(|| ACCOUNT_PLAIN_RE.find(s).map(|m| m.as_str().to_string()))
}

/// 金額後面緊接著稅率（例如「2,5005%」裡的「5%」）沒有分隔符號時，把最後一個數字
/// 當成稅率去掉，不要把它併進金額——阿波羅「營業稅」那一列的備註就是這樣接在金額後面。
fn money_before_percent_note(s: &str) -> Option<f64> {
    let m = NUMBER_RE.find(s)?;
    let mut raw = m.as_str().to_string();
    let rest = &s[m.end()..];
    if rest.starts_with('%') && raw.len() > 1 && raw.chars().last().is_some_and(|c| c.is_ascii_digit()) {
        raw.pop();
        while raw.ends_with(',') {
            raw.pop();
        }
    }
    raw.replace(',', "").parse::<f64>().ok()
}

/// 單個日期：西元 `YYYY-MM-DD`／`YYYY/MM/DD`，或民國 `YYY/MM/DD`、`YYY年MM月DD日`（年補 1911）。
fn parse_date_token(tok: &str) -> Option<String> {
    let caps = DATE_PARTS_RE.captures(tok)?;
    let mut y: i64 = caps[1].parse().ok()?;
    if y < 1000 {
        y += 1911;
    }
    let m: u32 = caps[2].parse().ok()?;
    let d: u32 = caps[3].parse().ok()?;
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

/// 一段文字裡的頭兩個日期當作「起、訖」。
fn parse_date_range(s: &str) -> (Option<String>, Option<String>) {
    let mut matches = DATE_TOKEN_RE.find_iter(s);
    let start = matches.next().and_then(|m| parse_date_token(m.as_str()));
    let end = matches.next().and_then(|m| parse_date_token(m.as_str()));
    (start, end)
}

// ---------------------------------------------------------------------------
// 三個解析函式：只讀主單頁
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct ParsedFields {
    purchase_kwh: Option<f64>,
    purchase_energy_fee: Option<f64>,
    business_tax: Option<f64>,
    payable_total: Option<f64>,
    billing_start: Option<String>,
    billing_end: Option<String>,
    account_number: Option<String>,
    /// 賣方（我方）。
    company_tax_id: Option<String>,
    /// 買方。
    buyer_tax_id: Option<String>,
}

/// 陽光伏特家「電廠電費單」：主單 1–3 頁，附件（4–17 台電費用清單）不讀。
fn parse_sunshine_volt(pages: &PageTokens) -> ParsedFields {
    const MAIN: RangeInclusive<u32> = 1..=3;
    let mut f = ParsedFields::default();
    if let Some(s) = value_after(pages, MAIN, "計費區間") {
        let (start, end) = parse_date_range(s);
        f.billing_start = start;
        f.billing_end = end;
    }
    // 「綠能電費」那一列常常只有品項名稱／單位費率，度數在下一列的開頭（實測：品項名稱與
    // 數字欄位不在同一條視覺列）；這一列數字不到兩個就改看下一列，取第一個數字當度數。
    // 費用本身不靠這一列（度數跟單位費率黏在一起時，中間常被逗號切得不乾淨），改用
    // 「小計（含稅）－營業稅＝未稅費用」反推，跟 S6 回歸用的台電單是同一條稽核邏輯
    // （未稅＋稅＝含稅），比直接切這一列數字可靠。
    if let Some(rows) = row_and_next_containing(pages, MAIN, "綠能電費") {
        let same_row_nums: Vec<f64> = numbers_in(&rows.0);
        let text = if same_row_nums.len() >= 2 { rows.0 } else { rows.1.unwrap_or(rows.0) };
        f.purchase_kwh = numbers_in(&text).first().copied();
    }
    f.business_tax = value_after(pages, MAIN, "營業稅").and_then(first_number);
    f.payable_total = value_after(pages, MAIN, "小計").and_then(first_number);
    f.purchase_energy_fee = match (f.payable_total, f.business_tax) {
        (Some(total), Some(tax)) => Some(total - tax),
        _ => None,
    };
    // 場所電號：第 2 頁是「場所電號／場所別名／本期度數」表頭，電號在下一列的開頭（實測）。
    f.account_number = value_after_or_next_row(pages, 2..=2, "場所電號").as_deref().and_then(first_account_number);
    // 第 1 頁（跑馬頭也會在每頁重複出現）「企業統編」＝買方；第 3 頁「發票開立資訊」底下的
    // 「統編:」（有冒號，跟頭字「企業統編」沒冒號不同，不會兩個標籤互相打架）＝賣方（我方）。
    f.buyer_tax_id = value_after(pages, 1..=1, "企業統編").and_then(first_tax_id);
    f.company_tax_id = value_after(pages, 3..=3, "統編:").and_then(first_tax_id);
    f
}

/// 阿波羅「發票申請單」：主單 1–2 頁，附件（3–4 繳款通知單、5–8 費用清單）不讀——
/// 繳款通知單自己有一組未稅／營業稅，不可以混進主單。
fn parse_apollo(pages: &PageTokens) -> ParsedFields {
    const MAIN: RangeInclusive<u32> = 1..=2;
    let mut f = ParsedFields::default();
    if let Some(s) = value_after(pages, MAIN, "計費期間") {
        let (start, end) = parse_date_range(s);
        f.billing_start = start;
        f.billing_end = end;
    }
    // 「本期轉供度數」「購電費率(未稅)」是同一列的兩個欄位標題，值在下一列才出現
    // （實測：標籤列 y、值列 y 差 31pt，不是標籤旁邊）。
    f.purchase_kwh = value_after_or_next_row(pages, MAIN, "本期轉供度數").as_deref().and_then(first_number);
    // 「本期購電價金」這一列是「電費計算」表裡的一行，標籤跟金額同一列，後面接著「未稅」字樣。
    f.purchase_energy_fee = value_after(pages, MAIN, "本期購電價金").and_then(first_number);
    // 「營業稅」那一列備註是稅率（例如 5%），直接接在金額後面沒有分隔符號。
    f.business_tax = value_after(pages, MAIN, "營業稅").and_then(money_before_percent_note);
    f.payable_total = value_after(pages, MAIN, "總金額").and_then(first_number);
    // 第 1、2 頁的「電號」是同一個；這個版型電號沒有破折號。
    f.account_number = value_after(pages, MAIN, "電號").and_then(first_account_number);
    let (buyer, seller) = apollo_tax_ids(pages, MAIN);
    f.buyer_tax_id = buyer;
    f.company_tax_id = seller;
    f
}

/// 表頭（供應商資訊之前）的「統一編號」＝買方；「供應商資訊」區塊之後的「統一編號」＝賣方
/// （我方）。靠**先後順序**分（供應商資訊是一行獨立的區塊標題，不會跟統一編號同一列），
/// 不是看同一列有沒有「供應商資訊」字樣。
fn apollo_tax_ids(pages: &PageTokens, page_range: RangeInclusive<u32>) -> (Option<String>, Option<String>) {
    let mut buyer = None;
    let mut seller = None;
    let mut in_vendor_section = false;
    for row in pages.iter().filter(|r| page_range.contains(&r.page)) {
        if row.text.contains("供應商資訊") {
            in_vendor_section = true;
        }
        let Some(idx) = row.text.find("統一編號") else { continue };
        let after = &row.text[idx + "統一編號".len()..];
        let Some(id) = first_tax_id(after) else { continue };
        if in_vendor_section {
            seller.get_or_insert(id);
        } else {
            buyer.get_or_insert(id);
        }
    }
    (buyer, seller)
}

/// 英業達「電能使用電費通知單」：單頁，沒有電號。本期發電度數，不是累積度數、也不是
/// 憑證中心回傳量——只認「本期發電度數」這個完整標籤。
fn parse_inventec(pages: &PageTokens) -> ParsedFields {
    const MAIN: RangeInclusive<u32> = 1..=1;
    let mut f = ParsedFields::default();
    if let Some(s) = value_after(pages, MAIN, "計費期間") {
        let (start, end) = parse_date_range(s);
        f.billing_start = start;
        f.billing_end = end;
    }
    // 「本期發電度數」是獨立一列的標籤（實測），值在下一列才出現。
    f.purchase_kwh = value_after_or_next_row(pages, MAIN, "本期發電度數").as_deref().and_then(first_number);
    f.purchase_energy_fee = value_after(pages, MAIN, "電能費用").and_then(first_number);
    f.business_tax = value_after(pages, MAIN, "營業稅").and_then(first_number);
    f.payable_total = value_after(pages, MAIN, "本期應付總金額").and_then(first_number);
    // 「用電戶名稱」是表頭（這一列只有欄名、沒有值），買方公司名稱＋統編在下一列。
    f.buyer_tax_id = tax_id_near_label(pages, MAIN, "用電戶名稱");
    for row in pages.iter().filter(|r| MAIN.contains(&r.page)) {
        if row.text.contains("統一編號") && !row.text.contains("用電戶名稱") {
            if let Some(id) = first_tax_id(&row.text) {
                f.company_tax_id = Some(id);
                break;
            }
        }
    }
    f
}

/// 跟 `value_after_or_next_row` 同一招，但用於整段都在「值」裡找統編（不是標籤右邊接著值，
/// 是標籤那一列只有欄名，統編在下一列的某處）。
fn tax_id_near_label(pages: &PageTokens, page_range: RangeInclusive<u32>, label: &str) -> Option<String> {
    let rows: Vec<&PageRow> = pages.iter().filter(|r| page_range.contains(&r.page)).collect();
    for (i, row) in rows.iter().enumerate() {
        if !row.text.contains(label) {
            continue;
        }
        if let Some(id) = first_tax_id(&row.text) {
            return Some(id);
        }
        if let Some(next) = rows.get(i + 1) {
            return first_tax_id(&next.text);
        }
        return None;
    }
    None
}

// ---------------------------------------------------------------------------
// 組 canonical：格式要跟外部服務回的一樣，`as_canonical_document` 要讀得回來
// ---------------------------------------------------------------------------

fn field_text(v: Option<String>) -> CanonicalField {
    match v {
        Some(s) if !s.is_empty() => {
            CanonicalField { value: Some(CanonicalValue::Text(s.clone())), raw_value: s, status: CanonicalFieldStatus::Extracted, evidence: None }
        }
        _ => CanonicalField { value: None, raw_value: String::new(), status: CanonicalFieldStatus::Missing, evidence: None },
    }
}

fn field_number(v: Option<f64>) -> CanonicalField {
    match v.and_then(serde_json::Number::from_f64) {
        Some(n) => CanonicalField {
            value: Some(CanonicalValue::Number(n.clone())),
            raw_value: n.to_string(),
            status: CanonicalFieldStatus::Extracted,
            evidence: None,
        },
        None => CanonicalField { value: None, raw_value: String::new(), status: CanonicalFieldStatus::Missing, evidence: None },
    }
}

fn field_not_applicable() -> CanonicalField {
    CanonicalField { value: None, raw_value: String::new(), status: CanonicalFieldStatus::NotApplicable, evidence: None }
}

fn to_canonical(template: RetailerTemplate, fields: &ParsedFields, source_file_id: &str, source_file_name: &str) -> Value {
    let mut map = BTreeMap::new();
    map.insert("purchaseKwh".to_string(), field_number(fields.purchase_kwh));
    map.insert("purchaseEnergyFee".to_string(), field_number(fields.purchase_energy_fee));
    map.insert("businessTax".to_string(), field_number(fields.business_tax));
    map.insert("payableTotal".to_string(), field_number(fields.payable_total));
    map.insert("billingStartDate".to_string(), field_text(fields.billing_start.clone()));
    map.insert("billingEndDate".to_string(), field_text(fields.billing_end.clone()));
    // 英業達這個版型沒有電號，標 not_applicable；不是抽不到才是 missing。
    map.insert(
        "accountNumber".to_string(),
        if template == RetailerTemplate::Inventec { field_not_applicable() } else { field_text(fields.account_number.clone()) },
    );
    map.insert("companyTaxId".to_string(), field_text(fields.company_tax_id.clone()));
    map.insert("buyerTaxId".to_string(), field_text(fields.buyer_tax_id.clone()));

    let record = CanonicalRecord {
        fields: map,
        extraction_method: Some(ExtractionMethod::PdfText),
        review_status: String::new(),
        template_id: Some(template.template_id().to_string()),
    };
    let doc = CanonicalDocument::Succeeded(CanonicalDocumentSuccess {
        source_file_id: source_file_id.to_string(),
        source_file_name: source_file_name.to_string(),
        record,
        issues: Vec::new(),
    });
    serde_json::to_value(&doc).expect("CanonicalDocument 一定序列化得出來")
}

/// 給 `recognition.rs` 的 send_chunk 用：PDF 位元組命中售電版型就回本機組好的 canonical；
/// 讀不出字、或三組錨點沒有全中，回 `None`——交給外部服務照舊處理。
pub fn recognize_retailer_pdf(bytes: &[u8], source_file_id: &str, source_file_name: &str) -> Option<Value> {
    let pages = extract_pages(bytes).ok()?;
    let template = route_retailer(&pages)?;
    let fields = match template {
        RetailerTemplate::SunshineVolt => parse_sunshine_volt(&pages),
        RetailerTemplate::Apollo => parse_apollo(&pages),
        RetailerTemplate::Inventec => parse_inventec(&pages),
    };
    Some(to_canonical(template, &fields, source_file_id, source_file_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taipower_extractor::as_canonical_document;

    fn row(page: u32, y: f64, text: &str) -> PageRow {
        PageRow { page, y, text: text.to_string() }
    }

    #[test]
    fn route_needs_all_three_anchors() {
        let pages = vec![row(1, 10.0, "電廠電費單 場所電號")];
        // 只有兩組錨點，不算命中。
        assert_eq!(route_retailer(&pages), None);
        let pages = vec![row(1, 10.0, "電廠電費單 場所電號 企業統編")];
        assert_eq!(route_retailer(&pages), Some(RetailerTemplate::SunshineVolt));
    }

    #[test]
    fn route_does_not_misfire_on_taipower_bill() {
        let pages = vec![row(1, 10.0, "台灣電力公司 電費通知 電號 01-2233-4455 用電度數 計費期間")];
        assert_eq!(route_retailer(&pages), None);
    }

    #[test]
    fn merge_digit_gaps_only_joins_digit_runs() {
        assert_eq!(merge_digit_gaps("1 2 3"), "123");
        assert_eq!(merge_digit_gaps("標籤 1 2 3"), "標籤 123");
        assert_eq!(merge_digit_gaps("1 2 標籤"), "12 標籤");
    }

    #[test]
    fn parse_sunshine_volt_fake_pages_round_trip() {
        let pages = vec![
            row(1, 10.0, "電廠電費單 企業統編12345678 場所電號"),
            row(1, 20.0, "計費區間 2026-01-01 至 2026-01-31"),
            row(1, 30.0, "綠能電費 度數 1,000 費用 50,000"),
            row(1, 40.0, "營業稅 2,500"),
            row(1, 50.0, "小計 52,500"),
            row(2, 10.0, "場所電號 01-2233-4455"),
            row(3, 10.0, "發票開立資訊 統編:87654321"),
        ];
        assert_eq!(route_retailer(&pages), Some(RetailerTemplate::SunshineVolt));
        let fields = parse_sunshine_volt(&pages);
        assert_eq!(fields.purchase_kwh, Some(1000.0));
        assert_eq!(fields.purchase_energy_fee, Some(50000.0));
        assert_eq!(fields.business_tax, Some(2500.0));
        assert_eq!(fields.payable_total, Some(52500.0));
        assert_eq!(fields.billing_start.as_deref(), Some("2026-01-01"));
        assert_eq!(fields.billing_end.as_deref(), Some("2026-01-31"));
        assert_eq!(fields.account_number.as_deref(), Some("01-2233-4455"));
        assert_eq!(fields.buyer_tax_id.as_deref(), Some("12345678"));
        assert_eq!(fields.company_tax_id.as_deref(), Some("87654321"));

        let canonical = to_canonical(RetailerTemplate::SunshineVolt, &fields, "1", "a.pdf");
        let doc = as_canonical_document(&canonical).expect("要讀得回來");
        let CanonicalDocument::Succeeded(s) = doc else { panic!("應為 succeeded") };
        assert_eq!(s.record.template_id.as_deref(), Some(TEMPLATE_RETAILER_SUNSHINE_VOLT));
    }

    #[test]
    fn parse_apollo_separates_buyer_and_seller_tax_id() {
        let pages = vec![
            row(1, 10.0, "阿波羅電力股份有限公司 統一編號11112222"),
            row(1, 20.0, "計費期間 2026-02-01 ~ 2026-02-28"),
            row(1, 30.0, "本期轉供度數 2,000"),
            row(1, 40.0, "本期購電價金（未稅） 80,000"),
            row(1, 50.0, "營業稅 4,000"),
            row(1, 60.0, "總金額（含稅） 84,000"),
            row(1, 70.0, "電號 02-3344-5566"),
            row(2, 10.0, "供應商資訊 統一編號33334444"),
            row(2, 20.0, "本期轉供度數"),
        ];
        assert_eq!(route_retailer(&pages), Some(RetailerTemplate::Apollo));
        let fields = parse_apollo(&pages);
        assert_eq!(fields.purchase_kwh, Some(2000.0));
        assert_eq!(fields.purchase_energy_fee, Some(80000.0));
        assert_eq!(fields.business_tax, Some(4000.0));
        assert_eq!(fields.payable_total, Some(84000.0));
        assert_eq!(fields.account_number.as_deref(), Some("02-3344-5566"));
        assert_eq!(fields.buyer_tax_id.as_deref(), Some("11112222"));
        assert_eq!(fields.company_tax_id.as_deref(), Some("33334444"));
    }

    #[test]
    fn parse_apollo_ignores_attachment_pages() {
        // 第 3 頁是繳款通知單（附件），有自己的一組未稅／營業稅，不可以混進主單。
        let pages = vec![
            row(1, 10.0, "阿波羅電力股份有限公司 統一編號11112222"),
            row(1, 20.0, "本期購電價金（未稅） 80,000"),
            row(1, 30.0, "供應商資訊 統一編號33334444"),
            row(3, 10.0, "繳款通知單 未稅 999,999 營業稅 999"),
        ];
        let fields = parse_apollo(&pages);
        assert_eq!(fields.purchase_energy_fee, Some(80000.0));
        assert_eq!(fields.business_tax, None, "附件頁的營業稅不可以混進主單");
    }

    #[test]
    fn parse_inventec_has_no_account_number_and_not_cumulative_kwh() {
        let pages = vec![
            row(1, 10.0, "電能使用電費通知單"),
            row(1, 20.0, "計費期間 2026-03-01 ~ 2026-03-31"),
            row(1, 30.0, "累積度數 999999"),
            row(1, 40.0, "本期發電度數 3,000"),
            row(1, 50.0, "電能費用 90,000"),
            row(1, 60.0, "營業稅 4,500"),
            row(1, 70.0, "本期應付總金額 94,500"),
            row(1, 80.0, "用電戶名稱／統一編號 某某公司22223333"),
            row(1, 90.0, "統一編號44445555"),
        ];
        assert_eq!(route_retailer(&pages), Some(RetailerTemplate::Inventec));
        let fields = parse_inventec(&pages);
        assert_eq!(fields.purchase_kwh, Some(3000.0), "要取本期發電度數，不是累積度數");
        assert_eq!(fields.purchase_energy_fee, Some(90000.0));
        assert_eq!(fields.business_tax, Some(4500.0));
        assert_eq!(fields.payable_total, Some(94500.0));
        assert_eq!(fields.buyer_tax_id.as_deref(), Some("22223333"));
        assert_eq!(fields.company_tax_id.as_deref(), Some("44445555"));
        assert_eq!(fields.account_number, None);

        let canonical = to_canonical(RetailerTemplate::Inventec, &fields, "1", "b.pdf");
        let doc = as_canonical_document(&canonical).expect("要讀得回來");
        let CanonicalDocument::Succeeded(s) = doc else { panic!("應為 succeeded") };
        let account = s.record.fields.get("accountNumber").expect("要有這個鍵");
        assert_eq!(account.status, CanonicalFieldStatus::NotApplicable);
        assert!(account.value.is_none());
    }
}
