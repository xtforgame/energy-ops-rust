import {expect,it} from 'vitest';
import {rentPreviewFields,rentCalculationDescription} from './rentPreviewFields';
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
