//! 整合頁：地主（四區：案場／現在正在發生／近期應付／近期事件）與案場（兩條效期軸）。
//! 流程綁定比電號、策略綁定比 station_id —— 兩張表的鍵不一樣，寫成同一種比對會有一邊永遠空白。

use super::{get_landowner, station_in_org, station_view, RECENT_BILL_LIMIT};
use crate::common::{day, day_opt, days_since, Ctx, Tx};
use crate::workflow::definition::in_effect;
use chrono::{DateTime, NaiveDate, Utc};
use eo_platform::error::ApiResult;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::collections::HashMap;

type Seg = (i64, String, i64, NaiveDate, Option<NaiveDate>);

/// 現在生效的那一段（多段都生效時取起日最晚的）。
fn active_binding<'a>(rows: impl Iterator<Item = &'a Seg>, today: NaiveDate) -> Option<&'a Seg> {
    rows.filter(|b| in_effect(b.3, b.4, today)).max_by_key(|b| b.3)
}

pub async fn landowner_overview(tx: &mut Tx<'_>, ctx: &Ctx, landowner_id: i64, stuck_days: i64) -> ApiResult<Value> {
    let mut base = get_landowner(tx, &ctx.access, landowner_id).await?;
    let org = ctx.org_id();
    let today = ctx.today();
    let stations: Vec<Value> = base["stations"].as_array().cloned().unwrap_or_default();
    let station_ids: Vec<i64> = stations.iter().filter_map(|s| s["id"].as_i64()).collect();
    let keys: Vec<String> = stations.iter().filter_map(|s| s["customerNumber"].as_str().map(String::from)).collect();
    // (id, 比對鍵, 版本 id, 起, 迄)
    let flows: Vec<Seg> = sqlx::query_as(
        "SELECT id, station_key, definition_id, valid_from, valid_to FROM tbl_energy_workflow_binding
          WHERE organization_id = $1 AND station_key = ANY($2) AND deleted_at IS NULL ORDER BY id",
    )
    .bind(org)
    .bind(&keys)
    .fetch_all(&mut **tx)
    .await?;
    let strats: Vec<Seg> = sqlx::query_as(
        "SELECT id, station_id::text, strategy_id, valid_from, valid_to FROM tbl_energy_rating_strategy_binding
          WHERE organization_id = $1 AND station_id = ANY($2) AND deleted_at IS NULL ORDER BY id",
    )
    .bind(org)
    .bind(&station_ids)
    .fetch_all(&mut **tx)
    .await?;
    let defs: HashMap<i64, (String, i32)> = sqlx::query_as::<_, (i64, String, i32)>(
        "SELECT id, name, version FROM tbl_energy_workflow_definition WHERE organization_id = $1 AND deleted_at IS NULL",
    )
    .bind(org)
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(|(id, n, v)| (id, (n, v)))
    .collect();
    let strategies: HashMap<i64, (String, String, i32)> = sqlx::query_as::<_, (i64, String, String, i32)>(
        "SELECT id, name, type, version FROM tbl_energy_rating_strategy WHERE organization_id = $1 AND deleted_at IS NULL",
    )
    .bind(org)
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(|(id, n, t, v)| (id, (n, t, v)))
    .collect();
    let stations_out: Vec<Value> = stations
        .into_iter()
        .map(|mut st| {
            let cn = st["customerNumber"].as_str().unwrap_or("").to_string();
            let sid = st["id"].as_i64().unwrap_or(0).to_string();
            let fb = active_binding(flows.iter().filter(|b| b.1 == cn), today);
            let sb = active_binding(strats.iter().filter(|b| b.1 == sid), today);
            st["workflow"] = fb
                .and_then(|b| defs.get(&b.2).map(|d| json!({ "definitionId": b.2, "name": d.0, "version": d.1 })))
                .unwrap_or(Value::Null);
            st["strategy"] = sb
                .and_then(|b| strategies.get(&b.2).map(|s| json!({ "strategyId": b.2, "name": s.0, "type": s.1, "version": s.2 })))
                .unwrap_or(Value::Null);
            st
        })
        .collect();

    // 現在正在發生：他的單卡在哪、卡多久。subjectType 不可省 —— 兩種 subject 的 id 空間會撞。
    let rows: Vec<(i64, i64, Option<String>, i64, Option<i64>, Option<Decimal>)> = sqlx::query_as(
        "SELECT e.id, e.definition_id, e.current_node_key, b.id, b.station_id, b.total_amount
           FROM tbl_energy_workflow_execution e
           JOIN tbl_energy_bill b ON b.id::text = e.subject_id AND b.organization_id = $1 AND b.deleted_at IS NULL
          WHERE e.organization_id = $1 AND e.status = 'running' AND e.subject_type = 'energyBill' AND e.deleted_at IS NULL
            AND b.station_id = ANY($2)
          ORDER BY e.id DESC",
    )
    .bind(org)
    .bind(&station_ids)
    .fetch_all(&mut **tx)
    .await?;
    let exec_ids: Vec<i64> = rows.iter().map(|r| r.0).collect();
    let open: Vec<(i64, i32, DateTime<Utc>)> = sqlx::query_as(
        "SELECT execution_id, seq, created_at FROM tbl_energy_workflow_task
          WHERE execution_id = ANY($1) AND state = ANY($2) AND deleted_at IS NULL ORDER BY id",
    )
    .bind(&exec_ids)
    .bind(&crate::workflow::model::ACTIVE_STATES[..])
    .fetch_all(&mut **tx)
    .await?;
    let node_names: HashMap<(i64, String), String> = sqlx::query_as::<_, (i64, String, String)>(
        "SELECT n.definition_id, n.node_key, n.name FROM tbl_energy_workflow_node n
          WHERE n.definition_id IN (SELECT definition_id FROM tbl_energy_workflow_execution WHERE id = ANY($1)) AND n.deleted_at IS NULL",
    )
    .bind(&exec_ids)
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(|(d, k, n)| ((d, k), n))
    .collect();
    let station_name: HashMap<i64, String> = stations_out
        .iter()
        .filter_map(|s| Some((s["id"].as_i64()?, s["name"].as_str()?.to_string())))
        .collect();
    let now = ctx.now();
    let in_flight: Vec<Value> = rows
        .iter()
        .map(|(eid, def, node, bill, sid, amount)| {
            let task = open.iter().filter(|t| t.0 == *eid).max_by_key(|t| t.1);
            let waiting = task.map(|t| days_since(now, t.2));
            json!({
                "executionId": eid, "billId": bill, "stationId": sid,
                "stationName": sid.and_then(|s| station_name.get(&s)),
                "totalAmount": amount.map(|a| a.to_string()),
                "nodeKey": node,
                "nodeName": node.as_ref().and_then(|k| node_names.get(&(*def, k.clone()))),
                "waitingDays": waiting,
                // 卡太久要標燒橘 —— 門檻跟著回，前端不 hardcode。
                "stuck": waiting.is_some_and(|w| w >= stuck_days),
            })
        })
        .collect();
    base["stations"] = json!(stations_out);
    base["inFlight"] = json!(in_flight);
    base["stuckDays"] = json!(stuck_days);
    Ok(base)
}

/// 案場詳情：兩條效期軸（每一段帶 `active`，前端不比日期）、契約、近期單據。
pub async fn station_overview(tx: &mut Tx<'_>, ctx: &Ctx, station_id: i64) -> ApiResult<Value> {
    let st = station_in_org(tx, &ctx.access, station_id).await?;
    let org = ctx.org_id();
    let today = ctx.today();
    let owner: Option<(i64, String)> =
        sqlx::query_as("SELECT id, name FROM tbl_energy_landowner WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL")
            .bind(st.landowner_id)
            .bind(org)
            .fetch_optional(&mut **tx)
            .await?;
    let flows: Vec<(i64, NaiveDate, Option<NaiveDate>, i64, Option<String>, Option<i32>, Option<String>)> = sqlx::query_as(
        "SELECT b.id, b.valid_from, b.valid_to, b.definition_id, d.name, d.version, d.key FROM tbl_energy_workflow_binding b
           LEFT JOIN tbl_energy_workflow_definition d ON d.id = b.definition_id AND d.deleted_at IS NULL
          WHERE b.organization_id = $1 AND b.station_key = $2 AND b.deleted_at IS NULL ORDER BY b.valid_from ASC, b.id ASC",
    )
    .bind(org)
    .bind(&st.customer_number)
    .fetch_all(&mut **tx)
    .await?;
    let strats: Vec<(i64, NaiveDate, Option<NaiveDate>, i64, Option<String>, Option<String>, Option<i32>)> = sqlx::query_as(
        "SELECT b.id, b.valid_from, b.valid_to, b.strategy_id, s.name, s.type, s.version FROM tbl_energy_rating_strategy_binding b
           LEFT JOIN tbl_energy_rating_strategy s ON s.id = b.strategy_id AND s.deleted_at IS NULL
          WHERE b.organization_id = $1 AND b.station_id = $2 AND b.deleted_at IS NULL ORDER BY b.valid_from ASC, b.id ASC",
    )
    .bind(org)
    .bind(st.id)
    .fetch_all(&mut **tx)
    .await?;
    let contracts: Vec<(i64, String, String, NaiveDate, Option<NaiveDate>)> = sqlx::query_as(
        "SELECT c.id, c.contract_no, c.stage, c.valid_from, c.valid_to FROM tbl_energy_contract c
          WHERE c.organization_id = $1 AND c.deleted_at IS NULL AND c.id IN (
            SELECT contract_id FROM mn_energy_contract_station WHERE station_id = $2 AND deleted_at IS NULL)
          ORDER BY c.id",
    )
    .bind(org)
    .bind(st.id)
    .fetch_all(&mut **tx)
    .await?;
    let bills: Vec<(i64, Option<NaiveDate>, Option<NaiveDate>, Option<Decimal>, String)> = sqlx::query_as(
        "SELECT id, billing_period_start, billing_period_end, total_amount, review_status FROM tbl_energy_bill
          WHERE organization_id = $1 AND station_id = $2 AND deleted_at IS NULL ORDER BY id DESC LIMIT $3",
    )
    .bind(org)
    .bind(st.id)
    .bind(RECENT_BILL_LIMIT)
    .fetch_all(&mut **tx)
    .await?;
    let owners: HashMap<i64, String> = owner.iter().cloned().collect();
    let mut out = station_view(&st, &owners);
    out["landowner"] = owner.map(|(id, name)| json!({ "id": id, "name": name })).unwrap_or(Value::Null);
    out["workflowAxis"] = json!(flows
        .iter()
        .map(|b| json!({
            "id": b.0, "validFrom": day(b.1), "validTo": day_opt(b.2), "active": in_effect(b.1, b.2, today),
            "definitionId": b.3, "name": b.4, "version": b.5, "key": b.6,
        }))
        .collect::<Vec<_>>());
    out["strategyAxis"] = json!(strats
        .iter()
        .map(|b| json!({
            "id": b.0, "validFrom": day(b.1), "validTo": day_opt(b.2), "active": in_effect(b.1, b.2, today),
            "strategyId": b.3, "name": b.4, "type": b.5, "version": b.6,
        }))
        .collect::<Vec<_>>());
    out["contracts"] = json!(contracts
        .iter()
        .map(|c| json!({ "id": c.0, "contractNo": c.1, "stage": c.2, "validFrom": day(c.3), "validTo": day_opt(c.4) }))
        .collect::<Vec<_>>());
    out["recentBills"] = json!(bills
        .iter()
        .map(|b| json!({
            "id": b.0, "billingPeriodStart": day_opt(b.1), "billingPeriodEnd": day_opt(b.2),
            "totalAmount": b.3.map(|a| a.to_string()), "reviewStatus": b.4,
        }))
        .collect::<Vec<_>>());
    Ok(out)
}
