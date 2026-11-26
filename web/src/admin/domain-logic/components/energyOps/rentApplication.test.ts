import {describe,it,expect} from 'vitest';
import {rentApplicationRows,rentApplicationHtml} from './rentalFormFields';
describe('rental handoff form',()=>{
 it('preserves known money and unknown operator fields without invented defaults',()=>{
  const rows=rentApplicationRows({fields:{summary:'支付麻豆租金',amount:'12982',companyCode:null,paymentDate:null}});
  expect(rows.find(r=>r[0]==='6. 總金額')?.[1]).toBe('12982');
  expect(rows.find(r=>r[0]==='公司代碼')?.[1]).toBe('');
  expect(rows.find(r=>r[0]==='8. 請安排於指定時間付款')?.[1]).toBe('');
 });
 it('exports saved status without claiming submitted or paid and escapes source data',()=>{
  const html=rentApplicationHtml({state:'confirmed_for_handoff',fields:{summary:'<script>x</script>',amount:'12982'},missingFields:['applicant']});
  expect(html).toContain('&lt;script&gt;');expect(html).not.toContain('<script>');
  expect(html).toContain('尚未送出 EasyFlow');expect(html).toContain('申請人');
 });
});
