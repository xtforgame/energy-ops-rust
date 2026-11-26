import {expect,it} from 'vitest';
import {rentPreviewFields,rentCalculationDescription} from './rentPreviewFields';
it('preserves a known summary without inventing a missing rental period',()=>{
 const fields=Object.fromEntries(rentPreviewFields({stationName:'春源鋼鐵',annualRentReference:'700000',periodAllocationUnknown:true,application:{fields:{summary:'支付春源鋼鐵租金（計算期間待確認）'}}} as any));
 expect(fields['申請摘要']).toBe('支付春源鋼鐵租金（計算期間待確認）');
 expect(fields['計算期間']).toBe('待確認');
 expect(fields['已知年租金']).toBe('700000');
 expect(JSON.stringify(fields)).not.toContain('undefined');
});
it('keeps all expense terms when formula summary is provided',()=>{
 const text=rentCalculationDescription({formulaSummary:'收入 × 8%，最低 200 / cycle',calculationType:'percentageFloor',basis:'untaxed',percent:'8',minimumRent:'200',minimumUnit:'cycle',rounding:'total',taxPercent:'5',taxMode:'included',paymentDeadline:'2026-12-01'} as any);
 for(const term of ['未稅售電收入','8%','200','四捨五入','內含','5%','2026-12-01'])expect(text).toContain(term);
});
it('separates reference rent, payable and deadline from scheduled payment',()=>{
 const r:any={stationName:'測試案場',periodStart:'2025-10-01',periodEnd:'2026-09-30',calculationType:'annualReference',referenceRent:'288262',referenceRatioPercent:'100.47',annualRent:'286900',tolerancePercent:'5',percent:'10',taxPercent:'0',grossRent:'286900',paymentDeadline:'2026-12-01'};
 const f=Object.fromEntries(rentPreviewFields(r));expect(f['參考年租金']).toBe('288262');expect(f['應付租金總額']).toBe('286900');expect(f['合約繳費期限']).toBe('2026-12-01');expect(f['安排付款日']).toBe('待作業確認');expect(rentCalculationDescription(r)).toContain('未達 5%');
});

it('keeps unrounded rent visible while rounding and tax remain unknown',()=>{
 const r:any={stationName:'測試案場',baseRentRaw:'123.4567',taxRaw:'6.172835',grossRentRaw:'129.629535',baseRent:null,tax:null,grossRent:null,rounding:null,basis:null,percent:'8',taxPercent:null};
 const f=Object.fromEntries(rentPreviewFields(r));
 expect(f['租金基額（未進位）']).toBe('123.4567');expect(f['應付租金總額']).toBeNull();
 expect(f['稅額（未進位）']).toBe('6.172835');expect(f['含稅試算合計（未進位）']).toBe('129.629535');
 expect(f['收入基底']).toBe('收入基底待確認');expect(rentCalculationDescription(r)).toContain('進位方式待確認');
});

it('shows a shared group once as reference and preserves unknown installment limits',()=>{
 const f=Object.fromEntries(rentPreviewFields({stationName:'埔心',groupReferenceRent:'213549',sharedGroupKey:'puxin',sharedGroupMembers:['一場','二場'],baseRent:null,grossRent:null} as any));
 expect(f['共用租金（整組，只計一次）']).toBe('213549');expect(f['共用金額說明']).toContain('尚未提供個別分攤');expect(f['共用案場']).toBe('一場、二場');expect(f['應付租金總額']).toBeNull();
 const annual=Object.fromEntries(rentPreviewFields({stationName:'矽品',annualRentReference:'1700000',periodAllocationUnknown:true,baseRent:null} as any));expect(annual['已知年租金']).toBe('1700000');expect(annual['分期說明']).toContain('不推定平均分期');expect(annual['租金基額（外加稅前）']).toBeNull();
});

it('shows separate annual reference and variable history from API known inputs without inventing payable',()=>{
 const fields=Object.fromEntries(rentPreviewFields({stationName:'佳南',baseRent:null,grossRent:null,knownInputs:{paymentComponents:[{payee:'陳雲清',cycleMonths:12,annualAmountReference:18000,sourceSheet:'2-1.租金付款紀錄',sourceRow:105},{payee:'陳琬瑜',cycleMonths:1,fixedAmount:null,observedAmounts:[12250,12500],sourceSheet:'2-1.租金付款紀錄',sourceRow:106}],sourceMissing:['monthlyComponentFormula','monthlyComponentBasis','paymentDeductions']}} as any));
 expect(fields['付款組成 1｜陳雲清']).toContain('年付來源參考金額 18000 元');expect(fields['付款組成 1｜陳雲清']).not.toContain('每月款計算公式');
 expect(fields['付款組成 2｜陳琬瑜']).toContain('12250、12500 元（不是固定月租）');expect(fields['付款組成 2｜陳琬瑜']).toContain('每月款計算公式待確認');expect(fields['付款組成 2｜陳琬瑜']).toContain('每月款計算基底待確認');expect(fields['付款組成 2｜陳琬瑜']).toContain('付款扣項待確認');expect(fields['應付租金總額']).toBeNull();
});

it('describes non applicable rent without a missing calculation formula',()=>{
 const r:any={stationName:'業主自建',calculationType:'notApplicable',rentRequired:false,baseRent:null,grossRent:null};expect(rentCalculationDescription(r)).toBe('業主自建案，不需付租金；不影響電費流程');const fields=Object.fromEntries(rentPreviewFields(r));expect(fields['租金適用狀態']).toContain('不需付租金');expect(fields['應付租金總額']).toBeNull();
});
it('describes floor parameters even without a formula summary',()=>{
 const text=rentCalculationDescription({calculationType:'percentageFloor',basis:'untaxed',percent:'10',minimumRent:'1200',minimumUnit:'year',minimumBasis:'untaxed'} as any);
 expect(text).toContain('1200');expect(text).toContain('每年');expect(text).toContain('最低額');
});
