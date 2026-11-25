export type InvoiceTiming = 'unknown' | 'before' | 'later';
type Source = {source:string;attachmentId:number;page:number;sourceRow?:string;billId?:number};
type Draft = {sourceContributions?:Partial<Source>[];application?:{voucherAttachmentIds?:number[];state?:string;draftRevision?:number};grossRent:string|null;issues:string[];lines:Source[];inputMethod?:string;evidenceStatus?:string;leaseEvidence?:{status:string;attachmentIds:number[]}};
export function rentDocumentChecklist(r:Draft, invoiceTiming:InvoiceTiming) {
 const refs=(source:string)=>{const rows=r.lines.filter(l=>l.source===source);return rows.length?rows:(r.sourceContributions??[]).filter(l=>l.source===source&&l.attachmentId).map(l=>({source,attachmentId:l.attachmentId!,page:l.page??0,billId:l.billId}));};
 const sourceStatus=(source:string,empty:string)=>{
  const rows=refs(source);if(!rows.length)return empty;
  const originals=rows.filter(l=>!!l.billId&&l.attachmentId>0).length;
  const others=rows.length-originals;
  if(originals)return `已連結 ${originals} 筆原始電費單附件${others?`；另有 ${others} 筆來源待核對`:''}；請核對期間與內容`;
  return `已引用 ${rows.length} 筆${rows.some(l=>l.sourceRow)?'試算表紀錄，尚未逐張核對原單':'來源頁；請核對期間'}`;
 };
 return [
  {key:'calculation',label:'租金計算表',status:r.grossRent!=null&&!r.issues.length?'已產出試算預覽（未核准）':'來源或計算尚待確認',references:[] as Source[]},
  {key:'contract',label:'合約租金頁面截圖',status:r.evidenceStatus==='complete'?'完整合約已備（請核對附件）':r.evidenceStatus==='excerpt'?'已有租金頁摘錄（完整合約待補）':'待附合約租金頁面',references:(r.leaseEvidence?.attachmentIds??[]).map(attachmentId=>({source:'lease',attachmentId,page:0}))},
  {key:'taipower',label:'台電電費單',status:sourceStatus('taipower','尚未引用台電來源'),references:refs('taipower')},
  {key:'transfer',label:'轉供電費單',status:sourceStatus('transfer','尚未引用；待確認是否適用'),references:refs('transfer')},
  {key:'invoice',label:'地主發票／收據',status:(r.application?.voucherAttachmentIds?.length??0)>0?`已連結 ${r.application!.voucherAttachmentIds!.length} 份憑證（請核對內容）`:invoiceTiming==='later'?'發票待後補（尚未收到）':invoiceTiming==='before'?'申請前需附發票（尚未確認收件）':'先附／後補待確認',references:(r.application?.voucherAttachmentIds??[]).map(attachmentId=>({source:'voucher',attachmentId,page:0}))},
 ];
}
export const escapeRentHtml=(s:unknown)=>String(s??'待確認').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]!));
export function rentDocumentChecklistHtml(r:Draft, timing:InvoiceTiming):string {
 return '<h2>申請附件清單</h2><p>以下為準備清單，未送出申請。單獨 HTML 不內嵌附件；ZIP 是否包含檔案請看包內清單。'+((r.application?.draftRevision??0)>0?'申請資料與憑證安排已保存於平台；':'目前為自動預覽，申請草稿尚未保存；')+'後補期限尚待確認，未自動啟用提醒。</p><table><tr><th>文件</th><th>目前狀態</th><th>來源附件／頁碼</th></tr>'+rentDocumentChecklist(r,timing).map(x=>`<tr><td>${escapeRentHtml(x.label)}</td><td>${escapeRentHtml(x.status)}</td><td>${x.references.map(l=>`${escapeRentHtml(l.attachmentId)}／${escapeRentHtml(l.page||'未指定頁碼')}`).join('、')||'—'}</td></tr>`).join('')+'</table>';
}
