import {cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,beforeEach,describe,expect,it,vi} from 'vitest';
import {energyHttp} from '@/api/energy';
import StationRentRules from './StationRentRules';
vi.mock('@/api/energy',()=>({energyHttp:{get:vi.fn(),post:vi.fn()}}));
const entry={id:7,version:2,rule:{payee:'地主甲',basis:'untaxed',percent:'8',taxPercent:'5',rounding:'total',transfer:true,cycleMonths:3,cycleAnchor:'2026-01-01',validFrom:'2026-01-01',validTo:null,status:'confirmed',ruleSource:'承辦確認',leaseEvidence:{status:'missing',attachmentIds:[]}}};
beforeEach(()=>vi.resetAllMocks());
afterEach(cleanup);
describe('station recurring rental rules',()=>{
 it('preserves imported raw values and notes when editing an unsupported pending rule with missing parameters',async()=>{
  const imported={...entry,rule:{...entry.rule,status:'pending',calculationType:'pendingUnknown',payee:null,percent:null,taxPercent:null,basis:null,rounding:null,transfer:null,cycleMonths:null,cycleAnchor:null,validFrom:null,sourceRow:18,sourceProjectCode:'TEST-18',sourceValues:{BA:'每坪單價待確認',AW:12000,BC:'一年',Y:'簽約日期原文'},notes:['請保留原始備註']}};
  vi.mocked(energyHttp.get).mockResolvedValue({data:{items:[imported],canManage:true}} as any);
  vi.mocked(energyHttp.post).mockResolvedValue({data:{...imported,version:3}} as any);
  render(<StationRentRules base="/api/test" stationId={70}/>);
  fireEvent.click(await screen.findByText('查看資產總表原始租金條件'));
  expect(screen.getByText('12000')).toBeTruthy();expect(screen.getByText('一年')).toBeTruthy();expect(screen.getByText(/來源專案：TEST-18；來源列：18/)).toBeTruthy();
  expect(screen.queryByText(/售電收入 ×/)).toBeNull();
  fireEvent.click(screen.getByText('編輯規則 #7'));
  expect(screen.getByText(/不會轉換為比例租金/)).toBeTruthy();
  fireEvent.change(screen.getByLabelText(/收款對象/),{target:{value:'確認後收款人'}});
  fireEvent.click(screen.getByText('儲存租金規則'));
  await waitFor(()=>expect(energyHttp.post).toHaveBeenCalledWith('/api/test/stations/70/rent-rules',expect.objectContaining({calculationType:'pendingUnknown',status:'pending',basis:null,percent:null,transfer:null,validFrom:null,payee:'確認後收款人',sourceValues:imported.rule.sourceValues,notes:imported.rule.notes,expectedVersion:2})));
 });
 it('shows confirmed rules independently of missing lease and hides editing for readers',async()=>{
  vi.mocked(energyHttp.get).mockResolvedValue({data:{items:[entry],canManage:false}} as any);
  render(<StationRentRules base="/api/test" stationId={70}/>);
  await screen.findByText('規則已確認');expect(screen.getByText('合約尚缺')).toBeTruthy();
  expect(screen.queryByText('新增租金規則')).toBeNull();expect(screen.queryByText('編輯規則 #7')).toBeNull();
 });
 it('sends the loaded revision and preserves edits when a concurrent write is rejected',async()=>{
  vi.mocked(energyHttp.get).mockResolvedValue({data:{items:[entry],canManage:true}} as any);
  vi.mocked(energyHttp.post).mockRejectedValue({response:{status:409,data:{code:'rent_rule_changed'}}});
  render(<StationRentRules base="/api/test" stationId={70}/>);
  fireEvent.click(await screen.findByText('編輯規則 #7'));
  fireEvent.change(screen.getByLabelText(/收款對象/),{target:{value:'地主乙'}});
  fireEvent.click(screen.getByText('儲存租金規則'));
  await screen.findByText(/本次未覆蓋既有規則/);
  expect(energyHttp.post).toHaveBeenCalledWith('/api/test/stations/70/rent-rules',expect.objectContaining({id:7,expectedVersion:2,payee:'地主乙',status:'confirmed',leaseEvidence:{status:'missing',attachmentIds:[]}}));
  expect((screen.getByLabelText(/收款對象/) as HTMLInputElement).value).toBe('地主乙');
 });
 it('reloads the persisted updated version after saving',async()=>{
  vi.mocked(energyHttp.get).mockResolvedValueOnce({data:{items:[entry],canManage:true}} as any).mockResolvedValue({data:{items:[{...entry,version:3}],canManage:true}} as any);
  vi.mocked(energyHttp.post).mockResolvedValue({data:{...entry,version:3}} as any);
  render(<StationRentRules base="/api/test" stationId={70}/>);fireEvent.click(await screen.findByText('編輯規則 #7'));fireEvent.click(screen.getByText('儲存租金規則'));
  await waitFor(()=>expect(screen.getByText(/規則 #7 · 第 3 版/)).toBeTruthy());
 });
});

it('shows a group and calculates raw rent without inventing tax or equal allocation',async()=>{
 const group={...entry,rule:{...entry.rule,status:'pending',taxPercent:null,rounding:null,payee:'柯進來三人',partyGroup:{label:'柯進來三人',count:3,members:[{name:'柯進來'}],allocationStatus:'unknown'}},assessment:{formulaSummary:'未稅售電收入 × 8%',missingInputs:['進位方式待確認','外加稅率待確認']}};
 vi.mocked(energyHttp.get).mockResolvedValue({data:{items:[group],canManage:false}} as any);
 vi.mocked(energyHttp.post).mockResolvedValue({data:{formulaSummary:'未稅售電收入 × 8%',baseRentRaw:'9876.5432',baseRent:null,tax:null,grossRent:null,missingInputs:['進位方式待確認','外加稅率待確認']}} as any);
 render(<StationRentRules base="/api/test" stationId={70}/>);
 expect(await screen.findByText(/收款群組：柯進來三人/)).toHaveTextContent('3 人');
 expect(screen.getByText(/收款群組：柯進來三人/)).toHaveTextContent('不推定均分');
 fireEvent.click(screen.getByText('公式試算與待補條件 #7'));
 fireEvent.change(screen.getByLabelText('手動收入（元）#7'),{target:{value:'123456.79'}});
 fireEvent.change(screen.getByLabelText('試算期間（月）#7'),{target:{value:'3'}});
 fireEvent.click(screen.getByText('手動試算'));
 await screen.findByText('租金基額（未進位）：9876.5432 元');
 expect(energyHttp.post).toHaveBeenCalledWith('/api/test/stations/70/rent-rules/7/preview',{incomeAmount:'123456.79',periodMonths:3});
 expect(screen.queryByText(/租金基額（試算）/)).toBeNull();
 expect(screen.getByText(/稅額：稅額待確認/)).toBeTruthy();
 fireEvent.change(screen.getByLabelText('手動收入（元）#7'),{target:{value:'200'}});
 expect(screen.queryByText('租金基額（未進位）：9876.5432 元')).toBeNull();
});
it('edits a fixed rule without requiring a fictitious income percentage',async()=>{
 const fixed={...entry,rule:{...entry.rule,status:'pending',calculationType:'fixed',fixedAmount:'12000',fixedUnit:'month',percent:null,sourceValues:{AW:12000},notes:['來源保留']}};
 vi.mocked(energyHttp.get).mockResolvedValue({data:{items:[fixed],canManage:true}} as any);
 vi.mocked(energyHttp.post).mockResolvedValue({data:fixed} as any);
 render(<StationRentRules base="/api/test" stationId={70}/>);
 fireEvent.click(await screen.findByText('編輯規則 #7'));
 expect(screen.queryByLabelText('租金比例（%）')).toBeNull();
 fireEvent.change(screen.getByLabelText('固定租金金額（元）'),{target:{value:'13000'}});
 fireEvent.click(screen.getByText('儲存租金規則'));
 await waitFor(()=>expect(energyHttp.post).toHaveBeenCalledWith('/api/test/stations/70/rent-rules',expect.objectContaining({fixedAmount:'13000',fixedUnit:'month',calculationType:'fixed',percent:null,sourceValues:{AW:12000},notes:['來源保留']})));
});

it('retains the percentage reference when a floor comparison still needs its tax basis',async()=>{
 vi.mocked(energyHttp.get).mockResolvedValue({data:{items:[{...entry,rule:{...entry.rule,calculationType:'percentageFloor',status:'pending'}}],canManage:false}} as any);
 vi.mocked(energyHttp.post).mockResolvedValue({data:{formulaSummary:'收入 × 8%，最低 8000 / month',referenceRent:'800',baseRentRaw:null,baseRent:null,missingInputs:['最低額稅基待確認']}} as any);
 render(<StationRentRules base="/api/test" stationId={70}/>);
 fireEvent.click(await screen.findByText('公式試算與待補條件 #7'));
 fireEvent.change(screen.getByLabelText('手動收入（元）#7'),{target:{value:'10000'}});
 fireEvent.click(screen.getByText('手動試算'));
 expect(await screen.findByText('比例計算參考額（尚未套用最低額／年度調整）：800 元')).toBeTruthy();
 expect(screen.getByText('・最低額稅基待確認')).toBeTruthy();
});
