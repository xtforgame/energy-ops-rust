//! 卡 E S2：檢查器核心規則 —— 工作天（假日＋補班）、事前提醒／逾期／隔天再提醒／同日不重發／記實際發生後停。

use crate::support::TestApp;
use chrono::{Datelike, NaiveDate, Weekday};
use eo_energy::reminders::check::{is_workday, run_check};

/// 從 `from` 往後找到第一個「是不是週末」符合 `weekend` 的日期（不猜死某年某月是星期幾）。
fn find_day(from: NaiveDate, weekend: bool) -> NaiveDate {
    let mut d = from;
    loop {
        let is_weekend = matches!(d.weekday(), Weekday::Sat | Weekday::Sun);
        if is_weekend == weekend {
            return d;
        }
        d = d.succ_opt().unwrap();
    }
}

#[actix_web::test]
async fn workday_respects_holidays_and_makeup_days() {
    let app = TestApp::new().await;
    let (_owner, org) = app.energy_org("e1-org").await;
    let base = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();

    let weekday = find_day(base, false);
    let weekend = find_day(base, true);
    let holiday_on_weekday = find_day(weekday, false); // 另一個平日，拿來當「國定假日」
    let makeup_on_weekend = find_day(weekend, true); // 另一個週末，拿來當「補班日」

    {
        let mut tx = app.core.pool.begin().await.unwrap();
        // 平日不特別設 → 照週曆算：上班。
        assert!(is_workday(&mut tx, org, weekday).await.unwrap(), "平日沒設假日表應該上班");
        // 週末不特別設 → 照週曆算：休假。
        assert!(!is_workday(&mut tx, org, weekend).await.unwrap(), "週末沒設假日表應該休假");
    }

    app.exec_sql(&format!(
        "INSERT INTO tbl_energy_holiday (organization_id, day, name, is_workday) VALUES ({org}, '{holiday_on_weekday}', '國定假日（測試）', false)"
    ))
    .await;
    app.exec_sql(&format!(
        "INSERT INTO tbl_energy_holiday (organization_id, day, name, is_workday) VALUES ({org}, '{makeup_on_weekend}', '補班日（測試）', true)"
    ))
    .await;

    let mut tx = app.core.pool.begin().await.unwrap();
    // 假日表標成國定假日的平日：改休假。
    assert!(!is_workday(&mut tx, org, holiday_on_weekday).await.unwrap(), "假日表的國定假日應該休假，即使它本來是平日");
    // 假日表標成補班日的週末：改上班。
    assert!(is_workday(&mut tx, org, makeup_on_weekend).await.unwrap(), "假日表的補班日應該上班，即使它本來是週末");
}

#[actix_web::test]
async fn run_check_upcoming_overdue_repeat_and_stop() {
    let app = TestApp::new().await;
    let (owner, org) = app.energy_org("e2-org").await;
    let ctx = app.ctx(&owner, org).await;

    // 規則：提前 3 天提醒、逾期後每 1 天再提醒（日曆天，方便寫死日期斷言）。
    app.exec_sql(&format!(
        "INSERT INTO tbl_energy_reminder_rule (organization_id, kind, lead_days, repeat_days, business_days, enabled)
         VALUES ({org}, 'installmentDue', 3, 1, false, true)"
    ))
    .await;
    let event_id: i64 = sqlx::query_scalar(
        "INSERT INTO tbl_energy_calendar_event (organization_id, event_date, kind, title, status)
         VALUES ($1, '2026-03-10', 'installmentDue', '測試事件', 'pending') RETURNING id",
    )
    .bind(org)
    .fetch_one(&app.core.pool)
    .await
    .unwrap();

    let day = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
    let status_of = |app: &TestApp, id: i64| {
        let pool = app.core.pool.clone();
        async move { sqlx::query_scalar::<_, String>("SELECT status FROM tbl_energy_calendar_event WHERE id = $1").bind(id).fetch_one(&pool).await.unwrap() }
    };

    // 1) 事前提醒：3 月 7 日（3 月 10 日前 3 個日曆天）第一次發「即將到期」。
    let mut tx = app.core.pool.begin().await.unwrap();
    let r1 = run_check(&mut tx, &ctx, day("2026-03-07")).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(r1["notifications"], 1, "事前提醒第一次要發 1 筆");
    assert_eq!(status_of(&app, event_id).await, "pending");

    // 2) 同一天重跑：不重發。
    let mut tx = app.core.pool.begin().await.unwrap();
    let r2 = run_check(&mut tx, &ctx, day("2026-03-07")).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(r2["notifications"], 0, "同一天重跑不該再發一次事前提醒");

    // 3) 3 月 11 日（次日）：轉逾期，發第一筆逾期通知。
    let mut tx = app.core.pool.begin().await.unwrap();
    let r3 = run_check(&mut tx, &ctx, day("2026-03-11")).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(status_of(&app, event_id).await, "overdue", "過了預定日還沒記實際發生要轉逾期");
    assert_eq!(r3["notifications"], 1, "逾期第一次要發 1 筆");

    // 4) 同一天（3/11）重跑：還沒到下一次提醒的日子，不重發。
    let mut tx = app.core.pool.begin().await.unwrap();
    let r4 = run_check(&mut tx, &ctx, day("2026-03-11")).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(r4["notifications"], 0, "同一天不該重發逾期通知");

    // 5) 3 月 12 日（隔天）：repeat_days=1，再發一次（隔天再提醒）。
    let mut tx = app.core.pool.begin().await.unwrap();
    let r5 = run_check(&mut tx, &ctx, day("2026-03-12")).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(r5["notifications"], 1, "隔天再提醒要發 1 筆");

    // 6) 記了實際發生日之後：不再提醒，即使隔天又跑了。
    app.exec_sql(&format!("UPDATE tbl_energy_calendar_event SET occurred_on = '2026-03-12', status = 'done' WHERE id = {event_id}")).await;
    let mut tx = app.core.pool.begin().await.unwrap();
    let r6 = run_check(&mut tx, &ctx, day("2026-03-13")).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(r6["notifications"], 0, "記了實際發生之後不該再發提醒");
}
