// Portable, review-first rental rule import. Node 18+. No local IDs or credentials in packages.
import fs from 'node:fs';
import {createHash} from 'node:crypto';
import {pathToFileURL} from 'node:url';
export const canonical=v=>JSON.stringify(v,(_k,x)=>x&&typeof x==='object'&&!Array.isArray(x)?Object.fromEntries(Object.entries(x).sort(([a],[b])=>a.localeCompare(b))):x);
const equal=(a,b)=>canonical(a)===canonical(b);
const portable=v=>Array.isArray(v)?v.map(portable):v&&typeof v==='object'?Object.fromEntries(Object.entries(v).filter(([k])=>k!=='stationId').map(([k,x])=>[k,portable(x)])):v;
const sourceEqual=(a,b)=>equal(portable(a),portable(b));
const meter=v=>String(v??'').replace(/\D/g,'');
const digest=v=>createHash('sha256').update(canonical(v)).digest('hex');
export function planEntry(entry,stations,rules){
 const hits=stations.filter(s=>meter(s.customerNumber)===meter(entry.customerNumber));
 if(meter(entry.customerNumber).length!==11||hits.length!==1)throw Error('電號未唯一對應案場：'+entry.customerNumber);
 const s=hits[0];if(s.name!==entry.canonicalName)throw Error('案名與電費主表不符：'+entry.customerNumber);
 const matches=rules.filter(r=>r.rule.sourceKey===entry.after.sourceKey);
 const common={stationId:s.id,customerNumber:entry.customerNumber,name:s.name};
 if(matches.length>1)throw Error('已有重複規則：'+entry.customerNumber);
 if(!matches.length){
  if(rules.length)throw Error('已有其他來源規則，需先核對，未新增：'+entry.customerNumber);
  return {...common,action:'create',before:null,after:{...entry.after,leaseEvidence:{status:'missing',attachmentIds:[]}}};
 }
 const current=matches[0];
 if(current.rule.sourceSha256!==entry.after.sourceSha256)throw Error('來源版本不同：'+entry.customerNumber);
 const after=structuredClone(current.rule),conflicts=[];
 // Only apply fields changed in this correction; keep target's other terms and attachments.
 for(const key of new Set([...Object.keys(entry.before),...Object.keys(entry.after)])){
  if(key==='leaseEvidence'||sourceEqual(entry.before[key],entry.after[key]))continue;
  if(sourceEqual(current.rule[key],entry.after[key]))continue;
  if(!sourceEqual(current.rule[key],entry.before[key])){conflicts.push(key);continue;}
  if(Object.hasOwn(entry.after,key))after[key]=entry.after[key];else delete after[key];
 }
 if(conflicts.length)throw Error('目標已有不同修改，未覆蓋：'+entry.customerNumber+' '+conflicts.join(','));
 return {...common,id:current.id,version:current.version,before:current.rule,after,action:equal(current.rule,after)?'unchanged':'update'};
}
export async function main(argv=process.argv.slice(2)){
 const arg=n=>{const i=argv.indexOf(n);return i<0?null:argv[i+1]};
 const base=arg('--base')?.replace(/\/$/,''),org=arg('--org'),envFile=arg('--env-file'),out=arg('--out');
 if(!base||!org||!envFile||!out||!/^\d+$/.test(org))throw Error('Required --base URL --org ID --env-file FILE --out REPORT plus --package FILE or --apply-plan PLAN');
 const env=Object.fromEntries(fs.readFileSync(envFile,'utf8').split(/\r?\n/).filter(l=>l.includes('=')&&!l.startsWith('#')).map(l=>{const p=l.indexOf('=');return[l.slice(0,p).trim(),l.slice(p+1).trim().replace(/^['"]|['"]$/g,'')]}));
 if(!env.ADMIN_USER||!env.ADMIN_PASSWORD)throw Error('env 檔需包含 ADMIN_USER 與 ADMIN_PASSWORD');
 const login=await fetch(base+'/api/v1/auth/login',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({username:env.ADMIN_USER,password:env.ADMIN_PASSWORD}),signal:AbortSignal.timeout(30000)});
 if(!login.ok)throw Error('登入失敗 '+login.status);
 const cookie=login.headers.get('set-cookie')?.split(';')[0];if(!cookie)throw Error('Missing session');
 async function request(path,body){const r=await fetch(base+'/api/v1/orgs/'+org+'/energy/'+path,{method:body?'POST':'GET',headers:{cookie,'content-type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(30000)});if(!r.ok)throw Error('API '+r.status+' '+path);return r.json();}
 const stations=(await request('station-list')).items;
 const apply=arg('--apply-plan');
 if(!apply){
  if(!arg('--package'))throw Error('Missing --package');
  const pack=JSON.parse(fs.readFileSync(arg('--package'),'utf8').replace(/^\uFEFF/,''));
  if(pack.schema!=='energy-rent-portable-v1')throw Error('Unknown package schema');
  const items=[],errors=[],seen=new Set();
  for(const entry of pack.items){try{
   const key=meter(entry.customerNumber);if(seen.has(key))throw Error('資料包電號重複：'+key);seen.add(key);
   const matches=stations.filter(s=>meter(s.customerNumber)===key);
   const rules=matches.length===1?(await request(`stations/${matches[0].id}/rent-rules`)).items:[];
   items.push(planEntry(entry,stations,rules));
  }catch(e){errors.push(e.message);}}
  const plan={schema:'energy-rent-plan-v1',base,org,createdAt:new Date().toISOString(),packageHash:digest(pack),items,errors};
  fs.writeFileSync(out,JSON.stringify(plan,null,2));
  console.log(JSON.stringify({mode:'plan',create:items.filter(x=>x.action==='create').length,update:items.filter(x=>x.action==='update').length,unchanged:items.filter(x=>x.action==='unchanged').length,blocked:errors.length,report:out}));
  if(errors.length)throw Error('預檢未通過，未寫入。請看報告 errors。');return;
 }
 const plan=JSON.parse(fs.readFileSync(apply,'utf8'));
 if(plan.schema!=='energy-rent-plan-v1'||plan.base!==base||String(plan.org)!==org||plan.errors.length)throw Error('計畫目標或預檢不符');
 const keys=new Set();
 async function check(step){
  const hit=stations.filter(s=>s.id===step.stationId&&meter(s.customerNumber)===meter(step.customerNumber)&&s.name===step.name);if(hit.length!==1)throw Error('案場自預檢後已變動');
  const all=(await request(`stations/${step.stationId}/rent-rules`)).items;
  const same=all.filter(r=>r.rule.sourceKey===step.after.sourceKey);
  if(same.length===1&&equal(same[0].rule,step.after))return 'unchanged';
  if(step.action==='create'){if(all.length)throw Error('預檢後已有規則，請重新產生計畫');return 'create';}
  if(same.length!==1||same[0].id!==step.id||same[0].version!==step.version||!equal(same[0].rule,step.before))throw Error('規則自預檢後已變更，未覆蓋：'+step.name);
  return step.action;
 }
 for(const step of plan.items){if(keys.has(step.stationId))throw Error('重複案場計畫');keys.add(step.stationId);await check(step);}
 const report={at:new Date().toISOString(),base,org,complete:false,items:[]};fs.writeFileSync(out,JSON.stringify(report,null,2));
 for(const step of plan.items){
  const action=await check(step);
  if(action==='unchanged'){report.items.push({stationId:step.stationId,action});}
  else{
   // Do not retry a POST on an ambiguous network error. Re-plan/read back first.
   const saved=await request(`stations/${step.stationId}/rent-rules`,{...step.after,...(action==='update'?{id:step.id,expectedVersion:step.version}:{})});
   const back=(await request(`stations/${step.stationId}/rent-rules`)).items;
   if(back.filter(r=>r.rule.sourceKey===step.after.sourceKey).length!==1||!back.some(r=>r.id===saved.id&&equal(r.rule,step.after)))throw Error('寫入後讀回核對失敗：'+step.name);
   report.items.push({stationId:step.stationId,id:saved.id,version:saved.version,action,before:step.before,after:step.after});
  }
  fs.writeFileSync(out,JSON.stringify(report,null,2));
 }
 report.complete=true;fs.writeFileSync(out,JSON.stringify(report,null,2));console.log(JSON.stringify({mode:'apply',complete:true,created:report.items.filter(x=>x.action==='create').length,updated:report.items.filter(x=>x.action==='update').length,unchanged:report.items.filter(x=>x.action==='unchanged').length,report:out}));
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href)main().catch(e=>{console.error(e.message);process.exitCode=1});
