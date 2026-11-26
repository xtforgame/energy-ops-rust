//! One saved rental handoff per station/rule/cycle. Never authorizes payment.
use actix_web::{web, HttpResponse};
use eo_platform::{auth::CurrentUser, core::CoreRef, error::ApiResult};
use crate::{gates, common::{err, err_d, Ctx, RequestMeta}, audit::{self, AuditInput}};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
const FIELDS: &[&str] = &["companyCode","companyName","applicant","department","applicationDate","summary","paymentMethod","paymentDate","contractOption","voucherStatus"];
fn same_cycle(a:&Value,b:&Value)->bool { ["ruleId","periodStart","periodEnd"].iter().all(|k|a[k]==b[k]) }
fn revision(item:&Value,defaults:&Value)->String {let mut source=item.clone();if let Some(o)=source.as_object_mut(){o.remove("voucherAttachments");}format!("{:x}",Sha256::digest(serde_json::to_vec(&json!(["rental-form.v2",source,defaults])).unwrap()))}
fn voucher_revision(v:&Value)->String {format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap()))}
async fn attach_vouchers(ctx:&Ctx,item:&mut Value,data:&Value)->ApiResult<()> {
 let ids:Vec<i64>=data["rentApplications"].as_array().and_then(|a|a.iter().find(|d|same_cycle(item,d))).and_then(|d|d["voucherAttachmentIds"].as_array()).map(|a|a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default();
 let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'originalFilename',original_filename,'hash',hash,'status',status) FROM tbl_energy_attachment WHERE id=ANY($1) AND organization_id=$2 AND deleted_at IS NULL ORDER BY id").bind(ids).bind(ctx.org_id()).fetch_all(&ctx.core.pool).await?;
 item["voucherAttachments"]=json!(rows);Ok(())
}
fn apply(item:&mut Value,data:&Value,can_review:bool){
 let source=revision(item,&data["easyflowDefaults"]);
 let saved=data["rentApplications"].as_array().and_then(|a|a.iter().find(|d|same_cycle(item,d))).cloned().unwrap_or(Value::Null);
 let timing=saved["invoiceTiming"].as_str().filter(|s|!s.trim().is_empty()).or(item["invoiceTiming"].as_str()).unwrap_or("unknown").to_string();
 // A declared invoice arrangement is evidence in its own right; a missing tax ID
 // must not hide a known "invoice later" instruction.
 let voucher=if item["payeeKind"]=="individual" {json!("有，租金計算表")}
  else if timing=="later" {json!("無，發票後補")}
  else if saved["invoiceTiming"].as_str().is_some_and(|s|Some(s)!=item["invoiceTiming"].as_str()) {Value::Null}
  else {item["voucherStatus"].clone()};
 let mut fields=serde_json::Map::new();
 for &key in FIELDS {
  let generated=if key=="voucherStatus" {Some(&voucher)}else{item.get(key)};
  let value=[saved["fields"].get(key),data["easyflowDefaults"].get(key),generated].into_iter().flatten()
   .find(|v|v.as_str().is_some_and(|s|!s.trim().is_empty())).cloned().unwrap_or(Value::Null);
  fields.insert(key.into(),value);
 }
 let missing:Vec<_>=FIELDS.iter().filter(|k|!fields[**k].as_str().is_some_and(|v|!v.trim().is_empty())).copied().collect();
 let stale=saved.is_object()&&(saved["sourceRevision"]!=source || (saved["voucherRevision"].is_string()&&saved["voucherRevision"]!=voucher_revision(&item["voucherAttachments"])));
 let vouchers_available=saved["voucherAttachmentIds"].as_array().into_iter().flatten().all(|id|item["voucherAttachments"].as_array().is_some_and(|rows|rows.iter().any(|r|r["id"]==*id&&r["status"]=="active")));
 let mut confirmation_blockers=Vec::new();if !confirmable(item){confirmation_blockers.push("租金規則、金額或來源尚未確認");}if !vouchers_available{confirmation_blockers.push("憑證附件缺漏或已換版");}
 item["invoiceTiming"]=json!(timing);
 item["application"]=json!({"fields":fields,"missingFields":missing,"sourceRevision":source,"draftRevision":saved["draftRevision"].as_u64().unwrap_or(0),"state":if stale{"stale"}else{saved["state"].as_str().unwrap_or("draft")},"stale":stale,"canReview":can_review,"canConfirm":can_review&&confirmable(item)&&vouchers_available,"confirmationBlockers":confirmation_blockers,"invoiceTiming":timing,"voucherAttachmentIds":saved["voucherAttachmentIds"].as_array().cloned().unwrap_or_default(),"confirmedBy":if stale{Value::Null}else{saved["confirmedBy"].clone()},"confirmedAt":if stale{Value::Null}else{saved["confirmedAt"].clone()},"paymentAllowed":false});
 item["paymentAllowed"]=json!(false);
}
fn apply_for_today(item:&mut Value,data:&Value,can_review:bool,today:chrono::NaiveDate){
 apply(item,data,can_review);
 // Date defaults are applied after the source hash, matching electricity drafts.
 // A new calendar day alone must not invalidate a saved confirmation.
 if item["application"]["fields"]["applicationDate"].is_null(){
  item["application"]["fields"]["applicationDate"]=json!(today.to_string());
  if let Some(missing)=item["application"]["missingFields"].as_array_mut(){missing.retain(|v|v!="applicationDate");}
 }
}
async fn can_review(ctx:&Ctx)->ApiResult<bool>{
 if !ctx.has("energy:flow:act").await? {return Ok(false)}
 if ctx.access.is_super_user()||ctx.has("energy:flow:admin").await? {return Ok(true)}
 let allowed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mn_energy_department_member m JOIN tbl_energy_department d ON d.id=m.department_id WHERE d.organization_id=$1 AND d.key=$2 AND d.deleted_at IS NULL AND m.deleted_at IS NULL AND m.user_id=$3)").bind(ctx.org_id()).bind(eo_domain::workflow_defs::DEPT_RENT_REVIEW).bind(ctx.user_id()).fetch_one(&ctx.core.pool).await?;
 Ok(allowed)
}
pub async fn decorate_items(ctx:&Ctx,_sid:i64,station:&Value,items:&mut [Value])->ApiResult<()> {
 let review=can_review(ctx).await?;
 for item in items {attach_vouchers(ctx,item,&station["data"]).await?;apply_for_today(item,&station["data"],review,ctx.today())}
 Ok(())
}
fn confirmable(item:&Value)->bool {
 (item["inputMethod"]!="autoBills"||item["status"]=="confirmed") && item["grossRent"].as_str().is_some_and(|v| v.parse::<rust_decimal::Decimal>().is_ok()) && ["issues","sourceBlockers"].iter().all(|k|item[*k].as_array().is_none_or(|a|a.is_empty()))
}
pub async fn save(core:web::Data<CoreRef>,user:CurrentUser,meta:RequestMeta,path:web::Path<(i64,i64)>,body:web::Json<Value>)->ApiResult<HttpResponse>{
 let(org,sid)=path.into_inner();let access=core.rbac.authorize_org(&user,org,gates::FLOW_ACT).await?;let ctx=Ctx::new(&core,access,&meta);
 let action=body["action"].as_str().filter(|s|["save","confirm"].contains(s)).ok_or_else(||err(400,"invalid_application_action"))?;
 let fields=body["fields"].as_object().ok_or_else(||err(400,"invalid_application_fields"))?;
 if fields.iter().any(|(k,v)|!FIELDS.contains(&k.as_str())||(!v.is_null()&&!v.is_string())||v.as_str().is_some_and(|s|s.len()>4000)){return Err(err(400,"invalid_application_fields"))}
 let timing=body["invoiceTiming"].as_str().filter(|s|["unknown","before","later"].contains(s)).ok_or_else(||err(400,"invalid_invoice_timing"))?;
 let review=can_review(&ctx).await?;
 if action=="confirm"&&!review{return Err(err(403,"rent_review_group_required"))}
 let mut tx=core.pool.begin().await?;
 let mut station:Value=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'name',name,'customerNumber',customer_number,'data',data) FROM tbl_energy_station WHERE id=$1 AND organization_id=$2 AND deleted_at IS NULL FOR UPDATE").bind(sid).bind(org).fetch_optional(&mut *tx).await?.ok_or_else(||err(404,"station_not_found"))?;
 // Stabilize existing source rows while the snapshot is calculated and saved.
 sqlx::query("SELECT id FROM tbl_energy_bill WHERE station_id=$1 AND organization_id=$2 FOR SHARE").bind(sid).bind(org).fetch_all(&mut *tx).await?;
 sqlx::query("SELECT id FROM tbl_energy_station_rent_rule WHERE station_id=$1 AND organization_id=$2 FOR SHARE").bind(sid).bind(org).fetch_all(&mut *tx).await?;
 let from=body["periodStart"].as_str().and_then(crate::common::parse_day).ok_or_else(||err(400,"invalid_rent_period"))?;
 let to=body["periodEnd"].as_str().and_then(crate::common::parse_day).ok_or_else(||err(400,"invalid_rent_period"))?;
 if from>to{return Err(err(400,"invalid_rent_period"))}
 let items=super::rent_payment::station_items(&core,org,sid,&station,Some((from,to))).await?;
 let mut item=items.into_iter().find(|i|same_cycle(i,&body)).ok_or_else(||err(404,"rent_cycle_not_found"))?;
 if !item["periodStart"].is_string()||!item["periodEnd"].is_string(){return Err(err(422,"rent_cycle_required"))}
 attach_vouchers(&ctx,&mut item,&station["data"]).await?;let mut raw=item.clone();apply_for_today(&mut item,&station["data"],review,ctx.today());
 if body["sourceRevision"]!=item["application"]["sourceRevision"]{return Err(err(409,"application_source_changed"))}
 if body["draftRevision"]!=item["application"]["draftRevision"]{return Err(err(409,"application_draft_changed"))}
 if action=="confirm"&&!confirmable(&raw){return Err(err_d(422,"rent_sources_incomplete",json!({"issues":raw["issues"],"sourceBlockers":raw["sourceBlockers"]})))}
 if action=="confirm" {for line in raw["lines"].as_array().into_iter().flatten(){if let Some(id)=line["attachmentId"].as_i64(){crate::attachments::service::attachment_in_org(&mut tx,org,id).await?;}}}
 let ids=body.get("voucherAttachmentIds").unwrap_or(&item["application"]["voucherAttachmentIds"]).as_array().ok_or_else(||err(400,"invalid_voucher_attachments"))?;
 if ids.len()>100{return Err(err(400,"invalid_voucher_attachments"))}
 let mut voucher_rows=Vec::new();
 for id in ids {let id=id.as_i64().filter(|n|*n>0).ok_or_else(||err(400,"invalid_voucher_attachments"))?;let row=crate::attachments::service::attachment_in_org(&mut tx,org,id).await?;if action=="confirm"&&row.status!="active"{return Err(err(422,"voucher_attachment_unavailable"))}voucher_rows.push(json!({"id":row.id,"originalFilename":row.original_filename,"hash":row.hash,"status":row.status}));}
 voucher_rows.sort_by_key(|r|r["id"].as_i64());voucher_rows.dedup_by_key(|r|r["id"].as_i64());raw["voucherAttachments"]=json!(voucher_rows);
 let mut merged=item["application"]["fields"].as_object().cloned().unwrap_or_default();merged.extend(fields.clone());
 let before=station["data"]["rentApplications"].as_array().cloned().unwrap_or_default();let mut drafts=before.clone();drafts.retain(|d|!same_cycle(d,&item));
 let saved=json!({"ruleId":item["ruleId"],"periodStart":item["periodStart"],"periodEnd":item["periodEnd"],"fields":merged,"sourceRevision":item["application"]["sourceRevision"],"draftRevision":item["application"]["draftRevision"].as_u64().unwrap_or(0)+1,"invoiceTiming":timing,"voucherAttachmentIds":ids,"voucherRevision":voucher_revision(&raw["voucherAttachments"]),"state":if action=="confirm"{"dataConfirmed"}else{"draft"},"confirmedBy":if action=="confirm"{json!(ctx.user_id())}else{Value::Null},"confirmedAt":if action=="confirm"{json!(ctx.now())}else{Value::Null},"updatedBy":ctx.user_id(),"updatedAt":ctx.now()});
 drafts.push(saved.clone());
 sqlx::query("UPDATE tbl_energy_station SET data=jsonb_set(COALESCE(data,'{}'::jsonb),'{rentApplications}',$1,true),updated_at=clock_timestamp() WHERE id=$2 AND organization_id=$3").bind(json!(drafts)).bind(sid).bind(org).execute(&mut *tx).await?;
 audit::record(&mut tx,&ctx,AuditInput::new("station.rent_application_saved",sid,"儲存租金交接資料（非 EF 送出或付款核可）").payload(json!({"action":action,"before":before,"application":saved}))).await?;
 tx.commit().await?;
 station["data"]["rentApplications"]=json!(drafts);item=raw;apply_for_today(&mut item,&station["data"],review,ctx.today());
 Ok(HttpResponse::Ok().insert_header(("Cache-Control","no-store")).json(item))
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn known_invoice_arrangement_does_not_require_tax_id(){
  let mut item=json!({"ruleId":16,"periodStart":"a","periodEnd":"b","invoiceTiming":"later","payeeKind":"unknown"});
  apply(&mut item,&json!({}),true);
  assert_eq!(item["application"]["fields"]["voucherStatus"],"無，發票後補");
 }
 #[test]fn rental_application_date_is_generated_but_saved_date_is_preserved(){
  let mut item=json!({"ruleId":16,"periodStart":"a","periodEnd":"b"});
  let today=chrono::NaiveDate::from_ymd_opt(2026,11,26).unwrap();
  apply_for_today(&mut item,&json!({}),true,today);
  assert_eq!(item["application"]["fields"]["applicationDate"],"2026-11-26");
  assert!(!item["application"]["missingFields"].as_array().unwrap().contains(&json!("applicationDate")));
  let data=json!({"rentApplications":[{"ruleId":16,"periodStart":"a","periodEnd":"b","fields":{"applicationDate":"2026-10-28"}}]});
  apply_for_today(&mut item,&data,true,today);
  assert_eq!(item["application"]["fields"]["applicationDate"],"2026-10-28");
 }
 #[test]fn blank_legacy_draft_uses_known_defaults_without_overwriting_edits(){
  let mut item=json!({"ruleId":16,"periodStart":"2026-10-01","periodEnd":"2026-10-31","contractOption":"一般付款 > 有合約／簽呈","paymentMethod":"匯款（請附存摺影本）","voucherStatus":"無，發票後補","summary":"自動摘要"});
  let data=json!({"easyflowDefaults":{"paymentMethod":null,"companyCode":"002"},"rentApplications":[{"ruleId":16,"periodStart":"2026-10-01","periodEnd":"2026-10-31","fields":{"contractOption":null,"paymentMethod":"  ","voucherStatus":"","companyCode":null,"summary":"承辦已修訂摘要"}}]});
  apply(&mut item,&data,true);
  let f=&item["application"]["fields"];
  assert_eq!(f["contractOption"],"一般付款 > 有合約／簽呈");
  assert_eq!(f["paymentMethod"],"匯款（請附存摺影本）");
  assert_eq!(f["voucherStatus"],"無，發票後補");
  assert_eq!(f["companyCode"],"002");
  assert_eq!(f["summary"],"承辦已修訂摘要");
  assert!(f["applicant"].is_null());
 }
 #[test]fn source_change_invalidates_confirmation_without_losing_fields(){let original=json!({"ruleId":1,"periodStart":"2026-01-01","periodEnd":"2026-12-31","grossRent":"100"});let mut i=original.clone();apply(&mut i,&json!({}),true);let data=json!({"rentApplications":[{"ruleId":1,"periodStart":"2026-01-01","periodEnd":"2026-12-31","sourceRevision":i["application"]["sourceRevision"],"draftRevision":1,"state":"dataConfirmed","fields":{"applicant":"王"}}]});let mut current=original.clone();apply(&mut current,&data,true);assert_eq!(current["application"]["state"],"dataConfirmed");let mut changed=original;changed["grossRent"]=json!("101");apply(&mut changed,&data,true);assert_eq!(changed["application"]["state"],"stale");assert_eq!(changed["application"]["fields"]["applicant"],"王");assert_eq!(changed["paymentAllowed"],false);}
 #[test]fn removed_voucher_invalidates_confirmation(){
 let mut raw=json!({"ruleId":1,"periodStart":"a","periodEnd":"b","grossRent":"100","voucherAttachments":[{"id":7,"hash":"abc","status":"active"}]});
 let source=revision(&raw,&Value::Null);let data=json!({"rentApplications":[{"ruleId":1,"periodStart":"a","periodEnd":"b","state":"dataConfirmed","sourceRevision":source,"voucherRevision":voucher_revision(&raw["voucherAttachments"])}]});
 let mut current=raw.clone();apply(&mut current,&data,true);assert_eq!(current["application"]["state"],"dataConfirmed");
 raw["voucherAttachments"]=json!([]);apply(&mut raw,&data,true);assert_eq!(raw["application"]["state"],"stale");assert_eq!(raw["application"]["sourceRevision"],source);
 }
 #[test]fn same_period_different_rules_stay_separate(){assert!(!same_cycle(&json!({"ruleId":1,"periodStart":"x","periodEnd":"y"}),&json!({"ruleId":2,"periodStart":"x","periodEnd":"y"})));}
 #[test]fn confirmation_requires_amount_and_sources_but_not_ef_fields(){assert!(confirmable(&json!({"grossRent":"0","issues":[],"sourceBlockers":[]})));assert!(!confirmable(&json!({"grossRent":null})));assert!(!confirmable(&json!({"grossRent":"100","inputMethod":"autoBills","status":"pending"})));assert!(!confirmable(&json!({"grossRent":"1","sourceBlockers":["missing"]})));}
}
