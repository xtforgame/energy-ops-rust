import * as React from 'react';
import { Alert, Button, Checkbox, Dialog, DialogActions, DialogContent, DialogTitle, MenuItem, Stack, Table, TableBody, TableCell, TableHead, TableRow, TextField, Typography } from '@mui/material';
import { energyHttp } from '@/api/energy';

type Plan = {stationId:number;anchor:string;months:number;transfer:boolean;kind:string;basis:string;sharePercent:string|null;area:string|null;unitPrice:string|null;unitPeriod:string;roundingPlaces:number;feeKind?:string|null;monthlyMinimum?:string|null;commercialOperationDate?:string|null};
type Row = {billId:number;start:string|null;end:string|null;documentNo:string|null;attachmentId:number|null;source:string;untaxed:string;gross:string;kwh:string;selected:boolean};
const message=(e:any)=>e?.response?.data?.details?.message || e?.response?.data?.error?.details?.message || '操作失敗：請確認權限、合約效期及輸入資料，修改後重試。';
const text=(v:unknown)=>v==null?'':String(v);

/** Separate rent-cycle preparation, never invokes the per-bill payout workflow. */
export default function RentCycle({base,contractId,stationId}:{base:string;contractId:number;stationId:number}) {
  const [open,setOpen]=React.useState(false),[busy,setBusy]=React.useState(false),[error,setError]=React.useState('');
  const [plan,setPlan]=React.useState<Plan>({stationId,anchor:'',months:3,transfer:false,kind:'revenueShare',basis:'untaxed',sharePercent:null,area:null,unitPrice:null,unitPeriod:'month',roundingPlaces:2});
  const [transferChoice,setTransferChoice]=React.useState('');
  const [start,setStart]=React.useState(''),[rows,setRows]=React.useState<Row[]>([]),[result,setResult]=React.useState<any>(null),[dirty,setDirty]=React.useState(true);
  const endpoint=`${base}/contracts/${contractId}/rent-cycle`;
  const change=(key:keyof Plan,value:any)=>{setPlan(p=>({...p,[key]:value}));setDirty(true);setResult(null);};
  const load=async()=>{
    setOpen(true);setBusy(true);setError('');setResult(null);setRows([]);setDirty(true);
    try {const p=(await energyHttp.get(endpoint)).data.plan;
      if(p && p.stationId!==stationId) {setError('此合約已為另一案場設定試算。請先拆分或確認跨案場合約規則，不可覆蓋。');return;}
      if(p){setPlan(p);setTransferChoice(String(p.transfer));setStart(p.anchor);setDirty(false);}
    }catch(e){setError(message(e));}finally{setBusy(false);}
  };
  const loadBills=async()=>{
    setBusy(true);setError('');setResult(null);setRows([]);
    try {
      if(dirty) {await energyHttp.post(endpoint,plan);setDirty(false);}
      const v=(await energyHttp.get(`${endpoint}/bills?start=${encodeURIComponent(start)}`)).data;
      setRows(v.items.map((b:any)=>({billId:b.billId,start:b.start,end:b.end,documentNo:b.documentNo,attachmentId:b.attachmentId,
        source:['renewableSurplus','renewableNoSurplus'].includes(b.docType)?'taipower':'',
        untaxed:text(b.details?.purchaseEnergyFee),gross:text(b.details?.electricityFeeTotal),kwh:text(b.details?.purchaseKwh),selected:false})));
    }catch(e){setError(message(e));}finally{setBusy(false);}
  };
  const updateRow=(id:number,key:keyof Row,value:any)=>{setResult(null);setRows(rs=>rs.map(r=>r.billId===id?{...r,[key]:value}:r));};
  const calculate=async()=>{
    setBusy(true);setError('');setResult(null);
    try {setResult((await energyHttp.post(`${endpoint}/preview`,{start,receipts:rows.filter(r=>r.selected).map(r=>({billId:r.billId,source:r.source,
      untaxed:r.untaxed.trim()||null,gross:r.gross.trim()||null,kwh:r.kwh.trim()||null}))})).data);}
    catch(e){setError(message(e));}finally{setBusy(false);}
  };
  const download=()=>{
    const url=URL.createObjectURL(new Blob([JSON.stringify(result,null,2)],{type:'application/json'}));
    const a=document.createElement('a');a.href=url;a.download=`rent-${contractId}-${start}.json`;a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);
  };
  return <>
    <Button size="small" onClick={load}>租金週期試算</Button>
    <Dialog open={open} onClose={()=>!busy&&setOpen(false)} fullWidth maxWidth="xl">
      <DialogTitle>地主租金週期試算</DialogTitle>
      <DialogContent>
        <Stack spacing={2}>
          <Alert severity="info">依合約付款週期彙整。這裡收取地主發票；與電費售電開票分開。設定僅供試算，不啟用既有逐單租金流程，也不產生付款。</Alert>
          <Alert severity="warning">稅費、扣繳與補充保費待實際範例確認，目前只算租金基底。面積與單價必須使用相同單位。轉供案場須收齊台電與轉供單據，零元也須有單據。</Alert>
          {error&&<Alert severity="error">{error}</Alert>}
          <Stack direction="row" spacing={2} flexWrap="wrap" useFlexGap>
            <TextField select label="費用性質" value={plan.feeKind??'rent'} onChange={e=>change('feeKind',e.target.value)}><MenuItem value="rent">租金</MenuItem><MenuItem value="management">管理費</MenuItem></TextField>
            <TextField label="第一期起日（每月一日）" type="date" InputLabelProps={{shrink:true}} value={plan.anchor} onChange={e=>{change('anchor',e.target.value);setStart(e.target.value);}}/>
            <TextField label="每幾個月付款" type="number" value={plan.months} inputProps={{min:1,max:12}} onChange={e=>change('months',Number(e.target.value))}/>
            <TextField select label="此期間是否已轉供" sx={{minWidth:170}} value={transferChoice} onChange={e=>{setTransferChoice(e.target.value);change('transfer',e.target.value==='true');}}>
              <MenuItem value="false">未轉供</MenuItem><MenuItem value="true">已轉供</MenuItem>
            </TextField>
            <TextField select label="租金方式" value={plan.kind} onChange={e=>{change('kind',e.target.value);if(e.target.value==='fixedArea')change('monthlyMinimum',null);}}><MenuItem value="fixedArea">面積 × 固定單價</MenuItem><MenuItem value="revenueShare">電費收入 × 比例</MenuItem></TextField>
            {plan.kind==='fixedArea'?<>
              <TextField label="面積" value={plan.area??''} onChange={e=>change('area',e.target.value)}/>
              <TextField label="每單位面積租金" value={plan.unitPrice??''} onChange={e=>change('unitPrice',e.target.value)}/>
              <TextField select label="單價週期" value={plan.unitPeriod} onChange={e=>change('unitPeriod',e.target.value)}><MenuItem value="month">每月</MenuItem><MenuItem value="cycle">整個付款週期</MenuItem></TextField>
            </>:<TextField label="租金比例（%）" value={plan.sharePercent??''} onChange={e=>change('sharePercent',e.target.value)}/>}
            <TextField select label="收入金額基底" value={plan.basis} onChange={e=>change('basis',e.target.value)}><MenuItem value="untaxed">未稅電費收入</MenuItem><MenuItem value="gross">含稅電費收入</MenuItem></TextField>
            <TextField select label="四捨五入位數" value={plan.roundingPlaces} onChange={e=>change('roundingPlaces',Number(e.target.value))}><MenuItem value={0}>整元</MenuItem><MenuItem value={1}>1 位</MenuItem><MenuItem value={2}>2 位</MenuItem></TextField>
            {plan.kind==='revenueShare'&&<TextField label="每月最低金額（無保底留空）" value={plan.monthlyMinimum??''} onChange={e=>change('monthlyMinimum',e.target.value.trim()||null)}/>}
            <TextField label="正式併聯發電日（起算條款適用時）" type="date" InputLabelProps={{shrink:true}} value={plan.commercialOperationDate??''} onChange={e=>change('commercialOperationDate',e.target.value||null)}/>
          </Stack>
          {plan.monthlyMinimum!=null&&<Alert severity="warning">每月保底需明確月別收入及正式併聯日期；先逐月取「收入 × 比例」與保底的較大值，再合計並依設定進位。整期併聯前不計費；首期不足月、跨月收入拆分及正式進位方式待確認，不自動推算。</Alert>}
          <Stack direction="row" spacing={2}>
            <TextField label="本次計算起日" type="date" InputLabelProps={{shrink:true}} value={start} onChange={e=>{setStart(e.target.value);setRows([]);setResult(null);}}/>
            <Button disabled={busy||!transferChoice||!start||!plan.anchor||Boolean(error.includes('另一案場'))} onClick={loadBills}>儲存試算設定並載入單據</Button>
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
            <Typography>本期：{result.periodStart}～{result.periodEnd}；{result.feeKind==='management'?'管理費':'租金'}基礎試算：{result.baseRent??'資料未齊，無法計算'} 元</Typography>
            <Alert severity="warning">尚非付款金額。稅費與扣繳待確認；正式送審、付款及排程待後續串接。</Alert>
            <Button onClick={download}>匯出試算 JSON（含來源與待確認事項）</Button>
          </>}
        </Stack>
      </DialogContent><DialogActions><Button disabled={busy} onClick={()=>setOpen(false)}>關閉</Button></DialogActions>
    </Dialog>
  </>;
}
