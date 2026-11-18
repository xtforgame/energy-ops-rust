//! 取版（「這個案場、這個業務日期，適用哪一版流程」）與定義的安裝。
//!
//! 取版的錨點是**業務日期**，不是今天；取不到是錯誤，不是預設值（靜默 fallback 會讓補登單走錯約）。

use super::model::{DefinitionRow, DEF_COLS};
use crate::common::{err_d, Ctx, Tx};
use chrono::NaiveDate;
use eo_domain::document_ingest::{doc_type_lookup_chain, EnergyDocType};
use eo_domain::workflow_defs::{ActionKind, NodeType, TargetMode, TransitionKind, WorkflowDefinition};
use eo_platform::error::ApiResult;
use eo_platform::rbac::OrgAccess;
use serde_json::{json, Value};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct BindingRow {
    pub id: i64,
    pub station_key: Option<String>,
    pub doc_type: Option<String>,
    pub workflow_key: String,
    pub valid_from: NaiveDate,
    pub valid_to: Option<NaiveDate>,
    pub station_id: Option<i64>,
    pub contract_id: Option<i64>,
    pub definition_id: i64,
}

pub const BINDING_COLS: &str = "id, station_key, doc_type, workflow_key, valid_from, valid_to, station_id, contract_id, definition_id";

/// 半開區間 `[from, to)`。
pub fn in_effect(from: NaiveDate, to: Option<NaiveDate>, d: NaiveDate) -> bool {
    from <= d && to.is_none_or(|t| d < t)
}

async fn published_definition(tx: &mut Tx<'_>, access: &OrgAccess, id: i64, workflow_key: Option<&str>) -> ApiResult<DefinitionRow> {
    let def = super::model::definition_by_id(tx, access.org_id(), id)
        .await?
        .ok_or_else(|| err_d(500, "definition_not_found", json!({ "workflowKey": workflow_key })))?;
    // 綁定只能指向已發佈的版本 —— 綁到 draft 等於讓還沒定稿的流程上線。
    if def.status != "published" {
        return Err(err_d(409, "definition_not_published", json!({ "workflowKey": workflow_key, "definitionId": def.id })));
    }
    Ok(def)
}

/// 依 (案場電號, 流程 key, 業務日期) 取版。效期重疊時取最近一段適用的。
pub async fn resolve_definition(
    tx: &mut Tx<'_>,
    access: &OrgAccess,
    station_key: &str,
    workflow_key: &str,
    business_date: NaiveDate,
) -> ApiResult<DefinitionRow> {
    let rows: Vec<BindingRow> = sqlx::query_as(&format!(
        "SELECT {BINDING_COLS} FROM tbl_energy_workflow_binding
          WHERE organization_id = $1 AND station_key = $2 AND workflow_key = $3 AND deleted_at IS NULL
          ORDER BY valid_from DESC, id DESC"
    ))
    .bind(access.org_id())
    .bind(station_key)
    .bind(workflow_key)
    .fetch_all(&mut **tx)
    .await?;
    let hit = rows.iter().find(|r| in_effect(r.valid_from, r.valid_to, business_date)).ok_or_else(|| {
        err_d(
            404,
            "no_binding_for_business_date",
            json!({ "stationKey": station_key, "workflowKey": workflow_key, "businessDate": crate::common::day(business_date) }),
        )
    })?;
    published_definition(tx, access, hit.definition_id, None).await
}

/// 一份在該業務日期對這座案場生效的契約，連同這一組配對的參數。
#[derive(Debug, Clone)]
pub struct ContractCoverage {
    pub contract_id: i64,
    pub contract_no: String,
    pub coverage_id: i64,
    pub stage: String,
    pub params: Value,
}

/// 這座案場在這個業務日期受哪些契約約束（可能多份）。效期兩層：配對自己的優先，沒填沿用契約的。
pub async fn effective_contracts(
    tx: &mut Tx<'_>,
    access: &OrgAccess,
    station_id: i64,
    business_date: NaiveDate,
) -> ApiResult<Vec<ContractCoverage>> {
    let rows: Vec<(i64, i64, String, Value, Option<NaiveDate>, Option<NaiveDate>, NaiveDate, Option<NaiveDate>, String)> = sqlx::query_as(
        "SELECT cs.id, c.id, c.contract_no, cs.params, cs.valid_from, cs.valid_to, c.valid_from, c.valid_to, c.stage
           FROM mn_energy_contract_station cs
           JOIN tbl_energy_contract c ON c.id = cs.contract_id AND c.organization_id = $2 AND c.deleted_at IS NULL
          WHERE cs.station_id = $1 AND cs.deleted_at IS NULL
          ORDER BY c.id, cs.id",
    )
    .bind(station_id)
    .bind(access.org_id())
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|r| {
            let from = r.4.unwrap_or(r.6);
            let to = r.5.or(r.7);
            in_effect(from, to, business_date)
        })
        .map(|r| ContractCoverage { coverage_id: r.0, contract_id: r.1, contract_no: r.2, params: r.3, stage: r.8 })
        .collect())
}

/// 解出來的一條流程。
#[derive(Debug, Clone)]
pub struct ResolvedFlow {
    pub workflow_key: String,
    /// 命中的是哪一種單別的綁定：細分、粗分，或不分單別（None）。
    pub matched_doc_type: Option<String>,
    pub definition: DefinitionRow,
    pub binding: BindingRow,
    /// 命中的契約（None ＝ 案場層那一段）。
    pub contract: Option<ContractCoverage>,
}

/// 從一堆綁定列挑出「這種單要走的那幾條」（契約層與案場層共用同一套規則）：
/// 每個流程 key 一段、由細到粗（細分 → 粗分 → 不分單別）。
async fn pick_flows(
    tx: &mut Tx<'_>,
    access: &OrgAccess,
    rows: &[BindingRow],
    doc_type: EnergyDocType,
    business_date: NaiveDate,
    coverage: Option<&ContractCoverage>,
) -> ApiResult<Vec<ResolvedFlow>> {
    let mut chain: Vec<Option<String>> = doc_type_lookup_chain(doc_type).into_iter().map(|t| Some(t.as_str().to_string())).collect();
    chain.push(None);
    let mut picked: Vec<(String, &BindingRow, Option<String>)> = Vec::new();
    for candidate in &chain {
        for row in rows {
            if &row.doc_type != candidate || !in_effect(row.valid_from, row.valid_to, business_date) {
                continue;
            }
            if !picked.iter().any(|p| p.0 == row.workflow_key) {
                picked.push((row.workflow_key.clone(), row, candidate.clone()));
            }
        }
    }
    let mut out = Vec::new();
    for (key, row, matched) in picked {
        let definition = published_definition(tx, access, row.definition_id, Some(&key)).await?;
        out.push(ResolvedFlow {
            workflow_key: key,
            matched_doc_type: matched,
            definition,
            binding: row.clone(),
            contract: coverage.cloned(),
        });
    }
    Ok(out)
}

/// 開流程時把契約與這一組配對的參數整包快照進屬性（計算用的是這一份，不是「現在的參數」）。
pub fn contract_attributes(base: Value, flow: &ResolvedFlow) -> Value {
    let Some(c) = &flow.contract else { return base };
    let mut out = if base.is_object() { base } else { json!({}) };
    out["contract"] = json!({ "id": c.contract_id, "contractNo": c.contract_no, "stage": c.stage, "params": c.params });
    out
}

/// 「這張單要開哪些流程」：契約優先（多份取聯集）、案場保底。順序穩定。
pub async fn resolve_flow_rows(
    tx: &mut Tx<'_>,
    access: &OrgAccess,
    station_key: &str,
    station_id: Option<i64>,
    doc_type: EnergyDocType,
    business_date: NaiveDate,
) -> ApiResult<Vec<ResolvedFlow>> {
    let coverages = match station_id {
        Some(id) => effective_contracts(tx, access, id, business_date).await?,
        None => vec![],
    };
    if !coverages.is_empty() {
        let mut flows = Vec::new();
        for cv in &coverages {
            let rows: Vec<BindingRow> = sqlx::query_as(&format!(
                "SELECT {BINDING_COLS} FROM tbl_energy_workflow_binding
                  WHERE organization_id = $1 AND contract_id = $2 AND deleted_at IS NULL ORDER BY valid_from DESC, id DESC"
            ))
            .bind(access.org_id())
            .bind(cv.contract_id)
            .fetch_all(&mut **tx)
            .await?;
            flows.extend(pick_flows(tx, access, &rows, doc_type, business_date, Some(cv)).await?);
        }
        if !flows.is_empty() {
            flows.sort_by(|a, b| {
                let ka = format!("{}{}", a.contract.as_ref().map(|c| c.contract_id.to_string()).unwrap_or_default(), a.workflow_key);
                let kb = format!("{}{}", b.contract.as_ref().map(|c| c.contract_id.to_string()).unwrap_or_default(), b.workflow_key);
                ka.cmp(&kb)
            });
            return Ok(flows);
        }
    }
    let rows: Vec<BindingRow> = sqlx::query_as(&format!(
        "SELECT {BINDING_COLS} FROM tbl_energy_workflow_binding
          WHERE organization_id = $1 AND station_key = $2 AND deleted_at IS NULL ORDER BY valid_from DESC, id DESC"
    ))
    .bind(access.org_id())
    .bind(station_key)
    .fetch_all(&mut **tx)
    .await?;
    let mut flows = pick_flows(tx, access, &rows, doc_type, business_date, None).await?;
    if flows.is_empty() {
        return Err(err_d(
            404,
            "no_binding_for_business_date",
            json!({ "stationKey": station_key, "docType": doc_type.as_str(), "businessDate": crate::common::day(business_date) }),
        ));
    }
    flows.sort_by(|a, b| a.workflow_key.cmp(&b.workflow_key));
    Ok(flows)
}

fn kind_str(k: ActionKind) -> &'static str {
    match k {
        ActionKind::Advance => "advance",
        ActionKind::Revert => "revert",
        ActionKind::SetState => "setState",
        ActionKind::SideEffect => "sideEffect",
        ActionKind::Abort => "abort",
    }
}

fn node_type_str(t: NodeType) -> &'static str {
    match t {
        NodeType::Start => "start",
        NodeType::HumanTask => "humanTask",
        NodeType::Calculation => "calculation",
        NodeType::AbortApproval => "abortApproval",
        NodeType::End => "end",
    }
}

fn target_mode_str(t: TargetMode) -> &'static str {
    match t {
        TargetMode::Transition => "transition",
        TargetMode::PreviousNode => "previousNode",
        TargetMode::ExplicitNode => "explicitNode",
        TargetMode::CancelExecution => "cancelExecution",
    }
}

/// 寫一份定義（新版本，版號依 (org, key) 自動配），直接發佈。
pub async fn install_definition(
    tx: &mut Tx<'_>,
    access: &OrgAccess,
    def: &WorkflowDefinition,
    parent_version_id: Option<i64>,
    now: chrono::DateTime<chrono::Utc>,
) -> ApiResult<DefinitionRow> {
    let max: Option<i32> = sqlx::query_scalar(
        "SELECT MAX(version) FROM tbl_energy_workflow_definition WHERE organization_id = $1 AND key = $2 AND deleted_at IS NULL",
    )
    .bind(access.org_id())
    .bind(&def.key)
    .fetch_one(&mut **tx)
    .await?;
    let row: DefinitionRow = sqlx::query_as(&format!(
        "INSERT INTO tbl_energy_workflow_definition
           (organization_id, key, version, parent_version_id, name, description, status, published_at, published_by_user_id,
            subject_type, abort_enabled, abort_approval_rule)
         VALUES ($1, $2, $3, $4, $5, $6, 'published', $7, $8, $9, $10, $11) RETURNING {DEF_COLS}"
    ))
    .bind(access.org_id())
    .bind(&def.key)
    .bind(max.unwrap_or(0) + 1)
    .bind(parent_version_id)
    .bind(&def.name)
    .bind(def.description.as_deref())
    .bind(now)
    .bind(access.user_id())
    .bind(&def.subject_type)
    .bind(def.abort_enabled)
    .bind(Value::from(def.abort_approval_rule.clone()))
    .fetch_one(&mut **tx)
    .await?;
    for n in &def.nodes {
        let node_id: i64 = sqlx::query_scalar(
            "INSERT INTO tbl_energy_workflow_node
               (definition_id, node_key, name, node_type, assignment_rule, allow_self_approval, revertable, allows_subject_edit,
                allow_jump_target, parallel, join_policy, join_node_key, service_key, layout, requirements)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15) RETURNING id",
        )
        .bind(row.id)
        .bind(&n.node_key)
        .bind(&n.name)
        .bind(node_type_str(n.node_type))
        .bind(Value::from(n.assignment_rule.clone()))
        .bind(n.allow_self_approval)
        .bind(n.revertable)
        .bind(n.allows_subject_edit)
        .bind(n.allow_jump_target)
        .bind(n.parallel)
        .bind(n.join_policy.and_then(|p| serde_json::to_value(p).ok().and_then(|v| v.as_str().map(String::from))))
        .bind(n.join_node_key.as_deref())
        .bind(n.service_key.as_deref())
        .bind(&n.layout)
        .bind(serde_json::to_value(&n.requirements).unwrap_or_else(|_| json!({})))
        .fetch_one(&mut **tx)
        .await?;
        for a in &n.actions {
            // revert／abort 的理由強制必填 —— 在安裝這一層擋，比執行期才發現好。
            let forced = matches!(a.kind, ActionKind::Revert | ActionKind::Abort);
            sqlx::query(
                "INSERT INTO tbl_energy_workflow_action
                   (definition_id, node_id, action_key, label_key, kind, target_mode, target_node_key, who, requires_reason,
                    locks_revert, side_effect_key, state_key, sort_order)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
            )
            .bind(row.id)
            .bind(node_id)
            .bind(&a.action_key)
            .bind(&a.label_key)
            .bind(kind_str(a.kind))
            .bind(a.target_mode.map(target_mode_str))
            .bind(a.target_node_key.as_deref())
            .bind(Value::from(a.who.clone()))
            .bind(forced || a.requires_reason)
            .bind(a.locks_revert)
            .bind(a.side_effect_key.as_deref())
            .bind(a.state_key.as_deref())
            .bind(a.sort_order as i32)
            .execute(&mut **tx)
            .await?;
        }
    }
    for t in &def.transitions {
        sqlx::query(
            "INSERT INTO tbl_energy_workflow_transition
               (definition_id, from_node_key, to_node_key, kind, label, condition, requires_reason, sort_order)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(row.id)
        .bind(&t.from_node_key)
        .bind(&t.to_node_key)
        .bind(match t.kind {
            TransitionKind::Forward => "forward",
            TransitionKind::SendBack => "sendBack",
        })
        .bind(t.label.as_deref())
        .bind(Value::from(t.condition.clone()))
        .bind(t.requires_reason)
        .bind(t.sort_order as i32)
        .execute(&mut **tx)
        .await?;
    }
    Ok(row)
}

/// 案場層的一段綁定（取版只比電號；station_id 是補充欄）。
pub async fn insert_station_binding(
    tx: &mut Tx<'_>,
    access: &OrgAccess,
    station_key: &str,
    station_id: Option<i64>,
    doc_type: Option<&str>,
    def: &DefinitionRow,
    valid_from: NaiveDate,
    valid_to: Option<NaiveDate>,
) -> ApiResult<BindingRow> {
    Ok(sqlx::query_as(&format!(
        "INSERT INTO tbl_energy_workflow_binding
           (organization_id, station_key, station_id, doc_type, workflow_key, definition_id, valid_from, valid_to)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING {BINDING_COLS}"
    ))
    .bind(access.org_id())
    .bind(station_key)
    .bind(station_id)
    .bind(doc_type)
    .bind(&def.key)
    .bind(def.id)
    .bind(valid_from)
    .bind(valid_to)
    .fetch_one(&mut **tx)
    .await?)
}

/// 內建流程的部門（定義的指派規則用這幾個 key）。
const BUILTIN_DEPARTMENTS: [(&str, &str); 5] = [
    (eo_domain::workflow_defs::DEPT_SALES, "業務部"),
    (eo_domain::workflow_defs::DEPT_ACCOUNTING, "會計部門"),
    (eo_domain::workflow_defs::DEPT_FINANCE, "財務部"),
    (eo_domain::workflow_defs::DEPT_BILL_REVIEW, "電費單平台資料審核人員"),
    (eo_domain::workflow_defs::DEPT_RENT_REVIEW, "租金平台資料審核人員"),
];

/// 安裝結果：這次真的新裝了哪些（已存在的略過）。
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReport {
    pub definitions: Vec<Value>,
    pub strategies: Vec<Value>,
    pub departments: Vec<Value>,
}

/// 冪等地安裝內建流程定義（現行四份：Q-04 平台資料審核、保證金請款、用電電費單、開立發票）、
/// 預設計算策略與定義引用到的部門。同 key 已有任何版本就不再裝（新版本要走換版，不是重裝）。
pub async fn install_builtin_definitions(tx: &mut Tx<'_>, ctx: &Ctx) -> ApiResult<InstallReport> {
    let mut report = InstallReport::default();
    let defs = [
        eo_domain::workflow_defs::platform_review_bill(),
        eo_domain::workflow_defs::deposit_request(),
        eo_domain::workflow_defs::utility_bill(),
        eo_domain::workflow_defs::invoice_issue(),
    ];
    for d in &defs {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM tbl_energy_workflow_definition WHERE organization_id = $1 AND key = $2 AND deleted_at IS NULL)",
        )
        .bind(ctx.org_id())
        .bind(&d.key)
        .fetch_one(&mut **tx)
        .await?;
        if exists {
            continue;
        }
        let row = install_definition(tx, &ctx.access, d, None, ctx.now()).await?;
        report.definitions.push(json!({ "id": row.id, "key": row.key, "version": row.version, "name": row.name }));
    }
    if let Some(s) = crate::rating::install_default_strategy(tx, ctx).await? {
        report.strategies.push(s);
    }
    for (key, name) in BUILTIN_DEPARTMENTS {
        if crate::org::departments::by_key(tx, &ctx.access, key).await?.is_some() {
            continue;
        }
        let input = eo_domain::schemas::CreateDepartmentInput { key: key.into(), name: name.into(), sort_order: None };
        let v = crate::org::departments::create_department(tx, ctx, &input).await?;
        report.departments.push(json!({ "id": v.id, "key": v.key, "name": v.name }));
    }
    Ok(report)
}
