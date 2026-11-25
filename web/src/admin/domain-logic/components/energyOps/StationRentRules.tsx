import * as React from 'react';
import {Accordion, AccordionSummary, AccordionDetails, Alert, Button, Card, CardContent, Chip, Dialog, DialogActions, DialogContent, DialogTitle, MenuItem, Stack, Table, TableBody, TableCell, TableRow, TextField, Typography} from '@mui/material';
import {energyHttp} from '@/api/energy';
import {apiErrorPayload} from '../../../dataProvider/apiError';

type Rule = {calculationType?:string;sourceValues?:Record<string,unknown>;sourceRow?:number;sourceProjectCode?:string;notes?:string[];payee?:string|null;basis?:string|null;percent?:string|null;taxPercent?:string|null;rounding?:string|null;transfer?:boolean|null;cycleMonths?:number|null;cycleAnchor?:string|null;validFrom?:string|null;validTo:string|null;status:string;ruleSource:string;leaseEvidence:{status:string;attachmentIds:number[]}};
type Entry = {id:number;version:number;rule:Rule};
const evidenceLabels:Record<string,string> = {missing:'合約尚缺',excerpt:'已有租金頁摘錄',complete:'完整合約已備'};
const sourceTerms:Record<string,string> = {AS:'面積',AT:'申報地價',AU:'年租率',AV:'稅額',AW:'固定金額',AY:'收入含稅',AZ:'稅額處理',BA:'比例',BB:'金額',BC:'週期',BD:'付款時點'};
const emptyRule = ():Rule => ({payee:'',basis:'untaxed',percent:'',taxPercent:'',rounding:'perReceipt',transfer:false,cycleMonths:3,cycleAnchor:'',validFrom:'',validTo:null,status:'pending',ruleSource:'',leaseEvidence:{status:'missing',attachmentIds:[]}});

/** The rule is reusable; lease evidence is tracked independently of confirmation. */
export default function StationRentRules({base,stationId}:{base:string;stationId:number}) {
  const [items,setItems]=React.useState<Entry[]>([]),[canManage,setCanManage]=React.useState(false);
  const [loading,setLoading]=React.useState(true),[saving,setSaving]=React.useState(false),[error,setError]=React.useState('');
  const [open,setOpen]=React.useState(false),[editing,setEditing]=React.useState<Entry|null>(null),[form,setForm]=React.useState<Rule>(emptyRule),[attachments,setAttachments]=React.useState('');
  const endpoint=`${base}/stations/${stationId}/rent-rules`;
  const load=React.useCallback(async()=>{
    setLoading(true);setError('');
    try {const {data}=await energyHttp.get(endpoint);setItems(data.items);setCanManage(data.canManage===true);}
    catch {setError('租金規則載入失敗，請重試。');setCanManage(false);}
    finally {setLoading(false);}
  },[endpoint]);
  React.useEffect(()=>{void load();},[load]);
  const edit=(entry:Entry|null)=>{setEditing(entry);setForm(entry?{...entry.rule,leaseEvidence:{...entry.rule.leaseEvidence}}:emptyRule());setAttachments(entry?.rule.leaseEvidence.attachmentIds.join('、')??'');setError('');setOpen(true);};
  const change=(key:keyof Rule,value:unknown)=>setForm(f=>({...f,[key]:value}));
  const unsupported=!!form.calculationType&&form.calculationType!=='percentage';
  const save=async()=>{
    if(unsupported&&form.status==='confirmed'){setError('此計算類型尚未支援，請保留待確認狀態。');return;}
    const ids=attachments.trim()?attachments.split(/[,，、\s]+/).map(Number):[];
    if(ids.some(n=>!Number.isSafeInteger(n)||n<=0)){setError('附件編號請填正整數，並以逗號分隔。');return;}
    setSaving(true);setError('');
    try {
      await energyHttp.post(endpoint,{...form,validFrom:form.validFrom||null,validTo:form.validTo||null,leaseEvidence:{...form.leaseEvidence,attachmentIds:ids},...(editing?{id:editing.id,expectedVersion:editing.version}:{})});
      setOpen(false);await load();
    } catch(e:any) {
      const details=apiErrorPayload(e)?.details;
      setError(e?.response?.status===409?'規則已被更新或效期重疊。請保留修改內容，關閉後重新載入確認；本次未覆蓋既有規則。':details?.message||'規則未儲存，請確認必填欄位、日期、附件與管理權限後重試。');
    } finally {setSaving(false);}
  };
  return <Card sx={{my:2}}><CardContent><Stack direction="row" justifyContent="space-between" alignItems="center"><Typography variant="h6">案場租金規則</Typography><Stack direction="row"><Button onClick={load} disabled={loading||saving}>重新載入規則</Button>{canManage&&<Button onClick={()=>edit(null)}>新增租金規則</Button>}</Stack></Stack>
    <Typography variant="body2" sx={{my:1}}>規則確認後可沿用至後續帳單，依付款週期自動彙整租金預覽。租約備件狀態另外追蹤；預覽不代表付款核准。</Typography>
    {error&&!open&&<Alert severity="error">{error}</Alert>}{loading&&<Typography>載入規則中…</Typography>}{!loading&&!items.length&&!error&&<Alert severity="info">尚無可沿用的租金規則。請由主檔管理人員建立。</Alert>}
    {items.map(entry=><Card key={entry.id} variant="outlined" sx={{mt:2}}><CardContent><Stack direction="row" spacing={1} alignItems="center"><Typography fontWeight={600}>{entry.rule.payee}｜規則 #{entry.id} · 第 {entry.version} 版</Typography><Chip size="small" label={entry.rule.status==='confirmed'?'規則已確認':'規則待確認'}/><Chip size="small" variant="outlined" label={evidenceLabels[entry.rule.leaseEvidence.status]??'租約待確認'}/></Stack>
      <Typography sx={{mt:1}}>{entry.rule.calculationType&&entry.rule.calculationType!=='percentage'?`計算類型 ${entry.rule.calculationType} 尚待確認`:`${entry.rule.basis==='gross'?'含稅':entry.rule.basis==='untaxed'?'未稅':'基底待確認'}售電收入 × ${entry.rule.percent??'待確認'}%，外加稅 ${entry.rule.taxPercent??'待確認'}%；${entry.rule.rounding==='perReceipt'?'逐筆四捨五入後加總':entry.rule.rounding==='total'?'合計後四捨五入':'進位方式待確認'}。`}</Typography>
      <Typography>每 {entry.rule.cycleMonths??'待確認'} 個月一期，起算日 {entry.rule.cycleAnchor??'待確認'}；{entry.rule.transfer==null?'轉供狀態待確認':entry.rule.transfer?'需彙整台電與轉供單':'彙整台電單'}。</Typography><Typography>效期：{entry.rule.validFrom??'待確認'} ～ {entry.rule.validTo||'未設迄日'}</Typography><Typography>規則來源：{entry.rule.ruleSource||'待補'}</Typography>
      {entry.rule.notes?.map((note,i)=><Typography key={i} variant="body2" color="text.secondary">{note}</Typography>)}
      {entry.rule.sourceValues&&<Accordion variant="outlined" disableGutters sx={{mt:1}}><AccordionSummary expandIcon={<span aria-hidden="true">⌄</span>}><Typography>查看資產總表原始租金條件</Typography></AccordionSummary><AccordionDetails>
        <Typography variant="body2">來源專案：{entry.rule.sourceProjectCode??String(entry.rule.sourceValues.C??'待確認')}；來源列：{entry.rule.sourceRow??'待確認'}</Typography>
        <Typography variant="caption" color="text.secondary">以下保留原表數值與單位寫法，尚未換算為已確認租金規則；空白不代表零。</Typography>
        <Table size="small" aria-label="資產總表原始租金條件"><TableBody>{Object.entries(sourceTerms).map(([column,label])=><TableRow key={column}><TableCell component="th" scope="row">{label}（{column}）</TableCell><TableCell>{entry.rule.sourceValues?.[column]==null||entry.rule.sourceValues[column]===''?'原表未填':String(entry.rule.sourceValues[column])}</TableCell></TableRow>)}</TableBody></Table>
      </AccordionDetails></Accordion>}
      {entry.rule.leaseEvidence.attachmentIds.map(id=><Button key={id} size="small" href={`${base}/attachments/${id}/download`} target="_blank" rel="noreferrer">查看租約附件 #{id}</Button>)}
      {canManage&&<Button onClick={()=>edit(entry)}>編輯規則 #{entry.id}</Button>}
    </CardContent></Card>)}
    <Dialog open={open} onClose={()=>!saving&&setOpen(false)} fullWidth maxWidth="md"><DialogTitle>{editing?`編輯租金規則（第 ${editing.version} 版）`:'新增租金規則'}</DialogTitle><DialogContent><Stack spacing={2} sx={{mt:1}}>{error&&<Alert severity="error">{error}</Alert>}
      <Alert severity="info">既有來源原值與備註會保留；未確認欄位可以保持待確認。</Alert>{unsupported&&<Alert severity="warning">此規則類型（{form.calculationType}）尚未支援確認；可更新備件及保留待確認狀態，不會轉換為比例租金。</Alert>}
      <TextField required label="收款對象" value={form.payee??''} onChange={e=>change('payee',e.target.value)}/>
      <TextField required label="規則來源／確認依據" value={form.ruleSource??''} onChange={e=>change('ruleSource',e.target.value)} helperText="例如：付款紀錄、租金頁摘錄或承辦確認日期"/>
      <Stack direction="row" spacing={2}><TextField select fullWidth label="收入基底" value={form.basis??''} onChange={e=>change('basis',e.target.value)}><MenuItem value="">待確認</MenuItem><MenuItem value="untaxed">未稅售電收入</MenuItem><MenuItem value="gross">含稅售電收入</MenuItem></TextField><TextField required label="租金比例（%）" value={form.percent??''} onChange={e=>change('percent',e.target.value)}/><TextField required label="外加稅率（%）" value={form.taxPercent??''} onChange={e=>change('taxPercent',e.target.value)}/></Stack>
      <TextField select label="進位方式" value={form.rounding??''} onChange={e=>change('rounding',e.target.value)}><MenuItem value="">待確認</MenuItem><MenuItem value="perReceipt">逐筆四捨五入後加總</MenuItem><MenuItem value="total">收入合計後計算並四捨五入</MenuItem></TextField>
      <TextField select label="是否已轉供" value={form.transfer==null?'':String(form.transfer)} onChange={e=>change('transfer',e.target.value===''?null:e.target.value==='true')}><MenuItem value="">待確認</MenuItem><MenuItem value="false">未轉供（台電單）</MenuItem><MenuItem value="true">已轉供（台電＋轉供單）</MenuItem></TextField>
      <TextField label="付款週期（月）" type="number" inputProps={{min:1,max:12}} value={form.cycleMonths??''} onChange={e=>change('cycleMonths',e.target.value===''?null:Number(e.target.value))}/>
      <TextField required type="date" label="週期起算日（每月 1～28 日）" InputLabelProps={{shrink:true}} value={form.cycleAnchor??''} onChange={e=>change('cycleAnchor',e.target.value)}/>
      <Stack direction="row" spacing={2}><TextField required fullWidth type="date" label="規則生效日" InputLabelProps={{shrink:true}} value={form.validFrom??''} onChange={e=>change('validFrom',e.target.value)}/><TextField fullWidth type="date" label="規則迄日（可留空）" InputLabelProps={{shrink:true}} value={form.validTo??''} onChange={e=>change('validTo',e.target.value)}/></Stack>
      <TextField select label="規則確認狀態" value={form.status} onChange={e=>change('status',e.target.value)}><MenuItem value="pending">待確認</MenuItem><MenuItem value="confirmed" disabled={unsupported}>已確認</MenuItem></TextField>
      <TextField select label="租約備件狀態" value={form.leaseEvidence.status} onChange={e=>change('leaseEvidence',{...form.leaseEvidence,status:e.target.value})}>{Object.entries(evidenceLabels).map(([key,label])=><MenuItem key={key} value={key}>{label}</MenuItem>)}</TextField>
      <TextField label="已上傳租約附件編號" value={attachments} onChange={e=>setAttachments(e.target.value)} helperText="多筆以逗號分隔；填入本組織已上傳附件的編號。完整合約與租金頁摘錄請依實際備件情況選擇。"/>
    </Stack></DialogContent><DialogActions><Button disabled={saving} onClick={()=>setOpen(false)}>取消</Button><Button variant="contained" disabled={saving||(form.status==='confirmed'&&(unsupported||!form.payee?.trim()||!form.ruleSource?.trim()||!form.percent?.trim()||!form.taxPercent?.trim()||!form.validFrom||!form.cycleAnchor))} onClick={save}>{saving?'儲存中…':'儲存租金規則'}</Button></DialogActions></Dialog>
  </CardContent></Card>;
}
