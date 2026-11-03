//! 主責者解析：task 進入 ready 的當下把候選池寫成快照（事後答得出「當初誰有權核、為什麼是他」）。
//! 候選池為空時不可靜默通過 —— task 停在 ready 等管理員處理，絕不自動跳關。

use super::super::model::{ExecutionRow, NodeRow};
use crate::common::{err_d, Ctx, Tx};
use eo_domain::workflow_defs::{AssignmentRule, ManagerFallback};
use eo_platform::error::ApiResult;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct Resolved {
    pub potential_owners: Vec<i64>,
    pub excluded_owners: Vec<i64>,
    pub resolved_by: String,
}

/// 這個組織內持有任一指定角色的使用者（依授權建立的先後）。
pub async fn users_with_roles(tx: &mut Tx<'_>, org_id: i64, roles: &[String]) -> ApiResult<Vec<i64>> {
    if roles.is_empty() {
        return Ok(vec![]);
    }
    Ok(sqlx::query_scalar(
        "SELECT g.user_id FROM tbl_user_rbac_role g
           JOIN tbl_rbac_role r ON r.id = g.role_id AND r.deleted_at IS NULL AND r.scope = 'organization'
          WHERE g.organization_id = $1 AND r.unique_name = ANY($2) AND g.deleted_at IS NULL
          GROUP BY g.user_id ORDER BY MIN(g.id)",
    )
    .bind(org_id)
    .bind(roles)
    .fetch_all(&mut **tx)
    .await?)
}

/// R1 的核心：這個人所屬部門（限本組織）的主管們。多人＝候選池，不去重成一個。
pub async fn managers_of_user(tx: &mut Tx<'_>, org_id: i64, user_id: i64) -> ApiResult<Vec<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT m.user_id FROM mn_energy_department_member m
          WHERE m.role = 'manager' AND m.deleted_at IS NULL AND m.department_id IN (
            SELECT mine.department_id FROM mn_energy_department_member mine
              JOIN tbl_energy_department d ON d.id = mine.department_id AND d.organization_id = $1 AND d.deleted_at IS NULL
             WHERE mine.user_id = $2 AND mine.deleted_at IS NULL)
          GROUP BY m.user_id ORDER BY MIN(m.id)",
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_all(&mut **tx)
    .await?)
}

/// 本組織某部門的成員；`scope='managers'` 只取主管。部門以 key 指涉，key 只在組織內唯一。
pub async fn users_in_department(tx: &mut Tx<'_>, org_id: i64, key: &str, scope: &str) -> ApiResult<Vec<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT m.user_id FROM mn_energy_department_member m
          WHERE m.deleted_at IS NULL AND ($3 <> 'managers' OR m.role = 'manager') AND m.department_id = (
            SELECT d.id FROM tbl_energy_department d
             WHERE d.organization_id = $1 AND d.key = $2 AND d.deleted_at IS NULL ORDER BY d.id LIMIT 1)
          GROUP BY m.user_id ORDER BY MIN(m.id)",
    )
    .bind(org_id)
    .bind(key)
    .bind(scope)
    .fetch_all(&mut **tx)
    .await?)
}

/// `who` 與中斷核可規則共用：把一個指派規則解析成使用者。
pub async fn resolve_rule(tx: &mut Tx<'_>, org_id: i64, rule: &Value) -> ApiResult<Vec<i64>> {
    match AssignmentRule::from(rule.clone()) {
        AssignmentRule::None => Ok(vec![]),
        AssignmentRule::Role { role_unique_names } => users_with_roles(tx, org_id, &role_unique_names.unwrap_or_default()).await,
        AssignmentRule::Department { department_key, scope } => {
            users_in_department(tx, org_id, department_key.as_deref().unwrap_or(""), scope.as_deref().unwrap_or("all")).await
        }
        other => Err(err_d(501, "unsupported_assignment_rule", json!({ "ruleType": other.rule_type() }))),
    }
}

/// 依節點規則解析候選池並寫成快照（可重跑：先軟刪舊的再寫新的）。
pub async fn resolve_candidates(tx: &mut Tx<'_>, ctx: &Ctx, exec: &ExecutionRow, node: &NodeRow, task_id: i64) -> ApiResult<Resolved> {
    let rule = AssignmentRule::from(node.assignment_rule.clone());
    let org = exec.organization_id;
    let starter = exec.started_by_user_id;
    let mut resolved_by = rule.rule_type().to_string();
    let mut potential: Vec<i64> = match &rule {
        AssignmentRule::None => vec![],
        AssignmentRule::Role { role_unique_names } => users_with_roles(tx, org, role_unique_names.as_deref().unwrap_or(&[])).await?,
        AssignmentRule::Department { department_key, scope } => {
            users_in_department(tx, org, department_key.as_deref().unwrap_or(""), scope.as_deref().unwrap_or("all")).await?
        }
        AssignmentRule::DepartmentManagerOfInitiator { fallback, role_unique_names } => {
            // 最多兩跳（同仁 → 部門主管 → CXO），不做遞迴主管鏈。
            let mut p = match starter {
                Some(s) => managers_of_user(tx, org, s).await?,
                None => vec![],
            };
            // 主管自己發起的單，他不能當自己的核可者 —— 這正是 fallback 存在的理由。
            p.retain(|u| Some(*u) != starter);
            if p.is_empty() {
                match fallback.clone().unwrap_or(ManagerFallback::Role) {
                    ManagerFallback::Role => {
                        let names = role_unique_names.clone().unwrap_or_else(|| vec!["energy:cxo".into()]);
                        p = users_with_roles(tx, org, &names).await?;
                        p.retain(|u| Some(*u) != starter);
                        // resolved_by 欄位只有 40 字，後綴只能短。
                        resolved_by = "departmentManagerOfInitiator:fallback".into();
                    }
                    ManagerFallback::Error => {
                        return Err(err_d(409, "no_department_manager_for_initiator", json!({ "starterId": starter })));
                    }
                    // 跳關＝無人核准就放行，本版不做。
                    ManagerFallback::SkipNode => {
                        return Err(err_d(501, "fallback_skip_node_not_supported", json!({ "starterId": starter })));
                    }
                    ManagerFallback::Other(_) => {}
                }
            }
            p
        }
        AssignmentRule::Unsupported { rule_type, .. } => {
            return Err(err_d(501, "unsupported_assignment_rule", json!({ "ruleType": rule_type })));
        }
    };

    // R4（永遠開啟）：不可自核時把發起人寫成一列 excludedOwner —— 做成資料，稽核才看得到排除了誰。
    let excluded: Vec<i64> = match starter {
        Some(s) if !node.allow_self_approval => vec![s],
        _ => vec![],
    };
    potential.retain(|u| !excluded.contains(u));

    sqlx::query("UPDATE tbl_energy_workflow_task_candidate SET deleted_at = $2 WHERE task_id = $1 AND deleted_at IS NULL")
        .bind(task_id)
        .bind(ctx.now())
        .execute(&mut **tx)
        .await?;
    for u in &potential {
        super::super::model::insert_candidate(tx, task_id, *u, "potentialOwner", &resolved_by).await?;
    }
    for u in &excluded {
        super::super::model::insert_candidate(tx, task_id, *u, "excludedOwner", "allowSelfApproval=false").await?;
    }
    Ok(Resolved { potential_owners: potential, excluded_owners: excluded, resolved_by })
}
