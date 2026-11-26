use super::bills::{BillRow,BILL_COLS};
use actix_web::{web,HttpResponse};
use eo_platform::{auth::CurrentUser,core::CoreRef,error::ApiResult};
use crate::{gates,common::{err,err_d,Ctx,RequestMeta},audit::{self,AuditInput}};
use serde_json::{Value,json};
type Core=web::Data<CoreRef>;
pub async fn list(core:Core,user:CurrentUser,path:web::Path<(i64,i64)>)->ApiResult<HttpResponse>{
 let(org,sid)=path.into_inner();core.rbac.authorize_org(&user,org,gates::FLOW_VIEW).await?;
 let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tbl_energy_station WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL)").bind(sid).bind(org).fetch_one(&core.pool).await?;if !exists{return Err(err(404,"station_not_found"))}
 let mut items=load(&core,org,sid).await?;
 for row in &mut items {row["assessment"]=eo_domain::rent_rules::assess(&row["rule"],&json!({}));}
 let can_manage=core.rbac.authorize_org(&user,org,gates::MASTER_MANAGE).await.is_ok();
 Ok(HttpResponse::Ok().insert_header(("Cache-Control","no-store")).json(json!({"items":items,"canManage":can_manage})))
}
async fn load(core:&CoreRef,org:i64,sid:i64)->ApiResult<Vec<Value>>{
 Ok(sqlx::query_scalar("SELECT jsonb_build_object('id',id,'version',version,'rule',rule) FROM tbl_energy_station_rent_rule WHERE organization_id=$1 AND station_id=$2 ORDER BY valid_from NULLS LAST,id").bind(org).bind(sid).fetch_all(&core.pool).await?)
}
pub async fn save(core:Core,user:CurrentUser,meta:RequestMeta,path:web::Path<(i64,i64)>,body:web::Json<Value>)->ApiResult<HttpResponse>{
 let(org,sid)=path.into_inner();let access=core.rbac.authorize_org(&user,org,gates::MASTER_MANAGE).await?;let ctx=Ctx::new(&core,access,&meta);
 let mut rule=body.into_inner();let id=rule["id"].as_i64();let expected=rule["expectedVersion"].as_i64();
 if let Some(o)=rule.as_object_mut(){o.remove("id");o.remove("expectedVersion");}
 eo_domain::rent_rules::validate(&rule).map_err(|m|err_d(422,"rent_rule_invalid",json!({"message":m})))?;
 let mut tx=core.pool.begin().await?;
 let station:Option<i64>=sqlx::query_scalar("SELECT id FROM tbl_energy_station WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL FOR UPDATE").bind(sid).bind(org).fetch_optional(&mut *tx).await?;if station.is_none(){return Err(err(404,"station_not_found"))}
 for id in rule["leaseEvidence"]["attachmentIds"].as_array().unwrap(){crate::attachments::service::attachment_in_org(&mut tx,org,id.as_i64().unwrap()).await?;}
 let from=rule["validFrom"].as_str();let to=rule["validTo"].as_str();
 if rule["status"]=="confirmed"{
 let overlap:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tbl_energy_station_rent_rule WHERE station_id=$1 AND organization_id=$2 AND status='confirmed' AND ($3::bigint IS NULL OR id<>$3) AND (valid_to IS NULL OR valid_to >= $4::text::date) AND ($5::text IS NULL OR valid_from <= $5::text::date))").bind(sid).bind(org).bind(id).bind(from).bind(to).fetch_one(&mut *tx).await?;
 if overlap{return Err(err(409,"rent_rule_overlap"))}}
 let (id,version)=if let Some(id)=id{
 let row:Option<(i64,i32)>=sqlx::query_as("UPDATE tbl_energy_station_rent_rule SET version=version+1,rule=$1,lease_evidence=$2,valid_from=$3::text::date,valid_to=$4::text::date,status=$5,updated_at=clock_timestamp() WHERE id=$6 AND station_id=$7 AND organization_id=$8 AND version=$9 RETURNING id,version")
 .bind(&rule).bind(&rule["leaseEvidence"]).bind(from).bind(to).bind(rule["status"].as_str()).bind(id).bind(sid).bind(org).bind(expected).fetch_optional(&mut *tx).await?;
 row.ok_or_else(||err(409,"rent_rule_changed"))?
 }else{sqlx::query_as::<_,(i64,i32)>("INSERT INTO tbl_energy_station_rent_rule(station_id,organization_id,rule,lease_evidence,valid_from,valid_to,status) VALUES($1,$2,$3,$4,$5::text::date,$6::text::date,$7) RETURNING id,version")
 .bind(sid).bind(org).bind(&rule).bind(&rule["leaseEvidence"]).bind(from).bind(to).bind(rule["status"].as_str()).fetch_one(&mut *tx).await?};
 sqlx::query("INSERT INTO tbl_energy_station_rent_rule_revision(rule_id,version,rule) VALUES($1,$2,$3)").bind(id).bind(version).bind(&rule).execute(&mut *tx).await?;
 audit::record(&mut tx,&ctx,AuditInput::new("station.rent_rule_saved",sid,"儲存可重用租金規則與獨立租約附件狀態").payload(json!({"ruleId":id,"version":version,"rule":rule}))).await?;
 tx.commit().await?;Ok(HttpResponse::Ok().json(json!({"id":id,"version":version,"rule":rule})))
}
/// Auto candidates never merge into manual historical worksheets.
pub async fn append(core:&CoreRef,org:i64,sid:i64,station:&Value,items:&mut Vec<Value>,filter:Option<(chrono::NaiveDate,chrono::NaiveDate)>)->ApiResult<()>{
 let rules=load(core,org,sid).await?;
 let bills:Vec<BillRow>=sqlx::query_as(&format!("SELECT {BILL_COLS} FROM tbl_energy_bill b WHERE organization_id=$1 AND station_id=$2 AND deleted_at IS NULL AND (NOT EXISTS(SELECT 1 FROM tbl_energy_ingest_item i WHERE i.organization_id=b.organization_id AND i.bill_id=b.id) OR EXISTS(SELECT 1 FROM tbl_energy_ingest_item i WHERE i.organization_id=b.organization_id AND i.bill_id=b.id AND i.deleted_at IS NULL AND i.status<>'discarded')) ORDER BY billing_period_start,id")).bind(org).bind(sid).fetch_all(&core.pool).await?;
 let mut sources=Vec::new();
 for b in &bills{
  let attached=if let Some(id)=b.source_attachment_id{let mut tx=core.pool.begin().await?;crate::attachments::service::attachment_in_org(&mut tx,org,id).await.is_ok()}else{false};
  let page=eo_domain::taipower_extractor::as_canonical_record(&b.ocr_raw).and_then(|record|record.fields.get("electricityFeeTotal").or_else(||record.fields.get("purchaseEnergyFee")).and_then(|f|f.evidence.as_ref()).and_then(|ev|ev.page_number)).filter(|p|*p>0);
  sources.push(json!({"id":b.id,"page":page,"docType":b.effective_doc_type().as_str(),"start":b.billing_period_start,"end":b.billing_period_end,"electricityFeeTotal":b.field_value("electricityFeeTotal"),"purchaseEnergyFee":b.field_value("purchaseEnergyFee"),"kwh":if b.generation_kwh.is_some(){b.field_value("generationKwh")}else{b.field_value("purchaseKwh")},"attachmentId":if attached{b.source_attachment_id}else{None}}));
 }
 for row in &rules{
 let rule=&row["rule"];let saved_dates=station["data"]["rentApplications"].as_array().into_iter().flatten().filter(|app|app["ruleId"]==row["id"]).filter_map(|app|app["periodStart"].as_str().and_then(|day|chrono::NaiveDate::parse_from_str(day,"%Y-%m-%d").ok()));
 let cycles=candidate_cycles(rule,bills.iter().filter_map(|bill|bill.billing_period_start).chain(saved_dates),filter);
 if cycles.is_empty(){let mut item=eo_domain::rent_rules::assess(rule,&json!({}));item["inputMode"]=json!("autoBills");item["inputMethod"]=json!("autoBills");item["sourceBlockers"]=if ["fixed","areaRate"].contains(&rule["calculationType"].as_str().unwrap_or("")){json!([])}else{json!(["尚無可組成完整週期的來源或週期錨點待確認"])};item["sourceContributions"]=eo_domain::rent_rules::source_contributions(rule,&sources.iter().filter(|s|filter.is_none_or(|(a,b)|s["start"].as_str().is_some_and(|x|x<=b.to_string().as_str())&&s["end"].as_str().is_some_and(|x|x>=a.to_string().as_str()))).cloned().collect::<Vec<_>>());decorate(&mut item,row,sid,station);items.push(item);continue}
 for(a,b)in cycles{
 let manual=station["data"]["rentPaymentDrafts"].as_array().is_some_and(|drafts|drafts.iter().any(|d|d["periodStart"].as_str().is_some_and(|x|x<=b.to_string().as_str())&&d["periodEnd"].as_str().is_some_and(|x|x>=a.to_string().as_str())));
 let overlap=rules.iter().any(|other|other["id"]!=row["id"]&&other["rule"]["status"]=="confirmed"&&other["rule"]["validFrom"].as_str().is_some_and(|x|x<=b.to_string().as_str())&&other["rule"]["validTo"].as_str().is_none_or(|x|x>=a.to_string().as_str()));
 let mut item=if manual{eo_domain::rent_rules::pending("此期間已有人工歷史草稿，未合併自動來源")}else if overlap{eo_domain::rent_rules::pending("同期間規則衝突") }else{eo_domain::rent_rules::preview(rule,&a.to_string(),&sources)};
 item["sourceContributions"]=eo_domain::rent_rules::source_contributions(rule,&sources.iter().filter(|s|s["start"].as_str().is_some_and(|x|x<=b.to_string().as_str())&&s["end"].as_str().is_some_and(|x|x>=a.to_string().as_str())).cloned().collect::<Vec<_>>());item["periodStart"]=json!(a.to_string());item["periodEnd"]=json!(b.to_string());decorate(&mut item,row,sid,station);items.push(item);
 }}Ok(())
}
fn candidate_cycles(rule:&Value,dates:impl IntoIterator<Item=chrono::NaiveDate>,filter:Option<(chrono::NaiveDate,chrono::NaiveDate)>)->std::collections::BTreeSet<(chrono::NaiveDate,chrono::NaiveDate)> {
 let mut cycles=std::collections::BTreeSet::new();
 let selected=filter.filter(|_|["fixed","areaRate"].contains(&rule["calculationType"].as_str().unwrap_or(""))).map(|(start,_)|start);
 for day in dates.into_iter().chain(selected) {if let Ok((a,b))=eo_domain::rent_rules::cycle(rule,&day.to_string()){
  if rule["validFrom"].as_str().is_some_and(|x|b.to_string().as_str()<x)||rule["validTo"].as_str().is_some_and(|x|a.to_string().as_str()>x){continue}
  if filter.is_some_and(|(x,y)|a>y||b<x){continue}cycles.insert((a,b));
 }}cycles
}

fn decorate(item:&mut Value,row:&Value,sid:i64,station:&Value){
 let rule=&row["rule"];
 for key in ["payee","basis","percent","taxPercent","taxMode","rounding","ruleSource","invoiceTiming","paymentDeadline","calculationType","notes","rentRequired"] {
  if item[key].is_null(){item[key]=rule[key].clone();}
 }
 if !item["paymentDeadline"].as_str().is_some_and(|s|!s.trim().is_empty()) {
  if let Some(s)=rule["sourceValues"]["BD"].as_str().filter(|s|!s.trim().is_empty()&&*s!="N/A"){item["paymentDeadline"]=json!(s);}
 }
 if item["lines"].is_null(){item["lines"]=json!([]);}
 if item["issues"].is_null(){item["issues"]=json!([]);}
 item["customerNumber"]=station["customerNumber"].clone();
 item["assessment"]=eo_domain::rent_rules::assess(rule,&json!({}));item["status"]=rule["status"].clone();item["ruleId"]=row["id"].clone();item["ruleVersion"]=row["version"].clone();item["stationId"]=json!(sid);item["stationName"]=station["name"].clone();item["evidenceStatus"]=rule["leaseEvidence"]["status"].clone();item["leaseEvidence"]=rule["leaseEvidence"].clone();
}

#[cfg(test)] mod preview_metadata_tests {
 use super::*;
 #[test] fn selected_fixed_period_needs_no_electricity_bill(){
  let day=chrono::NaiveDate::from_ymd_opt(2026,11,15).unwrap();
  for kind in ["fixed","areaRate"] {
   let rule=json!({"calculationType":kind,"cycleAnchor":"2026-01-01","cycleMonths":3,"validFrom":"2026-01-01"});
   let cycles=candidate_cycles(&rule,[],Some((day,day)));
   assert_eq!(cycles.len(),1);let (a,b)=cycles.first().unwrap();assert_eq!(a.to_string(),"2026-10-01");assert_eq!(b.to_string(),"2026-12-31");
   assert!(candidate_cycles(&rule,[],None).is_empty());
  }
  let rule=json!({"calculationType":"percentage","cycleAnchor":"2026-01-01","cycleMonths":1});assert!(candidate_cycles(&rule,[],Some((day,day))).is_empty());
  let expired=json!({"calculationType":"fixed","cycleAnchor":"2026-01-01","cycleMonths":1,"validTo":"2026-09-30"});assert!(candidate_cycles(&expired,[],Some((day,day))).is_empty());
 }
 #[test] fn pending_cycle_keeps_known_rule_values_and_lease_evidence(){
  let row=json!({"id":16,"version":2,"rule":{"payee":"力寶","percent":"8","invoiceTiming":"later","leaseEvidence":{"status":"excerpt","attachmentIds":[42]}}});
  let mut item=json!({"grossRent":null});
  decorate(&mut item,&row,70,&json!({"name":"麻豆","customerNumber":"20-72-7017-93-7"}));
  assert_eq!(item["payee"],"力寶");assert_eq!(item["percent"],"8");assert_eq!(item["invoiceTiming"],"later");assert_eq!(item["customerNumber"],"20-72-7017-93-7");assert!(item["grossRent"].is_null());assert_eq!(item["leaseEvidence"]["attachmentIds"][0],42);
 }
}

/// Stateless trial: does not create a bill, worksheet or payment instruction.
pub async fn trial(core:Core,user:CurrentUser,path:web::Path<(i64,i64,i64)>,body:web::Json<Value>)->ApiResult<HttpResponse>{
 let(org,sid,rid)=path.into_inner();core.rbac.authorize_org(&user,org,gates::FLOW_VIEW).await?;
 let rule:Option<Value>=sqlx::query_scalar("SELECT r.rule FROM tbl_energy_station_rent_rule r JOIN tbl_energy_station s ON s.id=r.station_id AND s.organization_id=r.organization_id WHERE r.id=$1 AND r.station_id=$2 AND r.organization_id=$3 AND s.deleted_at IS NULL").bind(rid).bind(sid).bind(org).fetch_optional(&core.pool).await?;
 let rule=rule.ok_or_else(||err(404,"rent_rule_not_found"))?;
 let mut result=eo_domain::rent_rules::assess(&rule,&body.into_inner());result["ruleId"]=json!(rid);result["stationId"]=json!(sid);
 Ok(HttpResponse::Ok().insert_header(("Cache-Control","no-store")).json(result))
}
