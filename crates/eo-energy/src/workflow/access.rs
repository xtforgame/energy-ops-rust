//! execution 是可視度的單位：掛在它底下的 task、候選池、副作用、血緣、附件，可視度一律等於
//! 「你看不看得到這個 execution」。
//!
//! 看得到 ⇔ 是參與者（發起人、任一 task 的 assignee／本人、候選池、上述任一人的有效代理人）
//!          ∨ 持有 `energy:flow:view-all`（或 `platform:rbac:manage`）。
//! 預設窄（參與者），把「看全部」做成一個明確、可稽核、可指派的權限。

use super::model::{self, ExecutionRow};
use crate::common::{err_d, has_permission, Tx};
use crate::gates;
use chrono::NaiveDate;
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use eo_platform::rbac::OrgAccess;
use serde_json::json;
use std::collections::{HashMap, HashSet};

/// 已通過 execution 可視度判定。欄位私有：只能從 `authorize_execution` 拿到。
#[derive(Debug, Clone)]
pub struct ExecutionAccess {
    org: OrgAccess,
    execution: ExecutionRow,
    reason: &'static str,
}

impl ExecutionAccess {
    pub fn org(&self) -> &OrgAccess {
        &self.org
    }
    pub fn execution(&self) -> &ExecutionRow {
        &self.execution
    }
    pub fn execution_id(&self) -> i64 {
        self.execution.id
    }
    /// `view-all` 或 `participant`。
    pub fn reason(&self) -> &'static str {
        self.reason
    }
}

/// 這個人看不看得到全組織的流程單。
pub async fn has_view_all(core: &CoreRef, access: &OrgAccess) -> ApiResult<bool> {
    has_permission(core, access, gates::FLOW_VIEW_ALL[0]).await
}

/// execution 可視度的單一入口。permission 那一半由呼叫端先過（`authorize_org`），這裡只做「能不能碰這一份」。
pub async fn authorize_execution(core: &CoreRef, tx: &mut Tx<'_>, access: &OrgAccess, execution_id: i64) -> ApiResult<ExecutionAccess> {
    let execution = model::execution_in_org(tx, access.org_id(), execution_id).await?;
    if has_view_all(core, access).await? {
        return Ok(ExecutionAccess { org: access.clone(), execution, reason: "view-all" });
    }
    let Some(uid) = access.user_id() else {
        return Ok(ExecutionAccess { org: access.clone(), execution, reason: "view-all" });
    };
    let visible = visible_execution_ids(tx, &[execution_id], uid, core.clock.today()).await?;
    if visible.contains(&execution_id) {
        Ok(ExecutionAccess { org: access.clone(), execution, reason: "participant" })
    } else {
        Err(err_d(403, "execution_not_visible", json!({ "reason": "not-a-participant" })))
    }
}

/// 批次可視度 —— 參與者規則的唯一實作（清單與詳情共用，兩份會漂移成「清單看得到、詳情打不開」）。
/// 回傳的是參與者判定，不含 view-all。
pub async fn visible_execution_ids(tx: &mut Tx<'_>, execution_ids: &[i64], user_id: i64, today: NaiveDate) -> ApiResult<HashSet<i64>> {
    let ids: Vec<i64> = crate::common::uniq(execution_ids.iter().copied());
    let mut out = HashSet::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let execs: Vec<(i64, i64, Option<i64>)> = sqlx::query_as(
        "SELECT id, organization_id, started_by_user_id FROM tbl_energy_workflow_execution WHERE id = ANY($1) AND deleted_at IS NULL",
    )
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    let mut parts: HashMap<i64, HashSet<i64>> = HashMap::new();
    for (id, _, starter) in &execs {
        let set = parts.entry(*id).or_default();
        if let Some(s) = starter {
            set.insert(*s);
        }
    }
    // 任一 task 的 assignee／本人（含已完成的：經手過就永遠看得到）。
    let tasks: Vec<(i64, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT execution_id, assignee_user_id, on_behalf_of_user_id FROM tbl_energy_workflow_task
          WHERE execution_id = ANY($1) AND deleted_at IS NULL",
    )
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    for (e, a, b) in tasks {
        let set = parts.entry(e).or_default();
        set.extend(a);
        set.extend(b);
    }
    // 候選池 —— 輪到我這關的人在動手之前就要看得到。
    let cands: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT t.execution_id, c.user_id FROM tbl_energy_workflow_task_candidate c
           JOIN tbl_energy_workflow_task t ON t.id = c.task_id AND t.deleted_at IS NULL
          WHERE t.execution_id = ANY($1) AND c.kind = 'potentialOwner' AND c.deleted_at IS NULL",
    )
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    for (e, u) in cands {
        parts.entry(e).or_default().insert(u);
    }
    // 我是任一參與者的有效代理人。
    let orgs: Vec<i64> = crate::common::uniq(execs.iter().map(|e| e.1));
    let mut delegators = HashSet::new();
    for org in orgs {
        delegators.extend(super::engine::delegation::delegators_for(tx, org, user_id, today).await?);
    }
    for (id, set) in parts {
        if set.contains(&user_id) || set.iter().any(|u| delegators.contains(u)) {
            out.insert(id);
        }
    }
    Ok(out)
}
