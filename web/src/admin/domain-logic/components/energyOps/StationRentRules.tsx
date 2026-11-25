import * as React from 'react';
import {Accordion, AccordionSummary, AccordionDetails, Alert, Button, Card, CardContent, Chip, Dialog, DialogActions, DialogContent, DialogTitle, MenuItem, Stack, Table, TableBody, TableCell, TableRow, TextField, Typography} from '@mui/material';
import {energyHttp} from '@/api/energy';
import {apiErrorPayload} from '../../../dataProvider/apiError';

type Assessment = {referenceRent?:string|null;formulaSummary?:string;readiness?:string;missingInputs?:string[];warnings?:string[];baseRentRaw?:string|null;baseRent?:string|null;taxRaw?:string|null;grossRentRaw?:string|null;tax?:string|null;grossRent?:string|null};
type Rule = {taxMode?:string|null;fixedAmount?:string|null;fixedUnit?:string|null;area?:string|null;areaUnit?:string|null;rateUnit?:string|null;rateAreaUnit?:string|null;minimumBasis?:string|null;unitRate?:string|null;ratePeriod?:string|null;minimumRent?:string|null;minimumUnit?:string|null;annualRent?:string|null;tolerancePercent?:string|null;partyGroup?:{label:string;count:number|null;members:{name:string;share?:string|null}[];allocationStatus:string};calculationType?:string;sourceValues?:Record<string,unknown>;sourceRow?:number;sourceProjectCode?:string;notes?:string[];payee?:string|null;basis?:string|null;percent?:string|null;taxPercent?:string|null;rounding?:string|null;transfer?:boolean|null;cycleMonths?:number|null;cycleAnchor?:string|null;validFrom?:string|null;validTo:string|null;status:string;ruleSource:string;leaseEvidence:{status:string;attachmentIds:number[]}};
type Entry = {id:number;version:number;rule:Rule;assessment?:Assessment};
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
  const unsupported=!!form.calculationType&&!['percentage','fixed','areaRate','percentageFloor','annualReference'].includes(form.calculationType);
  const confirmationUnsupported=!!form.calculationType&&form.calculationType!=='percentage';
  const isPercentage=!form.calculationType||['percentage','percentageFloor','annualReference'].includes(form.calculationType);
  const save=async()=>{
    if(confirmationUnsupported&&form.status==='confirmed'){setError('此計算類型尚未支援，請保留待確認狀態。');return;}
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
    {items.map(entry=><Card key={entry.id} variant="outlined" sx={{mt:2}}><CardContent><Stack direction="row" spacing={1} alignItems="center"><Typography fontWeight={600}>{entry.rule.payee}｜規則 #{entry.id} · 第 {entry.version} 版</Typography><Chip size="small" label={entry.rule.status==='confirmed'?'規則已確認':'來源條件已整理'}/><Chip size="small" variant="outlined" label={evidenceLabels[entry.rule.leaseEvidence.status]??'租約待確認'}/></Stack>
      <Typography sx={{mt:1}}>計算方式：{formulaText(entry.assessment?.formulaSummary||fallbackFormula(entry.rule))}</Typography>
      {entry.rule.partyGroup&&<Alert severity="info" sx={{my:1}}>收款群組：{entry.rule.partyGroup.label}（{entry.rule.partyGroup.count??'人數待確認'} 人）；成員：{entry.rule.partyGroup.members?.length?entry.rule.partyGroup.members.map(m=>`${m.name}${m.share?`（份額 ${m.share}）`:''}`).join('、'):'姓名待補'}。分配方式：{entry.rule.partyGroup.allocationStatus==='confirmed'?'已確認':'待確認，不推定均分'}。</Alert>}
      <Typography variant="body2">已知條件：{knownConditions(entry.rule)}</Typography>
      {!!entry.assessment?.missingInputs?.length&&<Typography variant="body2" color="text.secondary">仍待補：{entry.assessment.missingInputs.map(conditionText).join('；')}</Typography>}
      <RuleTrial key={`${entry.id}-${entry.version}`} entry={entry} endpoint={endpoint}/>
      <Typography>每 {entry.rule.cycleMonths??'待確認'} 個月一期，起算日 {entry.rule.cycleAnchor??'來源未載'}{['fixed','areaRate'].includes(entry.rule.calculationType??'')?'':`；${entry.rule.transfer==null?'帳單來源範圍未載':entry.rule.transfer?'彙整台電與轉供單':'彙整台電單'}`}。</Typography><Typography>效期：{entry.rule.validFrom??'來源未載'} ～ {entry.rule.validTo||'未設迄日'}</Typography><Typography>規則來源：{entry.rule.ruleSource||'待補'}</Typography>
      {entry.rule.status!=='confirmed'&&<Typography variant="caption" color="text.secondary">正式確認尚未完成；已知公式仍可手動試算。</Typography>}
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
      {confirmationUnsupported&&!unsupported&&<Alert severity="info">此公式可供手動試算；正式確認與自動付款週期仍待核對，請保留待確認狀態。</Alert>}
      <TextField required label={form.partyGroup?"收款群組原文":"收款對象"} value={form.payee??''} onChange={e=>change('payee',e.target.value)}/>
      <TextField required label="規則來源／確認依據" value={form.ruleSource??''} onChange={e=>change('ruleSource',e.target.value)} helperText="例如：付款紀錄、租金頁摘錄或承辦確認日期"/>
      <TextField select label="計算類型" value={form.calculationType??'percentage'} onChange={e=>change('calculationType',e.target.value)}>{Object.entries(calculationLabels).map(([k,v])=><MenuItem key={k} value={k}>{v}</MenuItem>)}{unsupported&&<MenuItem value={form.calculationType}>{form.calculationType}</MenuItem>}</TextField>
      {isPercentage&&<Stack direction="row" spacing={2}><TextField select fullWidth label="收入基底" value={form.basis??''} onChange={e=>change('basis',e.target.value||null)}><MenuItem value="">待確認</MenuItem><MenuItem value="untaxed">未稅售電收入</MenuItem><MenuItem value="gross">含稅售電收入</MenuItem></TextField><TextField label="租金比例（%）" value={form.percent??''} onChange={e=>change('percent',e.target.value||null)}/></Stack>}
      {form.calculationType==='fixed'&&<><TextField label="固定租金金額（元）" value={form.fixedAmount??''} onChange={e=>change('fixedAmount',e.target.value||null)}/><UnitField label="固定租金時間單位" value={form.fixedUnit} onChange={v=>change('fixedUnit',v)}/></>}
      {form.calculationType==='areaRate'&&<><TextField label="面積" value={form.area??''} onChange={e=>change('area',e.target.value||null)}/><TextField label="面積單位" value={form.areaUnit??''} onChange={e=>change('areaUnit',e.target.value||null)}/><TextField label="每單位面積租金（元）" value={form.unitRate??''} onChange={e=>change('unitRate',e.target.value||null)}/><TextField label="單價對應面積單位" value={form.rateAreaUnit??form.rateUnit??''} onChange={e=>change('rateAreaUnit',e.target.value||null)}/><UnitField label="面積租金時間單位" value={form.ratePeriod} onChange={v=>change('ratePeriod',v)}/></>}
      {form.calculationType==='percentageFloor'&&<><TextField label="保底租金（元）" value={form.minimumRent??''} onChange={e=>change('minimumRent',e.target.value||null)}/><TextField select label="保底金額稅基" value={form.minimumBasis??''} onChange={e=>change('minimumBasis',e.target.value||null)}><MenuItem value="">待確認</MenuItem><MenuItem value="untaxed">未稅</MenuItem><MenuItem value="gross">含稅</MenuItem></TextField><UnitField label="保底租金時間單位" value={form.minimumUnit} onChange={v=>change('minimumUnit',v)}/></>}
      {form.calculationType==='annualReference'&&<><TextField label="契約年租金（元）" value={form.annualRent??''} onChange={e=>change('annualRent',e.target.value||null)}/><TextField label="年租金比較門檻（%）" value={form.tolerancePercent??''} onChange={e=>change('tolerancePercent',e.target.value||null)}/></>}
      <TextField select label="稅額處理" value={form.taxMode??''} onChange={e=>change('taxMode',e.target.value||null)}><MenuItem value="">來源未載</MenuItem><MenuItem value="included">內含稅額</MenuItem><MenuItem value="added">外加稅額</MenuItem></TextField>
      <TextField label="租金稅率（%）" value={form.taxPercent??''} onChange={e=>change('taxPercent',e.target.value||null)}/>
      <TextField select label="進位方式" value={form.rounding??''} onChange={e=>change('rounding',e.target.value)}><MenuItem value="">待確認</MenuItem><MenuItem value="perReceipt">逐筆四捨五入後加總</MenuItem><MenuItem value="total">收入合計後計算並四捨五入</MenuItem></TextField>
      <TextField select label="是否已轉供" value={form.transfer==null?'':String(form.transfer)} onChange={e=>change('transfer',e.target.value===''?null:e.target.value==='true')}><MenuItem value="">待確認</MenuItem><MenuItem value="false">未轉供（台電單）</MenuItem><MenuItem value="true">已轉供（台電＋轉供單）</MenuItem></TextField>
      <TextField label="付款週期（月）" type="number" inputProps={{min:1,max:12}} value={form.cycleMonths??''} onChange={e=>change('cycleMonths',e.target.value===''?null:Number(e.target.value))}/>
      <TextField required type="date" label="週期起算日（每月 1～28 日）" InputLabelProps={{shrink:true}} value={form.cycleAnchor??''} onChange={e=>change('cycleAnchor',e.target.value)}/>
      <Stack direction="row" spacing={2}><TextField required fullWidth type="date" label="規則生效日" InputLabelProps={{shrink:true}} value={form.validFrom??''} onChange={e=>change('validFrom',e.target.value)}/><TextField fullWidth type="date" label="規則迄日（可留空）" InputLabelProps={{shrink:true}} value={form.validTo??''} onChange={e=>change('validTo',e.target.value)}/></Stack>
      <TextField select label="規則確認狀態" value={form.status} onChange={e=>change('status',e.target.value)}><MenuItem value="pending">待確認</MenuItem><MenuItem value="confirmed" disabled={confirmationUnsupported}>已確認</MenuItem></TextField>
      <TextField select label="租約備件狀態" value={form.leaseEvidence.status} onChange={e=>change('leaseEvidence',{...form.leaseEvidence,status:e.target.value})}>{Object.entries(evidenceLabels).map(([key,label])=><MenuItem key={key} value={key}>{label}</MenuItem>)}</TextField>
      <TextField label="已上傳租約附件編號" value={attachments} onChange={e=>setAttachments(e.target.value)} helperText="多筆以逗號分隔；填入本組織已上傳附件的編號。完整合約與租金頁摘錄請依實際備件情況選擇。"/>
    </Stack></DialogContent><DialogActions><Button disabled={saving} onClick={()=>setOpen(false)}>取消</Button><Button variant="contained" disabled={saving||(form.status==='confirmed'&&(confirmationUnsupported||!form.payee?.trim()||!form.ruleSource?.trim()||(isPercentage&&!form.percent?.trim())||!form.taxPercent?.trim()||!form.validFrom||!form.cycleAnchor))} onClick={save}>{saving?'儲存中…':'儲存租金規則'}</Button></DialogActions></Dialog>
  </CardContent></Card>;
}

const calculationLabels:Record<string,string>={percentage:'收入比例',fixed:'固定租金',areaRate:'面積計租',percentageFloor:'收入比例與保底比較',annualReference:'年租金參考比較'};
function fallbackFormula(r:Rule){
 if(r.calculationType==='fixed')return `固定租金 ${r.fixedAmount??'金額待補'} 元／${r.fixedUnit??'時間單位待補'}`;
 if(r.calculationType==='areaRate')return `${r.area??'面積待補'} ${r.areaUnit??'單位待補'} × ${r.unitRate??'單價待補'} 元／${r.ratePeriod??'時間單位待補'}`;
 if(r.calculationType&&r.calculationType!=='percentage')return calculationLabels[r.calculationType]??'來源公式尚待釐清';
 return `${r.basis==='gross'?'含稅':r.basis==='untaxed'?'未稅':'基底待確認'}售電收入 × ${r.percent??'比例待補'}%`;
}
function UnitField({label,value,onChange}:{label:string;value?:string|null;onChange:(v:string|null)=>void}){return <TextField select label={label} value={value??''} onChange={e=>onChange(e.target.value||null)}><MenuItem value="">待確認</MenuItem><MenuItem value="month">每月</MenuItem><MenuItem value="year">每年</MenuItem><MenuItem value="cycle">每付款週期</MenuItem></TextField>;}
function RuleTrial({entry,endpoint}:{entry:Entry;endpoint:string}){
 const [income,setIncome]=React.useState(''),[months,setMonths]=React.useState(''),[result,setResult]=React.useState<Assessment|null>(null),[error,setError]=React.useState(''),[loading,setLoading]=React.useState(false);
 const a=result??entry.assessment;
 const preview=async()=>{setLoading(true);setError('');setResult(null);try{const {data}=await energyHttp.post(`${endpoint}/${entry.id}/preview`,{...(income.trim()?{incomeAmount:income.trim()}:{}),...(months?{periodMonths:Number(months)}:{})});setResult(data);}catch{setError('試算失敗，請核對收入、期間與規則後重試。');}finally{setLoading(false);}};
 const invalidate=()=>{setResult(null);setError('');};
 return <Accordion variant="outlined" disableGutters sx={{my:1}}><AccordionSummary expandIcon={<span aria-hidden="true">⌄</span>}><Typography>公式試算與待補條件 #{entry.id}</Typography></AccordionSummary><AccordionDetails>
  <Alert severity="info">手動試算，不是實際帳單或付款申請。可先計算已知公式；稅額、進位與分配條件分別核對。</Alert>
  <Stack direction={{xs:'column',sm:'row'}} spacing={2} sx={{my:2}}><TextField disabled={loading} label={`手動收入（元）#${entry.id}`} value={income} onChange={e=>{setIncome(e.target.value);invalidate();}} helperText="依上方收入基底填寫；固定或面積租金可留空"/><TextField disabled={loading} label={`試算期間（月）#${entry.id}`} type="number" value={months} inputProps={{min:1,step:1}} onChange={e=>{setMonths(e.target.value);invalidate();}} helperText="依實際欲比較的期間輸入"/><Button disabled={loading} onClick={preview}>{loading?'試算中…':'手動試算'}</Button></Stack>
  {error&&<Alert severity="error">{error}</Alert>}
  {result&&<Typography>本次手動公式：{formulaText(result.formulaSummary??'')}</Typography>}
  {a?.baseRentRaw==null&&a?.referenceRent!=null&&<Typography fontWeight={600}>比例計算參考額（尚未套用最低額／年度調整）：{a.referenceRent} 元</Typography>}
  {a?.baseRentRaw!=null&&<Typography fontWeight={600}>租金基額（未進位）：{a.baseRentRaw} 元</Typography>}
  {a?.baseRent!=null&&<Typography>租金基額（試算）：{a.baseRent} 元</Typography>}
  {a?.taxRaw!=null&&<Typography>{entry.rule.taxMode==='included'?'內含稅額':'稅額'}（未進位）：{a.taxRaw} 元</Typography>}
  {a?.grossRentRaw!=null&&<Typography>含稅試算合計（未進位）：{a.grossRentRaw} 元</Typography>}
  {result&&<Typography>{entry.rule.taxMode==='included'?'內含稅額':'稅額'}：{a?.tax??'稅額待確認'}；含稅試算額：{a?.grossRent??'待稅額與進位條件確認'}</Typography>}
  {!!a?.missingInputs?.length&&<><Typography fontWeight={600} sx={{mt:1}}>待補條件</Typography>{a.missingInputs.map((v,i)=><Typography key={i}>・{conditionText(v)}</Typography>)}</>}
  {a?.warnings?.map((v,i)=><Typography key={i} color="text.secondary">{conditionText(v)}</Typography>)}
 </AccordionDetails></Accordion>;
}

function knownConditions(r:Rule){
 const fields:[keyof Rule,string][]=[['percent','租金比例 %'],['basis','收入基底'],['fixedAmount','固定金額'],['fixedUnit','固定金額單位'],['area','面積'],['areaUnit','面積單位'],['rateAreaUnit','單價對應面積單位'],['rateUnit','原單價面積單位'],['unitRate','面積單價'],['ratePeriod','單價期間'],['minimumRent','保底金額'],['minimumUnit','保底期間'],['minimumBasis','保底金額稅基'],['annualRent','契約年租金'],['tolerancePercent','比較門檻 %'],['taxMode','稅額處理'],['taxPercent','稅率 %']];
 const labels:Record<string,string>={included:'內含稅額',added:'外加稅額',gross:'含稅',untaxed:'未稅',month:'每月',year:'每年',cycle:'每付款週期'};
 return fields.filter(([k])=>r[k]!=null&&r[k]!=='').map(([k,label])=>`${label}：${labels[String(r[k])]??String(r[k])}`).join('；')||'來源原文已保留，數值尚待核對';
}

function conditionText(text:string){
 const labels:Record<string,string>={taxPercent:'租金稅率',cycleMonths:'付款週期',minimumBasis:'保底金額稅基',areaUnitConversion:'面積單位換算依據',annualAdjustmentMethod:'年度調整方式',annualAndPercentageRelationship:'年額與收入比例的關係',sharedGroupAllocation:'整組金額的案場分配',formula:'計算公式',incomeAmount:'試算收入',periodMonths:'試算月數',percent:'租金比例',fixedAmount:'固定租金金額',fixedUnit:'固定租金時間單位',area:'面積',areaUnit:'面積單位',rateAreaUnit:'單價面積單位',rateUnit:'單價面積單位',unitRate:'面積單價',ratePeriod:'單價期間',minimumRent:'保底租金',minimumUnit:'保底期間',annualRent:'契約年租金',tolerancePercent:'年租金比較門檻',basis:'收入基底'};
 if(/^缺少[\u4e00-\u9fff]/.test(text))return text.replace(/\s+[A-Za-z][A-Za-z /]*$/, '');
 return text.replace(/\b[A-Za-z]+\b/g,key=>labels[key]??key);
}

function formulaText(text:string){return text.replace(/\b(month|year|cycle|ping|sqm|m2|hectare|fenLand)\b/g,v=>({hectare:'公頃',fenLand:'分地',month:'月',year:'年',cycle:'付款週期',ping:'坪',sqm:'平方公尺',m2:'平方公尺'}[v]??v));}
