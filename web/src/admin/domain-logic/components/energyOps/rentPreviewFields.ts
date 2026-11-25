export type RentPaymentComponent={payee?:string|null;cycleMonths?:number|null;annualAmountReference?:number|string|null;observedAmounts?:(number|string)[];fixedAmount?:number|string|null;sourceSheet?:string;sourceRow?:number;evidenceKind?:string};
export type RentReferenceData={rentRequired?:boolean;readiness?:string;paymentComponents?:RentPaymentComponent[];sourceMissing?:string[];knownInputs?:{paymentComponents?:RentPaymentComponent[];sourceMissing?:string[]};groupReferenceRent?:string|null;annualRentReference?:string|null;groupAllocationMissing?:boolean;periodAllocationUnknown?:boolean;sharedGroupKey?:string|null;sharedGroupLabel?:string|null;sharedGroupMembers?:string[]|null};
export function rentReferenceFields(r:RentReferenceData):[string,string][] {
 const fields:[string,string][]=[];
 if(r.readiness==='notApplicable'||r.rentRequired===false)fields.push(['租金適用狀態','不需付租金；不影響電費流程']);
 const components=r.paymentComponents??r.knownInputs?.paymentComponents??[];
 const missing=r.sourceMissing??r.knownInputs?.sourceMissing??[];
 if(components.length){
  fields.push(['付款組成說明','以下為來源參考，未合計為正式付款；歷史金額不視為固定租金']);
  components.forEach((c,i)=>{
   const title=`付款組成 ${i+1}｜${c.payee??'收款人待確認'}`;
   const detail=[c.cycleMonths!=null?`每 ${c.cycleMonths} 個月一期`:'週期待確認'];
   if(c.annualAmountReference!=null)detail.push(`年付來源參考金額 ${c.annualAmountReference} 元`);
   if(c.observedAmounts?.length)detail.push(`歷史付款觀察值 ${c.observedAmounts.join('、')} 元（不是固定月租）`);
   if(c.cycleMonths===1&&c.fixedAmount==null){if(missing.includes('monthlyComponentFormula'))detail.push('每月款計算公式待確認');if(missing.includes('monthlyComponentBasis'))detail.push('每月款計算基底待確認');}
   if(missing.includes('paymentDeductions'))detail.push('付款扣項待確認');
   if(c.sourceSheet)detail.push(`來源：${c.sourceSheet}${c.sourceRow!=null?` 第 ${c.sourceRow} 列`:''}`);
   fields.push([title,detail.join('；')]);
  });
 }

 if(r.groupReferenceRent!=null){fields.push(['共用租金（整組，只計一次）',r.groupReferenceRent],['共用金額說明','尚未提供個別分攤；整組金額只計一次，不列為本案場應付']);if(r.sharedGroupLabel)fields.push(['共用群組',r.sharedGroupLabel]);if(r.sharedGroupKey)fields.push(['共用群組識別',r.sharedGroupKey]);if(r.sharedGroupMembers?.length)fields.push(['共用案場',r.sharedGroupMembers.join('、')]);}
 if(r.annualRentReference!=null&&r.periodAllocationUnknown){fields.push(['已知年租金',r.annualRentReference],['分期說明','分期算法待確認；不推定平均分期，可輸入 12 個月核對年額']);}
 return fields;
}
export type RentPreviewData=RentReferenceData & {taxMode?:string|null;baseRentRaw?:string|null;taxRaw?:string|null;grossRentRaw?:string|null;formulaSummary?:string;stationName:string;customerNumber:string;payee:string;periodStart:string;periodEnd:string;basis:string;percent:string;taxPercent:string;rounding:string;income:string;baseRent:string|null;tax:string|null;grossRent:string|null;calculationType?:string;referenceRent?:string;referenceRatioPercent?:string;annualRent?:string;paymentDeadline?:string;tolerancePercent?:string};
export function rentCalculationDescription(r:RentPreviewData):string {
 if(r.calculationType==='notApplicable'&&r.rentRequired===false)return '業主自建案，不需付租金；不影響電費流程';
 if(r.formulaSummary)return r.formulaSummary;
 if(r.calculationType==='annualReference')return `售電收入 × ${r.percent}%＝參考年租金 ${r.referenceRent} 元；占經營年租金 ${r.referenceRatioPercent}%。差距未達 ${r.tolerancePercent}% 維持契約年租金 ${r.annualRent} 元；達門檻時另核對級距，本預覽不推定。`;
 return `${r.percent??'比例待確認'}%；${r.rounding==='perReceipt'?'各筆租金四捨五入後加總':r.rounding==='total'?'收入加總後計算租金並四捨五入':'進位方式待確認'}，${r.taxMode==='included'?'內含':'租金'}稅率 ${r.taxPercent??'待確認'}${r.taxPercent==null?'':'%'}。`;
}
export function rentPreviewFields(r:RentPreviewData):[string,string|null|undefined][] {
 return [['案場',r.stationName],['電號',r.customerNumber],['收款對象',r.payee],['申請摘要',`支付${r.stationName}租金（${r.periodStart}～${r.periodEnd}）`],['計算期間',`${r.periodStart} ～ ${r.periodEnd}`],['收入基底',r.basis==='gross'?'含稅售電收入':r.basis==='untaxed'?'未稅售電收入':'收入基底待確認'],['收入合計',r.income],...rentReferenceFields(r),...(r.calculationType==='annualReference'?[['參考年租金',r.referenceRent],['契約經營年租金',r.annualRent],['參考年租金比例',`${r.referenceRatioPercent}%`]] as [string,string|undefined][]:[]),...(r.baseRentRaw!=null?[['租金基額（未進位）',r.baseRentRaw]] as [string,string][]:[]),['租金基額（外加稅前）',r.baseRent],...(r.taxRaw!=null?[[r.taxMode==='included'?'內含稅額（未進位）':'稅額（未進位）',r.taxRaw]] as [string,string][]:[]),...(r.grossRentRaw!=null?[['含稅試算合計（未進位）',r.grossRentRaw]] as [string,string][]:[]),[r.taxMode==='included'?'內含稅額':'稅額',r.tax],['應付租金總額',r.grossRent],['費用說明',rentCalculationDescription(r)],['合約繳費期限',r.paymentDeadline||'待確認'],['安排付款日','待作業確認'],['付款單號','尚未產生']];
}
