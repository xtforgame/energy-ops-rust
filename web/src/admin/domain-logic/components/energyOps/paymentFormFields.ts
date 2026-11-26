export const paymentFormSections=[['companyName','公司名稱（負責付款公司）'],['contractOption','有無合約'],['summary','申請摘要'],['counterpartyName','付款對象'],['paymentMethod','付款方式'],['paymentTerms','付款條件'],['voucherStatus','憑證單據']] as const;
type PaymentData={applicationType:string;fields:{key:string;value:string|null}[];paymentLineItems?:{description:string;amount:string;currency?:string}[];lineItems?:{description:string;amount:string;currency?:string}[]};
const esc=(v:unknown)=>String(v??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]!));
export function paymentFormHtml(data:PaymentData):string{
 if(data.applicationType!=='payment')return '';
 const val=(k:string)=>data.fields.find(f=>f.key===k)?.value;
 const rows=paymentFormSections.map(([k,label],i)=>`<tr><th>${i+1}. ${label}</th><td>${esc(val(k))||'<span class="missing">待補</span>'}</td></tr>`).join('');
 const lines=data.paymentLineItems??data.lineItems??[];
 return `<h2>電費付款申請｜八項填寫內容</h2><table>${rows}</table><h3>8. 申請事由、幣別、金額（各項分別填寫）</h3><table><tr><th>申請事由</th><th>幣別</th><th>金額</th></tr>${lines.map(l=>`<tr><td>${esc(l.description)}</td><td>${esc(l.currency??'TWD')}</td><td>${esc(l.amount)}</td></tr>`).join('')||'<tr><td colspan="3">費用分項待核對，請見下方原單細項。</td></tr>'}<tr><th colspan="2">付款申請總金額（TWD）</th><td>${esc(val('amount'))}</td></tr></table><p>分項依已核對的原單金額彙整；原始計費細項保留於下方，營業稅不重複加入。</p><h2>其他資料與原單對照</h2>`;
}
