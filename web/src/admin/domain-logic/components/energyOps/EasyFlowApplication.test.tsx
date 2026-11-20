import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { vi, describe, it, expect } from 'vitest';
import EasyFlowApplication from './EasyFlowApplication';
import { energyHttp } from '@/api/energy';
vi.mock('@/api/energy', () => ({ energyHttp: { get: vi.fn() } }));

describe('EasyFlow application preparation', () => {
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
