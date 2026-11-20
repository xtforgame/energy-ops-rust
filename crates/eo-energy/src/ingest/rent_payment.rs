use super::bills;
use actix_web::{web,HttpResponse};
use eo_platform::{auth::CurrentUser,core::CoreRef,error::ApiResult};
use crate::{gates,common::{err,err_d,Ctx,RequestMeta},audit::{self,AuditInput}};
use serde_json::{Value,json};
type Core=web::Data<CoreRef>;
pub async fn get(core:Core,user:CurrentUser,path:web::Path<(i64,i64)>)->ApiResult<HttpResponse>{
 let(org,id)=path.into_inner();core.rbac.authorize_org(&user,org,gates::FLOW_VIEW).await?;
 let mut tx=core.pool.begin().await?;let bill=bills::bill_in_org(&mut tx,org,id,false).await?;
 let station=if let Some(sid)=bill.station_id{sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('id',id,'name',name,'data',data) FROM tbl_energy_station WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL").bind(sid).bind(org).fetch_optional(&mut *tx).await?}else{None};
 let mut items=Vec::new();if let Some(s)=&station {if let Some(drafts)=s["data"]["rentPaymentDrafts"].as_array(){for d in drafts{
  // Show the cycle containing this bill; never silently display another period's payment.
  if let (Some(a),Some(b))=(bill.billing_period_start,bill.billing_period_end){if d["periodStart"].as_str().is_some_and(|x|x>b.to_string().as_str())||d["periodEnd"].as_str().is_some_and(|x|x<a.to_string().as_str()){continue;}}
  let mut result=eo_domain::rent_payment::calculate(d).map_err(|m|err_d(422,"rent_payment_invalid",json!({"message":m})))?;
  result["stationName"]=s["name"].clone();result["customerNumber"]=json!(bill.customer_number_raw);items.push(result);
 }}}
 Ok(HttpResponse::Ok().insert_header(("Cache-Control","no-store")).json(json!({"items":items,"billId":id,"paymentAllowed":false})))
}
pub async fn save(core:Core,user:CurrentUser,meta:RequestMeta,path:web::Path<(i64,i64)>,body:web::Json<Value>)->ApiResult<HttpResponse>{
 let(org,id)=path.into_inner();let access=core.rbac.authorize_org(&user,org,gates::MASTER_MANAGE).await?;let ctx=Ctx::new(&core,access,&meta);
 let draft=body.into_inner();let result=eo_domain::rent_payment::calculate(&draft).map_err(|m|err_d(422,"rent_payment_invalid",json!({"message":m})))?;
 let mut tx=core.pool.begin().await?;let bill=bills::bill_in_org(&mut tx,org,id,false).await?;let sid=bill.station_id.ok_or_else(||err(422,"station_required"))?;
 for r in draft["receipts"].as_array().unwrap(){crate::attachments::service::attachment_in_org(&mut tx,org,r["attachmentId"].as_i64().unwrap()).await?;}
 let data:Value=sqlx::query_scalar("SELECT data FROM tbl_energy_station WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL FOR UPDATE").bind(sid).bind(org).fetch_one(&mut *tx).await?;
 let mut drafts=data["rentPaymentDrafts"].as_array().cloned().unwrap_or_default();
 drafts.retain(|d|d["periodStart"]!=draft["periodStart"]||d["periodEnd"]!=draft["periodEnd"]);drafts.push(draft.clone());
 sqlx::query("UPDATE tbl_energy_station SET data=jsonb_set(COALESCE(data,'{}'::jsonb),'{rentPaymentDrafts}',$1,true),updated_at=clock_timestamp() WHERE id=$2 AND organization_id=$3").bind(json!(drafts)).bind(sid).bind(org).execute(&mut *tx).await?;
 audit::record(&mut tx,&ctx,AuditInput::new("station.rent_sources_updated",sid,"匯入已核對租金計算來源與規則").payload(json!({"billId":id,"periodStart":draft["periodStart"],"periodEnd":draft["periodEnd"],"inputMethod":"documentVerifiedInputs","before":data["rentPaymentDrafts"],"input":draft}))).await?;
 tx.commit().await?;Ok(HttpResponse::Ok().json(result))
}
