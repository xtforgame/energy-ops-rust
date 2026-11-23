import fs from 'node:fs/promises';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';

const escape = value => String(value ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
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
  return `<!doctype html><html lang="zh-Hant"><meta charset="utf-8"><title>EasyFlow 待審申請表</title><style>body{font:16px "Microsoft JhengHei",sans-serif;max-width:960px;margin:32px auto;color:#302440}h1{color:#654389}table{width:100%;border-collapse:collapse}th,td{padding:10px;border:1px solid #ddd;text-align:left}.pending{background:#fff4ce}</style><h1>EasyFlow ${application.applicationType==='invoice'?'開立發票':application.applicationType==='payment'?'付款':'類型待確認'}申請表</h1><p>待人工審核・尚未送出 EasyFlow。黃色為待補資料。</p><table>${application.fields.map(f=>`<tr class="${f.value===null?'pending':''}"><th>${escape(f.label)}</th><td>${escape(f.value??'待補資料')}</td></tr>`).join('')}</table><h2>來源附件</h2><ul>${application.attachments.map(a=>`<li>${escape(a.originalFilename)}</li>`).join('')}</ul><h2>待確認</h2><ul>${application.pending.map(p=>`<li>${escape(p.message)}</li>`).join('')}</ul></html>`;
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
      if(entry?.status==='ready'){report.push({name,status:'already_prepared',billId:entry.billId,reviewUrl:entry.reviewUrl});continue;}
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
        if(batch.items.length!==1 || batch.items[0].originalFilename!==path.basename(entry.name))throw Error('批次內容不符，請人工核對，未繼續');
        let item=batch.items[0];
        if(!item.billId && item.status==='uploaded')await request('POST',`/ingest-batches/${entry.batchId}/recognize`,{});
        const until=Date.now()+(config.waitMs??120000);
        while(!item.billId && !['failed','unclassified','discarded','duplicate'].includes(item.status) && Date.now()<until){
          await sleep(config.pollMs??2000);
          batch=await request('GET',`/ingest-batches/${entry.batchId}`);item=batch.items[0];
        }
        if(!item.billId){entry.status=item.status;await persist();report.push({name,status:item.status,batchId:entry.batchId});continue;}
        const application=await request('GET',`/bills/${item.billId}/easyflow-application`);
        const output=path.join(config.stateDir,'forms');await fs.mkdir(output,{recursive:true});
        await fs.writeFile(path.join(output,`${hash}.html`),readableForm(application));
        entry.status='ready';entry.billId=item.billId;
        entry.reviewUrl=`${base.origin}/admin/#/energyIngestConsole/${entry.batchId}?item=${item.id}&org=${config.orgId}`;
        await persist();report.push({name,status:'prepared',billId:item.billId,reviewUrl:entry.reviewUrl,html:path.join(output,`${hash}.html`)});
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
  ingestFolder({folder:get('--folder'),stateDir:get('--state'),baseUrl:get('--base-url'),orgId:Number(get('--org-id')),username:process.env.EO_INTAKE_USER,password:process.env.EO_INTAKE_PASSWORD})
    .then(r=>{console.log(JSON.stringify(r,null,2));if(r.some(x=>['error','needs_reconciliation','failed','unclassified','too_large','not_pdf'].includes(x.status)))process.exitCode=1;})
    .catch(e=>{console.error(e.message);process.exitCode=1;});
}
