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
    // 卡 A（20261118-0427）起：原 2 位個人地主 ＋ 5 位新類型受款對象（2 子公司／1 台電區處／1 公司／1 機關）＝ 7；
    // 原 6 座案場 ＋ 1 座「示範案場（雙電號）」＝ 7，卡 20261118-0811-d 起再 ＋ 1 座「示範案場（本關要求）」
    // （假電費單掛的案場，電號才能顯示成設計要的假電號，不被 workflow/subjects.rs 的既有邏輯蓋成真案場的電號）＝ 8。
    assert_eq!((c.landowners, c.stations, c.contracts, c.contract_stations), (7, 8, 6, 6));
    // 6 座舊案場各一個主電號＋100% 分配，加上新案場的 2 個電號＋2 筆分配（60／40）。
    assert_eq!((c.meters, c.allocations), (8, 8));
    assert_eq!((c.installments, c.calendar_events), (3, 3));
    assert_eq!((c.departments, c.department_members), (5, 10));
    // 四份內建定義 v1 ＋ 核可流程 v2；策略 v1（內建）＋ v2。
    assert_eq!((c.workflow_definitions, c.rating_strategies), (5, 2));
    // 案場層 7 段 ＋ 契約層 4 × 2 段。
    assert_eq!((c.workflow_bindings, c.rating_bindings), (15, 3));
    // 保證金請款那筆 ＋ 卡 20261118-0811-d 起：示範「本關要求」的假電費單（停在平台資料審核）。
    assert_eq!(c.executions, 2);

    let q = |sql: &str| sql.replace("$ORG", &org.to_string());
    assert_eq!(app.scalar_i64(&q("SELECT COUNT(*) FROM tbl_energy_station WHERE organization_id = $ORG")).await, 8);
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
    assert_eq!(v["items"].as_array().map(Vec::len), Some(8), "body: {v}");
}

/// 來源：az scripts/energy-ops-demo-seed（範本電費單那一段，非 NO_BILLS 模式）
#[actix_web::test]
async fn demo_sample_bills_set_up_the_four_states_and_are_idempotent() {
    let app = TestApp::new().await;
    let seed = eo_energy::demo_seed::run(&app.core).await.unwrap();
    let org = seed.org_id;
    let dir = std::path::Path::new("tests/fixtures/samples");
    let first = eo_energy::demo_seed::run_sample_bills(&app.core, &seed, dir).await.unwrap();
    assert!(first.created);
    assert_eq!(first.bill_ids.len(), 4, "四份範本都產了單：{first:?}");

    let q = |sql: &str| sql.replace("$ORG", &org.to_string());
    // 每一張都有原始檔，且都來自同一批。
    assert_eq!(app.scalar_i64(&q("SELECT COUNT(*) FROM tbl_energy_bill WHERE organization_id = $ORG AND source_attachment_id IS NOT NULL")).await, 4);
    // 四個電號都種成案場，四張都配得到。
    assert_eq!(app.scalar_i64(&q("SELECT COUNT(*) FROM tbl_energy_bill WHERE organization_id = $ORG AND station_id IS NULL")).await, 0);
    // 改過金額的那張：最終值 4800，辨識值不動。
    assert_eq!(
        app.scalar_i64(&q("SELECT COUNT(*) FROM tbl_energy_bill WHERE organization_id = $ORG AND total_amount = 4800.00 AND (ocr_extracted->>'totalAmount')::numeric <> 4800")).await,
        1
    );

    // 主線停在平台資料審核；候選池那筆已跑完。
    let main = first.main_execution_id.expect("main execution");
    let (status, node) = app.exec_state(main).await;
    assert_eq!((status.as_str(), node.as_deref()), ("running", Some("platform-review")));
    let cand = first.candidate_execution_id.expect("candidate execution");
    assert_eq!(app.exec_state(cand).await.0, "completed");
    // operator 是承辦確認的候選人，但從沒被指派（只有候選池這一層關聯）。
    let operator = seed.accounts.iter().find(|a| a.email == "operator@demo.energy").unwrap().user_id;
    let tasks = format!("SELECT id FROM tbl_energy_workflow_task WHERE execution_id = {cand}");
    assert_eq!(app.scalar_i64(&format!("SELECT COUNT(*) FROM tbl_energy_workflow_task_candidate WHERE task_id IN ({tasks}) AND user_id = {operator}")).await, 1);
    assert_eq!(app.scalar_i64(&format!("SELECT COUNT(*) FROM tbl_energy_workflow_task WHERE execution_id = {cand} AND assignee_user_id = {operator}")).await, 0);

    // 第二次整段略過。
    let second = eo_energy::demo_seed::run_sample_bills(&app.core, &seed, dir).await.unwrap();
    assert!(!second.created);
    assert_eq!(second.batch_id, first.batch_id);
    // 四份範本單 ＋ run() 那邊卡 20261118-0811-d 加的示範假單（1 筆）＝ 5。
    assert_eq!(app.scalar_i64(&q("SELECT COUNT(*) FROM tbl_energy_bill WHERE organization_id = $ORG")).await, 5);
}
