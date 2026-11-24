export type RentPreviewData={stationName:string;customerNumber:string;payee:string;periodStart:string;periodEnd:string;basis:string;percent:string;taxPercent:string;rounding:string;income:string;baseRent:string|null;tax:string|null;grossRent:string|null;calculationType?:string;referenceRent?:string;referenceRatioPercent?:string;annualRent?:string;paymentDeadline?:string;tolerancePercent?:string};
export function rentCalculationDescription(r:RentPreviewData):string {
 if(r.calculationType==='annualReference')return `售電收入 × ${r.percent}%＝參考年租金 ${r.referenceRent} 元；占經營年租金 ${r.referenceRatioPercent}%。差距未達 ${r.tolerancePercent}% 維持契約年租金 ${r.annualRent} 元；達門檻時另核對級距，本預覽不推定。`;
 return `${r.percent}%；${r.rounding==='perReceipt'?'各筆租金四捨五入後加總':'收入加總後計算租金並四捨五入'}，外加 ${r.taxPercent}% 稅。`;
}
export function rentPreviewFields(r:RentPreviewData):[string,string|null|undefined][] {
 return [['案場',r.stationName],['電號',r.customerNumber],['收款對象',r.payee],['申請摘要',`支付${r.stationName}租金（${r.periodStart}～${r.periodEnd}）`],['計算期間',`${r.periodStart} ～ ${r.periodEnd}`],['收入基底',r.basis==='gross'?'含稅售電收入':'未稅售電收入'],['收入合計',r.income],...(r.calculationType==='annualReference'?[['參考年租金',r.referenceRent],['契約經營年租金',r.annualRent],['參考年租金比例',`${r.referenceRatioPercent}%`]] as [string,string|undefined][]:[]),['租金基額（外加稅前）',r.baseRent],['外加稅額',r.tax],['應付租金總額',r.grossRent],['費用說明',rentCalculationDescription(r)],['合約繳費期限',r.paymentDeadline||'待確認'],['安排付款日','待作業確認'],['付款單號','尚未產生']];
}
