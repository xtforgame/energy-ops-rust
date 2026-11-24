//! 檢查器核心（卡 E S2）：工作天、三種事件產生器、事前提醒／逾期／隔天再提醒、冪等。
//! 背景迴圈照 `ingest/recognition.rs:540-552` 的 `spawn_background` 抄。
//!
//! 規則參數（`tbl_energy_reminder_rule`）全部允許空值：沒有列、或列存在但 `enabled=false`，
//! 一律視為「那一種提醒不發」——但事件本身仍然會依日期轉成 `overdue`（那是事件的事實，不是提醒設定）。

use crate::common::{day, Ctx, Tx};
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use serde_json::{json, Value};
use sqlx::FromRow;
use std::collections::HashMap;

#[derive(Debug, Clone, FromRow)]
pub struct RuleRow {
    pub kind: String,
    pub lead_days: Option<i32>,
    pub repeat_days: Option<i32>,
    pub business_days: bool,
    pub day_of_month: Option<i32>,
    pub enabled: bool,
}

/// 是不是工作天：假日表的日子優先（補班日 `is_workday=true` 上班、其餘假日休）；
/// 沒在假日表裡的，週六週日休，其餘上班。
pub async fn is_workday(tx: &mut Tx<'_>, org: i64, d: NaiveDate) -> ApiResult<bool> {
    let holiday: Option<bool> = sqlx::query_scalar(
        "SELECT is_workday FROM tbl_energy_holiday WHERE organization_id = $1 AND day = $2 AND deleted_at IS NULL",
    )
    .bind(org)
    .bind(d)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(is_workday) = holiday {
        return Ok(is_workday);
    }
    Ok(!matches!(d.weekday(), Weekday::Sat | Weekday::Sun))
}

/// 從 `from` 位移 `delta` 天（可正可負）；`business=true` 時只數工作天（照 `is_workday`），否則數日曆天。
pub async fn shift(tx: &mut Tx<'_>, org: i64, from: NaiveDate, delta: i64, business: bool) -> ApiResult<NaiveDate> {
    if !business {
        return Ok(from + Duration::days(delta));
    }
    let mut d = from;
    let step: i64 = if delta >= 0 { 1 } else { -1 };
    let mut remain = delta.abs();
    while remain > 0 {
        d += Duration::days(step);
        if is_workday(tx, org, d).await? {
            remain -= 1;
        }
    }
    Ok(d)
}

async fn load_rules(tx: &mut Tx<'_>, org: i64) -> ApiResult<HashMap<String, RuleRow>> {
    let rows: Vec<RuleRow> = sqlx::query_as(
        "SELECT kind, lead_days, repeat_days, business_days, day_of_month, enabled
           FROM tbl_energy_reminder_rule WHERE organization_id = $1 AND deleted_at IS NULL",
    )
    .bind(org)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows.into_iter().map(|r| (r.kind.clone(), r)).collect())
}

/// `contractExpiry`：每份有 `valid_to` 的合約到期各一筆（去重：kind+contract_id+event_date）。
async fn generate_contract_expiry(tx: &mut Tx<'_>, org: i64) -> ApiResult<i64> {
    let rows: Vec<(i64, String, NaiveDate, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT c.id, c.contract_no, c.valid_to, c.landowner_id,
                (SELECT station_id FROM mn_energy_contract_station cs WHERE cs.contract_id = c.id AND cs.deleted_at IS NULL ORDER BY cs.id LIMIT 1)
           FROM tbl_energy_contract c WHERE c.organization_id = $1 AND c.valid_to IS NOT NULL AND c.deleted_at IS NULL",
    )
    .bind(org)
    .fetch_all(&mut **tx)
    .await?;
    let mut n = 0i64;
    for (contract_id, contract_no, valid_to, landowner_id, station_id) in rows {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM tbl_energy_calendar_event
               WHERE organization_id = $1 AND kind = 'contractExpiry' AND contract_id = $2 AND event_date = $3 AND deleted_at IS NULL)",
        )
        .bind(org)
        .bind(contract_id)
        .bind(valid_to)
        .fetch_one(&mut **tx)
        .await?;
        if exists {
            continue;
        }
        sqlx::query(
            "INSERT INTO tbl_energy_calendar_event (organization_id, landowner_id, contract_id, station_id, event_date, kind, title, status)
             VALUES ($1,$2,$3,$4,$5,'contractExpiry',$6,'pending')",
        )
        .bind(org)
        .bind(landowner_id)
        .bind(contract_id)
        .bind(station_id)
        .bind(valid_to)
        .bind(format!("合約「{contract_no}」到期"))
        .execute(&mut **tx)
        .await?;
        n += 1;
    }
    Ok(n)
}

/// `contractRenewal`：每份有 `renewal_remind_on` 的合約各一筆。
async fn generate_contract_renewal(tx: &mut Tx<'_>, org: i64) -> ApiResult<i64> {
    let rows: Vec<(i64, String, NaiveDate, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT c.id, c.contract_no, c.renewal_remind_on, c.landowner_id,
                (SELECT station_id FROM mn_energy_contract_station cs WHERE cs.contract_id = c.id AND cs.deleted_at IS NULL ORDER BY cs.id LIMIT 1)
           FROM tbl_energy_contract c WHERE c.organization_id = $1 AND c.renewal_remind_on IS NOT NULL AND c.deleted_at IS NULL",
    )
    .bind(org)
    .fetch_all(&mut **tx)
    .await?;
    let mut n = 0i64;
    for (contract_id, contract_no, remind_on, landowner_id, station_id) in rows {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM tbl_energy_calendar_event
               WHERE organization_id = $1 AND kind = 'contractRenewal' AND contract_id = $2 AND event_date = $3 AND deleted_at IS NULL)",
        )
        .bind(org)
        .bind(contract_id)
        .bind(remind_on)
        .fetch_one(&mut **tx)
        .await?;
        if exists {
            continue;
        }
        sqlx::query(
            "INSERT INTO tbl_energy_calendar_event (organization_id, landowner_id, contract_id, station_id, event_date, kind, title, status)
             VALUES ($1,$2,$3,$4,$5,'contractRenewal',$6,'pending')",
        )
        .bind(org)
        .bind(landowner_id)
        .bind(contract_id)
        .bind(station_id)
        .bind(remind_on)
        .bind(format!("合約「{contract_no}」續約提醒"))
        .execute(&mut **tx)
        .await?;
        n += 1;
    }
    Ok(n)
}

/// `billExpected`：只在規則有 `dayOfMonth` 時，替每個啟用案場產生「這個月」當月一筆。
async fn generate_bill_expected(tx: &mut Tx<'_>, org: i64, as_of: NaiveDate, rules: &HashMap<String, RuleRow>) -> ApiResult<i64> {
    let Some(rule) = rules.get("billExpected").filter(|r| r.enabled) else { return Ok(0) };
    let Some(dom) = rule.day_of_month else { return Ok(0) };
    let last_day = NaiveDate::from_ymd_opt(as_of.year(), as_of.month(), 1)
        .and_then(|d| d.checked_add_months(chrono::Months::new(1)))
        .map(|d| d.pred_opt().unwrap())
        .map(|d| d.day())
        .unwrap_or(28);
    let Some(event_date) = NaiveDate::from_ymd_opt(as_of.year(), as_of.month(), (dom as u32).min(last_day)) else { return Ok(0) };
    let stations: Vec<(i64, String, Option<i64>)> =
        sqlx::query_as("SELECT id, name, landowner_id FROM tbl_energy_station WHERE organization_id = $1 AND enabled = true AND deleted_at IS NULL")
            .bind(org)
            .fetch_all(&mut **tx)
            .await?;
    let mut n = 0i64;
    for (station_id, name, landowner_id) in stations {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM tbl_energy_calendar_event
               WHERE organization_id = $1 AND kind = 'billExpected' AND station_id = $2 AND event_date = $3 AND deleted_at IS NULL)",
        )
        .bind(org)
        .bind(station_id)
        .bind(event_date)
        .fetch_one(&mut **tx)
        .await?;
        if exists {
            continue;
        }
        sqlx::query(
            "INSERT INTO tbl_energy_calendar_event (organization_id, landowner_id, station_id, event_date, kind, title, status)
             VALUES ($1,$2,$3,$4,'billExpected',$5,'pending')",
        )
        .bind(org)
        .bind(landowner_id)
        .bind(station_id)
        .bind(event_date)
        .bind(format!("預期收到電費單（{name}）"))
        .execute(&mut **tx)
        .await?;
        n += 1;
    }
    Ok(n)
}

/// `billExpected` 事件：該案場當月已經有電費單了就自動記 `occurred_on`（視同完成，不用再提醒）。
async fn auto_mark_bill_occurred(tx: &mut Tx<'_>, org: i64, as_of: NaiveDate) -> ApiResult<i64> {
    let rows: Vec<(i64, i64, NaiveDate)> = sqlx::query_as(
        "SELECT id, station_id, event_date FROM tbl_energy_calendar_event
          WHERE organization_id = $1 AND kind = 'billExpected' AND status IN ('pending','overdue')
            AND occurred_on IS NULL AND deleted_at IS NULL AND station_id IS NOT NULL",
    )
    .bind(org)
    .fetch_all(&mut **tx)
    .await?;
    let mut n = 0i64;
    for (id, station_id, event_date) in rows {
        let found: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM tbl_energy_bill b WHERE b.organization_id = $1 AND b.station_id = $2 AND b.deleted_at IS NULL
               AND (date_trunc('month', b.billing_period_start) = date_trunc('month', $3::date)
                    OR date_trunc('month', b.issued_at) = date_trunc('month', $3::date)))",
        )
        .bind(org)
        .bind(station_id)
        .bind(event_date)
        .fetch_one(&mut **tx)
        .await?;
        if found {
            sqlx::query("UPDATE tbl_energy_calendar_event SET occurred_on = $2, status = 'done' WHERE id = $1 AND deleted_at IS NULL")
                .bind(id)
                .bind(as_of)
                .execute(&mut **tx)
                .await?;
            n += 1;
        }
    }
    Ok(n)
}

/// 插一筆通知；同一事件同一種提醒同一天已經有就不重發（靠部分唯一索引 `ON CONFLICT DO NOTHING`）。回有沒有真的插入。
async fn insert_notification(tx: &mut Tx<'_>, org: i64, event_id: i64, kind: &str, notify_on: NaiveDate, message: &str) -> ApiResult<bool> {
    let id: Option<i64> = sqlx::query_scalar(
        "INSERT INTO tbl_energy_notification (organization_id, event_id, kind, message, notify_on)
         VALUES ($1,$2,$3,$4,$5)
         ON CONFLICT (event_id, kind, notify_on) WHERE deleted_at IS NULL DO NOTHING
         RETURNING id",
    )
    .bind(org)
    .bind(event_id)
    .bind(kind)
    .bind(message)
    .bind(notify_on)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(id.is_some())
}

#[derive(Debug, Clone, FromRow)]
struct EventRow {
    id: i64,
    kind: String,
    event_date: NaiveDate,
    status: String,
    title: String,
    last_notified_on: Option<NaiveDate>,
}

/// 檢查器主流程：產生事件 → 自動完成 `billExpected` → 逐一判斷事前提醒／逾期／隔天再提醒。
/// 冪等：同一天跑兩次不重發（`insert_notification` 靠唯一索引；狀態轉換用 `WHERE status='pending'` 不會重複轉）。
pub async fn run_check(tx: &mut Tx<'_>, ctx: &Ctx, as_of: NaiveDate) -> ApiResult<Value> {
    let org = ctx.org_id();
    let rules = load_rules(tx, org).await?;
    let generated = generate_contract_expiry(tx, org).await?
        + generate_contract_renewal(tx, org).await?
        + generate_bill_expected(tx, org, as_of, &rules).await?;
    let auto_done = auto_mark_bill_occurred(tx, org, as_of).await?;

    let events: Vec<EventRow> = sqlx::query_as(
        "SELECT id, kind, event_date, status, title, last_notified_on FROM tbl_energy_calendar_event
          WHERE organization_id = $1 AND deleted_at IS NULL AND status IN ('pending','overdue') AND occurred_on IS NULL
          ORDER BY event_date ASC, id ASC",
    )
    .bind(org)
    .fetch_all(&mut **tx)
    .await?;

    let mut notifications = 0i64;
    let mut overdue_marked = 0i64;
    for ev in events {
        // 逾期是事件的事實：到期了、還是 pending、沒有實際發生 → 轉 overdue。跟有沒有規則無關。
        if ev.event_date <= as_of && ev.status == "pending" {
            sqlx::query("UPDATE tbl_energy_calendar_event SET status = 'overdue' WHERE id = $1 AND status = 'pending'")
                .bind(ev.id)
                .execute(&mut **tx)
                .await?;
            overdue_marked += 1;
        }

        // 規則沒設定（或關掉）＝那一種提醒不發；狀態轉換已經做完，不受影響。
        let Some(rule) = rules.get(&ev.kind).filter(|r| r.enabled) else { continue };

        if ev.event_date > as_of {
            // 事前提醒：還沒到期，看 lead_days（工作天或日曆天），且這個事件**還沒發過** upcoming。
            if let Some(lead) = rule.lead_days {
                let threshold = shift(tx, org, ev.event_date, -(lead as i64), rule.business_days).await?;
                if as_of >= threshold {
                    let already: bool = sqlx::query_scalar(
                        "SELECT EXISTS (SELECT 1 FROM tbl_energy_notification WHERE event_id = $1 AND kind = 'upcoming' AND deleted_at IS NULL)",
                    )
                    .bind(ev.id)
                    .fetch_one(&mut **tx)
                    .await?;
                    if !already {
                        let msg = format!("「{}」即將到期（{}）", ev.title, day(ev.event_date));
                        if insert_notification(tx, org, ev.id, "upcoming", as_of, &msg).await? {
                            notifications += 1;
                        }
                    }
                }
            }
        } else {
            // 到期或已過期：第一次一定發；之後只在 as_of ≥ last_notified_on + repeat_days（隔天再提醒＝1）再發。
            let due = match ev.last_notified_on {
                None => true,
                Some(last) => match rule.repeat_days {
                    None => false,
                    Some(rep) => {
                        let next = shift(tx, org, last, rep as i64, rule.business_days).await?;
                        as_of >= next
                    }
                },
            };
            if due {
                let msg = format!("「{}」已逾期（原訂 {}）", ev.title, day(ev.event_date));
                if insert_notification(tx, org, ev.id, "overdue", as_of, &msg).await? {
                    sqlx::query("UPDATE tbl_energy_calendar_event SET last_notified_on = $2 WHERE id = $1")
                        .bind(ev.id)
                        .bind(as_of)
                        .execute(&mut **tx)
                        .await?;
                    notifications += 1;
                }
            }
        }
    }

    Ok(json!({
        "asOf": day(as_of), "generatedEvents": generated, "autoCompleted": auto_done,
        "overdueMarked": overdue_marked, "notifications": notifications,
    }))
}

/// 背景任務：每 `secs` 秒替每個組織跑一輪 `run_check(today)`。照 `recognition::spawn_background` 抄。
pub fn spawn_background(core: CoreRef, secs: u64) -> actix_web::rt::task::JoinHandle<()> {
    actix_web::rt::spawn(async move {
        let mut iv = actix_web::rt::time::interval(std::time::Duration::from_secs(secs));
        loop {
            iv.tick().await;
            match tick_all_orgs(&core).await {
                Ok(n) if n > 0 => log::info!("[energy][reminders] checked {n} orgs"),
                Ok(_) => {}
                Err(e) => log::error!("[energy][reminders] check failed: {e}"),
            }
        }
    })
}

async fn tick_all_orgs(core: &CoreRef) -> ApiResult<usize> {
    let org_ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM tbl_organization WHERE deleted_at IS NULL").fetch_all(&core.pool).await?;
    let today = core.clock.today();
    for org_id in &org_ids {
        let ctx = Ctx::system(core, *org_id);
        let mut tx = core.pool.begin().await?;
        run_check(&mut tx, &ctx, today).await?;
        tx.commit().await?;
    }
    Ok(org_ids.len())
}
