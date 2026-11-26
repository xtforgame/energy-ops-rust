import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {ingestFolder,readableForm} from './ingest-folder.mjs';

test('split PDF exports every bill and one rental cycle, never auto-approves',async t=>{
 const c=await setup(t),s=server();
 const fetch=async(url,o)=>{
  const p=new URL(url).pathname;
  if(p.endsWith('/rent-payment'))return new Response(JSON.stringify({items:[{stationId:70,stationName:'麻豆',ruleId:16,periodStart:'2026-10-01',periodEnd:'2026-10-31',grossRent:'12982',payee:'地主',lines:[],application:{fields:{companyName:'京承'},missingFields:['applicant']}}]}));
  const r=await s.fetch(url,o);
  if(p.endsWith('/ingest-batches/8')){const b=await r.json();if(b.items[0]?.billId)b.items=[{id:9,originalFilename:'bill.pdf',attachmentId:9,status:'classified'},... [10,11].map(id=>({id,billId:id,attachmentId:9,originalFilename:`bill-第${id}頁.pdf`,status:'extracted'}))];return new Response(JSON.stringify(b));}return r;
 };
 const [r]=await ingestFolder(c,fetch);
 assert.equal(r.status,'prepared',JSON.stringify(r));assert.equal(r.bills.length,2);assert.equal(r.rentForms.length,1);
 assert.match(await fs.readFile(r.rentForms[0].html,'utf8'),/12982/);
 assert.ok(r.rentForms[0].missing.includes('尚缺申請人'));
 const refreshed=await ingestFolder({...c,refreshForms:true},fetch);assert.equal(refreshed[0].status,'prepared');
 assert.equal(s.calls.filter(([m,p])=>m==='POST'&&p.endsWith('/attachments')).length,1);
 assert.ok(!s.calls.some(([,p])=>/\/submit$|\/actions$/.test(p)));
});

// stableMs=0 讓 ingest-folder.mjs:50 的 `Date.now()-stat.mtimeMs<stableMs` 幾乎必然跟時脈打平手；
// mtimeMs 帶小數、Date.now() 是整數毫秒，同一毫秒內寫完就讀會得到負的差值，負數<0成立，誤判成 still_downloading。
// 把檔案的修改時間撥回 60 秒前，讓「已經下載完」這個前提跟系統時脈無關，不必改 ingest-folder.mjs。
async function settle(file){const past=new Date(Date.now()-60000);await fs.utimes(file,past,past);}
async function setup(t){const root=await fs.mkdtemp(path.join(os.tmpdir(),'energy-mail-test-'));t.after(()=>fs.rm(root,{recursive:true,force:true}));const folder=path.join(root,'ready');await fs.mkdir(folder);const billPath=path.join(folder,'bill.pdf');await fs.writeFile(billPath,'%PDF-1.4\ntest');await settle(billPath);return{folder,stateDir:path.join(root,'state'),baseUrl:'http://127.0.0.1:9999',orgId:7,username:'test',password:'test',stableMs:0,pollMs:0,waitMs:100};}
function server({lostCreate=false,lostUpload=false}={}){const calls=[];let uploaded=false,recognized=false;const fetch=async(url,o)=>{const p=new URL(url).pathname;calls.push([o.method,p]);const response=v=>new Response(JSON.stringify(v),{status:200,headers:{'set-cookie':'test_session=synthetic; HttpOnly'}});if(p.endsWith('/login'))return response({});if(o.method==='POST'&&p.endsWith('/ingest-batches')){if(lostCreate)throw Error('lost response');return response({id:8});}if(p.endsWith('/attachments')){uploaded=true;if(lostUpload)throw Error('lost upload response');return response({id:9});}if(p.endsWith('/recognize')){recognized=true;return response({configured:true});}if(p.endsWith('/ingest-batches/8'))return response({items:uploaded?[{id:9,originalFilename:'bill.pdf',status:recognized?'extracted':'uploaded',billId:recognized?10:null}]:[]});if(p.endsWith('/rent-payment'))return response({items:p.includes('/stations/')?[{stationName:'測試租金',grossRent:'100',lines:[],issues:[]}]:[]});if(p.endsWith('/easyflow-application'))return response({applicationType:'invoice',fields:[{key:'siteName',label:'案場',value:'測試案場'}],attachments:[],pending:[]});throw Error('unexpected '+p);};return{fetch,calls};}
test('new PDF produces a review form; renamed or copied content does not upload again',async t=>{const c=await setup(t),s=server();const first=await ingestFolder(c,s.fetch);assert.equal(first[0].status,'prepared');assert.match(await fs.readFile(first[0].html,'utf8'),/測試案場/);const copyPath=path.join(c.folder,'copy.pdf');await fs.copyFile(path.join(c.folder,'bill.pdf'),copyPath);await settle(copyPath);const second=await ingestFolder(c,s.fetch);assert.ok(second.every(x=>x.status==='already_prepared'));assert.equal(s.calls.filter(([m,p])=>m==='POST'&&p.endsWith('/attachments')).length,1);assert.ok(!s.calls.some(([,p])=>p.endsWith('/submit')));});
test('ambiguous batch creation is not retried blindly',async t=>{const c=await setup(t),s=server({lostCreate:true});assert.equal((await ingestFolder(c,s.fetch))[0].status,'error');assert.equal((await ingestFolder(c,s.fetch))[0].status,'needs_reconciliation');assert.equal(s.calls.filter(([m,p])=>m==='POST'&&p.endsWith('/ingest-batches')).length,1);});
test('lost upload reply reconciles existing batch without duplicate upload',async t=>{const c=await setup(t),s=server({lostUpload:true});assert.equal((await ingestFolder(c,s.fetch))[0].status,'error');assert.equal((await ingestFolder(c,s.fetch))[0].status,'prepared');assert.equal(s.calls.filter(([m,p])=>m==='POST'&&p.endsWith('/attachments')).length,1);});
test('target scope and active lock cannot be bypassed; output escapes untrusted OCR text',async t=>{const c=await setup(t),s=server();await ingestFolder(c,s.fetch);await assert.rejects(ingestFolder({...c,orgId:8},s.fetch),/其他平台或組織/);await fs.writeFile(path.join(c.stateDir,'scan.lock'),'');await assert.rejects(ingestFolder(c,s.fetch),/已有匯入程序/);const html=readableForm({fields:[{label:'x',value:'<script>alert(1)</script>'}],attachments:[],pending:[]});assert.ok(!html.includes('<script>'));});

test('rental worksheet stops polling, exposes preview, and is deduplicated on the next run',async t=>{
 const c=await setup(t),s=server();let reads=0;
 const fetch=async(url,o)=>{
  const response=await s.fetch(url,o);
  if(new URL(url).pathname.endsWith('/ingest-batches/8')){
   const body=await response.json();reads++;
   if(body.items?.[0]?.billId){body.items[0]={id:9,originalFilename:'bill.pdf',status:'classified',rentalWorksheet:true,rentPayment:{stationId:91}};}
   return new Response(JSON.stringify(body),{status:200});
  }
  return response;
 };
 const first=await ingestFolder(c,fetch);assert.equal(first[0].status,'rental_prepared');assert.match(first[0].reviewUrl,/item=9/);assert.ok(reads<5);
 const second=await ingestFolder(c,fetch);assert.equal(second[0].status,'already_prepared');assert.equal(s.calls.filter(([m,p])=>m==='POST'&&p.endsWith('/attachments')).length,1);
 assert.ok(!s.calls.some(([,p])=>p.endsWith('/easyflow-application')||p.endsWith('/submit')));
});


test('partial split preserves unfinished children and resumes without re-upload',async t=>{
 const c=await setup(t),s=server();let done=false;
 const fetch=async(url,o)=>{const r=await s.fetch(url,o);if(new URL(url).pathname.endsWith('/ingest-batches/8')){const b=await r.json();if(b.items[0]?.billId)b.items.push({id:12,originalFilename:'bill-part.pdf',status:done?'extracted':'recognizing',billId:done?12:null});return new Response(JSON.stringify(b));}return r;};
 const first=await ingestFolder(c,fetch);assert.equal(first[0].status,'partial');assert.equal(first[0].unresolved.length,1);
 done=true;const next=await ingestFolder(c,fetch);assert.equal(next[0].status,'prepared');assert.equal(next[0].bills.length,2);
 assert.equal(s.calls.filter(([m,p])=>m==='POST'&&p.endsWith('/attachments')).length,1);
});
