import * as React from 'react';
import {rentCycleDocument} from './rentCycleDocument';
import { Alert, Button, Checkbox, Dialog, DialogActions, DialogContent, DialogTitle, MenuItem, Stack, Table, TableBody, TableCell, TableHead, TableRow, TextField, Typography } from '@mui/material';
import { energyHttp } from '@/api/energy';

type Plan = {stationId:number;anchor:string;months:number;transfer:boolean;kind:string;basis:string;sharePercent:string|null;area:string|null;unitPrice:string|null;unitPeriod:string;roundingPlaces:number;feeKind?:string|null;monthlyMinimum?:string|null;commercialOperationDate?:string|null;annualAmount?:string|null;ratePerKwh?:string|null};
type Row = {billId:number;start:string|null;end:string|null;documentNo:string|null;attachmentId:number|null;source:string;untaxed:string;gross:string;kwh:string;selected:boolean};
const message=(e:any)=>e?.response?.data?.details?.message || e?.response?.data?.error?.details?.message || '操作失敗：請確認權限、合約效期及輸入資料，修改後重試。';
const text=(v:unknown)=>v==null?'':String(v);

/** Separate rent-cycle preparation, never invokes the per-bill payout workflow. */
export default function RentCycle({base,contractId,stationId,rentCalcCode}:{base:string;contractId:number;stationId:number;rentCalcCode?:string|null}) {
  const [open,setOpen]=React.useState(false),[busy,setBusy]=React.useState(false),[error,setError]=React.useState('');
  const [plan,setPlan]=React.useState<Plan>({stationId,anchor:'',months:3,transfer:false,kind:'revenueShare',basis:'untaxed',sharePercent:null,area:null,unitPrice:null,unitPeriod:'month',roundingPlaces:0});
  const [transferChoice,setTransferChoice]=React.useState('');
  const [start,setStart]=React.useState(''),[rows,setRows]=React.useState<Row[]>([]),[result,setResult]=React.useState<any>(null),[dirty,setDirty]=React.useState(true);
  const [invoiceTiming,setInvoiceTiming]=React.useState('unknown'),[draftSaved,setDraftSaved]=React.useState<{start:string;end:string}|null>(null);
  // 合約改綁到別的案場時，試算原本設在哪個案場；null＝沒有衝突。確認過才能覆蓋存檔。
  const [rebindFrom,setRebindFrom]=React.useState<{id:number;name:string}|null>(null);
  const [confirmRebind,setConfirmRebind]=React.useState(false);
  const endpoint=`${base}/contracts/${contractId}/rent-cycle`;
  const change=(key:keyof Plan,value:any)=>{setPlan(p=>({...p,[key]:value}));setDirty(true);setResult(null);};
  const nameOfStation=async(id:number)=>{
    try {const list=(await energyHttp.get(`${base}/station-list`)).data;
      return (Array.isArray(list)?list:[]).find((s:any)=>s.id===id)?.name ?? `#${id}`;
    }catch{return `#${id}`;}
  };
  const load=async()=>{
    setOpen(true);setBusy(true);setError('');setResult(null);setRows([]);setDirty(true);setRebindFrom(null);setConfirmRebind(false);
    try {const p=(await energyHttp.get(endpoint)).data.plan;
      if(p && p.stationId!==stationId) {setRebindFrom({id:p.stationId,name:await nameOfStation(p.stationId)});return;}
      if(p){setPlan(p);setTransferChoice(String(p.transfer));setStart(p.anchor);setDirty(false);}
    }catch(e){setError(message(e));}finally{setBusy(false);}
  };
  const loadBills=async()=>{
    setBusy(true);setError('');setResult(null);setRows([]);
    try {
      if(dirty) {
        const url=confirmRebind?`${endpoint}?confirmStationChange=true`:endpoint;
        try {await energyHttp.post(url,plan);}
        catch(e:any){
          const code=e?.response?.data?.code || e?.response?.data?.error?.code;
          const from=e?.response?.data?.details?.fromStationId ?? e?.response?.data?.error?.details?.fromStationId;
          if(code==='rent_multisite_contract_requires_confirmation' && from!=null) {setRebindFrom({id:from,name:await nameOfStation(from)});return;}
          throw e;
        }
        setDirty(false);
        // 存檔成功＝衝突已經解決（已經改用本案場的試算）：警告與確認鈕收起來，不要留著誤導。
        setRebindFrom(null);setConfirmRebind(false);
      }
      const v=(await energyHttp.get(`${endpoint}/bills?start=${encodeURIComponent(start)}`)).data;
      setRows(v.items.map((b:any)=>({billId:b.billId,start:b.start,end:b.end,documentNo:b.documentNo,attachmentId:b.attachmentId,
        source:['renewableSurplus','renewableNoSurplus'].includes(b.docType)?'taipower':b.docType==='retailerSale'?'transfer':'',
        untaxed:text(b.details?.purchaseEnergyFee),gross:text(b.docType==='retailerSale'?b.details?.payableTotal:b.details?.electricityFeeTotal),kwh:text(b.details?.purchaseKwh),selected:false})));
    }catch(e){setError(message(e));}finally{setBusy(false);}
  };
  const updateRow=(id:number,key:keyof Row,value:any)=>{setResult(null);setRows(rs=>rs.map(r=>r.billId===id?{...r,[key]:value}:r));};
  const calculate=async()=>{
    setBusy(true);setError('');setResult(null);setDraftSaved(null);
    try {setResult((await energyHttp.post(`${endpoint}/preview`,{start,receipts:rows.filter(r=>r.selected).map(r=>({billId:r.billId,source:r.source,
      untaxed:r.untaxed.trim()||null,gross:r.gross.trim()||null,kwh:r.kwh.trim()||null}))})).data);}
    catch(e){setError(message(e));}finally{setBusy(false);}
  };
  /** S2：R-A 零單據也能存，不經 POST /bills/{id}/rent-payment（決定 1）。 */
  const saveDraft=async()=>{
    setBusy(true);setError('');
    try {
      const r=(await energyHttp.post(`${endpoint}/payment-draft`,{start,receipts:rows.filter(r=>r.selected).map(r=>({billId:r.billId,source:r.source,
        untaxed:r.untaxed.trim()||null,gross:r.gross.trim()||null,kwh:r.kwh.trim()||null})),invoiceTiming})).data;
      setDraftSaved({start:r.periodStart,end:r.periodEnd});
    } catch(e){setError(message(e));} finally{setBusy(false);}
  };
  const download=()=>{
    const url=URL.createObjectURL(new Blob([rentCycleDocument(result)],{type:'text/html;charset=utf-8'}));
    const a=document.createElement('a');a.href=url;a.download=`rent-${contractId}-${start}.html`;a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);
  };
  return <>
    <Button size="small" onClick={()=>{if(rentCalcCode==='R-0'){setOpen(true);}else{load();}}}>租金週期試算</Button>
    <Dialog open={open} onClose={()=>!busy&&setOpen(false)} fullWidth maxWidth="xl">
      <DialogTitle>地主租金週期試算</DialogTitle>
      <DialogContent>
        {rentCalcCode==='R-0' ? (
          <Alert severity="info" data-testid="rent-cycle-not-payable">R-0 業主自建案：不需付款，不產出租金計算表與付款申請</Alert>
        ) : (
        <Stack spacing={2}>
          <Alert severity="info">依合約付款週期彙整。這裡收取地主發票；與電費售電開票分開。設定僅供試算，不啟用既有逐單租金流程，也不產生付款。</Alert>
          <Alert severity="warning">稅費、扣繳與補充保費待實際範例確認，目前只算租金基底。面積與單價必須使用相同單位。轉供案場須收齊台電與轉供單據，零元也須有單據。</Alert>
          {error&&<Alert severity="error">{error}</Alert>}
          {rebindFrom&&(
            <Alert severity="warning" action={
              <Button color="inherit" size="small" data-testid="rent-cycle-confirm-rebind" onClick={()=>{setConfirmRebind(true);setDirty(true);}}>
                確認改用本案場試算
              </Button>
            }>
              這份合約的試算原本設在 {rebindFrom.name}。確認後會改用本案場試算，覆蓋原本的設定。
            </Alert>
          )}
          <Stack direction="row" spacing={2} flexWrap="wrap" useFlexGap>
            <TextField select label="費用性質" value={plan.feeKind??'rent'} onChange={e=>change('feeKind',e.target.value)}><MenuItem value="rent">租金</MenuItem><MenuItem value="management">管理費</MenuItem></TextField>
            <TextField label="第一期起日（每月一日）" type="date" inputProps={{'data-testid':'rent-cycle-anchor-date'}} InputLabelProps={{shrink:true}} value={plan.anchor} onChange={e=>{change('anchor',e.target.value);setStart(e.target.value);}}/>
            <TextField label="每幾個月付款" type="number" value={plan.months} inputProps={{min:1,max:12}} onChange={e=>change('months',Number(e.target.value))}/>
            <TextField select data-testid="rent-cycle-transfer-select" label="此期間是否已轉供" sx={{minWidth:170}} value={transferChoice} onChange={e=>{setTransferChoice(e.target.value);change('transfer',e.target.value==='true');}}>
              <MenuItem value="false">未轉供</MenuItem><MenuItem value="true">已轉供</MenuItem>
            </TextField>
            <TextField select data-testid="rent-cycle-kind-select" label="租金方式" value={plan.kind} onChange={e=>{change('kind',e.target.value);if(e.target.value!=='revenueShare')change('monthlyMinimum',null);}}>
              <MenuItem value="fixedArea">面積 × 固定單價</MenuItem>
              <MenuItem value="revenueShare">電費收入 × 比例</MenuItem>
              <MenuItem value="fixedAnnual">整組年額（不分面積）</MenuItem>
              <MenuItem value="annualTiered">年度基準＋級距調整</MenuItem>
              <MenuItem value="kwhRate">度數 × 每度費率 × 比例</MenuItem>
            </TextField>
            {plan.kind==='fixedArea'&&<>
              <TextField label="面積" value={plan.area??''} onChange={e=>change('area',e.target.value)}/>
              <TextField label="每單位面積租金" value={plan.unitPrice??''} onChange={e=>change('unitPrice',e.target.value)}/>
              <TextField select label="單價週期" value={plan.unitPeriod} onChange={e=>change('unitPeriod',e.target.value)}><MenuItem value="month">每月</MenuItem><MenuItem value="cycle">整個付款週期</MenuItem></TextField>
            </>}
            {plan.kind==='revenueShare'&&<TextField label="租金比例（%）" value={plan.sharePercent??''} onChange={e=>change('sharePercent',e.target.value)}/>}
            {plan.kind==='fixedAnnual'&&<TextField label="整組年額" inputProps={{'data-testid':'rent-cycle-annual-amount'}} value={plan.annualAmount??''} onChange={e=>change('annualAmount',e.target.value)}/>}
            {plan.kind==='annualTiered'&&<>
              <TextField label="年度基準（經營年租金）" value={plan.annualAmount??''} onChange={e=>change('annualAmount',e.target.value)}/>
              <TextField label="售電回饋百分比（%）" value={plan.sharePercent??''} onChange={e=>change('sharePercent',e.target.value)}/>
            </>}
            {plan.kind==='kwhRate'&&<>
              <TextField label="每度費率" value={plan.ratePerKwh??''} onChange={e=>change('ratePerKwh',e.target.value)}/>
              <TextField label="比例（%）" value={plan.sharePercent??''} onChange={e=>change('sharePercent',e.target.value)}/>
            </>}
            <TextField select label="收入金額基底" value={plan.basis} onChange={e=>change('basis',e.target.value)}><MenuItem value="untaxed">未稅電費收入</MenuItem><MenuItem value="gross">含稅電費收入</MenuItem></TextField>
            {plan.kind==='revenueShare'&&<TextField label="每月最低金額（無保底留空）" value={plan.monthlyMinimum??''} onChange={e=>change('monthlyMinimum',e.target.value.trim()||null)}/>}
            <TextField label="正式併聯發電日（起算條款適用時）" type="date" InputLabelProps={{shrink:true}} value={plan.commercialOperationDate??''} onChange={e=>change('commercialOperationDate',e.target.value||null)}/>
          </Stack>
          {plan.monthlyMinimum!=null&&<Alert severity="warning">每月保底需明確月別收入及正式併聯日期；先逐月取「收入 × 比例」與保底的較大值，再合計並依設定進位。整期併聯前不計費；首期不足月、跨月收入拆分及正式進位方式待確認，不自動推算。</Alert>}
          <Stack direction="row" spacing={2}>
            <TextField label="本次計算起日" type="date" InputLabelProps={{shrink:true}} value={start} onChange={e=>{setStart(e.target.value);setRows([]);setResult(null);}}/>
            <Button disabled={busy||!transferChoice||!start||!plan.anchor||(rebindFrom!=null&&!confirmRebind)} onClick={loadBills}>儲存試算設定並載入單據</Button>
          </Stack>
          <Typography variant="body2">請勾選本期收入單據並確認來源。收入使用購電電費／電費合計，不使用扣除電表租費後的淨額。缺值保持空白；可在此補入已確認數字，原始 OCR 不會被覆寫。跨期、重疊或多電號情況需先釐清，不自動分攤。</Typography>
          <Table size="small"><TableHead><TableRow>{['選用','單據／原檔','帳期','來源','未稅收入','含稅收入','度數'].map(x=><TableCell key={x}>{x}</TableCell>)}</TableRow></TableHead>
            <TableBody>{rows.map(r=><TableRow key={r.billId}>
              <TableCell><Checkbox checked={r.selected} onChange={e=>updateRow(r.billId,'selected',e.target.checked)} inputProps={{'aria-label':`選用單據 ${r.billId}`}}/></TableCell>
              <TableCell>{r.documentNo||`#${r.billId}`} {r.attachmentId&&<a href={`${base}/attachments/${r.attachmentId}/download`} target="_blank" rel="noreferrer">原檔</a>}</TableCell>
              <TableCell>{r.start||'缺起日'}～{r.end||'缺迄日'}</TableCell>
              <TableCell><TextField select size="small" value={r.source} sx={{minWidth:100}} onChange={e=>updateRow(r.billId,'source',e.target.value)}><MenuItem value="">待確認</MenuItem><MenuItem value="taipower">台電</MenuItem><MenuItem value="transfer">轉供</MenuItem></TextField></TableCell>
              {(['untaxed','gross','kwh'] as const).map(k=><TableCell key={k}><TextField size="small" value={r[k]} onChange={e=>updateRow(r.billId,k,e.target.value)} inputProps={{'aria-label':`${r.billId} ${k}`}}/></TableCell>)}
            </TableRow>)}</TableBody>
          </Table>
          <Button disabled={busy||dirty||!start} variant="contained" onClick={calculate}>檢查收齊狀態並試算</Button>
          {result&&<>
            {result.issues.map((s:string,i:number)=><Alert key={i} severity="warning">{s}</Alert>)}
            <Table size="small"><TableHead><TableRow><TableCell>來源</TableCell><TableCell>收件狀態</TableCell><TableCell>收入基底合計</TableCell><TableCell>度數合計</TableCell></TableRow></TableHead><TableBody>
              {['taipower','transfer'].map(s=><TableRow key={s}><TableCell>{s==='taipower'?'台電':'轉供'}</TableCell><TableCell>{{received:'已收齊',missing:'缺單',needsReview:'待確認',notApplicable:'不適用',notRequired:'本期不需單據計算'}[result.sources[s].status as string]}</TableCell><TableCell>{result.sources[s].amount??'—'}</TableCell><TableCell>{result.sources[s].kwh??'—'}</TableCell></TableRow>)}
            </TableBody></Table>
            {!!result.monthlyBreakdown?.length&&<Table size="small"><TableHead><TableRow>{['月份','收入基底','比例計算','每月保底','保底後金額（未進位）'].map(s=><TableCell key={s}>{s}</TableCell>)}</TableRow></TableHead><TableBody>
              {result.monthlyBreakdown.map((m:any)=><TableRow key={m.month}>{['month','income','proportionalAmount','minimum','amountAfterFloor'].map(k=><TableCell key={k}>{m[k]}</TableCell>)}</TableRow>)}
            </TableBody></Table>}
            {result.chargeStatus==='notStarted'&&<Alert severity="info">整個週期均在正式併聯發電日前，不計費。</Alert>}
            {plan.kind==='annualTiered'&&<Typography>參考年租金：{result.referenceRent??'—'} 元；比值：{result.ratioPercent??'—'}%；套用級距：{result.tierPercent??'—'}%</Typography>}
            <Typography>本期：{result.periodStart}～{result.periodEnd}；{result.feeKind==='management'?'管理費':'租金'}基礎試算：{result.baseRent??'資料未齊，無法計算'} 元</Typography>
            <Alert severity="warning">尚非付款金額。稅費與扣繳待確認；正式送審、付款及排程待後續串接。</Alert>
            <Button onClick={download}>下載可閱讀租金試算表</Button>
            <Stack direction="row" spacing={2} alignItems="center">
              <TextField select size="small" data-testid="rent-cycle-invoice-timing-select" label="憑證安排" sx={{minWidth:140}} value={invoiceTiming} onChange={e=>setInvoiceTiming(e.target.value)}>
                <MenuItem value="unknown">未定</MenuItem>
                <MenuItem value="before">先附發票</MenuItem>
                <MenuItem value="later">後補</MenuItem>
              </TextField>
              <Button disabled={busy||!result.baseRent} variant="contained" data-testid="rent-cycle-save-payment-draft" onClick={saveDraft}>存成租金付款草稿</Button>
            </Stack>
            {draftSaved&&<Alert severity="success">已存成 {draftSaved.start}～{draftSaved.end} 的租金付款草稿</Alert>}
          </>}
        </Stack>
        )}
      </DialogContent><DialogActions><Button disabled={busy} onClick={()=>setOpen(false)}>關閉</Button></DialogActions>
    </Dialog>
  </>;
}
