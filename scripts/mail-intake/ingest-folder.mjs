import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';
import {paymentFormHtml,rentApplicationHtml,rentPreviewFields,rentCalculationDescription,rentDocumentChecklistHtml} from './form-fields.generated.mjs';

const escape = value => String(value ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const fieldNames={applicant:'申請人',department:'申請部門',paymentDate:'指定付款日',companyCode:'公司代碼',companyName:'公司名稱'};
function rentGaps(r){return [...new Set([...(r.issues??[]),...(r.sourceBlockers??[]),...(r.requirements??[]),...(r.warnings??[]),...(r.application?.missingFields??[]).map(k=>`尚缺${fieldNames[k]??k}`)])];}
async function files(root) {
  const result = [];
  for (const entry of await fs.readdir(root, { withFileTypes:true })) {
    const name = path.join(root, entry.name);
    if (entry.isDirectory()) result.push(...await files(name));
    else if (entry.isFile() && /\.pdf$/i.test(entry.name)) result.push(name);
  }
  return result.sort();
}
export function readableForm(application) {
  const main=paymentFormHtml(application);
  return `<!doctype html><html lang="zh-Hant"><meta charset="utf-8"><title>EasyFlow 待審申請表</title><style>body{font:16px "Microsoft JhengHei",sans-serif;max-width:960px;margin:32px auto;color:#302440}h1{color:#654389}table{width:100%;border-collapse:collapse}th,td{padding:10px;border:1px solid #ddd;text-align:left}.pending{background:#fff4ce}</style><h1>EasyFlow ${application.applicationType==='invoice'?'開立發票':application.applicationType==='payment'?'付款':'類型待確認'}申請表</h1><p>待人工審核・尚未送出 EasyFlow。黃色為待補資料。</p>${main}<table>${application.fields.map(f=>`<tr class="${f.value===null?'pending':''}"><th>${escape(f.label)}</th><td>${escape(f.value??'待補資料')}</td></tr>`).join('')}</table><h2>來源附件</h2><ul>${application.attachments.map(a=>`<li>${escape(a.originalFilename)}</li>`).join('')}</ul><h2>待確認</h2><ul>${application.pending.map(p=>`<li>${escape(p.message)}</li>`).join('')}</ul></html>`;
}

export function readableRent(r){
 r={issues:[],lines:[],...r};
 const f={...r.application?.fields,amount:r.grossRent,subtotal:r.grossRent,payee:r.payee,currency:'TWD',expenseDescription:rentCalculationDescription(r),paymentDeadline:r.paymentDeadline};
 return `<!doctype html><meta charset="utf-8"><title>租金付款預覽</title><style>body{font:16px/1.7 sans-serif;max-width:1000px;margin:40px auto;color:#34243e}table{border-collapse:collapse;width:100%}td,th{border:1px solid #ddd;padding:10px;text-align:left}</style><h1>${escape(r.stationName)}｜租金付款預覽</h1><p>尚未送出EasyFlow；請至平台核對最新資料與審核。</p>${rentApplicationHtml({...r.application,fields:f})}<table>${rentPreviewFields(r).map(([k,v])=>`<tr><th>${escape(k)}</th><td>${escape(v??'待補資料')}</td></tr>`).join('')}</table>${rentDocumentChecklistHtml(r,r.invoiceTiming??'unknown')}<h2>尚缺資料</h2><ul>${rentGaps(r).map(s=>`<li>${escape(s)}</li>`).join('')}</ul><h2>規則資料備註</h2><ul>${(r.notes??[]).map(s=>`<li>${escape(s)}</li>`).join('')}</ul>`;
}

/** One bounded scan. Reuses persisted batch IDs; never submits/approves or changes source files. */
export async function ingestFolder(config, fetcher = fetch) {
  const base = new URL(config.baseUrl);
  if (base.protocol!=='https:' && !(base.protocol==='http:'&&['127.0.0.1','localhost','[::1]'].includes(base.hostname))) throw Error('請使用 HTTPS；只有本機允許 HTTP');
  if (base.username || base.password || base.search || base.hash || base.pathname!=='/') throw Error('base-url 必須是平台根網址');
  if (!Number.isSafeInteger(config.orgId)||config.orgId<1) throw Error('org-id 必須指定正確組織');
  if (!config.username||!config.password) throw Error('請設定 EO_INTAKE_USER 與 EO_INTAKE_PASSWORD');
  await fs.mkdir(config.stateDir,{recursive:true});
  const lockPath=path.join(config.stateDir,'scan.lock');
  const lock=await fs.open(lockPath,'wx').catch(()=>{throw Error('已有匯入程序或未清理的 scan.lock；先確認沒有其他程序執行');});
  await lock.writeFile(JSON.stringify({pid:process.pid,host:os.hostname(),startedAt:new Date().toISOString(),target:base.origin,orgId:config.orgId}));
  try {
    const statePath=path.join(config.stateDir,'state.json');
    let state;
    try { state=JSON.parse(await fs.readFile(statePath,'utf8')); }
    catch(e) { if(e.code==='ENOENT')state={version:1,target:`${base.origin}/org/${config.orgId}`,files:{}};else throw e; }
    if(state.target!==`${base.origin}/org/${config.orgId}`)throw Error('此狀態資料屬於其他平台或組織，請使用另一個 state 目錄');
    const persist=async()=>{await fs.writeFile(statePath+'.tmp',JSON.stringify(state,null,2));await fs.rename(statePath+'.tmp',statePath);};
    const login=await fetcher(base.origin+'/api/v1/auth/login',{method:'POST',redirect:'error',headers:{'content-type':'application/json'},body:JSON.stringify({username:config.username,password:config.password}),signal:AbortSignal.timeout(30000)});
    if(!login.ok)throw Error(`平台登入失敗：${login.status}`);
    const cookie=login.headers.get('set-cookie')?.split(';')[0];if(!cookie)throw Error('登入沒有取得 session');
    const prefix=`/api/v1/orgs/${config.orgId}/energy`;
    const request=async(method,url,body)=>{
      const r=await fetcher(base.origin+prefix+url,{method,redirect:'error',headers:{cookie,...(body instanceof FormData?{}:{'content-type':'application/json'})},body:body===undefined?undefined:body instanceof FormData?body:JSON.stringify(body),signal:AbortSignal.timeout(60000)});
      if(!r.ok)throw Error(`平台 ${method} ${url} 回應 ${r.status}`);
      return r.json();
    };
    const report=[];
    for(const name of await files(config.folder)) {
      const stat=await fs.stat(name);
      if(Date.now()-stat.mtimeMs<(config.stableMs??30000)){report.push({name,status:'still_downloading'});continue;}
      if(stat.size>100*1024*1024){report.push({name,status:'too_large'});continue;}
      const bytes=await fs.readFile(name),hash=createHash('sha256').update(bytes).digest('hex');
      const afterRead=await fs.stat(name);
      if(afterRead.size!==stat.size||afterRead.mtimeMs!==stat.mtimeMs){report.push({name,status:'still_downloading'});continue;}
      if(!bytes.subarray(0,1024).includes(Buffer.from('%PDF-'))){report.push({name,status:'not_pdf'});continue;}
      let entry=state.files[hash];
      if(entry?.status==='ready'&&entry.formsVersion===2&&!config.refreshForms){report.push({name,status:'already_prepared',billId:entry.billId,reviewUrl:entry.reviewUrl,bills:entry.bills,rentForms:entry.rentForms});continue;}
      if(entry?.status==='creating'){report.push({name,status:'needs_reconciliation',reason:'上次建立批次的回應不明，請核對平台後處理，避免重複上傳'});continue;}
      try {
        if(!entry){
          entry=state.files[hash]={name,hash,status:'creating'};await persist();
          const batch=await request('POST','/ingest-batches',{fileCount:1,note:'收信資料夾自動匯入（待人工審核）'});
          entry.batchId=batch.id;entry.status='created';await persist();
        }
        let batch=await request('GET',`/ingest-batches/${entry.batchId}`);
        if(!batch.items?.length){
          if(entry.status==='uploading')throw Error('上次上傳回應不明，請核對平台批次後再重試');
          entry.status='uploading';await persist();
          const form=new FormData();form.append('file',new Blob([bytes],{type:'application/pdf'}),path.basename(name));
          await request('POST',`/ingest-batches/${entry.batchId}/attachments`,form);
          entry.status='uploaded';await persist();
          batch=await request('GET',`/ingest-batches/${entry.batchId}`);
        }
        const original=batch.items.find(i=>i.originalFilename===path.basename(entry.name));
        if(!original || batch.items.some(i=>i.attachmentId && original.attachmentId && i.attachmentId!==original.attachmentId))throw Error('批次內容不符，請人工核對，未繼續');
        let item=original;
        if(!item.billId && item.status==='uploaded')await request('POST',`/ingest-batches/${entry.batchId}/recognize`,{});
        const until=Date.now()+(config.waitMs??120000);
        const waiting=()=>batch.items.some(i=>['uploaded','recognizing'].includes(i.status));
        while(waiting() && Date.now()<until){
          await sleep(config.pollMs??2000);
          batch=await request('GET',`/ingest-batches/${entry.batchId}`);item=batch.items[0];
        }
        const billItems=batch.items.filter(i=>i.billId);
        if(!billItems.length && item.rentalWorksheet){
          entry.status=item.rentPayment?'ready':'needs_rule';entry.formsVersion=2;
          entry.reviewUrl=`${base.origin}/admin/#/energyIngestConsole/${entry.batchId}?item=${item.id}&org=${config.orgId}`;
          const rentForms=[];
          if(item.rentPayment){
            const rent=await request('GET',`/stations/${item.rentPayment.stationId}/rent-payment`);
            const output=path.join(config.stateDir,'forms');await fs.mkdir(output,{recursive:true});
            for(const [n,r] of (rent.items??[]).entries()){
              const html=path.join(output,`${hash}-worksheet-${n}.html`);await fs.writeFile(html,readableRent(r));
              rentForms.push({stationName:r.stationName,periodStart:r.periodStart,periodEnd:r.periodEnd,html,amount:r.grossRent??null});
            }
            if(!rentForms.length)entry.status='needs_rule';
          }
          entry.rentForms=rentForms;await persist();report.push({name,status:entry.status==='ready'?'rental_prepared':'needs_rule',batchId:entry.batchId,reviewUrl:entry.reviewUrl,rentForms,message:item.classifyReason});continue;
        }
        if(!billItems.length){entry.status=item.status;await persist();report.push({name,status:item.status,batchId:entry.batchId});continue;}
        const output=path.join(config.stateDir,'forms');await fs.mkdir(output,{recursive:true});
        const bills=[],rentForms=[],seen=new Set(),missing=[];
        for(const billItem of billItems){
          const application=await request('GET',`/bills/${billItem.billId}/easyflow-application`);
          const html=path.join(output,`${hash}-bill-${billItem.billId}.html`);
          await fs.writeFile(html,readableForm(application));
          const reviewUrl=`${base.origin}/admin/#/energyIngestConsole/${entry.batchId}?item=${billItem.id}&org=${config.orgId}`;
          bills.push({billId:billItem.billId,html,reviewUrl});
          const rent=await request('GET',`/bills/${billItem.billId}/rent-payment`);
          const station=application.fields.find(f=>f.key==='siteName')?.value??'未對應案場';
          if(!rent.items?.length)missing.push({station,billId:billItem.billId,missing:['尚無對應租金規則或期間資料'],reviewUrl});
          for(const r of rent.items??[]){
            const key=JSON.stringify([r.stationId,r.ruleId,r.periodStart,r.periodEnd]);if(seen.has(key))continue;seen.add(key);
            const file=path.join(output,`${hash}-rent-${createHash('sha256').update(key).digest('hex').slice(0,16)}.html`);
            await fs.writeFile(file,readableRent(r));
            const gaps=rentGaps(r);
            rentForms.push({stationId:r.stationId,stationName:r.stationName,periodStart:r.periodStart,periodEnd:r.periodEnd,amount:r.grossRent??null,html:file,missing:gaps,reviewState:r.application?.state??'draft'});
            if(gaps.length)missing.push({station:r.stationName,periodStart:r.periodStart,periodEnd:r.periodEnd,missing:gaps,reviewUrl});
          }
        }
        const unresolved=batch.items.filter(i=>!i.billId&&['failed','unclassified','uploaded','recognizing'].includes(i.status));
        entry.status=unresolved.length?'partial':'ready';entry.formsVersion=2;entry.billId=bills[0].billId;entry.bills=bills;entry.rentForms=rentForms;entry.reviewUrl=bills[0].reviewUrl;
        await fs.writeFile(path.join(output,`${hash}-missing.json`),JSON.stringify({source:name,missing,unresolved},null,2));
        await persist();report.push({name,status:unresolved.length?'partial':'prepared',batchId:entry.batchId,billId:entry.billId,reviewUrl:entry.reviewUrl,html:bills[0].html,bills,rentForms,missing,unresolved});
      } catch(e) {report.push({name,status:'error',batchId:entry?.batchId,message:e.message});}
    }
    await fs.writeFile(path.join(config.stateDir,'last-run.json'),JSON.stringify({at:new Date().toISOString(),report},null,2));
    return report;
  } finally {await lock.close();await fs.unlink(lockPath);}
}

if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){
  const args=process.argv.slice(2),get=k=>args[args.indexOf(k)+1];
  const required=['--folder','--state','--base-url','--org-id'];
  if(required.some(k=>!args.includes(k)))throw Error('用法：node ingest-folder.mjs --folder <ready目錄> --state <狀態目錄> --base-url <平台網址> --org-id <組織ID>');
  ingestFolder({folder:get('--folder'),stateDir:get('--state'),baseUrl:get('--base-url'),orgId:Number(get('--org-id')),refreshForms:args.includes('--refresh-forms'),username:process.env.EO_INTAKE_USER,password:process.env.EO_INTAKE_PASSWORD})
    .then(r=>{console.log(JSON.stringify(r,null,2));if(r.some(x=>['error','needs_reconciliation','failed','unclassified','too_large','not_pdf','needs_rule','partial'].includes(x.status)))process.exitCode=1;})
    .catch(e=>{console.error(e.message);process.exitCode=1;});
}
