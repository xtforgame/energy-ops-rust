//! 血緣：來歷紀錄，不是控制通道 —— 寫進去不觸發任何行為，撤回不傳播。
//! 防環的理由是追溯查詢會無限迴圈（不是怕連動遞迴：沒有連動）。

use super::super::model;
use crate::audit::{self, AuditInput};
use crate::common::{err_d, Ctx, Tx};
use eo_platform::error::ApiResult;
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// 這張單怎麼來的。
    Up,
    /// 這張單衍生了什麼。
    Down,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineageHop {
    pub execution_id: i64,
    pub relation_kind: String,
    pub depth: u32,
}

/// 追溯（純查詢，限本組織）。
pub async fn trace_lineage(tx: &mut Tx<'_>, org_id: i64, execution_id: i64, direction: Direction, max_depth: u32) -> ApiResult<Vec<LineageHop>> {
    let mut out = Vec::new();
    let mut seen = vec![execution_id];
    let mut frontier = vec![execution_id];
    let mut depth = 1;
    while depth <= max_depth && !frontier.is_empty() {
        let sql = match direction {
            Direction::Up => {
                "SELECT upstream_execution_id, relation_kind FROM tbl_energy_execution_lineage
                  WHERE downstream_execution_id = ANY($1) AND organization_id = $2 AND deleted_at IS NULL ORDER BY id"
            }
            Direction::Down => {
                "SELECT downstream_execution_id, relation_kind FROM tbl_energy_execution_lineage
                  WHERE upstream_execution_id = ANY($1) AND organization_id = $2 AND deleted_at IS NULL ORDER BY id"
            }
        };
        let rows: Vec<(i64, String)> = sqlx::query_as(sql).bind(&frontier).bind(org_id).fetch_all(&mut **tx).await?;
        let mut next = Vec::new();
        for (other, kind) in rows {
            if seen.contains(&other) {
                continue;
            }
            seen.push(other);
            out.push(LineageHop { execution_id: other, relation_kind: kind, depth });
            next.push(other);
        }
        frontier = next;
        depth += 1;
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct NewLineage {
    pub upstream_execution_id: i64,
    pub downstream_execution_id: i64,
    pub relation_kind: Option<String>,
    pub note: Option<String>,
}

/// 連起兩筆 execution（兩端都必須屬於這個組織）。回傳新列 id。
pub async fn link_lineage(tx: &mut Tx<'_>, ctx: &Ctx, input: NewLineage) -> ApiResult<i64> {
    model::execution_in_org(tx, ctx.org_id(), input.upstream_execution_id).await?;
    model::execution_in_org(tx, ctx.org_id(), input.downstream_execution_id).await?;
    if input.upstream_execution_id == input.downstream_execution_id {
        return Err(err_d(400, "lineage_cycle", json!({ "reason": "self_link" })));
    }
    // downstream 已經是 upstream 的祖先 → 會成環。
    let ancestors = trace_lineage(tx, ctx.org_id(), input.upstream_execution_id, Direction::Up, 10).await?;
    if ancestors.iter().any(|a| a.execution_id == input.downstream_execution_id) {
        return Err(err_d(
            400,
            "lineage_cycle",
            json!({ "upstreamExecutionId": input.upstream_execution_id, "downstreamExecutionId": input.downstream_execution_id }),
        ));
    }
    let kind = input.relation_kind.clone().unwrap_or_else(|| "derived".into());
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO tbl_energy_execution_lineage
           (organization_id, upstream_execution_id, downstream_execution_id, relation_kind, note, created_by_user_id, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(ctx.org_id())
    .bind(input.upstream_execution_id)
    .bind(input.downstream_execution_id)
    .bind(&kind)
    .bind(input.note.as_deref())
    .bind(ctx.user_id())
    .bind(ctx.now())
    .fetch_one(&mut **tx)
    .await?;
    audit::record(
        tx,
        ctx,
        AuditInput::new(
            "lineage.linked",
            id,
            format!("建立血緣：#{} → #{}", input.upstream_execution_id, input.downstream_execution_id),
        )
        .payload(json!({
            "upstreamExecutionId": input.upstream_execution_id, "downstreamExecutionId": input.downstream_execution_id,
            "relationKind": kind, "note": input.note,
        })),
    )
    .await?;
    Ok(id)
}
