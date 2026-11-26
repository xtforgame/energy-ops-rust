import * as React from 'react';
import { Alert, Button, Dialog, DialogActions, DialogContent, DialogTitle, Paper, Stack, Table, TableBody, TableCell, TableHead, TableRow, Typography } from '@mui/material';
import { energyHttp } from '@/api/energy';

type Entry = {contractNo:string;rentCalcCode:string|null;kind:string;landownerNames:string[];landownerType:string;voucher:string;baseRent:string|null;hasIssue:boolean};
type Application = {summary:string;periodStart:string;periodEnd:string;companyCode:string;companyName:string;feeDescription:string;entries:Entry[];total:string|null;state:string;paymentAllowed:boolean;note:string};
type NotPayable = {contractId:number;contractNo:string};
const landownerTypeLabel:Record<string,string>={company:'公司',individual:'個人',unknown:'待確認'};

/** S1（卡 20261126-0416）：同案場同一期多位地主合一張 EasyFlow 租金付款申請；S3：R-0 另列不需付款。
 *  純讀取彙整，不送 EasyFlow、不產生付款，草稿由各合約的「租金週期試算」存入。 */
export default function RentApplication({base,stationId}:{base:string;stationId:number}){
  const [open,setOpen]=React.useState(false),[busy,setBusy]=React.useState(false),[error,setError]=React.useState('');
  const [applications,setApplications]=React.useState<Application[]>([]);
  const [notPayable,setNotPayable]=React.useState<NotPayable[]>([]);
  const load=async()=>{
    setOpen(true);setBusy(true);setError('');
    try {
      const d=(await energyHttp.get(`${base}/stations/${stationId}/rent-application`)).data;
      setApplications(d.applications??[]);setNotPayable(d.notPayable??[]);
    } catch { setError('無法取得租金付款申請，請確認權限或稍後重試。'); } finally { setBusy(false); }
  };
  return <>
    <Button size="small" onClick={load} data-testid="rent-application-open">租金付款申請（同期多地主合併）</Button>
    <Dialog open={open} onClose={()=>!busy&&setOpen(false)} fullWidth maxWidth="lg">
      <DialogTitle>租金付款申請（同期多地主合併）</DialogTitle>
      <DialogContent>
        <Stack spacing={2}>
          {busy&&<Typography>正在彙整案場合約與租金付款草稿…</Typography>}
          {error&&<Alert severity="error">{error}</Alert>}
          {!!notPayable.length&&(
            <Paper variant="outlined" sx={{p:2}} data-testid="rent-application-not-payable">
              <Typography fontWeight={700} sx={{mb:1}}>不需付款</Typography>
              {notPayable.map(c=><Typography key={c.contractId} variant="body2">{c.contractNo}：R-0 業主自建案，不需付款，不產出租金計算表與付款申請</Typography>)}
            </Paper>
          )}
          {!busy&&!applications.length&&<Typography data-testid="rent-application-none">本案場沒有要付的租金</Typography>}
          {applications.map((a,i)=>(
            <Paper key={i} variant="outlined" sx={{p:2.5}} data-testid="rent-application-card">
              <Typography variant="h6">EasyFlow 租金付款填寫項目</Typography>
              <Typography>1. 公司名稱（負責付款公司）：{a.companyName}</Typography>
              <Typography>2. 費用種類：一般付款 &gt; 有合約／簽呈</Typography>
              <Typography>3. 申請摘要：{a.summary}</Typography>
              <Typography>4. 費用說明：{a.feeDescription}；付款日期限制：待補</Typography>
              <Typography>5. 付款對象（廠商）：{a.entries.flatMap(e=>e.landownerNames).join('、')}</Typography>
              <Typography>6. 總金額：{a.total??''}；幣別：TWD；小計：{a.total??''}</Typography>
              <Typography>7. 付款方式：匯款（請附存摺影本）</Typography>
              <Typography>8. 請安排於指定時間付款：待補</Typography>
              <Typography>9. 憑證單據：{a.entries.map(e=>e.voucher).join('；')}</Typography>
              <Typography variant="h6" sx={{mt:2}}>合約與計算明細</Typography>
              <Typography variant="body2" sx={{mt:.5}}>申請公司：{a.companyCode}／{a.companyName}</Typography>
              <Typography variant="body2" sx={{mt:.5}}>費用說明：{a.feeDescription}；繳費期限：待補</Typography>
              <Table size="small" sx={{mt:1.5}}>
                <TableHead><TableRow>{['合約編號','地主','地主類型','算法','應付租金','憑證單據'].map(h=><TableCell key={h}>{h}</TableCell>)}</TableRow></TableHead>
                <TableBody>
                  {a.entries.map((e,j)=>(
                    <TableRow key={j} data-testid="rent-application-entry">
                      <TableCell>{e.contractNo}</TableCell>
                      <TableCell>{e.landownerNames.join('、')}</TableCell>
                      <TableCell>{landownerTypeLabel[e.landownerType]??e.landownerType}</TableCell>
                      <TableCell>{e.kind}</TableCell>
                      <TableCell>{e.baseRent??'資料未齊，無法計算'}</TableCell>
                      <TableCell>{e.voucher}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
              <Typography sx={{mt:1.5}} fontWeight={700}>合計：{a.total??'資料未齊，無法計算'} 元</Typography>
              <Alert severity="info" sx={{mt:1.5}}>{a.note||'預覽草稿・尚未送出、未付款'}</Alert>
            </Paper>
          ))}
        </Stack>
      </DialogContent>
      <DialogActions><Button disabled={busy} onClick={()=>setOpen(false)}>關閉</Button></DialogActions>
    </Dialog>
  </>;
}
