import {expect,it} from 'vitest';
import {rentCycleDocument} from './rentCycleDocument';
it('exports readable pending values without treating them as zero or executing source text',()=>{
 const html=rentCycleDocument({contractNo:'<script>bad</script>',baseRent:null,issues:['缺台電單'],sources:{taipower:{status:'missing',amount:null,kwh:null}}});
 expect(html).toContain('待確認 元');expect(html).toContain('缺台電單');expect(html).not.toContain('<script>');expect(html).toContain('尚未送審或付款');
});
