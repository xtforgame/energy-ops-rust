//! Explicit landlord-cycle drafts. Does not advance electricity sales or post payables.
use super::contracts::{contract_in_org, ContractRow, CONTRACT_COLS};
use crate::audit::{self, AuditInput};
use crate::common::{err, err_d, Ctx, RequestMeta, Tx};
use crate::gates;
use actix_web::{web, HttpResponse};
use chrono::NaiveDate;
use eo_domain::rent_cycle::{self, ApplicationEntryInput, Plan, Receipt};
use eo_platform::{auth::CurrentUser, core::CoreRef, error::ApiResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
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
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub struct SaveQuery {confirm_station_change:Option<bool>}
pub async fn save(core:Core,user:CurrentUser,meta:RequestMeta,path:web::Path<(i64,i64)>,q:web::Query<SaveQuery>,body:web::Json<Plan>)->ApiResult<HttpResponse> {
    let (org,id)=path.into_inner();let access=core.rbac.authorize_org(&user,org,gates::MASTER_MANAGE).await?;
    let confirm=q.confirm_station_change.unwrap_or(false);
    let ctx=Ctx::new(&core,access,&meta);let mut tx=core.pool.begin().await?;
    let c=contract_in_org(&mut tx,org,id).await?;
    rent_cycle::validate(&body).map_err(invalid)?;
    // 合約租金算法代號對租金試算的 kind：R-A 准 fixedArea（面積×單價）或 fixedAnnual（整組年額）、
    // R-B 只准 revenueShare、R-C 准 annualTiered（年度基準＋級距調整）、R-D 准 kwhRate（度數×費率×比例），
    // R-0 一律不合（還沒有對應算法），代號空的放行。
    let kind_ok=match c.rent_calc_code.as_deref() {
        None=>true,
        Some("R-A")=>body.kind=="fixedArea" || body.kind=="fixedAnnual",
        Some("R-B")=>body.kind=="revenueShare",
        Some("R-C")=>body.kind=="annualTiered",
        Some("R-D")=>body.kind=="kwhRate",
        Some(_)=>false,
    };
    if !kind_ok {
        return Err(err_d(422,"rent_kind_mismatch",json!({"rentCalcCode":c.rent_calc_code,"kind":body.kind})));
    }
    check_station(&mut tx,org,id,body.station_id).await?;
    check_dates(&c,&body,&body.anchor)?;
    let before:Value=sqlx::query_scalar("SELECT data FROM tbl_energy_contract WHERE id=$1 FOR UPDATE").bind(id).fetch_one(&mut *tx).await?;
    if let Some(from_station_id)=before["rentCycleDraft"]["stationId"].as_i64() {
        if from_station_id!=body.station_id && !confirm {
            return Err(err_d(422,"rent_multisite_contract_requires_confirmation",
                json!({"fromStationId":from_station_id,"toStationId":body.station_id})));
        }
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
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Input {
    bill_id:i64,source:String,untaxed:Option<String>,gross:Option<String>,kwh:Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preview {start:String,receipts:Vec<Input>}

/// `preview` 與 `payment_draft` 共用的計算核心：讀已存的試算設定、核對單據真的屬於這份合約的
/// 案場，交給 `eo_domain::rent_cycle::preview` 算。回傳 (這份計畫, 試算結果 JSON)。
async fn compute(tx:&mut Tx<'_>,org:i64,id:i64,c:&ContractRow,start:&str,receipts:&[Input])->ApiResult<(Plan,Value)> {
    let p=saved_plan(tx,org,id).await?;
    check_dates(c,&p,start)?;
    if receipts.len()>500 {return Err(err(422,"rent_too_many_bills"));}
    let ids:Vec<_>=receipts.iter().map(|r|r.bill_id).collect();
    let bills:Vec<(i64,Option<NaiveDate>,Option<NaiveDate>,Option<i64>)>=sqlx::query_as("SELECT id,billing_period_start,billing_period_end,source_attachment_id FROM tbl_energy_bill WHERE organization_id=$1 AND station_id=$2 AND id=ANY($3) AND deleted_at IS NULL")
        .bind(org).bind(p.station_id).bind(&ids).fetch_all(&mut **tx).await?;
    let mut rows=Vec::new();
    for r in receipts {
        let b=bills.iter().find(|b|b.0==r.bill_id).ok_or_else(||err(422,"rent_bill_not_in_station"))?;
        rows.push(Receipt{bill_id:r.bill_id,source:r.source.clone(),start:b.1.map(|d|d.to_string()).unwrap_or_default(),
            end:b.2.map(|d|d.to_string()).unwrap_or_default(),untaxed:r.untaxed.clone(),gross:r.gross.clone(),kwh:r.kwh.clone()});
    }
    let result=rent_cycle::preview(&p,start,&rows).map_err(invalid)?;
    Ok((p,result))
}
pub async fn preview(core:Core,user:CurrentUser,path:web::Path<(i64,i64)>,body:web::Json<Preview>)->ApiResult<HttpResponse> {
    let (org,id)=path.into_inner();core.rbac.authorize_org(&user,org,gates::FLOW_VIEW).await?;
    let mut tx=core.pool.begin().await?;
    let c=contract_in_org(&mut tx,org,id).await?;
    let (_,mut result)=compute(&mut tx,org,id,&c,&body.start,&body.receipts).await?;
    let ids:Vec<_>=body.receipts.iter().map(|r|r.bill_id).collect();
    let atts:Vec<(i64,Option<i64>)>=sqlx::query_as("SELECT id,source_attachment_id FROM tbl_energy_bill WHERE organization_id=$1 AND id=ANY($2) AND deleted_at IS NULL")
        .bind(org).bind(&ids).fetch_all(&mut *tx).await?;
    let attachments:Vec<Value>=body.receipts.iter().map(|r|{
        let a=atts.iter().find(|a|a.0==r.bill_id).and_then(|a|a.1);
        json!({"billId":r.bill_id,"attachmentId":a,"downloadPath":a.map(|a|format!("/api/v1/orgs/{org}/energy/attachments/{a}/download"))})
    }).collect();
    result["contractId"]=json!(id);result["contractNo"]=json!(c.contract_no);
    result["attachments"]=json!(attachments);
    result["inputProvenance"]=json!("userConfirmedDraftInputs; bill dates and station verified against database; amounts do not overwrite OCR");
    Ok(HttpResponse::Ok().json(result))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct PaymentDraftBody {start:String,receipts:Vec<Input>,invoice_timing:Option<String>}

/// S2：試算結果沒有 issue、算出 baseRent 才能存；R-A 零單據（`receipts` 空陣列）也走這條，
/// 不經舊路 `POST /bills/{id}/rent-payment`（決定 1）。鍵用 periodStart，同一期再存會蓋掉自己那一筆。
pub async fn payment_draft(core:Core,user:CurrentUser,meta:RequestMeta,path:web::Path<(i64,i64)>,body:web::Json<PaymentDraftBody>)->ApiResult<HttpResponse> {
    let (org,id)=path.into_inner();
    let access=core.rbac.authorize_org(&user,org,gates::MASTER_MANAGE).await?;
    let ctx=Ctx::new(&core,access,&meta);
    let mut tx=core.pool.begin().await?;
    let c=contract_in_org(&mut tx,org,id).await?;
    let (p,result)=compute(&mut tx,org,id,&c,&body.start,&body.receipts).await?;
    let issues_empty=result["issues"].as_array().map(|a|a.is_empty()).unwrap_or(false);
    let base_rent=result["baseRent"].as_str().map(str::to_string);
    if !issues_empty || base_rent.is_none() {
        return Err(err(422,"rent_payment_draft_incomplete"));
    }
    let period_start=result["periodStart"].as_str().unwrap_or_default().to_string();
    let period_end=result["periodEnd"].as_str().unwrap_or_default().to_string();
    let invoice_timing=body.invoice_timing.clone().unwrap_or_else(||"unknown".into());
    let draft=json!({"periodStart":period_start,"periodEnd":period_end,"kind":p.kind,"baseRent":base_rent,
        "receipts":body.receipts,"invoiceTiming":invoice_timing,"savedAt":ctx.now()});
    let before:Value=sqlx::query_scalar("SELECT data FROM tbl_energy_contract WHERE id=$1 FOR UPDATE").bind(id).fetch_one(&mut *tx).await?;
    let mut drafts=before["rentPaymentDrafts"].as_object().cloned().unwrap_or_default();
    drafts.insert(period_start.clone(),draft);
    let drafts=Value::Object(drafts);
    sqlx::query("UPDATE tbl_energy_contract SET data=jsonb_set(data,'{rentPaymentDrafts}',$1,true),updated_at=clock_timestamp() WHERE id=$2 AND organization_id=$3")
        .bind(&drafts).bind(id).bind(org).execute(&mut *tx).await?;
    audit::record(&mut tx,&ctx,AuditInput::new("contract.rent_payment_draft_saved",id,"存成租金付款草稿")
        .label(c.contract_no.clone()).payload(json!({"contractNo":c.contract_no,"periodStart":period_start,"periodEnd":period_end,"baseRent":base_rent}))).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({"periodStart":period_start,"periodEnd":period_end,"baseRent":base_rent,"state":"draft","paymentAllowed":false})))
}

/// S1：這個案場掛的所有合約，依已存的草稿重算（不存快照），依 (periodStart,periodEnd) 分組成
/// 一期一張申請，R-0 合約（決定 4）不重算、直接進 `notPayable`，不計入任何一期。
pub async fn station_application(core:Core,user:CurrentUser,path:web::Path<(i64,i64)>)->ApiResult<HttpResponse> {
    let (org,station_id)=path.into_inner();
    core.rbac.authorize_org(&user,org,gates::FLOW_VIEW).await?;
    let mut tx=core.pool.begin().await?;
    let station:Option<(String,Value)>=sqlx::query_as("SELECT name,data FROM tbl_energy_station WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL")
        .bind(station_id).bind(org).fetch_optional(&mut *tx).await?;
    let (station_name,station_data)=station.ok_or_else(||err(404,"station_not_found"))?;
    let defaults=&station_data["easyflowDefaults"];
    let company_code=defaults.get("companyCode").and_then(Value::as_str).filter(|s|!s.is_empty()).unwrap_or("待補").to_string();
    let company_name=defaults.get("companyName").and_then(Value::as_str).filter(|s|!s.is_empty()).unwrap_or("待補").to_string();

    let contracts:Vec<ContractRow>=sqlx::query_as(&format!(
        "SELECT {CONTRACT_COLS} FROM tbl_energy_contract c WHERE c.organization_id=$1 AND c.deleted_at IS NULL
           AND c.id IN (SELECT contract_id FROM mn_energy_contract_station WHERE station_id=$2 AND deleted_at IS NULL) ORDER BY c.id"))
        .bind(org).bind(station_id).fetch_all(&mut *tx).await?;

    let mut not_payable=Vec::new();
    let mut periods:BTreeMap<(String,String),Vec<ApplicationEntryInput>>=BTreeMap::new();
    for c in &contracts {
        if c.rent_calc_code.as_deref()==Some("R-0") {
            not_payable.push(json!({"contractId":c.id,"contractNo":c.contract_no}));
            continue;
        }
        let landowners:Vec<(String,Option<String>)>=sqlx::query_as(
            "SELECT lo.name,lo.tax_id FROM mn_energy_contract_landowner cl JOIN tbl_energy_landowner lo ON lo.id=cl.landowner_id
               WHERE cl.contract_id=$1 AND cl.deleted_at IS NULL ORDER BY cl.id")
            .bind(c.id).fetch_all(&mut *tx).await?;
        if landowners.is_empty() {continue;}
        let data:Value=sqlx::query_scalar("SELECT data FROM tbl_energy_contract WHERE id=$1").bind(c.id).fetch_one(&mut *tx).await?;
        let drafts=data["rentPaymentDrafts"].as_object().cloned().unwrap_or_default();
        for (period_start,draft) in drafts {
            let receipts:Vec<Input>=serde_json::from_value(draft["receipts"].clone()).unwrap_or_default();
            let (p,result)=match compute(&mut tx,org,c.id,c,&period_start,&receipts).await {
                Ok(v)=>v,
                // 草稿存檔後合約效期或單據被改掉，重算失敗；整列算待確認，不讓整支 API 掛掉。
                Err(_)=>continue,
            };
            let period_end=result["periodEnd"].as_str().unwrap_or_default().to_string();
            let issues_empty=result["issues"].as_array().map(|a|a.is_empty()).unwrap_or(false);
            let base_rent=result["baseRent"].as_str().map(str::to_string);
            let entry=ApplicationEntryInput{
                contract_no:c.contract_no.clone(),rent_calc_code:c.rent_calc_code.clone(),kind:p.kind.clone(),
                landowner_names:landowners.iter().map(|(n,_)|n.clone()).collect(),
                landowner_tax_id:landowners.first().and_then(|(_,t)|t.clone()),
                invoice_timing:draft["invoiceTiming"].as_str().map(str::to_string),
                base_rent,has_issue:!issues_empty,
            };
            periods.entry((period_start,period_end)).or_default().push(entry);
        }
    }
    let applications:Vec<Value>=periods.into_iter().map(|((ps,pe),entries)|
        rent_cycle::application(&station_name,&company_code,&company_name,&ps,&pe,&entries)).collect();
    Ok(HttpResponse::Ok().json(json!({"applications":applications,"notPayable":not_payable})))
}
