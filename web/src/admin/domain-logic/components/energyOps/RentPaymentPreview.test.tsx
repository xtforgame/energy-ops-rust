import {MemoryRouter} from 'react-router-dom';
import {cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,describe,it,expect,vi} from 'vitest';
import RentPaymentPreview from './RentPaymentPreview';
import {energyHttp} from '@/api/energy';
vi.mock('@/api/energy',()=>({energyHttp:{get:vi.fn(),post:vi.fn()}}));
afterEach(()=>{cleanup();vi.clearAllMocks();});
describe('rent attachment preview',()=>{
 it('shows existing source without inventing contract/invoice and persists the declared invoice arrangement',async()=>{
  vi.mocked(energyHttp.get).mockResolvedValue({data:{items:[{stationId:70,invoiceTiming:'unknown',voucherRevision:'v1',stationName:'麻豆',customerNumber:'20-72-7017-93-7',payee:'地主',periodStart:'2026-10-01',periodEnd:'2026-10-31',basis:'gross',percent:'8',taxPercent:'5',rounding:'perReceipt',ruleSource:'試算表',income:'154550',baseRent:'12364',tax:'618',grossRent:'12982',issues:[],notes:[],lines:[{source:'taipower',attachmentId:3,page:2,income:'5481',kwh:'1085',start:'2026-10-01',end:'2026-10-31'}],note:'非正式付款'}]}} as any);
  vi.mocked(energyHttp.post).mockImplementation(async()=>{const response=await energyHttp.get('/mock');response.data.items[0].invoiceTiming='later';response.data.items[0].voucherRevision='v2';return {data:{invoiceTiming:'later',voucherRevision:'v2'}} as any;});
  render(<MemoryRouter><RentPaymentPreview base="/api/test" billId={4}/></MemoryRouter>);
  fireEvent.click(screen.getByText('預覽地主租金付款'));
  await screen.findByText('12982');expect(screen.getByText('待附合約租金頁面')).toBeTruthy();
  fireEvent.mouseDown(screen.getByRole('combobox'));fireEvent.click(await screen.findByRole('option',{name:'發票後補'}));
  await screen.findByText('發票待後補（尚未收到）');expect(energyHttp.post).toHaveBeenCalledWith('/api/test/stations/70/rent-payment/voucher',expect.objectContaining({invoiceTiming:'later',voucherRevision:'v1'}));
  fireEvent.click(screen.getByText('關閉'));await waitFor(()=>expect(screen.queryByText('發票待後補（尚未收到）')).toBeNull());
  fireEvent.click(screen.getByText('預覽地主租金付款'));await screen.findByText('發票待後補（尚未收到）');
 });
});

it('shows automatic rule provenance and never saves its invoice choice through the legacy endpoint',async()=>{
 vi.mocked(energyHttp.get).mockResolvedValue({data:{items:[{stationId:70,inputMethod:'autoBills',ruleId:7,ruleVersion:3,evidenceStatus:'excerpt',leaseEvidence:{status:'excerpt',attachmentIds:[12]},sourceBillIds:[42],sourceContributions:[{billId:42,start:'2026-09-01',end:'2026-09-30',income:'16874',sourceContributionRaw:'674.96',note:'此筆貢獻不是完整付款週期'}],requirements:['缺少轉供單','待確認帳期'],invoiceTiming:'unknown',stationName:'麻豆',customerNumber:'20',payee:'地主',periodStart:'2026-10-01',periodEnd:'2026-12-31',basis:'gross',percent:'8',taxPercent:'5',rounding:'total',ruleSource:'承辦確認',income:'0',baseRent:null,tax:null,grossRent:null,issues:['缺少轉供單'],notes:[],lines:[],note:'自動彙整預覽'}]}} as any);
 render(<MemoryRouter><RentPaymentPreview base="/api/test" billId={4}/></MemoryRouter>);fireEvent.click(screen.getByText('預覽地主租金付款'));
 await screen.findByText(/規則 #7 · 第 3 版/);expect(screen.getByText('單筆帳單比例租金貢獻（未進位）：674.96 元')).toBeTruthy();expect(screen.getByText('此筆貢獻不是完整付款週期')).toBeTruthy();expect(screen.getAllByText('缺少轉供單')).toHaveLength(1);expect(screen.getByText('待確認帳期')).toBeTruthy();expect(screen.getByRole('link',{name:'來源電費單 #42'}).getAttribute('href')).toBe('/energyBillConsole/42/show');expect(screen.getByRole('link',{name:'租約附件 #12'}).getAttribute('href')).toBe('/api/test/attachments/12/download');expect(screen.getByText('已有租金頁摘錄（完整合約待補）')).toBeTruthy();
 fireEvent.mouseDown(screen.getByRole('combobox'));fireEvent.click(await screen.findByRole('option',{name:'發票後補'}));await screen.findByText('發票待後補（尚未收到）');expect(energyHttp.post).not.toHaveBeenCalled();
});

it('renders the known shared amount without treating it as this station payable',async()=>{
 vi.mocked(energyHttp.get).mockResolvedValue({data:{items:[{stationId:70,stationName:'埔心一',groupReferenceRent:'213549',sharedGroupLabel:'埔心兩案共用',sharedGroupMembers:['埔心一','埔心二'],baseRent:null,tax:null,grossRent:null,issues:[],notes:[],lines:[],note:'整組租金參考',inputMethod:'autoBills'}]}} as any);
 render(<MemoryRouter><RentPaymentPreview base="/api/test" stationId={70}/></MemoryRouter>);fireEvent.click(screen.getByText('預覽地主租金付款'));
 expect(await screen.findByText('共用租金（整組，只計一次）')).toBeTruthy();expect(screen.getByText('213549')).toBeTruthy();expect(screen.getByText('尚未提供個別分攤；整組金額只計一次，不列為本案場應付')).toBeTruthy();expect(screen.getByText('埔心一、埔心二')).toBeTruthy();
});
