use crate::support::{TestApp, TestUser};
use crate::support::energy::{date, TaskLite};
use serde_json::json;

#[actix_web::test]
async fn easyflow_draft_is_org_scoped_read_only_and_does_not_need_landlord_contract() {
    let app = TestApp::new().await;
    let (admin,org)=app.energy_org("ef-draft").await;
    let lo=app.landowner(org,"測試公司").await;
    let station=app.station(org,lo,"11-72-8007-94-7","測試案場").await;
    sqlx::query("UPDATE tbl_energy_station SET spv_payee_id=$1,data=$2 WHERE id=$3")
      .bind(lo).bind(json!({"plantNo":"32172","invoice":{"counterpartyName":"測試台電","counterpartyTaxId":"12345678"}})).bind(station)
      .execute(&app.core.pool).await.unwrap();
    let bill=app.bill(org,Some(station),"2026-09-01","2026-09-30",Some("16735"),None).await;
    sqlx::query("UPDATE tbl_energy_bill SET doc_type='renewableSurplus',review_status='pendingContract',detail_fields=$1 WHERE id=$2")
      .bind(json!({"purchaseEnergyFee":"16874","meterRentalFee":"-983"})).bind(bill).execute(&app.core.pool).await.unwrap();
    let url=app.e(org,&format!("/bills/{bill}/easyflow-application"));
    sqlx::query("UPDATE tbl_energy_bill SET detail_fields=detail_fields-'meterRentalFee',ocr_raw=$1 WHERE id=$2")
      .bind(json!({"record":{"fields":{"meterRentalSubtotal":{"value":-983,"status":"ocr_extracted"}}}}))
      .bind(bill).execute(&app.core.pool).await.unwrap();
    let v=expect_status!(app.get(&admin,&url).await,200);
    assert!(v["fields"].as_array().unwrap().iter().any(|f| f["key"]=="meterRentalFee" && f["value"]=="-983"));
    assert_eq!(v["applicationType"],"invoice");
    assert_eq!(v["submissionAllowed"],false);
    assert!(v["fields"].as_array().unwrap().iter().any(|f| f["key"]=="siteCode" && f["value"]=="32172"));
    assert!(v["fields"].as_array().unwrap().iter().any(|f| f["key"]=="amount" && f["value"]=="16874"));
    assert!(v["fields"].as_array().unwrap().iter().any(|f| f["key"]=="counterpartyName" && f["value"]=="測試台電"));
    // Bills received before master data was imported can still be previewed, without mutating workflow.
    sqlx::query("UPDATE tbl_energy_bill SET station_id=NULL,customer_number_raw='11728007947' WHERE id=$1")
      .bind(bill).execute(&app.core.pool).await.unwrap();
    let late=expect_status!(app.get(&admin,&url).await,200);
    assert!(late["fields"].as_array().unwrap().iter().any(|f| f["key"]=="siteCode" && f["value"]=="32172"));
    let revision=late["sourceRevision"].clone();
    assert!(revision.as_str().is_some());
    expect_status!(app.post(&admin,&url,json!({"sourceRevision":revision,"draftRevision":late["draftRevision"],"action":"confirm","values":{}})).await,422);
    let batch=expect_status!(app.post(&admin,&app.e(org,"/ingest-batches"),json!({"fileCount":1})).await,200);
    let (status,attachment)=crate::energy_recognition::upload_as(&app,&admin,&app.e(org,&format!("/ingest-batches/{}/attachments",batch["id"])),"source.pdf","application/pdf",b"%PDF-1.4\n% source test\n".to_vec()).await;
    assert_eq!(status,200);
    sqlx::query("UPDATE tbl_energy_bill SET source_attachment_id=$1 WHERE id=$2").bind(attachment["id"].as_i64().unwrap()).bind(bill).execute(&app.core.pool).await.unwrap();
    let with_attachment=expect_status!(app.get(&admin,&url).await,200);
    let revision=with_attachment["sourceRevision"].clone();
    let saved=expect_status!(app.post(&admin,&url,json!({"sourceRevision":revision,"draftRevision":with_attachment["draftRevision"],"action":"save","values":{"applicant":"測試承辦","department":"測試部門"}})).await,200);
    assert!(saved["fields"].as_array().unwrap().iter().any(|f|f["key"]=="applicant"&&f["value"]=="測試承辦"));
    expect_status!(app.post(&admin,&url,json!({"sourceRevision":revision,"draftRevision":with_attachment["draftRevision"],"action":"save","values":{"applicant":"舊分頁不得覆寫"}})).await,409);
    expect_status!(app.post(&admin,&url,json!({"sourceRevision":revision,"draftRevision":saved["draftRevision"],"action":"confirm","values":{}})).await,422);
    let confirmed=expect_status!(app.post(&admin,&url,json!({"sourceRevision":revision,"draftRevision":saved["draftRevision"],"action":"confirm","values":{"companyCode":"002","applicationDate":"2026-11-24","expectedReceiptDate":"2026-12-01"}})).await,200);
    assert_eq!(confirmed["applicationReview"]["state"],"confirmed_for_handoff");
    assert_eq!(confirmed["applicationReview"]["reviewedBy"],admin.id);
    assert_eq!(confirmed["submissionAllowed"],false);
    expect_status!(app.post(&admin,&url,json!({"sourceRevision":revision,"action":"save","values":{"amount":"1"}})).await,400);
    sqlx::query("UPDATE tbl_energy_bill SET detail_fields=jsonb_set(detail_fields,'{purchaseEnergyFee}','\"17000\"') WHERE id=$1").bind(bill).execute(&app.core.pool).await.unwrap();
    expect_status!(app.post(&admin,&url,json!({"sourceRevision":revision,"action":"save","values":{}})).await,409);
    let changed=expect_status!(app.get(&admin,&url).await,200);
    assert_eq!(changed["applicationReview"]["stale"],true);
    sqlx::query("UPDATE tbl_energy_bill SET detail_fields=jsonb_set(detail_fields,'{purchaseEnergyFee}','\"16874\"') WHERE id=$1").bind(bill).execute(&app.core.pool).await.unwrap();
    let after=expect_status!(app.get(&admin,&app.e(org,&format!("/bills/{bill}"))).await,200);
    assert_eq!(after["reviewStatus"],"pendingContract");
    assert!(after["executions"].as_array().unwrap().is_empty());
    let nobody=app.member(org,"no-role",None).await;
    expect_status!(app.get(&nobody,&url).await,403);
    expect_status!(app.post(&nobody,&url,json!({"sourceRevision":revision,"action":"save","values":{}})).await,403);
    let (other,org2)=app.energy_org("ef-other").await;
    expect_status!(app.get(&other,&app.e(org2,&format!("/bills/{bill}/easyflow-application"))).await,403);
    expect_status!(app.post(&other,&app.e(org2,&format!("/bills/{bill}/easyflow-application")),json!({"sourceRevision":revision,"action":"save","values":{}})).await,403);
    expect_status!(app.get(&admin,&format!("{url}?format=bad")).await,400);
    let csv=app.req(Some(&admin),reqwest::Method::GET,&format!("{url}?format=csv")).send().await.unwrap();
    assert_eq!(csv.status().as_u16(),200);
    assert!(csv.headers()["content-type"].to_str().unwrap().starts_with("text/csv"));
    let csv=csv.text().await.unwrap();
    assert!(csv.contains("16874"));
    assert!(csv.contains("來源附件清單"));
    assert!(csv.contains("待確認事項"));
}

async fn open(app: &TestApp, exec: i64) -> TaskLite {
    app.tasks(exec).await.into_iter().find(|t| (t.state == "ready" || t.state == "reserved") && t.node_type != "abortApproval").unwrap()
}

async fn step(app: &TestApp, org: i64, who: &TestUser, exec: i64, key: &str) {
    use eo_energy::workflow::engine::tasks::{self, ActionInput};
    let t = open(app, exec).await;
    let ctx = app.ctx(who, org).await;
    if t.state == "ready" {
        in_tx!(app, tx => tasks::claim(&mut tx, &ctx, t.id).await).unwrap();
    }
    let input = ActionInput { action_key: key.into(), ..Default::default() };
    in_tx!(app, tx => tasks::perform_action(&mut tx, &ctx, t.id, input).await).unwrap();
}

/// 卡 20261126-0214 S1～S3：PUT 預設值 → 帶入草稿 → 流程跑到 completed → 仍可存與確認、不鎖成 needs_recheck。
#[actix_web::test]
async fn easyflow_defaults_fill_draft_and_stay_confirmable_after_approval() {
    use eo_domain::workflow_defs::{default_bill_approval, DEPT_ACCOUNTING, DEPT_SALES};
    use eo_energy::workflow::engine::run::{start_execution, StartExecution};

    let app = TestApp::new().await;
    let owner = app.user("ef-flow owner", None).await;
    let org = app.org(&owner, "ef-flow").await;
    app.grant_role(org, &owner, "energy:admin").await;
    let clerk = app.member(org, "ef-clerk", Some("energy:operator")).await;
    let manager = app.member(org, "ef-manager", Some("energy:operator")).await;
    let finance = app.member(org, "ef-finance", Some("energy:finance")).await;
    let accountant = app.member(org, "ef-accountant", Some("energy:finance")).await;
    let sales = app.department(org, DEPT_SALES, "業務部").await;
    app.dept_member(sales, &clerk, "member").await;
    app.dept_member(sales, &manager, "manager").await;
    let acc_dept = app.department(org, DEPT_ACCOUNTING, "會計部門").await;
    app.dept_member(acc_dept, &accountant, "manager").await;

    let lo = app.landowner(org, "測試地主").await;
    let station_key = "EF-FLOW-0001".to_string();
    let station = app.station(org, lo, &station_key, "恆春菜市場（測試）").await;
    // 沒帶 spvPayee：companyName 要備援到 easyflowDefaults.companyName（S2 的「沒有才用 defaults」那一半，優先序另有 domain 單元測試）。
    sqlx::query("UPDATE tbl_energy_station SET data = $1 WHERE id = $2")
        .bind(json!({"invoice": {"counterpartyName": "測試台電"}}))
        .bind(station)
        .execute(&app.core.pool)
        .await
        .unwrap();
    let def = app.install_def(&owner, org, &default_bill_approval(), None).await;
    app.wf_binding(org, Some(&station_key), None, None, def, "2000-01-01", None).await;

    let bill = app.bill(org, Some(station), "2026-01-01", "2026-01-31", Some("50000"), None).await;
    sqlx::query("UPDATE tbl_energy_bill SET doc_type = 'renewableSurplus', detail_fields = $1 WHERE id = $2")
        .bind(json!({"purchaseEnergyFee": "50000"}))
        .bind(bill)
        .execute(&app.core.pool)
        .await
        .unwrap();
    // missing_handoff_fields 要求至少一份來源附件。
    let batch = expect_status!(app.post(&clerk, &app.e(org, "/ingest-batches"), json!({ "fileCount": 1 })).await, 200);
    let (status, attachment) = crate::energy_recognition::upload_as(
        &app, &clerk, &app.e(org, &format!("/ingest-batches/{}/attachments", batch["id"])),
        "source.pdf", "application/pdf", b"%PDF-1.4\n% ef flow test\n".to_vec(),
    ).await;
    assert_eq!(status, 200);
    sqlx::query("UPDATE tbl_energy_bill SET source_attachment_id = $1 WHERE id = $2")
        .bind(attachment["id"].as_i64().unwrap())
        .bind(bill)
        .execute(&app.core.pool)
        .await
        .unwrap();

    // 設定案場 EasyFlow 預設值（S1）：非法 key 先確認回 400。
    let defaults_url = app.e(org, &format!("/stations/{station}/easyflow-defaults"));
    expect_status!(app.put(&owner, &defaults_url, json!({ "notAllowedKey": "x" })).await, 400);
    expect_status!(app.put(&owner, &defaults_url, json!({ "expectedReceiptDays": 400 })).await, 400);
    let put = expect_status!(
        app.put(&owner, &defaults_url, json!({
            "companyName": "測試填值-備援公司", "companyCode": "測試填值-0001",
            "applicant": "測試填值-承辦", "department": "測試填值-部門", "expectedReceiptDays": 30,
        })).await,
        200
    );
    assert_eq!(put["easyflowDefaults"]["companyCode"], "測試填值-0001");
    let got = expect_status!(app.get(&owner, &defaults_url).await, 200);
    assert_eq!(got["easyflowDefaults"]["expectedReceiptDays"], 30);
    assert_eq!(got["canManage"], true);

    // 走完整個流程到 completed（金額低於門檻，不經加簽；仿 energy_default_flow.rs 的 end_to_end 走法）。
    let ctx = app.ctx(&clerk, org).await;
    let input = StartExecution {
        station_key: station_key.clone(), workflow_key: "bill-approval".into(), subject_type: "energyBill".into(),
        subject_id: bill.to_string(), business_date: date("2026-01-31"),
        attributes: json!({ "bill": { "totalAmount": 50000 } }), definition: None, station_id: Some(station),
    };
    let exec = in_tx!(app, tx => start_execution(&mut tx, &ctx, input).await).unwrap().id;
    step(&app, org, &manager, exec, "submit").await;
    assert_eq!(open(&app, exec).await.node_key, "manager-approve");
    step(&app, org, &manager, exec, "approve").await;
    assert_eq!(open(&app, exec).await.node_key, "finance-release", "金額低於門檻，不經加簽");
    step(&app, org, &finance, exec, "release").await;
    assert_eq!(open(&app, exec).await.node_key, "payment");
    {
        use eo_energy::workflow::engine::tasks::{self, ActionInput};
        let t = open(&app, exec).await;
        let acc_ctx = app.ctx(&accountant, org).await;
        in_tx!(app, tx => tasks::claim(&mut tx, &acc_ctx, t.id).await).unwrap();
        let mark = ActionInput { action_key: "markPaid".into(), note: Some("測試填值-匯款單號".into()), ..Default::default() };
        in_tx!(app, tx => tasks::perform_action(&mut tx, &acc_ctx, t.id, mark).await).unwrap();
        let submit = ActionInput { action_key: "submit".into(), ..Default::default() };
        in_tx!(app, tx => tasks::perform_action(&mut tx, &acc_ctx, t.id, submit).await).unwrap();
    }
    assert_eq!(app.exec_state(exec).await, ("completed".into(), Some("end".into())));

    // 審核已完成：草稿仍帶入、仍可改、仍可確認——不是 needs_recheck（S2／S3）。
    let app_url = app.e(org, &format!("/bills/{bill}/easyflow-application"));
    let preview = expect_status!(app.get(&owner, &app_url).await, 200);
    assert_eq!(preview["canReview"], true, "審核完成後仍要放行：Finished 也算 canReview");
    assert_eq!(preview["missingHandoffFields"], json!([]));
    assert_ne!(preview["applicationReview"]["state"], "needs_recheck");
    let field = |p: &serde_json::Value, key: &str| p["fields"].as_array().unwrap().iter().find(|f| f["key"] == key).unwrap()["value"].clone();
    assert_eq!(field(&preview, "companyName"), "測試填值-備援公司");
    assert_eq!(field(&preview, "companyCode"), "測試填值-0001");
    let today = (chrono::Utc::now() + chrono::Duration::hours(8)).date_naive().format("%Y-%m-%d").to_string();
    assert_eq!(field(&preview, "applicationDate"), today);
    assert!(field(&preview, "expectedReceiptDate").as_str().is_some(), "預計收款日要算出來");

    let confirmed = expect_status!(
        app.post(&owner, &app_url, json!({
            "sourceRevision": preview["sourceRevision"], "draftRevision": preview["draftRevision"],
            "action": "confirm", "values": {},
        })).await,
        200
    );
    assert_eq!(confirmed["applicationReview"]["state"], "confirmed_for_handoff");
    assert_eq!(confirmed["submissionAllowed"], false);
    let recheck = expect_status!(app.get(&owner, &app_url).await, 200);
    assert_ne!(recheck["applicationReview"]["state"], "needs_recheck", "確認過之後重開仍不是待補查");
}
