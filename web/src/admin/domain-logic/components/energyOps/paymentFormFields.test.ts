import {it,expect} from 'vitest';
import {paymentFormHtml} from './paymentFormFields';
it('renders eight payment sections and grouped EF amounts, retaining invoice separation',()=>{
 const data:any={applicationType:'payment',fields:[{key:'companyName',value:'京承能源'},{key:'amount',value:'4770'}],paymentLineItems:[{description:'八方夜市逆變器使用電費',amount:'3221',currency:'TWD'},{description:'八方夜市電表租費',amount:'1549',currency:'TWD'}],lineItems:[{description:'基本電費',amount:'514.2'}]};
 const html=paymentFormHtml(data);for(let i=1;i<=8;i++)expect(html).toContain(`${i}. `);expect(html).toContain('3221');expect(html).toContain('1549');expect(html).not.toContain('514.2');expect(html).toContain('付款對象');expect(paymentFormHtml({...data,applicationType:'invoice'})).toBe('');
});
