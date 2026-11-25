export type RentalApplication={fields:Record<string,string|null>;state?:string;stale?:boolean;missingFields?:string[];sourceRevision?:string;draftRevision?:number;canReview?:boolean;canConfirm?:boolean;confirmedAt?:string;confirmedBy?:number;invoiceTiming?:'unknown'|'before'|'later';voucherAttachmentIds?:number[]};
export const rentalLabels:Record<string,string>={companyName:'公司名稱',companyCode:'公司代碼',department:'申請部門',applicant:'申請人',applicationDate:'申請日期',summary:'申請摘要',expenseDescription:'費用說明',payee:'付款對象',amount:'申請金額',currency:'幣別',contractOption:'合約／簽呈',paymentMethod:'付款方式',paymentDate:'安排付款日',paymentDeadline:'合約繳費期限',voucherStatus:'憑證說明'};
export const rentalEditable=['companyCode','department','applicant','applicationDate','summary','paymentMethod','paymentDate','contractOption','voucherStatus'];
export const rentApplicationRows=(a:RentalApplication):[string,string][]=>Object.entries(rentalLabels).map(([k,label])=>[label,a.fields[k]??'']);
const esc=(v:unknown)=>String(v??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]!));
export function rentApplicationHtml(a:RentalApplication):string{
 const state=a.stale?'資料更新後待重新確認':a.state==='confirmed_for_handoff'?'內容已確認，待交接':a.state==='dataConfirmed'?'資料已覆核，尚未送出':'草稿待覆核';
 return `<h2>EasyFlow 租金付款申請單</h2><p>${state}；尚未送出 EasyFlow、未執行付款。</p><table>${rentApplicationRows(a).map(([k,v])=>`<tr${v?'':' style="background:#fff5d7"'}><th>${esc(k)}</th><td>${esc(v)}</td></tr>`).join('')}</table><h3>交接前待補</h3><p>${esc((a.missingFields??[]).map(k=>rentalLabels[k]??k).join('、')||'已知必填欄位已填；仍須核對實際 EF 畫面。')}</p>`;
}
