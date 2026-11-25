// Import prepared rules through the platform API; credentials and real data stay outside Git.
import fs from 'node:fs';
const arg=n=>{const i=process.argv.indexOf(n);return i<0?null:process.argv[i+1]};
const base=arg('--base'),org=arg('--org'),input=arg('--package'),envPath=arg('--env-file'),out=arg('--out');
if(!base||!org||!input||!envPath||!out)throw Error('Required: --base URL --org ID --package FILE --env-file FILE --out REPORT');
const env=Object.fromEntries(fs.readFileSync(envPath,'utf8').split(/\r?\n/).filter(l=>l.includes('=')&&!l.startsWith('#')).map(l=>{const p=l.indexOf('=');return[l.slice(0,p),l.slice(p+1).replace(/^['"]|['"]$/g,'')]}));
const auth=await fetch(base+'/api/v1/auth/login',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({username:env.ADMIN_USER,password:env.ADMIN_PASSWORD})});
if(!auth.ok)throw Error('Login failed: '+auth.status);
const cookie=auth.headers.get('set-cookie')?.split(';')[0];if(!cookie)throw Error('Login cookie missing');
async function req(method,url,body){const r=await fetch(base+url,{method,headers:{cookie,'content-type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(30000)});const d=await r.json();if(!r.ok)throw Error(`${method} ${url}: ${r.status} ${JSON.stringify(d)}`);return d;}
const pack=JSON.parse(fs.readFileSync(input,'utf8'));const report={at:new Date().toISOString(),sourceSha256:pack.sourceSha256,items:[],unmatched:pack.unmatched};
if(pack.items.some(e=>e.rule.leaseEvidence?.attachmentIds?.length)) {
 if(pack.attachmentScope?.base!==base||String(pack.attachmentScope?.organizationId)!==String(org))throw Error('Lease attachment IDs belong to one installation. Upload/map attachments in this target first and supply matching attachmentScope.');
}
// Database IDs are installation-local. Resolve the target by its unique meter,
// and preflight the entire package before any writes.
const normalize=v=>String(v??'').replace(/\D/g,'');
const stations=(await req('GET',`/api/v1/orgs/${org}/energy/station-list`)).items;
const targets=pack.items.map(entry=>{
 const meter=normalize(entry.customerNumber);
 const hits=stations.filter(s=>normalize(s.customerNumber)===meter);
 if(meter.length!==11||hits.length!==1)throw Error('Target station meter is missing or ambiguous: '+entry.customerNumber);
 return {...entry,stationId:hits[0].id};
});
for(const entry of targets){
 const path=`/api/v1/orgs/${org}/energy/stations/${entry.stationId}/rent-rules`;
 const found=(await req('GET',path)).items.filter(r=>r.rule.sourceKey===entry.rule.sourceKey);
 if(found.length>1)throw Error('Ambiguous existing source key at station '+entry.stationId);
 // Repeated imports never overwrite a rule reviewed on the platform.
 if(found.length){report.items.push({stationId:entry.stationId,id:found[0].id,status:'existing-preserved'});continue;}
 const result=await req('POST',path,entry.rule);
 report.items.push({stationId:entry.stationId,id:result.id,status:'created',ruleStatus:entry.rule.status});
 fs.writeFileSync(out,JSON.stringify(report,null,2));
}
fs.writeFileSync(out,JSON.stringify(report,null,2));console.log(JSON.stringify({created:report.items.filter(r=>r.status==='created').length,preserved:report.items.filter(r=>r.status==='existing-preserved').length,unmatched:pack.unmatched.length}));
