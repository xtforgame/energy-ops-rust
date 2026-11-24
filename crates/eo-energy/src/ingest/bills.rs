//! 電費單：辨識結果 → `energyBill`（三層值）、人工指認、校對、清單與詳情（照 App 5 `bills/BillIngestService.ts`）。
//!
//! 兩件事寫錯了會很久以後才爆：
//! 1. 三層值分開存：`ocrRaw`（整包）／`ocrExtracted`（抽出值）／最終值（可編輯欄位）。校對只動最終值。
//! 2. 電號對不上＝例外件，不是錯誤：`station_id` 留 NULL、`reviewStatus='pendingMatch'`，等人工指認。

use super::route::{js_num_str, js_number, js_str, money, walk_route};
use crate::audit::{self, AuditInput};
use crate::common::{day, day_opt, err, err_d, Ctx, Tx};
use crate::workflow::{self, ResolvedFlow};
use chrono::{DateTime, NaiveDate, Utc};
use eo_domain::bill_extractor::ExtractedBill;
use eo_domain::business_key::{bill_business_key, normalize_customer_number};
use eo_domain::document_ingest::EnergyDocType;
use eo_domain::route_preview::{gate_delta, gates_of};
use eo_domain::schemas::{correctable_doc_types, CorrectBillInput};
use eo_domain::taipower_extractor::{
    bill_doc_type, energy_bill_extractor, extracted_value_of, recognized_doc_type, BillDocTypeSource, ALL_BILL_REVIEW_FIELDS,
};
use eo_platform::error::ApiResult;
use eo_platform::rbac::OrgAccess;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde_json::{json, Map, Value};
use sqlx::FromRow;
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

#[derive(Debug, Clone, FromRow)]
pub struct BillRow {
    pub id: i64,
    pub customer_number_raw: Option<String>,
    pub ocr_raw: Value,
    pub ocr_extracted: Value,
    pub template_id: Option<String>,
    pub doc_type: Option<String>,
    pub due_date: Option<NaiveDate>,
    pub billing_period_start: Option<NaiveDate>,
    pub billing_period_end: Option<NaiveDate>,
    pub total_amount: Option<Decimal>,
    pub generation_kwh: Option<Decimal>,
    pub issued_at: Option<NaiveDate>,
    pub business_key: Option<String>,
    pub document_no: Option<String>,
    pub approval_state: String,
    pub approved_at: Option<DateTime<Utc>>,
    pub review_status: String,
    pub data: Value,
    pub organization_id: i64,
    pub station_id: Option<i64>,
    pub source_attachment_id: Option<i64>,
    /// 比對到的合約（S1 `resolve_match`）；`noContract`／`noStation` 時是 NULL。
    pub contract_id: Option<i64>,
    /// 比對用的是哪一個電號（一座案場可能有多個電號）；主電號比對不填。
    pub station_meter_id: Option<i64>,
    /// 電費單明細 19 欄（卡 B），欄位表驅動：鍵是 `ALL_BILL_REVIEW_FIELDS` 以外、
    /// `taipower_extractor::BILL_DETAIL_FIELDS` 的 camel 名；一個 jsonb 欄位頂 19 支具名欄，
    /// 不逐欄手寫 `field_value`／`column_of`／`patch_values` 的 match。
    pub detail_fields: Value,
}

pub const BILL_COLS: &str = "id, customer_number_raw, ocr_raw, ocr_extracted, template_id, doc_type, due_date, billing_period_start,
    billing_period_end, total_amount, generation_kwh, issued_at, business_key, document_no, approval_state, approved_at, review_status,
    data, organization_id, station_id, source_attachment_id, contract_id, station_meter_id, detail_fields";

impl BillRow {
    pub fn doc_type_source(&self) -> BillDocTypeSource<'_> {
        BillDocTypeSource { doc_type: self.doc_type.as_deref(), template_id: self.template_id.as_deref(), ocr_raw: Some(&self.ocr_raw) }
    }

    /// 這張單現在算哪一種（人改過就是人改的那個）。
    pub fn effective_doc_type(&self) -> EnergyDocType {
        bill_doc_type(self.doc_type_source())
    }

    /// 某個最終值欄位的值（DB 形狀：日期 `YYYY-MM-DD`、金額度數是帶尾零的 decimal 字串）。
    pub fn field_value(&self, f: &str) -> Value {
        let d = |v: Option<NaiveDate>| v.map(|x| json!(day(x))).unwrap_or(Value::Null);
        let n = |v: Option<Decimal>| v.map(|x| json!(x.to_string())).unwrap_or(Value::Null);
        match f {
            "customerNumberRaw" => self.customer_number_raw.as_ref().map(|s| json!(s)).unwrap_or(Value::Null),
            "billingPeriodStart" => d(self.billing_period_start),
            "billingPeriodEnd" => d(self.billing_period_end),
            "issuedAt" => d(self.issued_at),
            "dueDate" => d(self.due_date),
            "totalAmount" => n(self.total_amount),
            "generationKwh" => n(self.generation_kwh),
            "documentNo" => self.document_no.as_ref().map(|s| json!(s)).unwrap_or(Value::Null),
            // 卡 B 的 19 個明細欄位：一張表頂 19 支具名分支，值就在 `detail_fields` 這個 jsonb 裡。
            other => self.detail_fields.get(other).cloned().unwrap_or(Value::Null),
        }
    }

    /// 分支條件的屬性快照（金額門檻）。
    pub fn review_group(&self) -> Option<&str> {
        self.data["reviewGroup"].as_str().or_else(|| (self.effective_doc_type()!=EnergyDocType::RetailerSale).then_some("bill-reviewers"))
    }
    pub fn attributes(&self) -> Value {
        json!({ "reviewGroup":self.review_group(), "bill": { "totalAmount": self.total_amount.and_then(|d| d.to_f64()).unwrap_or(0.0) } })
    }

    /// 取版錨點：計費期間結束日，沒有就今天。
    pub fn business_date(&self, today: NaiveDate) -> NaiveDate {
        self.billing_period_end.unwrap_or(today)
    }
}

/// 值是否相等：兩邊都是數字就用數值比（DB 的 decimal 帶尾零、辨識值是 JSON number），否則比 `YYYY-MM-DD` 前綴。
/// 直接比字串會讓每一張單都被標成「被人工改過」。
pub fn same_value(a: Option<&Value>, b: Option<&Value>) -> bool {
    let num = |v: Option<&Value>| -> Option<f64> {
        match v {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if s.is_empty() => None,
            Some(Value::String(s)) => Some(js_number(s)),
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::Bool(b)) => Some(f64::from(u8::from(*b))),
            Some(_) => Some(f64::NAN),
        }
    };
    if let (Some(x), Some(y)) = (num(a), num(b)) {
        if x.is_finite() && y.is_finite() {
            return x == y;
        }
    }
    let text = |v: Option<&Value>| -> String {
        match v {
            None | Some(Value::Null) => String::new(),
            Some(v) => js_str(v).chars().take(10).collect(),
        }
    };
    text(a) == text(b)
}

/// 哪些最終值被人工改過（校對台的視覺標記用）。單別也算：它換掉的是這張單要走的流程。
pub fn changed_fields(b: &BillRow) -> Vec<String> {
    let mut out: Vec<String> = ALL_BILL_REVIEW_FIELDS
        .iter()
        .filter(|f| !same_value(extracted_value_of(&b.ocr_extracted, f), Some(&b.field_value(f))))
        .map(|f| f.to_string())
        .collect();
    let stored = b.doc_type.clone().unwrap_or_default();
    let recognized = recognized_doc_type(b.doc_type_source()).map(|t| t.as_str().to_string()).unwrap_or_default();
    if !stored.is_empty() && !recognized.is_empty() && stored != recognized {
        out.push("docType".into());
    }
    out
}

/// 哪些明細欄位（19 欄，卡 B）被人工改過 —— 跟 `changed_fields` 同一條規則，
/// 但掃的是 `BILL_DETAIL_FIELDS` 那張表，不是 `ALL_BILL_REVIEW_FIELDS`。
pub fn detail_changed_fields(b: &BillRow) -> Vec<String> {
    eo_domain::taipower_extractor::BILL_DETAIL_FIELDS
        .iter()
        .filter(|f| !same_value(b.ocr_extracted.get(f.camel), b.detail_fields.get(f.camel)))
        .map(|f| f.camel.to_string())
        .collect()
}

pub fn to_view(b: &BillRow) -> Value {
    json!({
        "id": b.id,
        "stationId": b.station_id,
        "customerNumberRaw": b.customer_number_raw,
        "sourceAttachmentId": b.source_attachment_id,
        "ocrExtracted": b.ocr_extracted,
        "billingPeriodStart": day_opt(b.billing_period_start),
        "billingPeriodEnd": day_opt(b.billing_period_end),
        "totalAmount": b.total_amount.map(|d| d.to_string()),
        "generationKwh": b.generation_kwh.map(|d| d.to_string()),
        "issuedAt": day_opt(b.issued_at),
        "dueDate": day_opt(b.due_date),
        "templateId": b.template_id,
        "docType": b.doc_type,
        "reviewGroup": b.review_group(),
        "recognizedDocType": recognized_doc_type(b.doc_type_source()).map(|t| t.as_str()),
        "reviewStatus": b.review_status,
        "changedFields": changed_fields(b),
        "businessKey": b.business_key,
        "documentNo": b.document_no,
        "approvalState": b.approval_state,
        "contractId": b.contract_id,
        "stationMeterId": b.station_meter_id,
        "detailFields": b.detail_fields,
        "detailChangedFields": detail_changed_fields(b),
    })
}

/// 本組織的某張單（查無 404、別家 403）。`for_update` 讓並行的校對排隊。
pub async fn bill_in_org(tx: &mut Tx<'_>, org_id: i64, bill_id: i64, for_update: bool) -> ApiResult<BillRow> {
    let lock = if for_update { " FOR UPDATE" } else { "" };
    let row: Option<BillRow> =
        sqlx::query_as(&format!("SELECT {BILL_COLS} FROM tbl_energy_bill WHERE id = $1 AND deleted_at IS NULL{lock}"))
            .bind(bill_id)
            .fetch_optional(&mut **tx)
            .await?;
    let row = row.ok_or_else(|| err(404, "bill_not_found"))?;
    if row.organization_id != org_id {
        return Err(err(403, "bill_not_in_org"));
    }
    Ok(row)
}

#[derive(Debug, Clone, FromRow)]
pub struct StationLite {
    pub id: i64,
    pub name: String,
    pub customer_number: String,
}

pub async fn station_by_id(tx: &mut Tx<'_>, org_id: i64, id: i64) -> ApiResult<Option<StationLite>> {
    Ok(sqlx::query_as(
        "SELECT id, name, customer_number FROM tbl_energy_station WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(org_id)
    .fetch_optional(&mut **tx)
    .await?)
}

/// 電號 → 案場＋是哪一個電號：先查 `tbl_energy_station_meter`（一對多；效期比**這張單的業務日期**
/// ——20261118-0810-b 卡起不再比 `CURRENT_DATE`，回頭補登的舊單才對得到當時有效的那個電號），
/// 查不到再比案場主電號（正規化值，找不到再退回原字串，相容舊資料，不是模糊比對；主電號沒有獨立 id）。
pub async fn find_station_and_meter(
    tx: &mut Tx<'_>,
    org_id: i64,
    customer_number: &str,
    business_date: NaiveDate,
) -> ApiResult<Option<(StationLite, Option<i64>)>> {
    if let Some(key) = normalize_customer_number(Some(customer_number)) {
        let via_meter: Option<(i64, i64, String, String)> = sqlx::query_as(
            "SELECT m.id, s.id, s.name, s.customer_number FROM tbl_energy_station_meter m
               JOIN tbl_energy_station s ON s.id = m.station_id AND s.deleted_at IS NULL
              WHERE m.organization_id = $1 AND m.customer_number_key = $2 AND m.deleted_at IS NULL
                AND (m.valid_to IS NULL OR m.valid_to >= $3) AND (m.valid_from IS NULL OR m.valid_from <= $3)
              ORDER BY m.id LIMIT 1",
        )
        .bind(org_id)
        .bind(&key)
        .bind(business_date)
        .fetch_optional(&mut **tx)
        .await?;
        if let Some((meter_id, station_id, name, customer_number)) = via_meter {
            return Ok(Some((StationLite { id: station_id, name, customer_number }, Some(meter_id))));
        }
        let hit: Option<StationLite> = sqlx::query_as(
            "SELECT id, name, customer_number FROM tbl_energy_station
              WHERE organization_id = $1 AND customer_number_key = $2 AND deleted_at IS NULL ORDER BY id LIMIT 1",
        )
        .bind(org_id)
        .bind(&key)
        .fetch_optional(&mut **tx)
        .await?;
        if let Some(station) = hit {
            return Ok(Some((station, None)));
        }
    }
    let hit: Option<StationLite> = sqlx::query_as(
        "SELECT id, name, customer_number FROM tbl_energy_station
          WHERE organization_id = $1 AND customer_number = $2 AND deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .bind(org_id)
    .bind(customer_number)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(hit.map(|s| (s, None)))
}

/// 相容舊呼叫點（卡 A2 的測試）：效期比今天，不帶 meter id。新碼請用 `find_station_and_meter`。
pub async fn find_station_by_customer_number(tx: &mut Tx<'_>, org_id: i64, customer_number: &str) -> ApiResult<Option<StationLite>> {
    Ok(find_station_and_meter(tx, org_id, customer_number, chrono::Utc::now().date_naive()).await?.map(|(s, _)| s))
}

/// 這張單的比對結果：電號 → 案場 → 這段業務日期生效的合約。三種之一，三個入口
/// （`ingest`／`match_station`／`correct`）＋ `rematch` 共用，不各寫一套。
#[derive(Debug, Clone)]
pub enum MatchOutcome {
    Matched { station: StationLite, meter_id: Option<i64>, contract_id: i64, contract_no: String },
    NoStation,
    NoContract { station: StationLite },
}

impl MatchOutcome {
    pub fn review_status(&self) -> &'static str {
        match self {
            MatchOutcome::Matched { .. } => "pendingReview",
            MatchOutcome::NoStation => "pendingMatch",
            MatchOutcome::NoContract { .. } => "pendingReview",
        }
    }
    pub fn station_id(&self) -> Option<i64> {
        match self {
            MatchOutcome::Matched { station, .. } | MatchOutcome::NoContract { station } => Some(station.id),
            MatchOutcome::NoStation => None,
        }
    }
    pub fn meter_id(&self) -> Option<i64> {
        match self {
            MatchOutcome::Matched { meter_id, .. } => *meter_id,
            _ => None,
        }
    }
    pub fn contract_id(&self) -> Option<i64> {
        match self {
            MatchOutcome::Matched { contract_id, .. } => Some(*contract_id),
            _ => None,
        }
    }
}

/// 案場已經定了（人工指認、或 `resolve_match` 自己查到的）—— 只差「這段期間有沒有生效合約」。
async fn resolve_contract_for_station(
    tx: &mut Tx<'_>,
    org_id: i64,
    station: StationLite,
    meter_id: Option<i64>,
    business_date: NaiveDate,
) -> ApiResult<MatchOutcome> {
    let coverages = crate::workflow::definition::effective_contracts_for_org(tx, org_id, station.id, business_date).await?;
    // 同一段期間兩份以上生效合約時記 id 最小的那一份（`ORDER BY c.id` 已經排好，DESIGN.md 暫定）。
    match coverages.into_iter().next() {
        Some(cv) => Ok(MatchOutcome::Matched { station, meter_id, contract_id: cv.contract_id, contract_no: cv.contract_no }),
        None => Ok(MatchOutcome::NoContract { station }),
    }
}

/// 比對到合約（S1）：電號 → 案場（含電號表，效期比業務日期）→ 這段期間生效的合約。
pub async fn resolve_match(
    tx: &mut Tx<'_>,
    org_id: i64,
    customer_number: &str,
    business_date: NaiveDate,
) -> ApiResult<MatchOutcome> {
    match find_station_and_meter(tx, org_id, customer_number, business_date).await? {
        Some((station, meter_id)) => resolve_contract_for_station(tx, org_id, station, meter_id, business_date).await,
        None => Ok(MatchOutcome::NoStation),
    }
}

fn decimal_of(v: f64) -> Option<Decimal> {
    if !v.is_finite() {
        return None;
    }
    Decimal::from_str(&js_num_str(v)).ok().or_else(|| Decimal::from_f64_retain(v))
}

fn date_of(s: Option<&str>) -> Option<NaiveDate> {
    s.and_then(crate::common::parse_day)
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestResult {
    pub bill_ids: Vec<i64>,
    pub matched: u32,
    pub unmatched: u32,
}

/// 吃辨識輸出，產出 N 筆 `energyBill`（一個上傳檔可以產出多筆）。
/// `doc_type`：分類器判定的單別，會落地；沒給就由辨識結果現算。
pub async fn ingest(
    tx: &mut Tx<'_>,
    ctx: &Ctx,
    ocr_json: &Value,
    source_attachment_id: Option<i64>,
    ingest_batch_id: Option<i64>,
    doc_type: Option<&str>,
) -> ApiResult<IngestResult> {
    let extracted: Vec<ExtractedBill> = energy_bill_extractor(ocr_json);
    if extracted.is_empty() {
        // 一筆都抽不到是真的錯（不是電費單／辨識整個失敗），與「抽到但配不上電號」是兩回事。
        return Err(err_d(422, "nothing_extracted", json!({ "hint": "OCR 輸出裡找不到任何可用欄位" })));
    }
    let ocr_raw = if ocr_json.is_null() { json!({}) } else { ocr_json.clone() };
    let mut out = IngestResult { bill_ids: vec![], matched: 0, unmatched: 0 };
    let today = ctx.today();
    for one in &extracted {
        let business_key =
            bill_business_key(one.customer_number.as_deref(), one.billing_period_start.as_deref(), one.billing_period_end.as_deref());
        let business_date = date_of(one.billing_period_end.as_deref()).unwrap_or(today);
        let outcome = match one.customer_number.as_deref() {
            Some(cn) if !cn.is_empty() => resolve_match(tx, ctx.org_id(), cn, business_date).await?,
            _ => MatchOutcome::NoStation,
        };
        let review_status = outcome.review_status();
        // 「配到了」在收件台的意義是「找到案場」—— 找得到案場但沒有生效合約，
        // 仍然算 matched（它不是 pendingMatch 那種要重新指認案場的例外）。
        if outcome.station_id().is_some() {
            out.matched += 1;
        } else {
            out.unmatched += 1;
        }
        let stored_doc_type = doc_type.map(String::from).or_else(|| {
            recognized_doc_type(BillDocTypeSource { doc_type: None, template_id: one.template_id.as_deref(), ocr_raw: Some(&ocr_raw) })
                .map(|t| t.as_str().to_string())
        });
        let business_key=eo_domain::business_key::source_bill_key(business_key,stored_doc_type.as_deref(),one.template_id.as_deref());
        let extracted_json = serde_json::to_value(one).unwrap_or_else(|_| json!({}));
        let detail_json = Value::Object(one.detail.clone());
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO tbl_energy_bill
               (organization_id, station_id, customer_number_raw, source_attachment_id, ocr_raw, ocr_extracted,
                billing_period_start, billing_period_end, total_amount, generation_kwh, issued_at, template_id, doc_type, due_date,
                business_key, document_no, review_status, contract_id, station_meter_id, detail_fields, data)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, '{}'::jsonb) RETURNING id",
        )
        .bind(ctx.org_id())
        .bind(outcome.station_id())
        .bind(one.customer_number.as_deref())
        .bind(source_attachment_id)
        .bind(&ocr_raw)
        .bind(&extracted_json)
        .bind(date_of(one.billing_period_start.as_deref()))
        .bind(date_of(one.billing_period_end.as_deref()))
        .bind(one.total_amount.and_then(decimal_of))
        .bind(one.generation_kwh.and_then(decimal_of))
        .bind(date_of(one.issued_at.as_deref()))
        .bind(one.template_id.as_deref())
        .bind(stored_doc_type.as_deref())
        .bind(date_of(one.due_date.as_deref()))
        .bind(business_key.as_deref())
        .bind(one.document_no.as_deref())
        .bind(review_status)
        .bind(outcome.contract_id())
        .bind(outcome.meter_id())
        .bind(&detail_json)
        .fetch_one(&mut **tx)
        .await?;
        out.bill_ids.push(id);
        let keys: Vec<String> = extracted_json.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
        let label = one.customer_number.clone().unwrap_or_else(|| "(無電號)".into());
        let input = match &outcome {
            MatchOutcome::Matched { station, .. } => {
                AuditInput::new("bill.matched", id, format!("辨識出電費單並配對到案場「{}」", station.name))
            }
            MatchOutcome::NoStation => AuditInput::new(
                "bill.unmatched",
                id,
                format!("電號「{}」找不到對應案場，轉為例外件待人工指認", one.customer_number.as_deref().unwrap_or("(無)")),
            )
            // 配不上不是失敗，是要有人處理的狀態 —— 用 outcome 標出來，稽核清單才篩得到。
            .outcome("failed"),
            MatchOutcome::NoContract { station } => AuditInput::new(
                "bill.no_contract",
                id,
                format!("案場「{}」在這段計費期間尚未設定租約；電費單可獨立審核，租金計算待設定", station.name),
            ),
        };
        audit::record(
            tx,
            ctx,
            input.label(label).payload(json!({
                "customerNumberRaw": one.customer_number,
                "stationId": outcome.station_id(),
                "contractId": outcome.contract_id(),
                "extractedFields": keys,
                "sourceAttachmentId": source_attachment_id,
                "ingestBatchId": ingest_batch_id,
            })),
        )
        .await?;
    }
    Ok(out)
}

/// 人工指認：把例外件配到一個案場，指認之後再查合約（S1：有生效合約才是「待校對」，沒有是「找不到生效合約」）。
pub async fn match_station(tx: &mut Tx<'_>, ctx: &Ctx, bill_id: i64, station_id: i64) -> ApiResult<BillRow> {
    let bill = bill_in_org(tx, ctx.org_id(), bill_id, true).await?;
    let station = station_by_id(tx, ctx.org_id(), station_id).await?.ok_or_else(|| err(404, "station_not_found"))?;
    let business_date = bill.business_date(ctx.today());
    let outcome = resolve_contract_for_station(tx, ctx.org_id(), station.clone(), None, business_date).await?;
    sqlx::query("UPDATE tbl_energy_bill SET station_id = $2, review_status = $3, contract_id = $4, station_meter_id = $5 WHERE id = $1 AND deleted_at IS NULL")
        .bind(bill_id)
        .bind(station_id)
        .bind(outcome.review_status())
        .bind(outcome.contract_id())
        .bind(outcome.meter_id())
        .execute(&mut **tx)
        .await?;
    // 收件台看的是 item 的狀態：與「改電號配上案場」是同一件事的兩個入口，狀態轉換也要一樣。
    let unlocked: Option<i64> = sqlx::query_scalar(
        "UPDATE tbl_energy_ingest_item SET status = 'extracted'
          WHERE id = (SELECT id FROM tbl_energy_ingest_item WHERE organization_id = $1 AND bill_id = $2 AND deleted_at IS NULL ORDER BY id LIMIT 1)
            AND status = 'unmatched' RETURNING id",
    )
    .bind(ctx.org_id())
    .bind(bill_id)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(item_id) = unlocked {
        super::service::recompute_batch_status_of_item(tx, item_id).await?;
    }
    let cn = bill.customer_number_raw.clone();
    audit::record(
        tx,
        ctx,
        AuditInput::new(
            "bill.matched",
            bill_id,
            format!("人工指認：電號「{}」→ 案場「{}」", cn.as_deref().unwrap_or("(無)"), station.name),
        )
        .label(cn.clone().unwrap_or_else(|| bill_id.to_string()))
        .payload(json!({ "customerNumberRaw": cn, "stationId": station_id, "previousStationId": bill.station_id, "manual": true, "contractId": outcome.contract_id() })),
    )
    .await?;
    bill_in_org(tx, ctx.org_id(), bill_id, false).await
}

/// 合約後補之後，在校對台按「重新比對」：用現在的電號與計費迄日重跑一次 `resolve_match`，
/// 不用改電號、改期間那種大費周章的方式去觸發。
pub async fn rematch(tx: &mut Tx<'_>, ctx: &Ctx, bill_id: i64) -> ApiResult<BillRow> {
    let bill = bill_in_org(tx, ctx.org_id(), bill_id, true).await?;
    let business_date = bill.business_date(ctx.today());
    let outcome = match bill.customer_number_raw.as_deref().filter(|s| !s.is_empty()) {
        Some(cn) => resolve_match(tx, ctx.org_id(), cn, business_date).await?,
        None => MatchOutcome::NoStation,
    };
    let before_status = bill.review_status.clone();
    let after_status =
        if bill.review_status == "reviewed" { bill.review_status.clone() } else { outcome.review_status().to_string() };
    sqlx::query("UPDATE tbl_energy_bill SET station_id = $2, review_status = $3, contract_id = $4, station_meter_id = $5 WHERE id = $1 AND deleted_at IS NULL")
        .bind(bill_id)
        .bind(outcome.station_id())
        .bind(&after_status)
        .bind(outcome.contract_id())
        .bind(outcome.meter_id())
        .execute(&mut **tx)
        .await?;
    audit::record(
        tx,
        ctx,
        AuditInput::new("bill.rematched", bill_id, format!("重新比對：校對狀態由「{before_status}」變成「{after_status}」"))
            .label(bill.customer_number_raw.clone().unwrap_or_else(|| bill_id.to_string()))
            .payload(json!({ "from": before_status, "to": after_status, "contractId": outcome.contract_id() })),
    )
    .await?;
    bill_in_org(tx, ctx.org_id(), bill_id, false).await
}

/// 要寫回 DB 的一個欄位值。
enum Col {
    Text(Option<String>),
    Date(Option<NaiveDate>),
    Dec(Option<Decimal>),
    Int(Option<i64>),
    /// 只用在 `detailFields`：寫回去是 `detail_fields = detail_fields || $patch`，不是整欄覆蓋。
    Json(Value),
}

fn column_of(field: &str) -> &'static str {
    match field {
        "customerNumberRaw" => "customer_number_raw",
        "billingPeriodStart" => "billing_period_start",
        "billingPeriodEnd" => "billing_period_end",
        "totalAmount" => "total_amount",
        "generationKwh" => "generation_kwh",
        "issuedAt" => "issued_at",
        "dueDate" => "due_date",
        "stationId" => "station_id",
        "reviewStatus" => "review_status",
        "docType" => "doc_type",
        "businessKey" => "business_key",
        "documentNo" => "document_no",
        "contractId" => "contract_id",
        "stationMeterId" => "station_meter_id",
        "detailFields" => "detail_fields",
        _ => unreachable!("未知欄位"),
    }
}

/// patch 裡給了哪些欄位、各是什麼值（JSON 形狀，用來比對與記稽核）。
fn patch_values(p: &CorrectBillInput) -> Vec<(&'static str, Value)> {
    let mut v = Vec::new();
    let s = |x: &Option<String>| x.as_ref().map(|t| json!(t)).unwrap_or(Value::Null);
    let n = |x: &Option<f64>| x.map(|t| json!(t)).unwrap_or(Value::Null);
    if let Some(x) = &p.customer_number_raw {
        v.push(("customerNumberRaw", s(x)));
    }
    if let Some(x) = &p.billing_period_start {
        v.push(("billingPeriodStart", s(x)));
    }
    if let Some(x) = &p.billing_period_end {
        v.push(("billingPeriodEnd", s(x)));
    }
    if let Some(x) = &p.total_amount {
        v.push(("totalAmount", n(x)));
    }
    if let Some(x) = &p.generation_kwh {
        v.push(("generationKwh", n(x)));
    }
    if let Some(x) = &p.issued_at {
        v.push(("issuedAt", s(x)));
    }
    if let Some(x) = &p.due_date {
        v.push(("dueDate", s(x)));
    }
    if let Some(x) = &p.document_no {
        v.push(("documentNo", s(x)));
    }
    v
}

fn to_col(field: &str, v: &Value) -> Col {
    match field {
        "customerNumberRaw" | "documentNo" => Col::Text(v.as_str().map(String::from)),
        "totalAmount" | "generationKwh" => Col::Dec(v.as_f64().and_then(decimal_of)),
        _ => Col::Date(v.as_str().and_then(crate::common::parse_day)),
    }
}

/// 這張單有沒有正在跑的流程（running／abortPending）。
pub async fn live_execution(tx: &mut Tx<'_>, org_id: i64, bill_id: i64) -> ApiResult<Option<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM tbl_energy_workflow_execution
          WHERE organization_id = $1 AND subject_type = 'energyBill' AND subject_id = $2
            AND status = ANY($3) AND deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .bind(org_id)
    .bind(bill_id.to_string())
    .bind(&["running", "abortPending"][..])
    .fetch_optional(&mut **tx)
    .await?)
}

/// 單別的值域（校對台可以指定的那幾種）。畫面選不到的值送進來是 400，不是靜靜地寫進去。
pub fn check_doc_type(raw: &Value) -> ApiResult<()> {
    let ok = raw.as_str().and_then(EnergyDocType::parse).is_some_and(|t| correctable_doc_types().contains(&t));
    if ok {
        Ok(())
    } else {
        Err(err_d(400, "invalid_doc_type", json!({ "docType": raw })))
    }
}

/// 校對：只改最終值（`ocrRaw`／`ocrExtracted` 一個欄位都不碰）。
pub async fn correct(tx: &mut Tx<'_>, ctx: &Ctx, bill_id: i64, input: &CorrectBillInput) -> ApiResult<BillRow> {
    let bill = bill_in_org(tx, ctx.org_id(), bill_id, true).await?;
    let mut changes = Map::new();
    if let Some(group)=&input.review_group {
        if bill.approval_state=="approved" || live_execution(tx,ctx.org_id(),bill_id).await?.is_some(){return Err(err(409,"review_group_locked"));}
        if bill.review_group()!=Some(group.as_str()){
            sqlx::query("UPDATE tbl_energy_bill SET data=jsonb_set(COALESCE(data,'{}'::jsonb),'{reviewGroup}',$1,true) WHERE id=$2 AND organization_id=$3").bind(json!(group)).bind(bill_id).bind(ctx.org_id()).execute(&mut **tx).await?;
            changes.insert("reviewGroup".into(),json!({"from":bill.review_group(),"to":group}));
        }
    }
    let mut patch: BTreeMap<&'static str, Col> = BTreeMap::new();
    let mut patched_json: HashMap<&'static str, Value> = HashMap::new();
    for (f, to) in patch_values(input) {
        let from = bill.field_value(f);
        // 一定要走數值相等：DB 的 decimal 回來是帶尾零的字串。
        if same_value(Some(&from), Some(&to)) {
            continue;
        }
        patch.insert(f, to_col(f, &to));
        patched_json.insert(f, to.clone());
        changes.insert(f.to_string(), json!({ "from": from, "to": to }));
    }

    // 改了電號或計費迄日就重新走一次 S1 的比對（`resolve_match`）—— 電號決定配到哪座案場，
    // 迄日決定業務日期（合約效期比的是它），兩個都可能改變「這段期間有沒有生效合約」。
    let mut station_changed: Option<(Option<i64>, Option<i64>)> = None;
    let mut station_after = bill.station_id;
    if patched_json.contains_key("customerNumberRaw") || patched_json.contains_key("billingPeriodEnd") {
        // 已經在跑的單不准改電號：流程是照送審那一刻的電號解出來的。
        if patched_json.contains_key("customerNumberRaw") {
            if let Some(e) = live_execution(tx, ctx.org_id(), bill_id).await? {
                return Err(err_d(409, "customer_number_locked_by_execution", json!({ "executionId": e })));
            }
        }
        let raw = match patched_json.get("customerNumberRaw") {
            Some(v) => v.as_str().map(String::from).filter(|s| !s.is_empty()),
            None => bill.customer_number_raw.clone(),
        };
        let business_date = match patched_json.get("billingPeriodEnd") {
            Some(v) => v.as_str().and_then(crate::common::parse_day),
            None => bill.billing_period_end,
        }
        .unwrap_or_else(|| ctx.today());
        let outcome = match &raw {
            Some(r) => resolve_match(tx, ctx.org_id(), r, business_date).await?,
            None => MatchOutcome::NoStation,
        };
        let after = outcome.station_id();
        if bill.station_id != after {
            patch.insert("stationId", Col::Int(after));
            station_changed = Some((bill.station_id, after));
            changes.insert("stationId".into(), json!({ "from": bill.station_id, "to": after }));
        }
        station_after = after;
        // 已經校對過的單不要被「只是順手改一下期間」靜默打回待校對 —— 跟改電號原本的規則一樣，
        // 只在還沒校對完（pendingMatch／pendingReview／pendingContract）那幾個狀態之間自動轉換。
        if bill.review_status != "reviewed" && outcome.review_status() != bill.review_status {
            patch.insert("reviewStatus", Col::Text(Some(outcome.review_status().into())));
        }
        patch.insert("contractId", Col::Int(outcome.contract_id()));
        patch.insert("stationMeterId", Col::Int(outcome.meter_id()));
        // 送件那一列的狀態跟著動（收件台看的是 item）：找不到案場才擋送審，找不到合約不擋 item（擋在送審那一關 409）。
        let linked: Option<(i64, String)> = sqlx::query_as(
            "SELECT id, status FROM tbl_energy_ingest_item WHERE organization_id = $1 AND bill_id = $2 AND deleted_at IS NULL ORDER BY id LIMIT 1",
        )
        .bind(ctx.org_id())
        .bind(bill_id)
        .fetch_optional(&mut **tx)
        .await?;
        if let Some((item_id, status)) = linked {
            let next = match (after.is_some(), status.as_str()) {
                (true, "unmatched") => Some("extracted"),
                (false, "extracted") => Some("unmatched"),
                _ => None,
            };
            if let Some(n) = next {
                sqlx::query("UPDATE tbl_energy_ingest_item SET status = $2 WHERE id = $1 AND deleted_at IS NULL")
                    .bind(item_id)
                    .bind(n)
                    .execute(&mut **tx)
                    .await?;
                super::service::recompute_batch_status_of_item(tx, item_id).await?;
            }
        }
    }

    // 單別也是辨識結果，所以也要能改；鎖與電號同一條規則。
    if let Some(to) = input.doc_type {
        let to = to.as_str().to_string();
        let from = bill.doc_type.clone().unwrap_or_default();
        if from != to {
            if let Some(e) = live_execution(tx, ctx.org_id(), bill_id).await? {
                return Err(err_d(409, "doc_type_locked_by_execution", json!({ "executionId": e })));
            }
            patch.insert("docType", Col::Text(Some(to.clone())));
            changes.insert("docType".into(), json!({ "from": if from.is_empty() { Value::Null } else { json!(from) }, "to": to }));
        }
    }

    // 電費單明細 19 欄（卡 B）—— 欄位表驅動：逐一對照 `BILL_DETAIL_FIELDS` 比對、算 from/to，
    // 寫回去是整包 merge 進 `detail_fields`（`||`），不是 19 支具名 column。
    if !input.detail.is_empty() {
        let mut detail_patch = Map::new();
        for f in eo_domain::taipower_extractor::BILL_DETAIL_FIELDS {
            let Some(to) = input.detail.get(f.camel) else { continue };
            let from = bill.detail_fields.get(f.camel).cloned().unwrap_or(Value::Null);
            if same_value(Some(&from), Some(to)) {
                continue;
            }
            detail_patch.insert(f.camel.to_string(), to.clone());
            changes.insert(f.camel.to_string(), json!({ "from": from, "to": to }));
        }
        if !detail_patch.is_empty() {
            patch.insert("detailFields", Col::Json(Value::Object(detail_patch)));
        }
    }

    // 還沒指認案場的單不能標成「已校對」—— 判斷要用這次改完之後的案場。
    let mark_reviewed = input.mark_reviewed.unwrap_or(false);
    if mark_reviewed && station_after.is_none() {
        return Err(err(409, "station_required_before_review"));
    }
    if mark_reviewed {
        patch.insert("reviewStatus", Col::Text(Some("reviewed".into())));
    }

    // 改了電號或計費期間就重算業務身分（否則去重靜默失效）。清掉也算改：null 不可以退回舊值。
    if input.doc_type.is_some() || ["customerNumberRaw", "billingPeriodStart", "billingPeriodEnd"].iter().any(|f| patched_json.contains_key(f)) {
        let pick = |f: &str| -> Option<String> {
            match patched_json.get(f) {
                Some(v) => v.as_str().map(String::from),
                None => bill.field_value(f).as_str().map(String::from),
            }
        };
        let key = bill_business_key(
            pick("customerNumberRaw").as_deref(),
            pick("billingPeriodStart").as_deref(),
            pick("billingPeriodEnd").as_deref(),
        );
        let kind=input.doc_type.unwrap_or_else(||bill.effective_doc_type());
        let key=eo_domain::business_key::source_bill_key(key,Some(kind.as_str()),bill.template_id.as_deref());
        patch.insert("businessKey", Col::Text(key));
    }

    if !patch.is_empty() {
        let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new("UPDATE tbl_energy_bill SET ");
        let mut sep = qb.separated(", ");
        for (f, v) in patch {
            let col = column_of(f);
            // `detailFields` 是整包 merge（`col = col || $patch`），不是覆蓋整欄 ——
            // 沒被這次校對碰到的舊明細值要留著。
            sep.push(if matches!(v, Col::Json(_)) { format!("{col} = {col} || ") } else { format!("{col} = ") });
            match v {
                Col::Text(x) => sep.push_bind_unseparated(x),
                Col::Date(x) => sep.push_bind_unseparated(x),
                Col::Dec(x) => sep.push_bind_unseparated(x),
                Col::Int(x) => sep.push_bind_unseparated(x),
                Col::Json(x) => sep.push_bind_unseparated(x),
            };
        }
        qb.push(" WHERE id = ").push_bind(bill_id).push(" AND deleted_at IS NULL");
        qb.build().execute(&mut **tx).await?;
    }

    // 沒有任何改變就不記事件（稽核不該有 noise），但「標記已校對」本身算一次動作。
    if !changes.is_empty() || mark_reviewed {
        let n = changes.len();
        let summary = if n == 0 {
            "標記電費單已校對".to_string()
        } else {
            let base = format!("校對電費單：修正 {n} 個欄位");
            match station_changed {
                Some((from, to)) => format!(
                    "{base}（電號改動 → 案場由 #{} 改為 #{}）",
                    from.map(|x| x.to_string()).unwrap_or_else(|| "無".into()),
                    to.map(|x| x.to_string()).unwrap_or_else(|| "無（例外件）".into())
                ),
                None => base,
            }
        };
        let total_change = changes.get("totalAmount").cloned();
        let event_id = audit::record(
            tx,
            ctx,
            AuditInput::new("bill.corrected", bill_id, summary)
                .label(bill.customer_number_raw.clone().unwrap_or_else(|| bill_id.to_string()))
                .payload(json!({ "changes": Value::Object(changes), "markReviewed": mark_reviewed, "ocrExtracted": bill.ocr_extracted })),
        )
        .await?;
        // 這個修改改變了審批路線嗎？不擋、不問，要的是留痕講清楚；在同一個交易裡（fail-closed）。
        if let Some(c) = total_change {
            record_route_change(tx, ctx, &bill, &c["from"], &c["to"], event_id).await?;
        }
    }
    bill_in_org(tx, ctx.org_id(), bill_id, false).await
}

async fn record_route_change(tx: &mut Tx<'_>, ctx: &Ctx, bill: &BillRow, from: &Value, to: &Value, caused_by: i64) -> ApiResult<()> {
    // 還沒指認案場 → 沒有綁定可查 → 這時本來就送不出去。
    let Some(station_id) = bill.station_id else { return Ok(()) };
    let Some(station) = station_by_id(tx, ctx.org_id(), station_id).await? else { return Ok(()) };
    let business_date = bill.business_date(ctx.today());
    // 走綁定取版（不是最新版）—— 跟送審那一刻取到的必須是同一版。
    let def = match workflow::resolve_definition(tx, &ctx.access, &station.customer_number, "bill-approval", business_date).await {
        Ok(d) => d,
        Err(_) => return Ok(()),
    };
    let num = |v: &Value| -> f64 {
        match v {
            Value::Null => 0.0,
            Value::Number(n) => n.as_f64().unwrap_or(0.0),
            Value::String(s) => js_number(s),
            _ => f64::NAN,
        }
    };
    let before = walk_route(tx, def.id, &json!({ "bill": { "totalAmount": num(from) } })).await?;
    let after = walk_route(tx, def.id, &json!({ "bill": { "totalAmount": num(to) } })).await?;
    let gates_before = gates_of(&before);
    let gates_after = gates_of(&after);
    let delta = gate_delta(&gates_before, &gates_after);
    if !delta.changed {
        return Ok(());
    }
    let name_of = |k: &str| after.name_by_key.get(k).or_else(|| before.name_by_key.get(k)).cloned().unwrap_or_else(|| k.to_string());
    let mut gate_names = Map::new();
    for k in gates_before.iter().chain(gates_after.iter()) {
        gate_names.insert(k.clone(), json!(name_of(k)));
    }
    // 少一關優先 —— 同時有增有減時，要看的是被拿掉的那個。
    let removed = !delta.removed.is_empty();
    let names = |ks: &[String]| ks.iter().map(|k| name_of(k)).collect::<Vec<_>>().join("、");
    let summary = if removed {
        format!("總金額 {} → {}，因此少了一關「{}」", money(from), money(to), names(&delta.removed))
    } else {
        format!("總金額 {} → {}，因此多了一關「{}」", money(from), money(to), names(&delta.added))
    };
    let s = |v: &Value| if v.is_null() { Value::Null } else { json!(js_str(v)) };
    audit::record(
        tx,
        ctx,
        AuditInput::new(if removed { "bill.route_gate_removed" } else { "bill.route_gate_added" }, bill.id, summary)
            .label(bill.customer_number_raw.clone().unwrap_or_else(|| bill.id.to_string()))
            .caused_by(Some(caused_by))
            .payload(json!({
                "field": "totalAmount",
                "before": s(from),
                "after": s(to),
                "gatesBefore": gates_before,
                "gatesAfter": gates_after,
                "gatesDelta": { "added": delta.added, "removed": delta.removed },
                "gateNames": gate_names,
            })),
    )
    .await?;
    Ok(())
}

/// 例外件清單。
pub async fn list_unmatched(tx: &mut Tx<'_>, access: &OrgAccess) -> ApiResult<Vec<Value>> {
    let rows: Vec<BillRow> = sqlx::query_as(&format!(
        "SELECT {BILL_COLS} FROM tbl_energy_bill WHERE organization_id = $1 AND review_status = 'pendingMatch' AND deleted_at IS NULL ORDER BY id DESC"
    ))
    .bind(access.org_id())
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows.iter().map(to_view).collect())
}

#[derive(Debug, Default)]
pub struct BillFilter {
    pub review_status: Option<String>,
    pub station_id: Option<i64>,
    pub limit: Option<i64>,
    /// 計費期間起日的半開區間 `[from, to)`。
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

/// 單據清單。每張單只帶一個 `executionId`（null ＝ 尚未送審）—— 關卡、等待天數是 execution 的欄位，不搬過來。
pub async fn list_bills(tx: &mut Tx<'_>, access: &OrgAccess, f: &BillFilter) -> ApiResult<Vec<Value>> {
    let limit = f.limit.filter(|n| *n > 0).unwrap_or(100).min(500);
    let rows: Vec<BillRow> = sqlx::query_as(&format!(
        "SELECT {BILL_COLS} FROM tbl_energy_bill
          WHERE organization_id = $1 AND ($2::text IS NULL OR review_status = $2) AND ($3::bigint IS NULL OR station_id = $3)
            AND ($4::date IS NULL OR billing_period_start >= $4) AND ($5::date IS NULL OR billing_period_start < $5)
            AND deleted_at IS NULL
          ORDER BY id DESC LIMIT $6"
    ))
    .bind(access.org_id())
    .bind(f.review_status.as_deref().filter(|s| !s.is_empty()))
    .bind(f.station_id)
    .bind(f.from)
    .bind(f.to)
    .bind(limit)
    .fetch_all(&mut **tx)
    .await?;
    if rows.is_empty() {
        return Ok(vec![]);
    }
    let ids: Vec<String> = rows.iter().map(|r| r.id.to_string()).collect();
    // subjectId 是字串且多型 —— subjectType 一定要一起比，不然會撈到 id 相同的保證金。
    let execs: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, subject_id FROM tbl_energy_workflow_execution
          WHERE organization_id = $1 AND subject_type = 'energyBill' AND subject_id = ANY($2) AND deleted_at IS NULL ORDER BY id DESC",
    )
    .bind(access.org_id())
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    let mut latest: HashMap<String, i64> = HashMap::new();
    for (id, sid) in execs {
        latest.entry(sid).or_insert(id);
    }
    Ok(rows
        .iter()
        .map(|r| {
            let mut v = to_view(r);
            v["executionId"] = json!(latest.get(&r.id.to_string()));
            v
        })
        .collect())
}

/// 這張單被哪些流程處理過（新到舊；一張單可能被中斷重跑、也可能同時開多條）。
pub async fn executions_of_bill(tx: &mut Tx<'_>, org_id: i64, bill_id: i64) -> ApiResult<Vec<Value>> {
    let rows: Vec<(i64, String, Option<String>, NaiveDate, Option<String>)> = sqlx::query_as(
        "SELECT id, status, current_node_key, business_date, workflow_key FROM tbl_energy_workflow_execution
          WHERE organization_id = $1 AND subject_type = 'energyBill' AND subject_id = $2 AND deleted_at IS NULL ORDER BY id DESC",
    )
    .bind(org_id)
    .bind(bill_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| json!({ "id": r.0, "status": r.1, "currentNodeKey": r.2, "businessDate": day(r.3), "workflowKey": r.4 }))
        .collect())
}

/// 這張單是哪一次送件產生的（null 是正常的：直接建檔的單沒有送件批次）。
pub async fn ingest_of_bill(tx: &mut Tx<'_>, org_id: i64, bill_id: i64) -> ApiResult<Value> {
    let item: Option<(i64, i64, Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT id, batch_id, original_filename, attachment_id FROM tbl_energy_ingest_item
          WHERE organization_id = $1 AND bill_id = $2 AND deleted_at IS NULL ORDER BY id DESC LIMIT 1",
    )
    .bind(org_id)
    .bind(bill_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((item_id, batch_id, name, attachment_id)) = item else { return Ok(Value::Null) };
    let batch: Option<(DateTime<Utc>, Option<i64>)> =
        sqlx::query_as("SELECT submitted_at, submitted_by_user_id FROM tbl_energy_ingest_batch WHERE id = $1 AND deleted_at IS NULL")
            .bind(batch_id)
            .fetch_optional(&mut **tx)
            .await?;
    let submitter = match batch.as_ref().and_then(|b| b.1) {
        Some(u) => crate::common::user_name(tx, u).await?,
        None => None,
    };
    Ok(json!({
        "batchId": batch_id,
        "itemId": item_id,
        "originalFilename": name,
        "attachmentId": attachment_id,
        "submittedAt": batch.as_ref().map(|b| b.0),
        "submittedByName": submitter,
    }))
}

/// 單別現在改不改得動（有流程在跑就不行 —— 與校對同一條規則）。
pub async fn doc_type_editable(tx: &mut Tx<'_>, org_id: i64, bill_id: i64) -> ApiResult<bool> {
    Ok(live_execution(tx, org_id, bill_id).await?.is_none())
}

fn flow_view(f: &ResolvedFlow) -> Value {
    json!({
        "workflowKey": f.workflow_key,
        "workflowName": f.definition.name,
        "version": f.definition.version,
        "matchedDocType": f.matched_doc_type,
        "contractId": f.contract.as_ref().map(|c| c.contract_id),
        "contractNo": f.contract.as_ref().map(|c| c.contract_no.clone()),
    })
}

/// 這張單送出去會開哪些流程 —— 送審前就要看得到（未綁定的案場收不了單，要在校對台載入時就講）。
pub async fn binding_status(tx: &mut Tx<'_>, access: &OrgAccess, bill: &BillRow, today: NaiveDate) -> ApiResult<Value> {
    let doc_type = bill.effective_doc_type().as_str();
    let Some(station_id) = bill.station_id else {
        return Ok(json!({ "bound": false, "reason": "no_station", "docType": doc_type, "flows": [] }));
    };
    let Some(station) = station_by_id(tx, access.org_id(), station_id).await? else {
        return Ok(json!({ "bound": false, "reason": "no_station", "docType": doc_type, "flows": [] }));
    };
    let business_date = bill.business_date(today);
    if eo_domain::document_ingest::is_bill_doc_type(bill.effective_doc_type().as_str()) {
        let Some(group)=bill.review_group() else {return Ok(json!({"bound":false,"reason":"review_group_required","docType":doc_type,"flows":[],"rentNotice":"請確認這份轉供單由哪組承辦審核；不影響辨識或申請預覽。"}));};
        let key=if group=="tianneng-reviewers"{"tianneng-data-review"}else{"bill-data-review"};
        return Ok(json!({"bound": true, "reason": null, "docType": doc_type, "stationName": station.name,
            "workflowName": "電費單平台資料審核", "defaultWorkflow": true,
            "flows": [{"workflowKey": key, "workflowName": if group=="tianneng-reviewers"{"天能電費單平台資料審核"}else{"台電電費單平台資料審核"}}],
            "rentNotice": "地主租金另依租約與付款週期處理，不影響電費資料審核。"}));
    }
    match workflow::resolve_flow_rows(tx, access, &station.customer_number, Some(station.id), bill.effective_doc_type(), business_date).await {
        Ok(flows) => Ok(json!({
            "bound": true,
            "reason": null,
            "docType": doc_type,
            "stationName": station.name,
            "flows": flows.iter().map(flow_view).collect::<Vec<_>>(),
            "workflowName": flows.first().map(|f| f.definition.name.clone()),
            "version": flows.first().map(|f| f.definition.version),
        })),
        Err(e) => Ok(json!({
            "bound": false,
            "reason": if e.code == "definition_not_published" { "definition_not_published" } else { "no_binding" },
            "docType": doc_type,
            "flows": [],
            "stationName": station.name,
            "customerNumber": station.customer_number,
        })),
    }
}

/// 改金額 → 分支即時重算：假設金額是 `total_amount`，這張單會經過哪些關卡（與跨門檻稽核同一支 walk_route）。
pub async fn branch_preview(tx: &mut Tx<'_>, access: &OrgAccess, bill: &BillRow, total_amount: f64, today: NaiveDate) -> ApiResult<Value> {
    let Some(station) = (match bill.station_id {
        Some(id) => station_by_id(tx, access.org_id(), id).await?,
        None => None,
    }) else {
        return Ok(json!({ "available": false, "reason": "no_station", "nodes": [], "threshold": null }));
    };
    let business_date = bill.business_date(today);
    // 用這張單自己會走的流程（不是寫死核可流程）；多條時取第一條。
    let flows = workflow::resolve_flow_rows(tx, access, &station.customer_number, Some(station.id), bill.effective_doc_type(), business_date).await;
    let Some(definition) = flows.ok().and_then(|f| f.into_iter().next()).map(|f| f.definition) else {
        return Ok(json!({ "available": false, "reason": "no_binding", "nodes": [], "threshold": null }));
    };
    let current = walk_route(tx, definition.id, &bill.attributes()).await?;
    let preview = walk_route(tx, definition.id, &json!({ "bill": { "totalAmount": total_amount } })).await?;
    let name_of = |k: &str| {
        preview.name_by_key.get(k).or_else(|| current.name_by_key.get(k)).cloned().unwrap_or_else(|| k.to_string())
    };
    let nodes: Vec<Value> = preview
        .path
        .iter()
        .map(|k| json!({ "nodeKey": k, "name": name_of(k), "added": !current.path.contains(k) }))
        .collect();
    let removed: Vec<Value> = current
        .path
        .iter()
        .filter(|k| !preview.path.contains(k))
        .map(|k| json!({ "nodeKey": k, "name": name_of(k) }))
        .collect();
    Ok(json!({
        "available": true,
        "reason": null,
        "definitionId": definition.id,
        "definitionName": definition.name,
        "version": definition.version,
        "currentAmount": bill.total_amount.map(|d| d.to_string()),
        "previewAmount": js_num_str(total_amount),
        "nodes": nodes,
        "removed": removed,
    }))
}

/// 這張單算出來的應付紀錄（全部，含被取代的）＋策略名稱與版本。
pub async fn payouts(tx: &mut Tx<'_>, access: &OrgAccess, bill_id: i64) -> ApiResult<Vec<Value>> {
    let rows = crate::rating::history_for_bill(tx, access, bill_id).await?;
    let ids: Vec<i64> = crate::common::uniq(rows.iter().filter_map(|r| r.strategy_id));
    let strategies: Vec<(i64, String, i32)> = if ids.is_empty() {
        vec![]
    } else {
        sqlx::query_as("SELECT id, name, version FROM tbl_energy_rating_strategy WHERE id = ANY($1) AND organization_id = $2 AND deleted_at IS NULL")
            .bind(&ids)
            .bind(access.org_id())
            .fetch_all(&mut **tx)
            .await?
    };
    Ok(rows
        .iter()
        .map(|r| {
            let mut v = serde_json::to_value(r).unwrap_or_else(|_| json!({}));
            let st = r.strategy_id.and_then(|id| strategies.iter().find(|s| s.0 == id));
            v["strategyName"] = json!(st.map(|s| s.1.clone()));
            v["strategyVersion"] = json!(st.map(|s| s.2));
            v
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::same_value;
    use serde_json::json;

    /// 來源：BillIngestService.ts `sameValue` 的註解（decimal 帶尾零、dateonly 前綴）。
    #[test]
    fn numeric_equality_ignores_trailing_zeros() {
        assert!(same_value(Some(&json!(4321.5)), Some(&json!("4321.500"))));
        assert!(same_value(Some(&json!("98765.00")), Some(&json!(98765))));
        assert!(!same_value(Some(&json!("98765.00")), Some(&json!(98000))));
        assert!(same_value(None, Some(&json!(null))));
        assert!(same_value(Some(&json!("2026-01-31")), Some(&json!("2026-01-31T00:00:00Z"))));
        assert!(!same_value(Some(&json!("03-1111-2222")), Some(&json!("99-0000-0000"))));
        assert!(!same_value(Some(&json!(null)), Some(&json!(0))));
    }
}
