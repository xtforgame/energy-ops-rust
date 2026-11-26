import * as React from 'react';
import {Alert,Box,Button,MenuItem,TextField,Typography} from '@mui/material';
import {energyHttp} from '@/api/energy';
import {rentalLabels,rentalEditable,type RentalApplication} from './rentalFormFields';
type Case={payee?:string;paymentDeadline?:string;formulaSummary?:string;stationId:number;ruleId?:number;periodStart:string;periodEnd:string;grossRent:string|null;application?:RentalApplication;invoiceTiming?:string};
export default function RentalApplicationPanel({base,r,onSaved,onDirty}:{base:string;r:Case;onSaved:(r:any)=>void;onDirty:(dirty:boolean)=>void}){
 const [fields,setFields]=React.useState(r.application?.fields??{}),[timing,setTiming]=React.useState(r.application?.invoiceTiming??'unknown'),[ids,setIds]=React.useState((r.application?.voucherAttachmentIds??[]).join(',')),[busy,setBusy]=React.useState(false),[error,setError]=React.useState('');
 React.useEffect(()=>{setFields(r.application?.fields??{});setTiming(r.application?.invoiceTiming??'unknown');setIds((r.application?.voucherAttachmentIds??[]).join(','));onDirty(false);},[r]);
 if(!r.application)return null;
 const save=async(action:'save'|'confirm')=>{setBusy(true);setError('');try{
  const voucherAttachmentIds=ids.trim()?ids.split(',').map(s=>Number(s.trim())):[];
  if(voucherAttachmentIds.some(id=>!Number.isSafeInteger(id)||id<=0))throw Error('invalidIds');
  const {data}=await energyHttp.post(`${base}/stations/${r.stationId}/rent-payment/application`,{ruleId:r.ruleId??null,periodStart:r.periodStart,periodEnd:r.periodEnd,sourceRevision:r.application!.sourceRevision,draftRevision:r.application!.draftRevision,fields:Object.fromEntries([...rentalEditable,'companyName'].map(k=>[k,fields[k]??null])),invoiceTiming:timing,voucherAttachmentIds,action});onSaved(data);
 }catch(e:any){setError(e?.response?.status===409?'來源或草稿已更新，請重新開啟表單，避免覆蓋他人修改。':e?.response?.status===403?'目前帳號沒有租金覆核權限。':'未儲存，請核對計算缺項、附件編號與權限。');}finally{setBusy(false);}};
 return <Box sx={{my:2}}><Typography variant="h6">EasyFlow 租金付款申請資料</Typography>
 {error&&<Alert severity="error">{error}</Alert>}{r.application.stale&&<Alert severity="warning">來源或規則已改變，先前確認已失效；請重新核對。</Alert>}
 {r.application.state==='dataConfirmed'&&<Alert severity="success">已由一位租金審核人員確認資料；尚未送出 EasyFlow、未執行付款。</Alert>}
 {!!r.application.missingFields?.length&&<Alert severity="warning">交接前待補：{r.application.missingFields.map(k=>rentalLabels[k]??k).join('、')}。可先保存草稿。</Alert>}
 <Box sx={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:2,my:2}}>{['companyName','contractOption','summary','expenseDescription','payee','amount','currency','subtotal','paymentMethod','paymentDate','voucherStatus','companyCode','department','applicant','applicationDate'].map(k=><TextField key={k} fullWidth size="small" label={rentalLabels[k]} value={({expenseDescription:r.formulaSummary,payee:r.payee,amount:r.grossRent,currency:'TWD',subtotal:r.grossRent} as Record<string,string|null|undefined>)[k]??fields[k]??''} disabled={busy||['expenseDescription','payee','amount','currency','subtotal'].includes(k)} onChange={e=>{setFields(v=>({...v,[k]:e.target.value}));onDirty(true);}}/>)}</Box>
 <TextField fullWidth select label="發票方式（儲存後生效）" value={timing} disabled={busy} onChange={e=>{setTiming(e.target.value as typeof timing);onDirty(true);}}><MenuItem value="unknown">待確認</MenuItem><MenuItem value="before">先附發票</MenuItem><MenuItem value="later">發票後補</MenuItem></TextField>
 <TextField fullWidth sx={{my:2}} label="已收到的發票／收據附件編號（多筆以逗號分隔）" helperText="填平台已上傳附件的編號；留空表示尚未收到，不會因選擇先附就視為齊件。" value={ids} disabled={busy} onChange={e=>{setIds(e.target.value);onDirty(true);}}/>
 <Button disabled={busy||!r.periodStart||!r.periodEnd} onClick={()=>save('save')}>儲存租金申請草稿</Button>
 {r.application.canReview&&<Button disabled={busy||!r.periodStart||r.grossRent==null||r.application.canConfirm===false} variant="contained" onClick={()=>save('confirm')}>確認本期資料（任一人通過）</Button>}
 </Box>;
}
