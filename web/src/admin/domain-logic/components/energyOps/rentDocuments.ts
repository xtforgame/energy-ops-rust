export type InvoiceTiming = 'unknown' | 'before' | 'later';
type Source = {source:string;attachmentId:number;page:number;sourceRow?:string};
type Draft = {grossRent:string|null;issues:string[];lines:Source[]};
export function rentDocumentChecklist(r:Draft, invoiceTiming:InvoiceTiming) {
 const refs=(source:string)=>r.lines.filter(l=>l.source===source);
 return [
  {key:'calculation',label:'租金計算表',status:r.grossRent!=null&&!r.issues.length?'已產出試算預覽（未核准）':'來源或計算尚待確認',references:[] as Source[]},
  {key:'contract',label:'合約租金頁面截圖',status:'待附合約租金頁面',references:[] as Source[]},
  {key:'taipower',label:'台電電費單',status:refs('taipower').length?`已引用 ${refs('taipower').length} 筆${refs('taipower').some(l=>l.sourceRow)?'試算表紀錄，尚未逐張核對原單':'來源頁；請核對期間'}`:'尚未引用台電來源',references:refs('taipower')},
  {key:'transfer',label:'轉供電費單',status:refs('transfer').length?`已引用 ${refs('transfer').length} 筆${refs('transfer').some(l=>l.sourceRow)?'試算表紀錄，尚未逐張核對原單':'來源頁；請核對期間'}`:'尚未引用；待確認是否適用',references:refs('transfer')},
  {key:'invoice',label:'地主發票／收據',status:invoiceTiming==='later'?'發票待後補（尚未收到）':invoiceTiming==='before'?'申請前需附發票（尚未確認收件）':'先附／後補待確認',references:[] as Source[]},
 ];
}
export const escapeRentHtml=(s:unknown)=>String(s??'待確認').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]!));
export function rentDocumentChecklistHtml(r:Draft, timing:InvoiceTiming):string {
 return '<h2>申請附件清單</h2><p>以下為準備清單，未打包附件、未送出申請。憑證安排保存於平台；後補期限尚待確認，未自動啟用提醒。</p><table><tr><th>文件</th><th>目前狀態</th><th>來源附件／頁碼</th></tr>'+rentDocumentChecklist(r,timing).map(x=>`<tr><td>${escapeRentHtml(x.label)}</td><td>${escapeRentHtml(x.status)}</td><td>${x.references.map(l=>`${escapeRentHtml(l.attachmentId)}／${escapeRentHtml(l.page)}`).join('、')||'—'}</td></tr>`).join('')+'</table>';
}
