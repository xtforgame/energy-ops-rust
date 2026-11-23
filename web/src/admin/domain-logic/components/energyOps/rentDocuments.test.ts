import { describe,it,expect } from 'vitest';
import { rentDocumentChecklist, rentDocumentChecklistHtml } from './rentDocuments';
describe('rent application documents',()=>{
 it('exports the selected follow-up state and explicitly excludes submission',()=>{
  const html=rentDocumentChecklistHtml({grossRent:'12982',issues:[],lines:[{source:'taipower',attachmentId:3,page:2}]},'later');
  expect(html).toContain('發票待後補（尚未收到）');expect(html).toContain('3／2');expect(html).toContain('未送出申請');
 });
 it('does not pretend a formula reference is a contract attachment',()=>{
  const rows=rentDocumentChecklist({grossRent:'12982',issues:[],lines:[{source:'taipower',attachmentId:3,page:2},{source:'transfer',attachmentId:3,page:3}]},'unknown');
  expect(rows.find(x=>x.key==='contract')?.status).toBe('待附合約租金頁面');
  expect(rows.find(x=>x.key==='invoice')?.status).toBe('先附／後補待確認');
  expect(rows.find(x=>x.key==='transfer')?.status).toContain('1 筆');
 });
 it('marks deferred invoice as pending, never received',()=>{
  expect(rentDocumentChecklist({grossRent:'0',issues:[],lines:[]},'later').find(x=>x.key==='invoice')?.status).toBe('發票待後補（尚未收到）');
 });
 it('retains missing transfer ambiguity and failed calculation',()=>{
  const rows=rentDocumentChecklist({grossRent:null,issues:['缺單'],lines:[]},'before');
  expect(rows[0].status).toBe('來源或計算尚待確認');
  expect(rows.find(x=>x.key==='transfer')?.status).toContain('待確認是否適用');
  expect(rows.find(x=>x.key==='invoice')?.status).toBe('申請前需附發票（尚未確認收件）');
 });
});
