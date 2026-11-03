//! 計算應付金額。
//!
//! 應付紀錄的契約：金額、輸入快照、策略、參數快照一經寫入永不 UPDATE／DELETE；
//! 唯一可以動的是 `status` 與 `superseded_at` 兩個指標欄，只在被新的一筆取代時動一次。
//! 計算與流程分離：流程呼叫計算，不內含計算；換策略不用改流程。

use crate::audit::{self, AuditInput};
use crate::common::{err, err_d, Ctx, Tx};
use chrono::{DateTime, NaiveDate, Utc};
use eo_domain::rating::{evaluate_strategy, get_rating_strategy_def, DecimalInput, RatingError, RatingInput};
use eo_platform::error::{ApiError, ApiResult};
use eo_platform::rbac::OrgAccess;
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::{json, Map, Value};
use sqlx::FromRow;

/// 預設的策略 key —— 一個電廠一組分潤條件。
pub const DEFAULT_STRATEGY_KEY: &str = "station-payout";

#[derive(Debug, Clone, FromRow)]
pub struct StrategyRow {
    pub id: i64,
    pub key: String,
    pub version: i32,
    pub name: String,
    #[sqlx(rename = "type")]
    pub strategy_type: String,
    pub params: Value,
    pub status: String,
    pub currency: String,
    pub organization_id: i64,
}

const STRATEGY_COLS: &str = "id, key, version, name, type, params, status, currency, organization_id";

#[derive(Debug, Clone, FromRow)]
pub struct PayoutRecord {
    pub id: i64,
    pub strategy_type: String,
    pub params_snapshot: Value,
    pub input_snapshot: Value,
    pub amount: Decimal,
    pub currency: String,
    pub business_date: NaiveDate,
    pub computed_at: DateTime<Utc>,
    pub trigger: String,
    pub superseded_at: Option<DateTime<Utc>>,
    pub status: String,
    pub organization_id: i64,
    pub station_id: i64,
    pub bill_id: i64,
    pub strategy_id: Option<i64>,
    pub computed_by_user_id: Option<i64>,
    pub execution_id: Option<i64>,
    pub supersedes_record_id: Option<i64>,
    pub data: Value,
}

const PAYOUT_COLS: &str = "id, strategy_type, params_snapshot, input_snapshot, amount, currency, business_date, computed_at, trigger,
    superseded_at, status, organization_id, station_id, bill_id, strategy_id, computed_by_user_id, execution_id, supersedes_record_id, data";

impl PayoutRecord {
    /// 金額字串（兩位小數）。
    pub fn amount_str(&self) -> String {
        format!("{:.2}", self.amount)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PayoutView {
    pub id: i64,
    pub bill_id: i64,
    pub station_id: i64,
    pub strategy_id: Option<i64>,
    pub strategy_type: String,
    pub params_snapshot: Value,
    pub input_snapshot: Value,
    pub amount: String,
    pub currency: String,
    pub business_date: String,
    pub computed_at: DateTime<Utc>,
    pub computed_by_user_id: Option<i64>,
    pub trigger: String,
    pub execution_id: Option<i64>,
    pub supersedes_record_id: Option<i64>,
    pub status: String,
}

impl From<&PayoutRecord> for PayoutView {
    fn from(r: &PayoutRecord) -> Self {
        PayoutView {
            id: r.id,
            bill_id: r.bill_id,
            station_id: r.station_id,
            strategy_id: r.strategy_id,
            strategy_type: r.strategy_type.clone(),
            params_snapshot: r.params_snapshot.clone(),
            input_snapshot: r.input_snapshot.clone(),
            amount: r.amount_str(),
            currency: r.currency.clone(),
            business_date: crate::common::day(r.business_date),
            computed_at: r.computed_at,
            computed_by_user_id: r.computed_by_user_id,
            trigger: r.trigger.clone(),
            execution_id: r.execution_id,
            supersedes_record_id: r.supersedes_record_id,
            status: r.status.clone(),
        }
    }
}

fn rating_error(e: RatingError) -> ApiError {
    match e {
        RatingError::Evaluation { code, details } => err_d(422, code, details),
        RatingError::NotADecimal(s) => err_d(422, "not_a_decimal", json!({ "value": s })),
    }
}

/// 依案場＋業務日期取策略版本（半開區間、效期重疊取最近一段）。取不到是錯誤，不是預設值。
pub async fn resolve_strategy(
    tx: &mut Tx<'_>,
    access: &OrgAccess,
    station_id: i64,
    strategy_key: Option<&str>,
    business_date: NaiveDate,
) -> ApiResult<StrategyRow> {
    let key = strategy_key.unwrap_or(DEFAULT_STRATEGY_KEY);
    let rows: Vec<(i64, NaiveDate, Option<NaiveDate>)> = sqlx::query_as(
        "SELECT strategy_id, valid_from, valid_to FROM tbl_energy_rating_strategy_binding
          WHERE organization_id = $1 AND station_id = $2 AND strategy_key = $3 AND deleted_at IS NULL
          ORDER BY valid_from DESC, id DESC",
    )
    .bind(access.org_id())
    .bind(station_id)
    .bind(key)
    .fetch_all(&mut **tx)
    .await?;
    let hit = rows
        .iter()
        .find(|r| crate::workflow::definition::in_effect(r.1, r.2, business_date))
        .ok_or_else(|| {
            err_d(
                404,
                "no_strategy_binding_for_business_date",
                json!({ "stationId": station_id, "strategyKey": key, "businessDate": crate::common::day(business_date) }),
            )
        })?;
    let s: Option<StrategyRow> = sqlx::query_as(&format!(
        "SELECT {STRATEGY_COLS} FROM tbl_energy_rating_strategy WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL"
    ))
    .bind(hit.0)
    .bind(access.org_id())
    .fetch_optional(&mut **tx)
    .await?;
    let s = s.ok_or_else(|| err(500, "strategy_not_found"))?;
    // 綁到 draft 等於讓還沒定稿的費率上線。
    if s.status != "published" {
        return Err(err_d(409, "strategy_not_published", json!({ "strategyId": s.id })));
    }
    Ok(s)
}

#[derive(FromRow)]
struct BillForRating {
    organization_id: i64,
    station_id: Option<i64>,
    billing_period_start: Option<NaiveDate>,
    billing_period_end: Option<NaiveDate>,
    total_amount: Option<Decimal>,
    generation_kwh: Option<Decimal>,
}

/// 流程開始時快照下來的契約參數 —— 足以決定算式（白名單內的 strategyType）才用它。
async fn contract_rating(tx: &mut Tx<'_>, execution_id: Option<i64>) -> ApiResult<Option<(String, Value, Option<i64>)>> {
    let Some(id) = execution_id else { return Ok(None) };
    let data: Option<Value> =
        sqlx::query_scalar("SELECT data FROM tbl_energy_workflow_execution WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?;
    let contract = data.as_ref().and_then(|d| d.get("attributes")).and_then(|a| a.get("contract")).cloned();
    let Some(contract) = contract else { return Ok(None) };
    let Some(params) = contract.get("params").filter(|p| p.is_object()).cloned() else { return Ok(None) };
    let t = params.get("strategyType").and_then(Value::as_str).unwrap_or("").to_string();
    if t.is_empty() || get_rating_strategy_def(&t).is_none() {
        return Ok(None);
    }
    Ok(Some((t, params, contract.get("id").and_then(Value::as_i64))))
}

/// 算一筆（給收件與計算節點用）。在呼叫端交易內：算 → 寫應付紀錄 →（有舊筆）標記取代 → 稽核。
/// 算不出來就回錯（交易應 rollback），並在交易之外留一筆 `payout.calculation_failed`。
pub async fn compute_for_bill(tx: &mut Tx<'_>, ctx: &Ctx, bill_id: i64, trigger: &str, execution_id: Option<i64>) -> ApiResult<PayoutRecord> {
    compute_for_bill_with_key(tx, ctx, bill_id, trigger, execution_id, None).await
}

pub async fn compute_for_bill_with_key(
    tx: &mut Tx<'_>,
    ctx: &Ctx,
    bill_id: i64,
    trigger: &str,
    execution_id: Option<i64>,
    strategy_key: Option<&str>,
) -> ApiResult<PayoutRecord> {
    match compute_inner(tx, ctx, bill_id, trigger, execution_id, strategy_key).await {
        Ok(r) => Ok(r),
        Err(e) => {
            // 「算不出來」也要留痕；必須寫在交易之外 —— 業務寫入一定要 rollback，稽核才一定要活下來。
            let reason = e.code.clone();
            let _ = audit::record_best_effort(
                &ctx.core.pool,
                ctx,
                AuditInput::new("payout.calculation_failed", bill_id, format!("無法計算應付金額：{reason}"))
                    .outcome("failed")
                    .payload(json!({ "reason": reason, "details": e.details, "trigger": trigger })),
            )
            .await;
            Err(e)
        }
    }
}

async fn compute_inner(
    tx: &mut Tx<'_>,
    ctx: &Ctx,
    bill_id: i64,
    trigger: &str,
    execution_id: Option<i64>,
    strategy_key: Option<&str>,
) -> ApiResult<PayoutRecord> {
    let bill: Option<BillForRating> = sqlx::query_as(
        "SELECT organization_id, station_id, billing_period_start, billing_period_end, total_amount, generation_kwh
           FROM tbl_energy_bill WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(bill_id)
    .fetch_optional(&mut **tx)
    .await?;
    let bill = bill.ok_or_else(|| err(404, "bill_not_found"))?;
    if bill.organization_id != ctx.org_id() {
        return Err(err(403, "bill_not_in_org"));
    }
    // 沒指認案場就算不了 —— 例外件要先過人工指認。
    let station_id = bill.station_id.ok_or_else(|| err_d(409, "bill_has_no_station", json!({ "billId": bill_id })))?;
    // 取版錨點＝計費期間結束日。
    let business_date =
        bill.billing_period_end.ok_or_else(|| err_d(409, "bill_has_no_billing_period", json!({ "billId": bill_id })))?;

    // 契約參數優先（流程開始那一刻的快照，事後改比例不影響已跑完的單）；沒給算式就回頭用案場綁定的策略。
    let from_contract = contract_rating(tx, execution_id).await?;
    let (strategy, strategy_type, params) = match &from_contract {
        Some((t, p, _)) => (None, t.clone(), p.clone()),
        None => {
            let s = resolve_strategy(tx, &ctx.access, station_id, strategy_key, business_date).await?;
            let (t, p) = (s.strategy_type.clone(), s.params.clone());
            (Some(s), t, p)
        }
    };
    let def = get_rating_strategy_def(&strategy_type).ok_or_else(|| err_d(500, "unknown_strategy_type", json!({ "type": strategy_type })))?;

    // 只取算式要的欄位＋期間 —— 不整包複製 bill，機敏欄位不進快照。
    let mut snapshot = Map::new();
    let mut input = RatingInput {
        billing_period_start: bill.billing_period_start.map(crate::common::day),
        billing_period_end: Some(crate::common::day(business_date)),
        ..Default::default()
    };
    for f in def.required_bill_fields {
        let v = match *f {
            "totalAmount" => bill.total_amount,
            "generationKwh" => bill.generation_kwh,
            _ => None,
        };
        snapshot.insert((*f).to_string(), v.map(|d| json!(d.to_string())).unwrap_or(Value::Null));
        let di = v.map(|d| DecimalInput::Text(d.to_string()));
        match *f {
            "totalAmount" => input.total_amount = di,
            "generationKwh" => input.generation_kwh = di,
            _ => {}
        }
    }
    snapshot.insert("billingPeriodStart".into(), json!(input.billing_period_start));
    snapshot.insert("billingPeriodEnd".into(), json!(input.billing_period_end));

    let amount = evaluate_strategy(&strategy_type, &params, &input).map_err(rating_error)?;
    let amount_dec: Decimal = amount.parse().map_err(|_| err(500, "invalid_amount"))?;

    let previous: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM tbl_energy_payout_record WHERE bill_id = $1 AND status = 'active' AND deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .bind(bill_id)
    .fetch_optional(&mut **tx)
    .await?;
    let currency = strategy.as_ref().map(|s| s.currency.clone()).unwrap_or_else(|| "TWD".into());
    // 這筆錢是照誰的參數算的 —— 事後追帳第一個要問的就是這件事。
    let data = match &from_contract {
        Some((_, _, cid)) => json!({ "paramsSource": "contract", "contractId": cid }),
        None => json!({ "paramsSource": "stationStrategy" }),
    };
    let now = ctx.now();
    let record: PayoutRecord = sqlx::query_as(&format!(
        "INSERT INTO tbl_energy_payout_record
           (organization_id, station_id, bill_id, strategy_id, strategy_type, params_snapshot, input_snapshot, amount, currency,
            business_date, computed_at, computed_by_user_id, trigger, execution_id, supersedes_record_id, status, data)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, 'active', $16) RETURNING {PAYOUT_COLS}"
    ))
    .bind(ctx.org_id())
    .bind(station_id)
    .bind(bill_id)
    .bind(strategy.as_ref().map(|s| s.id))
    .bind(&strategy_type)
    .bind(&params)
    .bind(Value::Object(snapshot))
    .bind(amount_dec)
    .bind(&currency)
    .bind(business_date)
    .bind(now)
    .bind(ctx.user_id())
    .bind(trigger)
    .bind(execution_id)
    .bind(previous)
    .bind(&data)
    .fetch_one(&mut **tx)
    .await?;

    if let Some(prev) = previous {
        // 本表唯一被允許的 UPDATE：兩個指標欄，一次。
        sqlx::query("UPDATE tbl_energy_payout_record SET status = 'superseded', superseded_at = $2 WHERE id = $1 AND deleted_at IS NULL")
            .bind(prev)
            .bind(now)
            .execute(&mut **tx)
            .await?;
        audit::record(
            tx,
            ctx,
            AuditInput::new("payout.superseded", prev, format!("應付紀錄被重算取代（新：#{}）", record.id))
                .payload(json!({ "replacedByRecordId": record.id })),
        )
        .await?;
    }
    let source = match &strategy {
        Some(s) => format!("策略「{}」v{}", s.name, s.version),
        None => format!(
            "契約 #{} 的參數",
            from_contract.as_ref().and_then(|c| c.2).map(|i| i.to_string()).unwrap_or_else(|| "?".into())
        ),
    };
    audit::record(
        tx,
        ctx,
        AuditInput::new("payout.calculated", record.id, format!("計算應付金額 {amount}（{source}）"))
            .label(format!("{amount} {currency}"))
            .payload(json!({
                "billId": bill_id,
                "strategyId": strategy.as_ref().map(|s| s.id),
                "version": strategy.as_ref().map(|s| s.version),
                "contractId": from_contract.as_ref().and_then(|c| c.2),
                "paramsSource": if from_contract.is_some() { "contract" } else { "stationStrategy" },
                "strategyType": strategy_type,
                "amount": amount,
                "trigger": trigger,
                "supersedesRecordId": previous,
            })),
    )
    .await?;
    Ok(record)
}

/// 這張單現行的應付。
pub async fn current_for_bill(tx: &mut Tx<'_>, access: &OrgAccess, bill_id: i64) -> ApiResult<Option<PayoutRecord>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {PAYOUT_COLS} FROM tbl_energy_payout_record
          WHERE organization_id = $1 AND bill_id = $2 AND status = 'active' AND deleted_at IS NULL ORDER BY id LIMIT 1"
    ))
    .bind(access.org_id())
    .bind(bill_id)
    .fetch_optional(&mut **tx)
    .await?)
}

/// 這張單算過幾次（新到舊）。
pub async fn history_for_bill(tx: &mut Tx<'_>, access: &OrgAccess, bill_id: i64) -> ApiResult<Vec<PayoutView>> {
    let rows: Vec<PayoutRecord> = sqlx::query_as(&format!(
        "SELECT {PAYOUT_COLS} FROM tbl_energy_payout_record
          WHERE organization_id = $1 AND bill_id = $2 AND deleted_at IS NULL ORDER BY computed_at DESC, id DESC"
    ))
    .bind(access.org_id())
    .bind(bill_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows.iter().map(PayoutView::from).collect())
}

/// 計費期間跨了兩段效期嗎（v1 不拆算，用期末命中的那一版算整張；這是已知的近似，要看得出來）。
pub async fn spans_multiple_bindings(tx: &mut Tx<'_>, access: &OrgAccess, record: &PayoutRecord) -> ApiResult<bool> {
    let from = record.input_snapshot.get("billingPeriodStart").and_then(Value::as_str).and_then(crate::common::parse_day);
    let to = record.input_snapshot.get("billingPeriodEnd").and_then(Value::as_str).and_then(crate::common::parse_day);
    let (Some(from), Some(to)) = (from, to) else { return Ok(false) };
    if from == to {
        return Ok(false);
    }
    let rows: Vec<(NaiveDate, Option<NaiveDate>)> = sqlx::query_as(
        "SELECT valid_from, valid_to FROM tbl_energy_rating_strategy_binding
          WHERE organization_id = $1 AND station_id = $2 AND deleted_at IS NULL ORDER BY id",
    )
    .bind(access.org_id())
    .bind(record.station_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows.iter().filter(|(vf, vt)| *vf <= to && vt.is_none_or(|t| from < t)).count() > 1)
}

/// 預設計算策略（`station-payout` v1，售電分潤 10%）。同 key 已有任何版本就不裝。
pub async fn install_default_strategy(tx: &mut Tx<'_>, ctx: &Ctx) -> ApiResult<Option<Value>> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM tbl_energy_rating_strategy WHERE organization_id = $1 AND key = $2 AND deleted_at IS NULL)",
    )
    .bind(ctx.org_id())
    .bind(DEFAULT_STRATEGY_KEY)
    .fetch_one(&mut **tx)
    .await?;
    if exists {
        return Ok(None);
    }
    let params = json!({ "sharePercent": 10 });
    let def = get_rating_strategy_def("revenueShare").ok_or_else(|| err(500, "unknown_strategy_type"))?;
    def.parse_params(&params).map_err(rating_error)?;
    let s: StrategyRow = sqlx::query_as(&format!(
        "INSERT INTO tbl_energy_rating_strategy
           (organization_id, key, version, name, type, params, status, published_at, published_by_user_id, currency)
         VALUES ($1, $2, 1, '售電分潤 10%', 'revenueShare', $3, 'published', $4, $5, 'TWD') RETURNING {STRATEGY_COLS}"
    ))
    .bind(ctx.org_id())
    .bind(DEFAULT_STRATEGY_KEY)
    .bind(&params)
    .bind(ctx.now())
    .bind(ctx.user_id())
    .fetch_one(&mut **tx)
    .await?;
    audit::record(
        tx,
        ctx,
        AuditInput::new("strategy.published", s.id, format!("發佈計算策略「{}」v{}", s.name, s.version))
            .label(s.name.clone())
            .payload(json!({ "key": s.key, "version": s.version, "strategyType": s.strategy_type })),
    )
    .await?;
    Ok(Some(json!({ "id": s.id, "key": s.key, "version": s.version, "name": s.name, "type": s.strategy_type })))
}
