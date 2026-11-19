import * as React from 'react';
import { Alert, Button, Dialog, DialogActions, DialogContent, DialogTitle, Stack, Table, TableBody, TableCell, TableHead, TableRow, Typography } from '@mui/material';
import DownloadIcon from '@mui/icons-material/Download';
import { energyHttp } from '@/api/energy';

type Application = {
  applicationType: string;
  fields: Array<{key:string;label:string;value:string|null;source:string;status:string}>;
  pending: Array<{key:string;message:string}>;
  attachments: Array<{id:number;originalFilename:string}>;
};

function download(data: BlobPart, type: string, filename: string) {
  const url = URL.createObjectURL(new Blob([data], {type}));
  const a = document.createElement('a'); a.href=url; a.download=filename;
  document.body.appendChild(a); a.click(); a.remove();
  setTimeout(()=>URL.revokeObjectURL(url), 1000);
}

/** A read-only saved-data snapshot, available before contract/workflow setup is complete. */
export default function EasyFlowApplication({base,billId}:{base:string;billId:number}) {
  const [open,setOpen]=React.useState(false);
  const [loading,setLoading]=React.useState(false);
  const [data,setData]=React.useState<Application|null>(null);
  const [error,setError]=React.useState('');
  const endpoint=`${base}/bills/${billId}/easyflow-application`;
  const load=async()=>{
    setOpen(true);setLoading(true);setData(null);setError('');
    try {setData((await energyHttp.get(endpoint)).data);}
    catch {setError('無法取得申請資料，請重試。');}
    finally {setLoading(false);}
  };
  const csv=async()=>{
    setError('');
    try {
      const r=await energyHttp.get(`${endpoint}?format=csv`,{responseType:'blob'});
      download(r.data,'text/csv;charset=utf-8',`easyflow-bill-${billId}.csv`);
    } catch {setError('下載失敗，請重試。');}
  };
  return <>
    <Button variant="outlined" startIcon={<DownloadIcon />} onClick={load} data-testid="easyflow-prepare">
      準備 EasyFlow 申請資料
    </Button>
    <Dialog open={open} onClose={()=>setOpen(false)} fullWidth maxWidth="lg">
      <DialogTitle>EasyFlow 申請資料{data ? `／${data.applicationType==='invoice'?'開票':data.applicationType==='payment'?'付款':'類型待確認'}`:''}</DialogTitle>
      <DialogContent>
        <Alert severity="info" sx={{mb:2}}>以目前已儲存資料產生草稿；請先儲存校對內容再開啟。匯出不代表核准，也不會送出 EasyFlow。JSON 含來源附件清單，CSV 可用 Excel 開啟。</Alert>
        {loading && <Typography>讀取中…</Typography>}
        {error && <Alert severity="error">{error}</Alert>}
        {data && <>
          <Stack spacing={1} sx={{mb:2}}>{data.pending.map(p=><Alert severity="warning" key={p.key}>{p.message}</Alert>)}</Stack>
          <Table size="small"><TableHead><TableRow><TableCell>欄位</TableCell><TableCell>值</TableCell><TableCell>來源</TableCell></TableRow></TableHead>
            <TableBody>{data.fields.map(f=><TableRow key={f.key} sx={f.status==='pending'?{backgroundColor:'#fff4ce'}:{}}>
              <TableCell>{f.label}</TableCell><TableCell>{f.value??'待確認／未取得'}</TableCell><TableCell>{f.source}</TableCell>
            </TableRow>)}</TableBody>
          </Table>
          <Typography variant="subtitle2" sx={{mt:2}}>來源附件</Typography>
          {data.attachments.length ? data.attachments.map(a=><Typography key={a.id}>{a.originalFilename}</Typography>):<Typography>尚未取得來源附件</Typography>}
        </>}
      </DialogContent>
      <DialogActions>
        <Button onClick={()=>setOpen(false)}>關閉</Button>
        {data && <><Button onClick={csv}>下載 CSV（Excel 開啟）</Button>
          <Button variant="contained" onClick={()=>download(JSON.stringify(data,null,2),'application/json',`easyflow-bill-${billId}.json`)}>下載 JSON（自動化用）</Button></>}
      </DialogActions>
    </Dialog>
  </>;
}
