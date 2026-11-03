//! 示範資料：帳號登得進去、主檔與綁定的數量對得上、重跑不新建任何東西。

use crate::support::{TestApp, TestUser};
use reqwest::Method;
use serde_json::json;

/// 來源：dev/demo-seed.ts（冪等）
#[actix_web::test]
async fn demo_seed_is_idempotent_and_accounts_work() {
    let app = TestApp::new().await;
    let first = eo_energy::demo_seed::run(&app.core).await.unwrap();
    let org = first.org_id;

    let c = &first.created;
    assert_eq!(c.organizations, 1);
    assert_eq!((c.users, c.grants), (7, 7));
    assert_eq!((c.landowners, c.stations, c.contracts, c.contract_stations), (2, 6, 6, 6));
    assert_eq!((c.installments, c.calendar_events), (3, 3));
    assert_eq!((c.departments, c.department_members), (3, 7));
    // 四份內建定義 v1 ＋ 核可流程 v2；策略 v1（內建）＋ v2。
    assert_eq!((c.workflow_definitions, c.rating_strategies), (5, 2));
    // 案場層 7 段 ＋ 契約層 4 × 2 段。
    assert_eq!((c.workflow_bindings, c.rating_bindings), (15, 3));
    assert_eq!(c.executions, 1);

    let q = |sql: &str| sql.replace("$ORG", &org.to_string());
    assert_eq!(app.scalar_i64(&q("SELECT COUNT(*) FROM tbl_energy_station WHERE organization_id = $ORG")).await, 6);
    assert_eq!(app.scalar_i64(&q("SELECT COUNT(*) FROM tbl_energy_contract WHERE organization_id = $ORG")).await, 6);
    assert_eq!(app.scalar_i64(&q("SELECT COUNT(*) FROM tbl_energy_installment WHERE organization_id = $ORG")).await, 3);
    assert_eq!(app.scalar_i64(&q("SELECT COUNT(*) FROM tbl_energy_workflow_binding WHERE organization_id = $ORG")).await, 15);
    assert_eq!(app.scalar_i64(&q("SELECT COUNT(*) FROM tbl_energy_workflow_binding WHERE organization_id = $ORG AND contract_id IS NOT NULL")).await, 8);
    assert_eq!(app.scalar_i64(&q("SELECT COUNT(*) FROM tbl_energy_rating_strategy_binding WHERE organization_id = $ORG")).await, 3);
    // 電號一律有正規化鍵（配對靠它）。
    assert_eq!(app.scalar_i64(&q("SELECT COUNT(*) FROM tbl_energy_station WHERE organization_id = $ORG AND customer_number_key IS NULL")).await, 0);

    // 保證金請款跑到完成，期數因此成立。
    let exec = first.deposit_execution_id.expect("deposit execution");
    assert_eq!(app.exec_state(exec).await.0, "completed");
    assert_eq!(
        app.scalar_i64(&q("SELECT COUNT(*) FROM tbl_energy_installment WHERE organization_id = $ORG AND seq = 1 AND approval_state = 'approved'")).await,
        1
    );

    // 第二次：什麼都不新建，同一個組織、同一筆執行。
    let audit_before = app.scalar_i64(&q("SELECT COUNT(*) FROM tbl_energy_audit_event WHERE organization_id = $ORG")).await;
    let second = eo_energy::demo_seed::run(&app.core).await.unwrap();
    assert_eq!(second.org_id, org);
    assert_eq!(second.created.total(), 0, "second run created: {:?}", second.created);
    assert_eq!(second.deposit_execution_id, Some(exec));
    assert_eq!(app.scalar_i64(&q("SELECT COUNT(*) FROM tbl_energy_audit_event WHERE organization_id = $ORG")).await, audit_before);
    assert_eq!(app.scalar_i64(&q("SELECT COUNT(*) FROM tbl_energy_workflow_binding WHERE organization_id = $ORG")).await, 15);

    // 七個帳號都登得進去，角色對得上。
    let roles: Vec<(&str, &str)> = second.accounts.iter().map(|a| (a.email.as_str(), a.role.as_str())).collect();
    assert_eq!(
        roles,
        vec![
            ("admin@demo.energy", "energy:admin"),
            ("operator@demo.energy", "energy:operator"),
            ("manager@demo.energy", "energy:operator"),
            ("finance@demo.energy", "energy:finance"),
            ("accounting@demo.energy", "energy:finance"),
            ("cxo@demo.energy", "energy:cxo"),
            ("viewer@demo.energy", "energy:viewer"),
        ]
    );
    let mut operator = None;
    for a in &second.accounts {
        let v = expect_status!(
            TestApp::send(
                app.req(None, Method::POST, "/api/v1/auth/login")
                    .json(&json!({ "username": a.email, "password": eo_energy::demo_seed::DEMO_PASSWORD }))
            )
            .await,
            200
        );
        let token = v["token"].as_str().unwrap().to_string();
        if a.email == "operator@demo.energy" {
            operator = Some(TestUser { id: a.user_id, email: a.email.clone(), token });
        }
    }

    // 承辦看得到案場清單。
    let operator = operator.unwrap();
    let v = expect_status!(app.get(&operator, &app.e(org, "/stations")).await, 200);
    assert_eq!(v["items"].as_array().map(Vec::len), Some(6), "body: {v}");
}
