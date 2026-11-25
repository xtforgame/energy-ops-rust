import * as React from 'react';
import {
  Card, CardContent, Typography, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, Tooltip, IconButton, TextField, Button, Dialog, DialogTitle, DialogContent, DialogActions,
  MenuItem, Select, InputLabel, FormControl, Autocomplete, Divider, List, ListItem, ListItemText,
} from '@mui/material';
import RefreshIcon from '@mui/icons-material/Refresh';
import AddIcon from '@mui/icons-material/Add';
import UploadFileIcon from '@mui/icons-material/UploadFile';
import { energyHttp, energyBase, attachmentDownloadUrl } from '@/api/energy';
import { useNotify, useTranslate } from 'react-admin';
import { useSelection } from '@/platform/selection/store';
import { apiErrorCode } from '../../../dataProvider/apiError';
import { EnergyPageFrame, EmptyRow } from './shared';

/** 卡 20261118-0810-a2 小修（照卡 A commit fdbc0db 的寫法）：附件上傳時間存的是 UTC ISO 字串，
 *  直接印出來給人看會變成 UTC，要轉成本地時間。後端存的 UTC 不動，只管顯示這一層。 */
function pad2(n: number): string {
  return String(n).padStart(2, '0');
}

function localDateTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return String(iso);
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())} ${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`;
}

interface ContractRow {
  id: number; contractNo: string; contractType: string | null; landownerId: number | null; landownerName: string | null;
  landowners: Array<{ id: number; name: string | null }>;
  stations: Array<{ id: number; name: string }>; signedOn: string | null; validFrom: string; validTo: string | null;
  renewalRemindOn: string | null; termsVersion: string | null; paymentFrequency: string | null; paymentTerms: string | null;
  stage: string; attachmentCount: number;
  billCodes: string[]; rentCalcCode: string | null; rentPayCode: string | null;
}

interface WorkflowBinding {
  stationId: number; stationName: string; code: string; docType: string; workflowKey: string;
  result: 'created' | 'alreadyBound' | 'noMeter' | 'notInstalled';
}

/** 代號＋名稱，照 `scripts/site-workflow-inventory/代號表.csv`（代號定義是中文文件，名稱不走 i18n，照 CONTRACT_TYPE_SUGGESTIONS 的慣例）。 */
const BILL_CODE_NAMES: Record<string, string> = {
  'E-A': '台電開票（有餘電）', 'E-B': '台電付款（有合約）', 'E-C': '轉供開票（天能與其他售電業）', 'E-D': '逆變器（無合約付款）',
};
const RENT_CALC_CODE_NAMES: Record<string, string> = {
  'R-0': '不付租金', 'R-A': '固定金額', 'R-B': '售電收入比例', 'R-C': '級距', 'R-D': '度數×差購費率',
};
const RENT_PAY_CODE_NAMES: Record<string, string> = {
  'RP-1': '一般（公司地主，憑證先附）', 'RP-2': '憑證後補', 'RP-3': '機關（公文加會計師簽證）', 'RP-4': '個人地主',
};
const BILL_CODES = Object.keys(BILL_CODE_NAMES);
const RENT_CALC_CODES = Object.keys(RENT_CALC_CODE_NAMES);
const RENT_PAY_CODES = Object.keys(RENT_PAY_CODE_NAMES);

interface AttachmentRow {
  id: number; originalFilename: string; size: number; uploadedAt: string;
}

const CONTRACT_TYPE_SUGGESTIONS = ['土地租賃', '屋頂租賃', '綠電', '維運'];
const PAYMENT_FREQUENCIES = ['monthly', 'bimonthly', 'quarterly', 'semiannual', 'annual', 'other'];

interface FormState {
  id?: number;
  contractNo: string; contractType: string; landownerIds: number[]; stationIds: number[];
  signedOn: string; validFrom: string; validTo: string; renewalRemindOn: string;
  termsVersion: string; paymentFrequency: string; paymentTerms: string;
  billCodes: string[]; rentCalcCode: string; rentPayCode: string;
}

const emptyForm: FormState = {
  contractNo: '', contractType: '', landownerIds: [], stationIds: [],
  signedOn: '', validFrom: '', validTo: '', renewalRemindOn: '', termsVersion: '', paymentFrequency: '', paymentTerms: '',
  billCodes: [], rentCalcCode: '', rentPayCode: '',
};

/**
 * 卡 20261118-0810-a2 S1／S2：合約清單＋新增編輯＋合約檔上傳（照 `LandownersPage.tsx` 抄）。
 */
const ContractsPage: React.FC = () => {
  const notify = useNotify();
  const translate = useTranslate();
  const orgId = useSelection()?.organizationId;
  const base = energyBase(orgId);

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [rows, setRows] = React.useState<ContractRow[]>([]);
  const [keyword, setKeyword] = React.useState('');
  const [landowners, setLandowners] = React.useState<Array<{ id: number; name: string }>>([]);
  const [stations, setStations] = React.useState<Array<{ id: number; name: string }>>([]);
  const [open, setOpen] = React.useState(false);
  const [form, setForm] = React.useState<FormState>(emptyForm);
  const [saving, setSaving] = React.useState(false);
  const [attachments, setAttachments] = React.useState<AttachmentRow[]>([]);
  const [uploading, setUploading] = React.useState(false);
  const fileRef = React.useRef<HTMLInputElement>(null);

  const load = React.useCallback(async () => {
    if (!orgId) { setRows([]); setDenied(false); return; }
    setLoading(true); setDenied(false);
    try {
      const { data } = await energyHttp.get(`${base}/contracts`, {
        params: keyword.trim() ? { q: keyword.trim() } : {},
      });
      setRows(Array.isArray(data) ? data : []);
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
    } finally {
      setLoading(false);
    }
  }, [orgId, base, keyword, notify]);

  const loadOptions = React.useCallback(async () => {
    if (!orgId) return;
    try {
      const [lo, st] = await Promise.all([
        energyHttp.get(`${base}/landowners`),
        energyHttp.get(`${base}/station-list`),
      ]);
      setLandowners(Array.isArray(lo.data) ? lo.data : []);
      setStations(Array.isArray(st.data) ? st.data : []);
    } catch { /* 下拉抓不到就留空，不擋主畫面 */ }
  }, [orgId, base]);

  React.useEffect(() => { load(); }, [load]);
  React.useEffect(() => { loadOptions(); }, [loadOptions]);

  const loadAttachments = React.useCallback(async (contractId: number) => {
    try {
      const { data } = await energyHttp.get(`${base}/contracts/${contractId}/attachments`);
      setAttachments(Array.isArray(data) ? data : []);
    } catch {
      setAttachments([]);
    }
  }, [base]);

  const openCreate = () => { setForm(emptyForm); setAttachments([]); setOpen(true); };
  const openEdit = (r: ContractRow) => {
    setForm({
      id: r.id, contractNo: r.contractNo, contractType: r.contractType ?? '',
      landownerIds: r.landowners?.length ? r.landowners.map((l) => l.id) : (r.landownerId ? [r.landownerId] : []),
      stationIds: r.stations.map((s) => s.id), signedOn: r.signedOn ?? '', validFrom: r.validFrom, validTo: r.validTo ?? '',
      renewalRemindOn: r.renewalRemindOn ?? '', termsVersion: r.termsVersion ?? '',
      paymentFrequency: r.paymentFrequency ?? '', paymentTerms: r.paymentTerms ?? '',
      billCodes: r.billCodes ?? [], rentCalcCode: r.rentCalcCode ?? '', rentPayCode: r.rentPayCode ?? '',
    });
    loadAttachments(r.id);
    setOpen(true);
  };

  /** 使用者 2026-11-25：存檔時依代號建工作流綁定，結果用一句話告訴使用者；noMeter／notInstalled 不算錯誤。 */
  const summarizeBindings = (bindings: WorkflowBinding[]): string => {
    const created = bindings.filter((b) => b.result === 'created');
    const noMeterStationIds = new Set(bindings.filter((b) => b.result === 'noMeter').map((b) => b.stationId));
    const parts: string[] = [];
    if (created.length) {
      const names = Array.from(new Set(created.map((b) => b.stationName)));
      const label = names.length === 1 ? names[0] : `${names.length} 個案場`;
      parts.push(translate('energy.contracts.bindingsCreated', { smart_count: created.length, station: label, count: created.length }));
    }
    if (noMeterStationIds.size) {
      parts.push(translate('energy.contracts.bindingsNoMeter', { smart_count: noMeterStationIds.size, count: noMeterStationIds.size }));
    }
    return parts.join('；');
  };

  const save = async () => {
    if (!orgId) return;
    setSaving(true);
    try {
      const body: Record<string, unknown> = {
        contractNo: form.contractNo, contractType: form.contractType || undefined,
        landownerIds: form.landownerIds, stationIds: form.stationIds,
        signedOn: form.signedOn || undefined, validFrom: form.validFrom, validTo: form.validTo || undefined,
        renewalRemindOn: form.renewalRemindOn || undefined, termsVersion: form.termsVersion || undefined,
        paymentFrequency: form.paymentFrequency || undefined, paymentTerms: form.paymentTerms || undefined,
        billCodes: form.billCodes, rentCalcCode: form.rentCalcCode || undefined, rentPayCode: form.rentPayCode || undefined,
      };
      const { data } = form.id ? await energyHttp.patch(`${base}/contracts/${form.id}`, body) : await energyHttp.post(`${base}/contracts`, body);
      // 使用者 2026-11-25：存檔結果用一句話告訴使用者；react-admin 一次只顯示一則通知，
      // 兩則連著發第二則會被排到第一則消失（約 4 秒）後才出現，使用者看不到——改成只發一則。
      const summary = summarizeBindings(Array.isArray(data?.workflowBindings) ? data.workflowBindings : []);
      notify(summary || 'ra.notification.updated', { type: 'info', messageArgs: { smart_count: 1 } });
      setOpen(false);
      load();
    } catch (e: any) {
      notify(apiErrorCode(e) || e?.message || 'save failed', { type: 'error' });
    } finally {
      setSaving(false);
    }
  };

  const upload = async (file: File) => {
    if (!form.id) return;
    setUploading(true);
    try {
      const data = new FormData();
      data.append('file', file, file.name);
      await energyHttp.post(`${base}/contracts/${form.id}/attachments`, data);
      notify('ra.notification.updated', { type: 'info', messageArgs: { smart_count: 1 } });
      await loadAttachments(form.id);
      load();
    } catch (e: any) {
      notify(apiErrorCode(e) || e?.message || 'upload failed', { type: 'error' });
    } finally {
      setUploading(false);
      if (fileRef.current) fileRef.current.value = '';
    }
  };

  return (
    <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2}>
        <Typography variant="h6">{translate('energy.contracts.title')}</Typography>
        <Stack direction="row" gap={1} alignItems="center">
          <TextField
            size="small"
            label={translate('energy.contracts.search')}
            value={keyword}
            onChange={(e) => setKeyword(e.target.value)}
            inputProps={{ 'data-testid': 'contract-search' }}
          />
          <Tooltip title={translate('ra.action.refresh')}>
            <IconButton onClick={load} disabled={loading}><RefreshIcon /></IconButton>
          </Tooltip>
          <Button variant="contained" startIcon={<AddIcon />} onClick={openCreate} data-testid="contract-new">
            {translate('energy.contracts.newContract')}
          </Button>
        </Stack>
      </Stack>

      <Card>
        <CardContent>
          <Table size="small" data-testid="contract-list-table">
            <TableHead>
              <TableRow>
                <TableCell>{translate('energy.contracts.no')}</TableCell>
                <TableCell>{translate('energy.contracts.type')}</TableCell>
                <TableCell>{translate('energy.contracts.landowner')}</TableCell>
                <TableCell>{translate('energy.contracts.stations')}</TableCell>
                <TableCell>{translate('energy.contracts.signedOn')}</TableCell>
                <TableCell>{translate('energy.stations.validFrom')}</TableCell>
                <TableCell>{translate('energy.stations.validTo')}</TableCell>
                <TableCell>{translate('energy.contracts.renewalRemindOn')}</TableCell>
                <TableCell>{translate('energy.contracts.paymentFrequency')}</TableCell>
                <TableCell>{translate('energy.contracts.codes')}</TableCell>
                <TableCell align="right">{translate('energy.contracts.attachmentCount')}</TableCell>
                <TableCell />
              </TableRow>
            </TableHead>
            <TableBody>
              {rows.length ? rows.map((r) => (
                <TableRow key={r.id} hover data-testid="contract-row">
                  <TableCell><Typography variant="body2" fontWeight={600}>{r.contractNo}</Typography></TableCell>
                  <TableCell>{r.contractType || '—'}</TableCell>
                  <TableCell>{(r.landowners?.length ? r.landowners.map((l) => l.name).join('、') : r.landownerName) || '—'}</TableCell>
                  <TableCell>{r.stations.map((s) => s.name).join('、') || '—'}</TableCell>
                  <TableCell>{r.signedOn || '—'}</TableCell>
                  <TableCell>{r.validFrom}</TableCell>
                  <TableCell>{r.validTo || '—'}</TableCell>
                  <TableCell>{r.renewalRemindOn || '—'}</TableCell>
                  <TableCell>{r.paymentFrequency ? translate(`energy.paymentFrequency.${r.paymentFrequency}`) : '—'}</TableCell>
                  <TableCell data-testid="contract-codes">
                    {[r.billCodes?.join('、'), r.rentCalcCode, r.rentPayCode].filter(Boolean).join('／') || '—'}
                  </TableCell>
                  <TableCell align="right" data-testid="contract-attachment-count">{r.attachmentCount}</TableCell>
                  <TableCell>
                    <Button size="small" onClick={() => openEdit(r)} data-testid="contract-edit">
                      {translate('ra.action.edit')}
                    </Button>
                  </TableCell>
                </TableRow>
              )) : <EmptyRow colSpan={12} loading={loading} />}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <Dialog open={open} onClose={() => setOpen(false)} fullWidth maxWidth="sm" data-testid="contract-form-dialog">
        <DialogTitle>{form.id ? translate('ra.action.edit') : translate('energy.contracts.newContract')}</DialogTitle>
        <DialogContent>
          <Stack gap={2} mt={1}>
            <TextField label={translate('energy.contracts.no')} value={form.contractNo} onChange={(e) => setForm({ ...form, contractNo: e.target.value })} data-testid="contract-form-no" required />
            <Autocomplete
              freeSolo
              options={CONTRACT_TYPE_SUGGESTIONS}
              value={form.contractType}
              onInputChange={(_, v) => setForm({ ...form, contractType: v })}
              renderInput={(params) => <TextField {...params} label={translate('energy.contracts.type')} data-testid="contract-form-type" />}
            />
            <FormControl fullWidth required data-testid="contract-form-landowner">
              <InputLabel>{translate('energy.contracts.landowners')}</InputLabel>
              <Select
                multiple
                label={translate('energy.contracts.landowners')}
                value={form.landownerIds}
                onChange={(e) => setForm({ ...form, landownerIds: (e.target.value as number[]) })}
                renderValue={(ids) => landowners.filter((l) => (ids as number[]).includes(l.id)).map((l) => l.name).join('、')}
              >
                {landowners.map((l) => <MenuItem key={l.id} value={l.id}>{l.name}</MenuItem>)}
              </Select>
            </FormControl>
            <FormControl fullWidth data-testid="contract-form-stations">
              <InputLabel>{translate('energy.contracts.stations')}</InputLabel>
              <Select
                multiple
                label={translate('energy.contracts.stations')}
                value={form.stationIds}
                onChange={(e) => setForm({ ...form, stationIds: (e.target.value as number[]) })}
                renderValue={(ids) => stations.filter((s) => (ids as number[]).includes(s.id)).map((s) => s.name).join('、')}
              >
                {stations.map((s) => <MenuItem key={s.id} value={s.id}>{s.name}</MenuItem>)}
              </Select>
            </FormControl>
            <TextField
              label={translate('energy.contracts.signedOn')} type="date" value={form.signedOn}
              onChange={(e) => setForm({ ...form, signedOn: e.target.value })} InputLabelProps={{ shrink: true }} data-testid="contract-form-signedon"
            />
            <TextField
              label={translate('energy.stations.validFrom')} type="date" value={form.validFrom}
              onChange={(e) => setForm({ ...form, validFrom: e.target.value })} InputLabelProps={{ shrink: true }} data-testid="contract-form-validfrom" required
            />
            <TextField
              label={translate('energy.stations.validTo')} type="date" value={form.validTo}
              onChange={(e) => setForm({ ...form, validTo: e.target.value })} InputLabelProps={{ shrink: true }} data-testid="contract-form-validto"
            />
            <TextField
              label={translate('energy.contracts.renewalRemindOn')} type="date" value={form.renewalRemindOn}
              onChange={(e) => setForm({ ...form, renewalRemindOn: e.target.value })} InputLabelProps={{ shrink: true }} data-testid="contract-form-renewal"
            />
            <TextField label={translate('energy.contracts.termsVersion')} value={form.termsVersion} onChange={(e) => setForm({ ...form, termsVersion: e.target.value })} data-testid="contract-form-termsversion" />
            <FormControl fullWidth data-testid="contract-form-billcodes">
              <InputLabel>{translate('energy.contracts.billCodes')}</InputLabel>
              <Select
                multiple
                label={translate('energy.contracts.billCodes')}
                value={form.billCodes}
                onChange={(e) => setForm({ ...form, billCodes: (e.target.value as string[]) })}
                renderValue={(codes) => (codes as string[]).join('、')}
              >
                {BILL_CODES.map((c) => <MenuItem key={c} value={c}>{`${c} ${BILL_CODE_NAMES[c]}`}</MenuItem>)}
              </Select>
            </FormControl>
            <FormControl fullWidth data-testid="contract-form-rentcalc">
              <InputLabel>{translate('energy.contracts.rentCalcCode')}</InputLabel>
              <Select
                label={translate('energy.contracts.rentCalcCode')}
                value={form.rentCalcCode}
                onChange={(e) => setForm({ ...form, rentCalcCode: e.target.value })}
              >
                <MenuItem value="">—</MenuItem>
                {RENT_CALC_CODES.map((c) => <MenuItem key={c} value={c}>{`${c} ${RENT_CALC_CODE_NAMES[c]}`}</MenuItem>)}
              </Select>
            </FormControl>
            <FormControl fullWidth data-testid="contract-form-rentpay">
              <InputLabel>{translate('energy.contracts.rentPayCode')}</InputLabel>
              <Select
                label={translate('energy.contracts.rentPayCode')}
                value={form.rentPayCode}
                onChange={(e) => setForm({ ...form, rentPayCode: e.target.value })}
              >
                <MenuItem value="">—</MenuItem>
                {RENT_PAY_CODES.map((c) => <MenuItem key={c} value={c}>{`${c} ${RENT_PAY_CODE_NAMES[c]}`}</MenuItem>)}
              </Select>
            </FormControl>
            <FormControl fullWidth data-testid="contract-form-paymentfrequency">
              <InputLabel>{translate('energy.contracts.paymentFrequency')}</InputLabel>
              <Select
                label={translate('energy.contracts.paymentFrequency')}
                value={form.paymentFrequency}
                onChange={(e) => setForm({ ...form, paymentFrequency: e.target.value })}
              >
                <MenuItem value="">—</MenuItem>
                {PAYMENT_FREQUENCIES.map((f) => <MenuItem key={f} value={f}>{translate(`energy.paymentFrequency.${f}`)}</MenuItem>)}
              </Select>
            </FormControl>
            <TextField
              label={translate('energy.contracts.paymentTerms')} value={form.paymentTerms}
              onChange={(e) => setForm({ ...form, paymentTerms: e.target.value })} multiline minRows={2} data-testid="contract-form-paymentterms"
            />

            {form.id && (
              <>
                <Divider />
                <Typography variant="subtitle2">{translate('energy.contracts.attachments')}</Typography>
                <List dense data-testid="contract-attachment-list">
                  {attachments.length ? attachments.map((a) => (
                    <ListItem
                      key={a.id}
                      secondaryAction={
                        <Button size="small" href={attachmentDownloadUrl(a.id, orgId)} data-testid="contract-attachment-download">
                          {translate('energy.contracts.download')}
                        </Button>
                      }
                    >
                      <ListItemText primary={a.originalFilename} secondary={`${a.size} bytes · ${localDateTime(a.uploadedAt)}`} />
                    </ListItem>
                  )) : (
                    <Typography variant="body2" color="text.secondary">{translate('energy.contracts.noAttachments')}</Typography>
                  )}
                </List>
                <Button
                  variant="outlined"
                  component="label"
                  startIcon={<UploadFileIcon />}
                  disabled={uploading}
                  data-testid="contract-attachment-upload"
                >
                  {translate('energy.contracts.uploadAttachment')}
                  <input
                    ref={fileRef}
                    type="file"
                    hidden
                    onChange={(e) => { const f = e.target.files?.[0]; if (f) upload(f); }}
                  />
                </Button>
              </>
            )}
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setOpen(false)}>{translate('ra.action.cancel')}</Button>
          <Button variant="contained" onClick={save} disabled={saving || !form.contractNo || form.landownerIds.length === 0 || !form.validFrom} data-testid="contract-form-save">
            {translate('ra.action.save')}
          </Button>
        </DialogActions>
      </Dialog>
    </EnergyPageFrame>
  );
};

export default ContractsPage;
