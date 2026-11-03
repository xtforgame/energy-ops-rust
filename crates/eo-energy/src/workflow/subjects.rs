//! execution 的對象（subject）是多型的：電費單或保證金期數。這裡一次撈齊顯示要的摘要，
//! 每一種 subjectType 都要展開（多型只做一半，保證金那類就會變成點不動的空白列）。

use super::model::ExecutionRow;
use crate::common::{numeric_id, Tx};
use chrono::NaiveDate;
use eo_platform::error::ApiResult;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::FromRow;
use std::collections::HashMap;

#[derive(Debug, Clone, FromRow)]
pub struct BillLite {
    pub id: i64,
    pub station_id: Option<i64>,
    pub customer_number_raw: Option<String>,
    pub billing_period_start: Option<NaiveDate>,
    pub billing_period_end: Option<NaiveDate>,
    pub total_amount: Option<Decimal>,
    pub review_status: String,
    pub document_no: Option<String>,
    pub business_key: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct InstallmentLite {
    pub id: i64,
    pub contract_id: i64,
    pub seq: i32,
    pub due_date: NaiveDate,
    pub amount: Decimal,
    pub document_no: Option<String>,
    pub business_key: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct StationLite {
    pub id: i64,
    pub name: String,
    pub customer_number: String,
    pub landowner_id: i64,
}

#[derive(Debug, Clone, Default)]
pub struct SubjectIndex {
    pub bills: HashMap<i64, BillLite>,
    pub installments: HashMap<i64, InstallmentLite>,
    /// 契約 id → (契約編號, 地主 id)。
    pub contracts: HashMap<i64, (String, i64)>,
    /// 契約 → 它涵蓋的案場（順序穩定，畫面取第一個當代表）。
    pub stations_of_contract: HashMap<i64, Vec<i64>>,
    pub stations: HashMap<i64, StationLite>,
    pub owners: HashMap<i64, String>,
}

fn ids_of(execs: &[&ExecutionRow], subject_type: &str) -> Vec<i64> {
    crate::common::uniq(execs.iter().filter(|e| e.subject_type == subject_type).filter_map(|e| numeric_id(&e.subject_id)))
}

/// 一次撈齊（限本組織）；不逐列打 DB。
pub async fn load(tx: &mut Tx<'_>, org_id: i64, execs: &[&ExecutionRow]) -> ApiResult<SubjectIndex> {
    let mut idx = SubjectIndex::default();
    let bill_ids = ids_of(execs, "energyBill");
    if !bill_ids.is_empty() {
        let rows: Vec<BillLite> = sqlx::query_as(
            "SELECT id, station_id, customer_number_raw, billing_period_start, billing_period_end, total_amount, review_status,
                    document_no, business_key
               FROM tbl_energy_bill WHERE id = ANY($1) AND organization_id = $2 AND deleted_at IS NULL ORDER BY id",
        )
        .bind(&bill_ids)
        .bind(org_id)
        .fetch_all(&mut **tx)
        .await?;
        idx.bills = rows.into_iter().map(|b| (b.id, b)).collect();
    }
    let inst_ids = ids_of(execs, "energyInstallment");
    if !inst_ids.is_empty() {
        let rows: Vec<InstallmentLite> = sqlx::query_as(
            "SELECT id, contract_id, seq, due_date, amount, document_no, business_key
               FROM tbl_energy_installment WHERE id = ANY($1) AND organization_id = $2 AND deleted_at IS NULL ORDER BY id",
        )
        .bind(&inst_ids)
        .bind(org_id)
        .fetch_all(&mut **tx)
        .await?;
        idx.installments = rows.into_iter().map(|r| (r.id, r)).collect();
    }
    let contract_ids: Vec<i64> = crate::common::uniq(idx.installments.values().map(|i| i.contract_id));
    if !contract_ids.is_empty() {
        let rows: Vec<(i64, String, i64)> = sqlx::query_as(
            "SELECT id, contract_no, landowner_id FROM tbl_energy_contract WHERE id = ANY($1) AND organization_id = $2 AND deleted_at IS NULL",
        )
        .bind(&contract_ids)
        .bind(org_id)
        .fetch_all(&mut **tx)
        .await?;
        idx.contracts = rows.into_iter().map(|(id, no, owner)| (id, (no, owner))).collect();
        let cov: Vec<(i64, i64)> = sqlx::query_as(
            "SELECT contract_id, station_id FROM mn_energy_contract_station
              WHERE contract_id = ANY($1) AND station_id IS NOT NULL AND deleted_at IS NULL ORDER BY id",
        )
        .bind(&contract_ids)
        .fetch_all(&mut **tx)
        .await?;
        for (c, s) in cov {
            idx.stations_of_contract.entry(c).or_default().push(s);
        }
    }
    let mut station_ids: Vec<i64> = idx.bills.values().filter_map(|b| b.station_id).collect();
    station_ids.extend(idx.stations_of_contract.values().flatten().copied());
    let station_ids = crate::common::uniq(station_ids);
    if !station_ids.is_empty() {
        let rows: Vec<StationLite> = sqlx::query_as(
            "SELECT id, name, customer_number, landowner_id FROM tbl_energy_station
              WHERE id = ANY($1) AND organization_id = $2 AND deleted_at IS NULL",
        )
        .bind(&station_ids)
        .bind(org_id)
        .fetch_all(&mut **tx)
        .await?;
        idx.stations = rows.into_iter().map(|s| (s.id, s)).collect();
    }
    let mut owner_ids: Vec<i64> = idx.stations.values().map(|s| s.landowner_id).collect();
    owner_ids.extend(idx.contracts.values().map(|c| c.1));
    let owner_ids = crate::common::uniq(owner_ids);
    if !owner_ids.is_empty() {
        let rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT id, name FROM tbl_energy_landowner WHERE id = ANY($1) AND organization_id = $2 AND deleted_at IS NULL",
        )
        .bind(&owner_ids)
        .bind(org_id)
        .fetch_all(&mut **tx)
        .await?;
        idx.owners = rows.into_iter().collect();
    }
    Ok(idx)
}

fn dec(d: Option<Decimal>) -> Value {
    d.map(|v| json!(v.to_string())).unwrap_or(Value::Null)
}

fn d(v: Option<NaiveDate>) -> Value {
    v.map(|x| json!(crate::common::day(x))).unwrap_or(Value::Null)
}

impl SubjectIndex {
    fn contract_station(&self, contract_id: i64) -> Option<&StationLite> {
        self.stations_of_contract.get(&contract_id).and_then(|v| v.first()).and_then(|s| self.stations.get(s))
    }

    /// 清單／詳情用的摘要（只回識別資料，不回前端路由）。
    pub fn list_summary(&self, e: &ExecutionRow) -> Value {
        let id = numeric_id(&e.subject_id);
        match e.subject_type.as_str() {
            "energyBill" => {
                let b = id.and_then(|i| self.bills.get(&i));
                let st = b.and_then(|b| b.station_id).and_then(|s| self.stations.get(&s));
                let owner = st.map(|s| s.landowner_id);
                json!({
                    "kind": "energyBill",
                    "billId": b.map(|b| b.id),
                    "documentNo": b.and_then(|b| b.document_no.clone()),
                    "businessKey": b.and_then(|b| b.business_key.clone()),
                    "stationId": st.map(|s| s.id),
                    "stationName": st.map(|s| s.name.clone()),
                    "landownerId": owner,
                    "landownerName": owner.and_then(|o| self.owners.get(&o).cloned()),
                    "amount": dec(b.and_then(|b| b.total_amount)),
                    "periodStart": d(b.and_then(|b| b.billing_period_start)),
                    "periodEnd": d(b.and_then(|b| b.billing_period_end)),
                })
            }
            "energyInstallment" => {
                let i = id.and_then(|x| self.installments.get(&x));
                let c = i.and_then(|i| self.contracts.get(&i.contract_id).map(|c| (i.contract_id, c)));
                let st = c.and_then(|(cid, _)| self.contract_station(cid));
                let owner = c.map(|(_, c)| c.1);
                json!({
                    "kind": "energyInstallment",
                    "installmentId": i.map(|i| i.id),
                    "documentNo": i.and_then(|i| i.document_no.clone()),
                    "businessKey": i.and_then(|i| i.business_key.clone()),
                    "contractNo": c.map(|(_, c)| c.0.clone()),
                    "stationId": st.map(|s| s.id),
                    "stationName": st.map(|s| s.name.clone()),
                    "landownerId": owner,
                    "landownerName": owner.and_then(|o| self.owners.get(&o).cloned()),
                    "seq": i.map(|i| i.seq),
                    "amount": i.map(|i| json!(i.amount.to_string())),
                    "dueDate": i.map(|i| crate::common::day(i.due_date)),
                })
            }
            other => json!({ "kind": other }),
        }
    }

    /// 工作台列用的摘要（案場 · 地主 · 金額）。保證金那支用同一個 `totalAmount` 欄位名。
    pub fn task_summary(&self, e: &ExecutionRow) -> Value {
        let id = numeric_id(&e.subject_id);
        match e.subject_type.as_str() {
            "energyBill" => {
                let Some(b) = id.and_then(|i| self.bills.get(&i)) else { return Value::Null };
                let st = b.station_id.and_then(|s| self.stations.get(&s));
                let owner = st.map(|s| s.landowner_id);
                json!({
                    "kind": "energyBill",
                    "billId": b.id,
                    // 電號：優先用指認到的案場；沒指認就用原樣值（例外件）。
                    "customerNumber": st.map(|s| s.customer_number.clone()).or_else(|| b.customer_number_raw.clone()),
                    "stationName": st.map(|s| s.name.clone()),
                    "stationId": st.map(|s| s.id),
                    "landownerId": owner,
                    "landownerName": owner.and_then(|o| self.owners.get(&o).cloned()),
                    "billingPeriodStart": d(b.billing_period_start),
                    "billingPeriodEnd": d(b.billing_period_end),
                    "totalAmount": dec(b.total_amount),
                    "reviewStatus": b.review_status,
                })
            }
            "energyInstallment" => {
                let Some(i) = id.and_then(|x| self.installments.get(&x)) else { return Value::Null };
                let c = self.contracts.get(&i.contract_id);
                let st = self.contract_station(i.contract_id);
                let owner = c.map(|c| c.1);
                json!({
                    "kind": "energyInstallment",
                    "installmentId": i.id,
                    "contractNo": c.map(|c| c.0.clone()),
                    "seq": i.seq,
                    "stationId": st.map(|s| s.id),
                    "stationName": st.map(|s| s.name.clone()),
                    "customerNumber": st.map(|s| s.customer_number.clone()),
                    "landownerId": owner,
                    "landownerName": owner.and_then(|o| self.owners.get(&o).cloned()),
                    "totalAmount": i.amount.to_string(),
                    "dueDate": crate::common::day(i.due_date),
                })
            }
            _ => Value::Null,
        }
    }
}
