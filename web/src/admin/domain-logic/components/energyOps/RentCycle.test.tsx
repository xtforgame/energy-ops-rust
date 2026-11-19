import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import RentCycle from './RentCycle';
import { energyHttp } from '@/api/energy';
vi.mock('@/api/energy',()=>({energyHttp:{get:vi.fn(),post:vi.fn()}}));
const plan={stationId:1,anchor:'2026-07-01',months:3,transfer:true,kind:'revenueShare',basis:'untaxed',sharePercent:'10',area:null,unitPrice:null,unitPeriod:'month',roundingPlaces:2};
const response=(data:any)=>({data,status:200,statusText:'OK',headers:{},config:{headers:{} as any}});
describe('landlord rent cycle preparation',()=>{
  beforeEach(()=>vi.clearAllMocks());
  it('loads bill income rather than net total, preserves zero, and invalidates stale results',async()=>{
    vi.mocked(energyHttp.get).mockResolvedValueOnce(response({plan})).mockResolvedValueOnce(response({items:[{billId:7,docType:'renewableSurplus',start:'2026-07-01',end:'2026-09-30',details:{purchaseEnergyFee:'0',electricityFeeTotal:'0',purchaseKwh:'0'},totalAmount:'-983'}]}));
    vi.mocked(energyHttp.post).mockResolvedValue(response({issues:[],sources:{taipower:{status:'received',amount:'0',kwh:'0'},transfer:{status:'missing',amount:null,kwh:null}},baseRent:null,periodStart:'2026-07-01',periodEnd:'2026-09-30'}));
    render(<RentCycle base="/api/test" contractId={2} stationId={1}/>);
    fireEvent.click(screen.getByRole('button',{name:'租金週期試算'}));
    await waitFor(()=>expect((screen.getByLabelText('本次計算起日') as HTMLInputElement).value).toBe('2026-07-01'));
    fireEvent.click(screen.getByRole('button',{name:'儲存試算設定並載入單據'}));
    await screen.findByLabelText('7 untaxed');
    expect((screen.getByLabelText('7 untaxed') as HTMLInputElement).value).toBe('0');
    fireEvent.click(screen.getByLabelText('選用單據 7'));
    fireEvent.click(screen.getByRole('button',{name:'檢查收齊狀態並試算'}));
    await screen.findByText('缺單');
    expect(energyHttp.post).toHaveBeenCalledWith('/api/test/contracts/2/rent-cycle/preview',{start:'2026-07-01',receipts:[{billId:7,source:'taipower',untaxed:'0',gross:'0',kwh:'0'}]});
    expect(screen.queryByRole('button',{name:'付款'})).toBeNull();
    fireEvent.change(screen.getByLabelText('7 untaxed'),{target:{value:'100'}});
    expect(screen.queryByRole('button',{name:'匯出試算 JSON（含來源與待確認事項）'})).toBeNull();
  });
  it('does not overwrite another site plan',async()=>{
    vi.mocked(energyHttp.get).mockResolvedValue(response({plan:{...plan,stationId:99}}));
    render(<RentCycle base="/api/test" contractId={2} stationId={1}/>);
    fireEvent.click(screen.getByRole('button',{name:'租金週期試算'}));
    await screen.findByText(/此合約已為另一案場設定試算/);
    expect((screen.getByRole('button',{name:'儲存試算設定並載入單據'}) as HTMLButtonElement).disabled).toBe(true);
    expect(energyHttp.post).not.toHaveBeenCalled();
  });
});
