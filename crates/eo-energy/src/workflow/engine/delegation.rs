//! 代理：`X 可以代 Y 行事` ⟺ 有一列 delegator=Y、delegate=X、未收回、validFrom ≤ 今天 ≤ validTo（或沒有迄日）。
//!
//! 這裡用「今天」不是業務日期 —— 代理是關於誰現在在辦公室，與取版刻意不同。
//! 代理的 validTo 含當天（與綁定的半開區間不同）：「代理到 6/30」在使用者心中 6/30 當天還能代。

use crate::audit::{self, AuditInput};
use crate::common::{err, Ctx, Tx};
use chrono::{DateTime, NaiveDate, Utc};
use eo_platform::error::ApiResult;
use serde::Serialize;
use serde_json::json;
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationRow {
    pub id: i64,
    pub delegator_user_id: i64,
    pub delegate_user_id: i64,
    pub valid_from: NaiveDate,
    pub valid_to: Option<NaiveDate>,
    pub reason: Option<String>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_by_user_id: Option<i64>,
    pub created_at: DateTime<Utc>,
}

const COLS: &str = "id, delegator_user_id, delegate_user_id, valid_from, valid_to, reason, revoked_at, created_by_user_id, created_at";

pub fn is_effective(d: &DelegationRow, today: NaiveDate) -> bool {
    d.revoked_at.is_none() && d.valid_from <= today && d.valid_to.is_none_or(|t| today <= t)
}

/// 今天我可以代誰行事。
pub async fn delegators_for(tx: &mut Tx<'_>, org_id: i64, delegate: i64, today: NaiveDate) -> ApiResult<Vec<i64>> {
    let rows: Vec<DelegationRow> = sqlx::query_as(&format!(
        "SELECT {COLS} FROM tbl_energy_workflow_delegation
          WHERE organization_id = $1 AND delegate_user_id = $2 AND revoked_at IS NULL AND deleted_at IS NULL ORDER BY id"
    ))
    .bind(org_id)
    .bind(delegate)
    .fetch_all(&mut **tx)
    .await?;
    Ok(crate::common::uniq(rows.iter().filter(|r| is_effective(r, today)).map(|r| r.delegator_user_id)))
}

#[derive(Debug, Clone)]
pub struct NewDelegation {
    pub delegator_user_id: i64,
    pub delegate_user_id: i64,
    pub valid_from: NaiveDate,
    pub valid_to: Option<NaiveDate>,
    pub reason: Option<String>,
}

pub async fn create_delegation(tx: &mut Tx<'_>, ctx: &Ctx, input: NewDelegation) -> ApiResult<DelegationRow> {
    if input.delegator_user_id == input.delegate_user_id {
        return Err(err(400, "cannot_delegate_to_self"));
    }
    if input.valid_to.is_some_and(|t| t < input.valid_from) {
        return Err(err(400, "invalid_date_range"));
    }
    for u in [input.delegator_user_id, input.delegate_user_id] {
        if crate::common::user_name(tx, u).await?.is_none() {
            return Err(err(404, "user_not_found"));
        }
    }
    let row: DelegationRow = sqlx::query_as(&format!(
        "INSERT INTO tbl_energy_workflow_delegation
           (organization_id, delegator_user_id, delegate_user_id, valid_from, valid_to, reason, created_by_user_id, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING {COLS}"
    ))
    .bind(ctx.org_id())
    .bind(input.delegator_user_id)
    .bind(input.delegate_user_id)
    .bind(input.valid_from)
    .bind(input.valid_to)
    .bind(input.reason.as_deref())
    .bind(ctx.user_id())
    .bind(ctx.now())
    .fetch_one(&mut **tx)
    .await?;
    audit::record(
        tx,
        ctx,
        AuditInput::new(
            "delegation.created",
            row.id,
            format!("建立代理：#{} → #{}", input.delegator_user_id, input.delegate_user_id),
        )
        .payload(json!({
            "delegatorUserId": input.delegator_user_id, "delegateUserId": input.delegate_user_id,
            "validFrom": crate::common::day(input.valid_from), "validTo": crate::common::day_opt(input.valid_to),
            "reason": input.reason,
        })),
    )
    .await?;
    Ok(row)
}

/// 本組織的一筆代理（別的組織的當作不存在）。
pub async fn delegation_in_org(tx: &mut Tx<'_>, org_id: i64, id: i64) -> ApiResult<DelegationRow> {
    let row: Option<DelegationRow> = sqlx::query_as(&format!(
        "SELECT {COLS} FROM tbl_energy_workflow_delegation WHERE id = $1 AND organization_id = $2 AND deleted_at IS NULL"
    ))
    .bind(id)
    .bind(org_id)
    .fetch_optional(&mut **tx)
    .await?;
    row.ok_or_else(|| err(404, "delegation_not_found"))
}

/// 提前收回。不刪列（append-only），revokedAt 有值即代表已收回。
pub async fn revoke_delegation(tx: &mut Tx<'_>, ctx: &Ctx, id: i64) -> ApiResult<()> {
    let row = delegation_in_org(tx, ctx.org_id(), id).await?;
    if row.revoked_at.is_some() {
        return Err(err(409, "delegation_already_revoked"));
    }
    sqlx::query("UPDATE tbl_energy_workflow_delegation SET revoked_at = $2 WHERE id = $1")
        .bind(id)
        .bind(ctx.now())
        .execute(&mut **tx)
        .await?;
    audit::record(
        tx,
        ctx,
        AuditInput::new("delegation.revoked", id, format!("收回代理 #{id}"))
            .payload(json!({ "delegatorUserId": row.delegator_user_id, "delegateUserId": row.delegate_user_id })),
    )
    .await?;
    Ok(())
}

/// 我設的與替我設的（`all` 時列全組織，給有代理管理權的人）。新到舊。
pub async fn list_delegations(tx: &mut Tx<'_>, org_id: i64, user_id: Option<i64>, all: bool) -> ApiResult<Vec<DelegationRow>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {COLS} FROM tbl_energy_workflow_delegation
          WHERE organization_id = $1 AND deleted_at IS NULL AND ($3 OR delegator_user_id = $2 OR delegate_user_id = $2)
          ORDER BY id DESC"
    ))
    .bind(org_id)
    .bind(user_id)
    .bind(all)
    .fetch_all(&mut **tx)
    .await?)
}
