import {test} from 'node:test';
import assert from 'node:assert/strict';
import {planEntry,main} from './import-rent-package.mjs';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
const stations=[{id:999,name:'示例案場',customerNumber:'01-23-4567-89-0'}];
const before={sourceKey:'asset-rent:demo',sourceSha256:'hash',status:'pending',percent:'8',basis:null};
const after={...before,basis:'gross',sourceOwnerEvidence:[{name:'示例地主'}]};
const entry={customerNumber:'01-23-4567-89-0',canonicalName:'示例案場',before,after};
test('uses target IDs and preserves target attachments and unrelated edits',()=>{
 const current={...before,cycleAnchor:'2026-01-01',leaseEvidence:{status:'complete',attachmentIds:[777]}};
 const p=planEntry(entry,stations,[{id:888,version:19,rule:current}]);assert.equal(p.id,888);assert.equal(p.version,19);assert.equal(p.after.cycleAnchor,'2026-01-01');assert.deepEqual(p.after.leaseEvidence,current.leaseEvidence);assert.equal(p.after.basis,'gross');
});
test('same package rerun does not create or update',()=>{
 const p=planEntry(entry,stations,[{id:888,version:20,rule:{...after,leaseEvidence:{status:'missing',attachmentIds:[]}}}]);assert.equal(p.action,'unchanged');
});
test('creates only when station has no rules; never imports attachment IDs',()=>{
 const p=planEntry(entry,stations,[]);assert.equal(p.action,'create');assert.deepEqual(p.after.leaseEvidence,{status:'missing',attachmentIds:[]});
});
test('conflicting edits are not overwritten',()=>assert.throws(()=>planEntry(entry,stations,[{id:888,version:2,rule:{...before,basis:'untaxed'}}]),/目標已有不同修改/));
test('other rule or duplicate source key blocks creation',()=>{
 assert.throws(()=>planEntry(entry,stations,[{id:1,rule:{...before,sourceKey:'different'}}]),/已有其他來源規則/);
 assert.throws(()=>planEntry(entry,stations,[{id:1,rule:before},{id:2,rule:before}]),/已有重複規則/);
});
test('missing/ambiguous meter or wrong station name blocks import',()=>{
 assert.throws(()=>planEntry(entry,[],[]),/未唯一對應/);assert.throws(()=>planEntry(entry,[...stations,...stations],[]),/未唯一對應/);assert.throws(()=>planEntry(entry,[{...stations[0],name:'另一案場'}],[]),/案名/);
});
test('source fingerprint mismatch blocks update',()=>assert.throws(()=>planEntry(entry,stations,[{id:1,version:2,rule:{...before,sourceSha256:'other'}}]),/來源版本不同/));
test('group references do not import or compare local station IDs',()=>{
 const e={...entry,after:{...after,groupMembers:[{name:'第二案場'}]}};
 const current={...e.after,groupMembers:[{name:'第二案場',stationId:7000}]};
 assert.equal(planEntry(e,stations,[{id:888,version:2,rule:current}]).action,'unchanged');
 assert(!JSON.stringify(planEntry(e,stations,[]).after).includes('stationId'));
});
test('HTTP plan/apply/readback/repeat updates once and creates once',async()=>{
 const tmp=fs.mkdtempSync(path.join(os.tmpdir(),'rent-import-'));
 const second={id:1000,name:'第二案場',customerNumber:'11-23-4567-89-0'};
 const stored={999:[{id:888,version:7,rule:{...before,leaseEvidence:{status:'complete',attachmentIds:[777]}}}],1000:[]};
 let writes=0;
 const server=http.createServer(async(req,res)=>{
  res.setHeader('content-type','application/json');
  if(req.url==='/api/v1/auth/login'){res.setHeader('set-cookie','test=session; Path=/');res.end('{}');return;}
  if(req.url.endsWith('/station-list')){res.end(JSON.stringify({items:[...stations,second]}));return;}
  const match=req.url.match(/stations\/(\d+)\/rent-rules$/);if(!match){res.statusCode=404;res.end('{}');return;}
  const sid=Number(match[1]);
  if(req.method==='GET'){res.end(JSON.stringify({items:stored[sid]}));return;}
  let text='';for await(const chunk of req)text+=chunk;
  const body=JSON.parse(text);const id=body.id,version=body.expectedVersion;delete body.id;delete body.expectedVersion;
  if(id){const existing=stored[sid].find(x=>x.id===id);assert.equal(existing.version,version);existing.version++;existing.rule=body;res.end(JSON.stringify(existing));}
  else {const added={id:901,version:1,rule:body};stored[sid].push(added);res.end(JSON.stringify(added));}writes++;
 });
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
 try{
  fs.writeFileSync(path.join(tmp,'auth.env'),'ADMIN_USER=test\nADMIN_PASSWORD=test\n');
  fs.writeFileSync(path.join(tmp,'pack.json'),JSON.stringify({schema:'energy-rent-portable-v1',items:[entry,{...entry,customerNumber:second.customerNumber,canonicalName:second.name}]}));
  const args=['--base',`http://127.0.0.1:${server.address().port}`,'--org','7','--env-file',path.join(tmp,'auth.env')];
  const plan=path.join(tmp,'plan.json');
  await main([...args,'--package',path.join(tmp,'pack.json'),'--out',plan]);assert.equal(writes,0);
  await main([...args,'--apply-plan',plan,'--out',path.join(tmp,'apply.json')]);assert.equal(writes,2);
  await main([...args,'--apply-plan',plan,'--out',path.join(tmp,'retry.json')]);assert.equal(writes,2);
  await main([...args,'--package',path.join(tmp,'pack.json'),'--out',path.join(tmp,'recheck.json')]);
  assert(JSON.parse(fs.readFileSync(path.join(tmp,'recheck.json'))).items.every(x=>x.action==='unchanged'));
  assert.equal(stored[1000].length,1);assert.deepEqual(stored[999][0].rule.leaseEvidence.attachmentIds,[777]);
 }finally{await new Promise(resolve=>server.close(resolve));fs.rmSync(tmp,{recursive:true,force:true});}
});
