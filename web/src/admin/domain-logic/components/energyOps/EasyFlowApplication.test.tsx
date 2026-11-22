import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { vi, describe, it, expect } from 'vitest';
import EasyFlowApplication from './EasyFlowApplication';
import { energyHttp } from '@/api/energy';
vi.mock('@/api/energy', () => ({ energyHttp: { get: vi.fn(), post:vi.fn() } }));

describe('EasyFlow application preparation', () => {
  it('shows signed inverter charges and the explicit rounding explanation',async()=>{
    vi.mocked(energyHttp.get).mockResolvedValue({data:{applicationType:'payment',fields:[],pending:[],attachments:[],lineItems:[{key:'discount',description:'電子帳單優惠減收金額',amount:'-10'}],lineItemsReconciled:true,reconciliation:{lineSum:'5034.9',roundedLineSum:'5035',roundingDifference:'0.1',taxBreakdownReconciled:true,message:'營業稅不重複加入明細。'}}} as any);
    render(<EasyFlowApplication base="/api/test" billId={45} autoOpen/>);
    await screen.findByText('-10 TWD');
    expect(screen.getByText(/明細合計 5034.9 元/)).toBeTruthy();
  });
  it('locks editing while a save is pending so later input cannot be discarded',async()=>{
    const data={applicationType:'invoice',canReview:true,sourceRevision:'s',draftRevision:'d',fields:[{key:'applicant',label:'申請人',value:null,status:'pending',source:'master'}],pending:[],attachments:[]};
    vi.mocked(energyHttp.get).mockResolvedValue({data} as any);
    let finish!:(value:any)=>void;
    vi.mocked(energyHttp.post).mockImplementation(()=>new Promise(resolve=>{finish=resolve;}));
    render(<EasyFlowApplication base="/api/test" billId={44} autoOpen/>);
    const input=await screen.findByLabelText('申請人');
    fireEvent.click(screen.getByRole('button',{name:'儲存申請草稿'}));
    expect((input as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByRole('button',{name:'關閉'}) as HTMLButtonElement).disabled).toBe(true);
    await act(async()=>finish({data}));
    expect((input as HTMLInputElement).disabled).toBe(false);
  });
  it('opens from intake and saves application fields for review without submitting to EF',async()=>{
    const data={applicationType:'invoice',canReview:true,sourceRevision:'source-1',draftRevision:'draft-1',applicationReview:{state:'draft'},fields:[{key:'applicant',label:'申請人',value:null,status:'pending',source:'master'}],pending:[],attachments:[]};
    vi.mocked(energyHttp.get).mockResolvedValue({data} as any);
    vi.mocked(energyHttp.post).mockResolvedValue({data:{...data,applicationReview:{state:'prepared',stale:false}}} as any);
    render(<EasyFlowApplication base="/api/test" billId={43} autoOpen />);
    fireEvent.change(await screen.findByLabelText('申請人'),{target:{value:'承辦甲'}});
    fireEvent.click(screen.getByRole('button',{name:'儲存申請草稿'}));
    await waitFor(()=>expect(energyHttp.post).toHaveBeenCalledWith('/api/test/bills/43/easyflow-application',expect.objectContaining({sourceRevision:'source-1',draftRevision:'draft-1',action:'save',values:expect.objectContaining({applicant:'承辦甲'})})));
    expect(screen.queryByRole('button',{name:'送出 EasyFlow'})).toBeNull();
  });
  it('loads saved server values, shows pending rules and never offers submit', async () => {
    vi.mocked(energyHttp.get).mockResolvedValue({ status:200,statusText:'OK',headers:{},config:{headers:{} as any}, data: {
      applicationType:'invoice', submissionAllowed:false,
      fields:[{key:'amount',label:'開立金額（未稅）',value:'16874',source:'bill.detailFields.purchaseEnergyFee',status:'available'}],
      pending:[{key:'date',message:'日期定義待確認'}], attachments:[],
    }});
    render(<EasyFlowApplication base="/api/test" billId={42} />);
    fireEvent.click(screen.getByRole('button',{name:'預覽 EasyFlow 申請表'}));
    await screen.findByText('16874');
    expect(energyHttp.get).toHaveBeenCalledWith('/api/test/bills/42/easyflow-application');
    expect(screen.getByText('日期定義待確認')).toBeTruthy();
    expect(screen.queryByRole('button',{name:'送出 EasyFlow'})).toBeNull();
  });
  it('shows a failed request without leaving a stale download', async () => {
    vi.mocked(energyHttp.get).mockRejectedValue(new Error('offline'));
    render(<EasyFlowApplication base="/api/test" billId={42} />);
    fireEvent.click(screen.getByRole('button',{name:'預覽 EasyFlow 申請表'}));
    await waitFor(()=>expect(screen.getByText('無法取得申請資料，請重試。')).toBeTruthy());
    expect(screen.queryByRole('button',{name:'下載 JSON（自動化用）'})).toBeNull();
  });
});
