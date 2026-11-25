// Reviewable update of existing source-derived rules. Never creates duplicate rules.
import fs from 'node:fs';
const arg=n=>{const i=process.argv.indexOf(n);return i<0?null:process.argv[i+1]};
const base=arg('--base'),org=arg('--org'),input=arg('--package'),envPath=arg('--env-file'),out=arg('--out');
if(!base||!org||!input||!envPath||!out)throw Error('Required: --base URL --org ID --package FILE --env-file FILE --out REPORT [--apply]');
const pack=JSON.parse(fs.readFileSync(input,'utf8').replace(/^\uFEFF/,''));
if(pack.items.some(e=>e.rule.leaseEvidence?.attachmentIds?.length)&&(pack.attachmentScope?.base!==base||String(pack.attachmentScope?.organizationId)!==org))throw Error('Attachment IDs require matching installation scope');
const env=Object.fromEntries(fs.readFileSync(envPath,'utf8').split(/\r?\n/).filter(l=>l.includes('=')&&!l.startsWith('#')).map(l=>{const p=l.indexOf('=');return[l.slice(0,p),l.slice(p+1).replace(/^['"]|['"]$/g,'')]}));
const auth=await fetch(base+'/api/v1/auth/login',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({username:env.ADMIN_USER,password:env.ADMIN_PASSWORD})});
if(!auth.ok)throw Error('Login failed '+auth.status);
const cookie=auth.headers.get('set-cookie')?.split(';')[0];if(!cookie)throw Error('Missing session');
async function req(method,path,body){const r=await fetch(base+path,{method,headers:{cookie,'content-type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(30000)});const d=await r.json();if(!r.ok)throw Error(`${method} ${path}: ${r.status} ${JSON.stringify(d)}`);return d;}
const canonical=v=>JSON.stringify(v,(_k,x)=>x&&typeof x==='object'&&!Array.isArray(x)?Object.fromEntries(Object.entries(x).sort(([a],[b])=>a.localeCompare(b))):x);
const norm=v=>String(v??'').replace(/\D/g,'');
const stations=(await req('GET',`/api/v1/orgs/${org}/energy/station-list`)).items;
const plan=[],keys=new Set();
for(const entry of pack.items){
 const meter=norm(entry.customerNumber),hits=stations.filter(s=>norm(s.customerNumber)===meter);
 if(meter.length!==11||hits.length!==1)throw Error('Meter not uniquely mapped '+entry.customerNumber);
 const path=`/api/v1/orgs/${org}/energy/stations/${hits[0].id}/rent-rules`;
 const matches=(await req('GET',path)).items.filter(r=>r.rule.sourceKey===entry.rule.sourceKey);
 if(matches.length!==1)throw Error('Expected exactly one existing source rule '+entry.rule.sourceKey);
 const current=matches[0];const key=`${hits[0].id}:${current.id}`;if(keys.has(key))throw Error('Duplicate update target');keys.add(key);
 if(canonical(current.rule)===canonical(entry.rule)){plan.push({stationId:hits[0].id,id:current.id,status:'unchanged'});continue;}
 if(current.version!==entry.expectedVersion)throw Error('Version changed; inspect before update '+key);
 if(current.rule.sourceSha256!==entry.rule.sourceSha256)throw Error('Source fingerprint changed; review required '+key);
 plan.push({stationId:hits[0].id,id:current.id,path,expectedVersion:current.version,before:current.rule,after:entry.rule,status:'planned'});
}
const report={at:new Date().toISOString(),applied:false,items:plan};fs.writeFileSync(out,JSON.stringify(report,null,2));
if(process.argv.includes('--apply')){
 for(const step of plan.filter(x=>x.status==='planned')){const saved=await req('POST',step.path,{...step.after,id:step.id,expectedVersion:step.expectedVersion});step.status='updated';step.newVersion=saved.version;fs.writeFileSync(out,JSON.stringify(report,null,2));}
 report.applied=true;fs.writeFileSync(out,JSON.stringify(report,null,2));
}
console.log(JSON.stringify({applied:report.applied,updated:plan.filter(x=>x.status==='updated').length,planned:plan.filter(x=>x.status==='planned').length,unchanged:plan.filter(x=>x.status==='unchanged').length}));
