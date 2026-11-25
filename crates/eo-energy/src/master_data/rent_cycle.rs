//! Explicit landlord-cycle drafts. Does not advance electricity sales or post payables.
use super::contracts::{contract_in_org, ContractRow};
use crate::audit::{self, AuditInput};
use crate::common::{err, err_d, Ctx, RequestMeta, Tx};
use crate::gates;
use actix_web::{web, HttpResponse};
use chrono::NaiveDate;
use eo_domain::rent_cycle::{self, Plan, Receipt};
use eo_platform::{auth::CurrentUser, core::CoreRef, error::ApiResult};
use serde::Deserialize;
use serde_json::{json, Value};
type Core=web::Data<CoreRef>;

fn invalid(message:String)->eo_platform::error::ApiError {err_d(422,"rent_cycle_invalid",json!({"message":message}))}

async fn check_station(tx:&mut Tx<'_>,org:i64,id:i64,station:i64)->ApiResult<()> {
    let found:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mn_energy_contract_station cs JOIN tbl_energy_station s ON s.id=cs.station_id WHERE cs.contract_id=$1 AND cs.station_id=$2 AND cs.deleted_at IS NULL AND s.deleted_at IS NULL AND s.organization_id=$3)")
        .bind(id).bind(station).bind(org).fetch_one(&mut **tx).await?;
    if !found {return Err(err(422,"rent_station_not_in_contract"));} Ok(())
}
async fn saved_plan(tx:&mut Tx<'_>,org:i64,id:i64)->ApiResult<Plan> {
    contract_in_org(tx,org,id).await?;
    let data:Value=sqlx::query_scalar("SELECT data FROM tbl_energy_contract WHERE id=$1").bind(id).fetch_one(&mut **tx).await?;
    let p:Plan=serde_json::from_value(data["rentCycleDraft"].clone()).map_err(|_|err(422,"rent_plan_required"))?;
    check_station(tx,org,id,p.station_id).await?;
    Ok(p)
}
fn check_dates(c:&ContractRow,p:&Plan,start:&str)->ApiResult<(NaiveDate,NaiveDate)> {
    let (a,b)=rent_cycle::bounds(p,start).map_err(invalid)?;
    if a<c.valid_from || c.valid_to.is_some_and(|d|b>d) {return Err(err(422,"rent_cycle_outside_contract"));}
    Ok((a,b))
}
pub async fn get(core:Core,user:CurrentUser,path:web::Path<(i64,i64)>)->ApiResult<HttpResponse> {
    let (org,id)=path.into_inner();core.rbac.authorize_org(&user,org,gates::FLOW_VIEW).await?;
    let mut tx=core.pool.begin().await?;contract_in_org(&mut tx,org,id).await?;
    let data:Value=sqlx::query_scalar("SELECT data FROM tbl_energy_contract WHERE id=$1").bind(id).fetch_one(&mut *tx).await?;
    Ok(HttpResponse::Ok().json(json!({"plan":data["rentCycleDraft"],"state":"draft","paymentAllowed":false})))
}
pub async fn save(core:Core,user:CurrentUser,meta:RequestMeta,path:web::Path<(i64,i64)>,body:web::Json<Plan>)->ApiResult<HttpResponse> {
    let (org,id)=path.into_inner();let access=core.rbac.authorize_org(&user,org,gates::MASTER_MANAGE).await?;
    let ctx=Ctx::new(&core,access,&meta);let mut tx=core.pool.begin().await?;
    let c=contract_in_org(&mut tx,org,id).await?;
    rent_cycle::validate(&body).map_err(invalid)?;
    // 合約租金算法代號對租金試算的 kind：R-A 只准 fixedArea、R-B 只准 revenueShare，R-0/R-C/R-D 一律不合（還沒有對應算法），代號空的放行。
    let kind_ok=match c.rent_calc_code.as_deref() {
        None=>true,
        Some("R-A")=>body.kind=="fixedArea",
        Some("R-B")=>body.kind=="revenueShare",
        Some(_)=>false,
    };
    if !kind_ok {
        return Err(err_d(422,"rent_kind_mismatch",json!({"rentCalcCode":c.rent_calc_code,"kind":body.kind})));
    }
    check_station(&mut tx,org,id,body.station_id).await?;
    check_dates(&c,&body,&body.anchor)?;
    let before:Value=sqlx::query_scalar("SELECT data FROM tbl_energy_contract WHERE id=$1 FOR UPDATE").bind(id).fetch_one(&mut *tx).await?;
    if before["rentCycleDraft"]["stationId"].as_i64().is_some_and(|s|s!=body.station_id) {
        return Err(err(422,"rent_multisite_contract_requires_confirmation"));
    }
    let plan=serde_json::to_value(body.into_inner()).map_err(|_|err(400,"invalid_rent_plan"))?;
    sqlx::query("UPDATE tbl_energy_contract SET data=jsonb_set(data,'{rentCycleDraft}',$1,true),updated_at=clock_timestamp() WHERE id=$2 AND organization_id=$3")
        .bind(&plan).bind(id).bind(org).execute(&mut *tx).await?;
    audit::record(&mut tx,&ctx,AuditInput::new("contract.updated",id,"更新地主租金週期試算設定")
        .label(c.contract_no.clone()).payload(json!({"contractNo":c.contract_no,"landownerId":c.landowner_id,
            "changes":{"rentCycleDraft":{"from":before["rentCycleDraft"],"to":plan}}}))).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({"plan":plan,"state":"draft","paymentAllowed":false})))
}
#[derive(Deserialize)]
pub struct Query {start:String}
pub async fn bills(core:Core,user:CurrentUser,path:web::Path<(i64,i64)>,query:web::Query<Query>)->ApiResult<HttpResponse> {
    let (org,id)=path.into_inner();core.rbac.authorize_org(&user,org,gates::FLOW_VIEW).await?;
    let mut tx=core.pool.begin().await?;let c=contract_in_org(&mut tx,org,id).await?;
    let p=saved_plan(&mut tx,org,id).await?;let (a,b)=check_dates(&c,&p,&query.start)?;
    // Include undated bills for visibility; never silently assume their period or source.
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('billId',id,'documentNo',document_no,'docType',doc_type,'start',billing_period_start,'end',billing_period_end,'details',detail_fields,'attachmentId',source_attachment_id) FROM tbl_energy_bill WHERE organization_id=$1 AND station_id=$2 AND deleted_at IS NULL AND (billing_period_start IS NULL OR billing_period_end IS NULL OR (billing_period_start<=$4 AND billing_period_end>=$3)) ORDER BY billing_period_start NULLS FIRST,id LIMIT 501")
        .bind(org).bind(p.station_id).bind(a).bind(b).fetch_all(&mut *tx).await?;
    if rows.len()>500 {return Err(err(422,"rent_too_many_bills"));}
    Ok(HttpResponse::Ok().json(json!({"items":rows,"periodStart":a,"periodEnd":b})))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Input {
    bill_id:i64,source:String,untaxed:Option<String>,gross:Option<String>,kwh:Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preview {start:String,receipts:Vec<Input>}
pub async fn preview(core:Core,user:CurrentUser,path:web::Path<(i64,i64)>,body:web::Json<Preview>)->ApiResult<HttpResponse> {
    let (org,id)=path.into_inner();core.rbac.authorize_org(&user,org,gates::FLOW_VIEW).await?;
    let mut tx=core.pool.begin().await?;
    let c=contract_in_org(&mut tx,org,id).await?;let p=saved_plan(&mut tx,org,id).await?;
    check_dates(&c,&p,&body.start)?;
    if body.receipts.len()>500 {return Err(err(422,"rent_too_many_bills"));}
    let ids:Vec<_>=body.receipts.iter().map(|r|r.bill_id).collect();
    let bills:Vec<(i64,Option<NaiveDate>,Option<NaiveDate>,Option<i64>)>=sqlx::query_as("SELECT id,billing_period_start,billing_period_end,source_attachment_id FROM tbl_energy_bill WHERE organization_id=$1 AND station_id=$2 AND id=ANY($3) AND deleted_at IS NULL")
        .bind(org).bind(p.station_id).bind(&ids).fetch_all(&mut *tx).await?;
    let mut rows=Vec::new();let mut attachments=Vec::new();
    for r in &body.receipts {
        let b=bills.iter().find(|b|b.0==r.bill_id).ok_or_else(||err(422,"rent_bill_not_in_station"))?;
        rows.push(Receipt{bill_id:r.bill_id,source:r.source.clone(),start:b.1.map(|d|d.to_string()).unwrap_or_default(),
            end:b.2.map(|d|d.to_string()).unwrap_or_default(),untaxed:r.untaxed.clone(),gross:r.gross.clone(),kwh:r.kwh.clone()});
        attachments.push(json!({"billId":r.bill_id,"attachmentId":b.3,"downloadPath":b.3.map(|a|format!("/api/v1/orgs/{org}/energy/attachments/{a}/download"))}));
    }
    let mut result=rent_cycle::preview(&p,&body.start,&rows).map_err(invalid)?;
    result["contractId"]=json!(id);result["contractNo"]=json!(c.contract_no);
    result["attachments"]=json!(attachments);
    result["inputProvenance"]=json!("userConfirmedDraftInputs; bill dates and station verified against database; amounts do not overwrite OCR");
    Ok(HttpResponse::Ok().json(result))
}
