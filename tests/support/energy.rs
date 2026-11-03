//! 能源測試的資料佈景：主檔直接寫 DB（v1 沒有主檔寫入端點），流程定義用產品自己的安裝函式。

#![allow(dead_code)]

use super::{TestApp, TestUser};
use eo_energy::common::{Ctx, RequestMeta};
use eo_platform::error::ApiResult;
use serde_json::{json, Value};

/// 在一個交易裡跑一段引擎呼叫；成功才 commit。
#[macro_export]
macro_rules! in_tx {
    ($app:expr, $tx:ident => $body:expr) => {{
        let mut $tx = $app.core.pool.begin().await.unwrap();
        let r = $body;
        if r.is_ok() {
            $tx.commit().await.unwrap();
        }
        r
    }};
}

/// 錯誤的 (HTTP 狀態, code)。
pub fn code_of<T: std::fmt::Debug>(r: ApiResult<T>) -> (u16, String) {
    let e = r.expect_err("expected an error");
    (e.status.as_u16(), e.code)
}

pub fn date(s: &str) -> chrono::NaiveDate {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

impl TestApp {
    /// 以某位使用者在某組織的身分操作（只要求是成員；權限由各函式自己的規則決定）。
    pub async fn ctx(&self, user: &TestUser, org: i64) -> Ctx {
        let cu = eo_platform::auth::resolve_session(&self.core, &user.token).await.unwrap().unwrap();
        let access = self.core.rbac.authorize_member(&cu, org).await.unwrap();
        Ctx::new(&self.core, access, &RequestMeta { ip: Some("10.0.0.1".into()), trace_id: Some("test-trace".into()) })
    }

    /// 建使用者、加入組織、授予能源角色（None ＝ 只是成員）。
    pub async fn member(&self, org: i64, name: &str, role: Option<&str>) -> TestUser {
        let u = self.user(name, None).await;
        self.add_member(org, &u).await;
        if let Some(r) = role {
            self.grant_role(org, &u, r).await;
        }
        u
    }

    /// 建一個組織，擁有者另持 energy:admin。回傳 (擁有者, org)。
    pub async fn energy_org(&self, unique: &str) -> (TestUser, i64) {
        let owner = self.user(&format!("{unique} owner"), None).await;
        let org = self.org(&owner, unique).await;
        self.grant_role(org, &owner, "energy:admin").await;
        (owner, org)
    }

    pub async fn exec_sql(&self, sql: &str) {
        sqlx::query(sql).execute(&self.core.pool).await.unwrap();
    }

    pub async fn scalar_i64(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(&self.core.pool).await.unwrap()
    }

    pub async fn landowner(&self, org: i64, name: &str) -> i64 {
        sqlx::query_scalar("INSERT INTO tbl_energy_landowner (organization_id, name, status) VALUES ($1, $2, 'active') RETURNING id")
            .bind(org)
            .bind(name)
            .fetch_one(&self.core.pool)
            .await
            .unwrap()
    }

    pub async fn station(&self, org: i64, landowner: i64, customer_number: &str, name: &str) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO tbl_energy_station (organization_id, landowner_id, customer_number, name, status)
             VALUES ($1, $2, $3, $4, 'operating') RETURNING id",
        )
        .bind(org)
        .bind(landowner)
        .bind(customer_number)
        .bind(name)
        .fetch_one(&self.core.pool)
        .await
        .unwrap()
    }

    /// 電費單。金額、度數用字串給（numeric）。
    pub async fn bill(&self, org: i64, station: Option<i64>, start: &str, end: &str, total: Option<&str>, kwh: Option<&str>) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO tbl_energy_bill (organization_id, station_id, customer_number_raw, billing_period_start, billing_period_end,
                                          total_amount, generation_kwh, review_status)
             VALUES ($1, $2, 'RAW-0001', $3::date, $4::date, $5::numeric, $6::numeric, $7) RETURNING id",
        )
        .bind(org)
        .bind(station)
        .bind(start)
        .bind(end)
        .bind(total)
        .bind(kwh)
        .bind(if station.is_some() { "reviewed" } else { "pendingMatch" })
        .fetch_one(&self.core.pool)
        .await
        .unwrap()
    }

    /// 計算策略（版號依 (org, key) 自動配，直接發佈）。
    pub async fn strategy(&self, org: i64, key: &str, strategy_type: &str, params: Value, name: &str) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO tbl_energy_rating_strategy (organization_id, key, version, name, type, params, status, published_at, currency)
             VALUES ($1, $2, (SELECT COALESCE(MAX(version), 0) + 1 FROM tbl_energy_rating_strategy WHERE organization_id = $1 AND key = $2),
                     $3, $4, $5, 'published', now(), 'TWD') RETURNING id",
        )
        .bind(org)
        .bind(key)
        .bind(name)
        .bind(strategy_type)
        .bind(params)
        .fetch_one(&self.core.pool)
        .await
        .unwrap()
    }

    pub async fn strategy_binding(&self, org: i64, station: i64, strategy: i64, from: &str, to: Option<&str>) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO tbl_energy_rating_strategy_binding (organization_id, station_id, strategy_key, strategy_id, valid_from, valid_to)
             VALUES ($1, $2, (SELECT key FROM tbl_energy_rating_strategy WHERE id = $3), $3, $4::date, $5::date) RETURNING id",
        )
        .bind(org)
        .bind(station)
        .bind(strategy)
        .bind(from)
        .bind(to)
        .fetch_one(&self.core.pool)
        .await
        .unwrap()
    }

    /// 安裝一份流程定義（產品自己的安裝函式），回傳 definition id。
    pub async fn install_def(&self, owner: &TestUser, org: i64, def: &eo_domain::workflow_defs::WorkflowDefinition, parent: Option<i64>) -> i64 {
        let ctx = self.ctx(owner, org).await;
        let mut tx = self.core.pool.begin().await.unwrap();
        let row = eo_energy::workflow::definition::install_definition(&mut tx, &ctx.access, def, parent, ctx.now()).await.unwrap();
        tx.commit().await.unwrap();
        row.id
    }

    /// 案場層（或契約層）的流程綁定。
    pub async fn wf_binding(
        &self,
        org: i64,
        station_key: Option<&str>,
        contract: Option<i64>,
        doc_type: Option<&str>,
        def_id: i64,
        from: &str,
        to: Option<&str>,
    ) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO tbl_energy_workflow_binding (organization_id, station_key, contract_id, doc_type, workflow_key, definition_id, valid_from, valid_to)
             VALUES ($1, $2, $3, $4, (SELECT key FROM tbl_energy_workflow_definition WHERE id = $5), $5, $6::date, $7::date) RETURNING id",
        )
        .bind(org)
        .bind(station_key)
        .bind(contract)
        .bind(doc_type)
        .bind(def_id)
        .bind(from)
        .bind(to)
        .fetch_one(&self.core.pool)
        .await
        .unwrap()
    }

    pub async fn department(&self, org: i64, key: &str, name: &str) -> i64 {
        sqlx::query_scalar("INSERT INTO tbl_energy_department (organization_id, key, name) VALUES ($1, $2, $3) RETURNING id")
            .bind(org)
            .bind(key)
            .bind(name)
            .fetch_one(&self.core.pool)
            .await
            .unwrap()
    }

    pub async fn dept_member(&self, dept: i64, user: &TestUser, role: &str) {
        sqlx::query("INSERT INTO mn_energy_department_member (department_id, user_id, role) VALUES ($1, $2, $3)")
            .bind(dept)
            .bind(user.id)
            .bind(role)
            .execute(&self.core.pool)
            .await
            .unwrap();
    }

    pub async fn contract(&self, org: i64, landowner: i64, no: &str, from: &str, to: Option<&str>) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO tbl_energy_contract (organization_id, landowner_id, contract_no, valid_from, valid_to)
             VALUES ($1, $2, $3, $4::date, $5::date) RETURNING id",
        )
        .bind(org)
        .bind(landowner)
        .bind(no)
        .bind(from)
        .bind(to)
        .fetch_one(&self.core.pool)
        .await
        .unwrap()
    }

    pub async fn contract_station(&self, contract: i64, station: i64, params: Value, from: Option<&str>, to: Option<&str>) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO mn_energy_contract_station (contract_id, station_id, params, valid_from, valid_to)
             VALUES ($1, $2, $3, $4::date, $5::date) RETURNING id",
        )
        .bind(contract)
        .bind(station)
        .bind(params)
        .bind(from)
        .bind(to)
        .fetch_one(&self.core.pool)
        .await
        .unwrap()
    }

    pub async fn installment(&self, org: i64, contract: i64, seq: i32, due: &str, amount: &str) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO tbl_energy_installment (organization_id, contract_id, seq, due_date, amount)
             VALUES ($1, $2, $3, $4::date, $5::numeric) RETURNING id",
        )
        .bind(org)
        .bind(contract)
        .bind(seq)
        .bind(due)
        .bind(amount)
        .fetch_one(&self.core.pool)
        .await
        .unwrap()
    }

    /// 某目標的稽核列（舊到新）：(type, payload, outcome, actor_user_id)。
    pub async fn audit_rows(&self, org: i64, target_type: &str, target_id: i64) -> Vec<(String, Value, String, Option<i64>)> {
        sqlx::query_as(
            "SELECT type, payload, outcome, actor_user_id FROM tbl_energy_audit_event
              WHERE organization_id = $1 AND target_type = $2 AND target_id = $3 ORDER BY id",
        )
        .bind(org)
        .bind(target_type)
        .bind(target_id.to_string())
        .fetch_all(&self.core.pool)
        .await
        .unwrap()
    }

    /// 某一筆 execution 與它所有 task 的稽核 type（依寫入順序）。
    pub async fn audit_types_for_execution(&self, org: i64, execution: i64) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT a.type FROM tbl_energy_audit_event a
              WHERE a.organization_id = $1 AND (
                (a.target_type = 'energyWorkflowExecution' AND a.target_id = $2::text)
                OR (a.target_type = 'energyWorkflowTask' AND a.target_id IN (
                      SELECT id::text FROM tbl_energy_workflow_task WHERE execution_id = $2)))
              ORDER BY a.id",
        )
        .bind(org)
        .bind(execution)
        .fetch_all(&self.core.pool)
        .await
        .unwrap()
    }

    /// (id, node_key, state, outcome, assignee, seq, payload, branch_key)
    pub async fn tasks(&self, execution: i64) -> Vec<TaskLite> {
        sqlx::query_as(
            "SELECT id, node_key, state, outcome, assignee_user_id, seq, payload, branch_key, on_behalf_of_user_id, node_type
               FROM tbl_energy_workflow_task WHERE execution_id = $1 AND deleted_at IS NULL ORDER BY seq",
        )
        .bind(execution)
        .fetch_all(&self.core.pool)
        .await
        .unwrap()
    }

    /// (status, current_node_key)
    pub async fn exec_state(&self, execution: i64) -> (String, Option<String>) {
        sqlx::query_as("SELECT status, current_node_key FROM tbl_energy_workflow_execution WHERE id = $1")
            .bind(execution)
            .fetch_one(&self.core.pool)
            .await
            .unwrap()
    }

    /// 某 task 的候選人 (kind, user, resolved_by)。
    pub async fn candidates(&self, task: i64) -> Vec<(String, i64, String)> {
        sqlx::query_as(
            "SELECT kind, user_id, resolved_by FROM tbl_energy_workflow_task_candidate WHERE task_id = $1 AND deleted_at IS NULL ORDER BY id",
        )
        .bind(task)
        .fetch_all(&self.core.pool)
        .await
        .unwrap()
    }

    pub fn e(&self, org: i64, p: &str) -> String {
        format!("/api/v1/orgs/{org}/energy{p}")
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TaskLite {
    pub id: i64,
    pub node_key: String,
    pub state: String,
    pub outcome: Option<String>,
    pub assignee_user_id: Option<i64>,
    pub seq: i32,
    pub payload: Value,
    pub branch_key: Option<String>,
    pub on_behalf_of_user_id: Option<i64>,
    pub node_type: String,
}

pub fn potential(c: &[(String, i64, String)]) -> Vec<i64> {
    c.iter().filter(|x| x.0 == "potentialOwner").map(|x| x.1).collect()
}

pub fn excluded(c: &[(String, i64, String)]) -> Vec<i64> {
    c.iter().filter(|x| x.0 == "excludedOwner").map(|x| x.1).collect()
}

pub fn sorted(mut v: Vec<i64>) -> Vec<i64> {
    v.sort();
    v
}

pub fn empty() -> Value {
    json!({})
}
