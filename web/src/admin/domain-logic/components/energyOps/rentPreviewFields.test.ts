import {expect,it} from 'vitest';
import {rentPreviewFields,rentCalculationDescription} from './rentPreviewFields';
it('separates reference rent, payable and deadline from scheduled payment',()=>{
 const r:any={stationName:'測試案場',periodStart:'2025-10-01',periodEnd:'2026-09-30',calculationType:'annualReference',referenceRent:'288262',referenceRatioPercent:'100.47',annualRent:'286900',tolerancePercent:'5',percent:'10',taxPercent:'0',grossRent:'286900',paymentDeadline:'2026-12-01'};
 const f=Object.fromEntries(rentPreviewFields(r));expect(f['參考年租金']).toBe('288262');expect(f['應付租金總額']).toBe('286900');expect(f['合約繳費期限']).toBe('2026-12-01');expect(f['安排付款日']).toBe('待作業確認');expect(rentCalculationDescription(r)).toContain('未達 5%');
});
