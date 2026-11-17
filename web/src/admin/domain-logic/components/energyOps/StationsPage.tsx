import * as React from 'react';
import {
  Card, CardContent, Typography, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, Tooltip, IconButton, TextField, Button, Dialog, DialogTitle, DialogContent, DialogActions,
  MenuItem, Select, InputLabel, FormControl, Chip, FormControlLabel, Switch,
} from '@mui/material';
import RefreshIcon from '@mui/icons-material/Refresh';
import AddIcon from '@mui/icons-material/Add';
import { energyHttp, energyBase } from '@/api/energy';
import { useNotify, useTranslate } from 'react-admin';
import { useNavigate } from 'react-router-dom';
import { useSelection } from '@/platform/selection/store';
import { apiErrorCode } from '../../../dataProvider/apiError';
import {
  EnergyPageFrame, EmptyRow, StatusChip, jvTokens,
} from './shared';
import { stationRoute } from './routes';

interface StationRow {
  id: number; name: string; status: string; projectCode: string | null; siteType: string | null;
  spvPayeeId: number | null; serviceDistrict: string | null; capacityKw: string | null; gridDate: string | null;
  serialNo: number | null; shortName: string | null; region: string | null; category: string | null;
  address: string | null;
  meterCount: number; enabled: boolean; landownerId: number; landownerName?: string | null;
  projectCodeMissing: string[];
}

const SITE_TYPES = ['fishery', 'water', 'rooftop', 'other'];

interface FormState {
  id?: number;
  name: string; customerNumber: string; landownerId: string;
  shortName: string; region: string; category: string; siteType: string; spvPayeeId: string;
  serviceDistrict: string; capacityKw: string; gridDate: string; serialNo: string; enabled: boolean;
  address: string;
}

const emptyForm: FormState = {
  name: '', customerNumber: '', landownerId: '', shortName: '', region: '', category: '', siteType: '',
  spvPayeeId: '', serviceDistrict: '', capacityKw: '', gridDate: '', serialNo: '', enabled: true, address: '',
};

/**
 * 卡 20261118-0427-a S1：案場清單（資源 `energyStationConsole` 的 `list`）。
 * 欄位照卡上「使用者怎麼看」：專案編號、型別、SPV、區處、容量、併網日、電號數、啟用。
 */
const StationsPage: React.FC = () => {
  const notify = useNotify();
  const translate = useTranslate();
  const navigate = useNavigate();
  const orgId = useSelection()?.organizationId;
  const base = energyBase(orgId);

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [rows, setRows] = React.useState<StationRow[]>([]);
  const [siteTypeFilter, setSiteTypeFilter] = React.useState('');
  const [landowners, setLandowners] = React.useState<Array<{ id: number; name: string; partyType: string; code?: string | null }>>([]);
  const [open, setOpen] = React.useState(false);
  const [form, setForm] = React.useState<FormState>(emptyForm);
  const [saving, setSaving] = React.useState(false);

  const load = React.useCallback(async () => {
    if (!orgId) { setRows([]); setDenied(false); return; }
    setLoading(true); setDenied(false);
    try {
      const { data } = await energyHttp.get(`${base}/station-list`, {
        params: siteTypeFilter ? { siteType: siteTypeFilter } : {},
      });
      setRows(Array.isArray(data) ? data : []);
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
    } finally {
      setLoading(false);
    }
  }, [orgId, base, siteTypeFilter, notify]);

  const loadLandowners = React.useCallback(async () => {
    if (!orgId) return;
    try {
      const { data } = await energyHttp.get(`${base}/landowners`);
      setLandowners(Array.isArray(data) ? data : []);
    } catch { /* 下拉抓不到就留空，不擋主畫面 */ }
  }, [orgId, base]);

  React.useEffect(() => { load(); }, [load]);
  React.useEffect(() => { loadLandowners(); }, [loadLandowners]);

  const subsidiaries = landowners.filter((l) => l.partyType === 'subsidiary');

  const openCreate = () => { setForm(emptyForm); setOpen(true); };
  const openEdit = (r: StationRow) => {
    // 編輯是整包送上來（後端：缺的欄位視同清空）——**一定要把現有值全部帶回表單**，
    // 不然使用者只改一個欄位，其他欄位（尤其序號，專案編號靠它）會被靜默清掉。
    setForm({
      id: r.id, name: r.name, customerNumber: '', landownerId: String(r.landownerId),
      shortName: r.shortName ?? '', region: r.region ?? '', category: r.category ?? '', siteType: r.siteType ?? '',
      spvPayeeId: r.spvPayeeId ? String(r.spvPayeeId) : '', serviceDistrict: r.serviceDistrict ?? '',
      capacityKw: r.capacityKw ?? '', gridDate: r.gridDate ?? '', serialNo: r.serialNo != null ? String(r.serialNo) : '',
      enabled: r.enabled, address: r.address ?? '',
    });
    setOpen(true);
  };

  const save = async () => {
    if (!orgId) return;
    setSaving(true);
    try {
      const body: Record<string, unknown> = {
        name: form.name, landownerId: Number(form.landownerId) || undefined,
        shortName: form.shortName || undefined, region: form.region || undefined, category: form.category || undefined,
        siteType: form.siteType || undefined, spvPayeeId: form.spvPayeeId ? Number(form.spvPayeeId) : undefined,
        serviceDistrict: form.serviceDistrict || undefined, capacityKw: form.capacityKw || undefined,
        gridDate: form.gridDate || undefined, serialNo: form.serialNo ? Number(form.serialNo) : undefined,
        enabled: form.enabled, address: form.address || undefined,
      };
      if (form.id) {
        await energyHttp.patch(`${base}/stations/${form.id}`, body);
      } else {
        await energyHttp.post(`${base}/stations`, { ...body, customerNumber: form.customerNumber });
      }
      notify('ra.notification.updated', { type: 'info', messageArgs: { smart_count: 1 } });
      setOpen(false);
      load();
    } catch (e: any) {
      notify(apiErrorCode(e) || e?.message || 'save failed', { type: 'error' });
    } finally {
      setSaving(false);
    }
  };

  return (
    <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2}>
        <Typography variant="h6">{translate('energy.stations.listTitle')}</Typography>
        <Stack direction="row" gap={1} alignItems="center">
          <FormControl size="small" sx={{ minWidth: 140 }} data-testid="station-sitetype-filter">
            <InputLabel>{translate('energy.stations.siteType')}</InputLabel>
            <Select
              label={translate('energy.stations.siteType')}
              value={siteTypeFilter}
              onChange={(e) => setSiteTypeFilter(e.target.value)}
            >
              <MenuItem value="">{translate('energy.common.all')}</MenuItem>
              {SITE_TYPES.map((t) => (
                <MenuItem key={t} value={t}>{translate(`energy.siteType.${t}`)}</MenuItem>
              ))}
            </Select>
          </FormControl>
          <Tooltip title={translate('ra.action.refresh')}>
            <IconButton onClick={load} disabled={loading}><RefreshIcon /></IconButton>
          </Tooltip>
          <Button variant="contained" startIcon={<AddIcon />} onClick={openCreate} data-testid="station-new">
            {translate('energy.stations.newStation')}
          </Button>
        </Stack>
      </Stack>

      <Card>
        <CardContent>
          <Table size="small" data-testid="station-list-table">
            <TableHead>
              <TableRow>
                <TableCell>{translate('energy.stations.projectCode')}</TableCell>
                <TableCell>{translate('energy.stations.name')}</TableCell>
                <TableCell>{translate('energy.stations.siteType')}</TableCell>
                <TableCell>{translate('energy.stations.spv')}</TableCell>
                <TableCell>{translate('energy.stations.serviceDistrict')}</TableCell>
                <TableCell align="right">{translate('energy.stations.capacityKw')}</TableCell>
                <TableCell>{translate('energy.stations.gridDate')}</TableCell>
                <TableCell align="right">{translate('energy.stations.meterCount')}</TableCell>
                <TableCell>{translate('energy.stations.enabled')}</TableCell>
                <TableCell />
              </TableRow>
            </TableHead>
            <TableBody>
              {rows.length ? rows.map((r) => (
                <TableRow key={r.id} hover data-testid="station-row">
                  <TableCell
                    sx={{ cursor: 'pointer' }}
                    onClick={() => navigate(stationRoute(r.id))}
                    data-testid="station-project-code"
                  >
                    {r.projectCode ?? (
                      <Tooltip title={`${translate('energy.stations.projectCodeMissing')}：${r.projectCodeMissing.join('、')}`}>
                        <Chip size="small" variant="outlined" label={translate('energy.stations.projectCodeIncomplete')} sx={{ color: jvTokens.warn, borderColor: jvTokens.warn }} />
                      </Tooltip>
                    )}
                  </TableCell>
                  <TableCell sx={{ cursor: 'pointer' }} onClick={() => navigate(stationRoute(r.id))}>
                    <Typography variant="body2" fontWeight={600}>{r.name}</Typography>
                  </TableCell>
                  <TableCell>{r.siteType ? translate(`energy.siteType.${r.siteType}`) : '—'}</TableCell>
                  <TableCell>{landowners.find((l) => l.id === r.spvPayeeId)?.name ?? '—'}</TableCell>
                  <TableCell>{r.serviceDistrict ?? '—'}</TableCell>
                  <TableCell align="right">{r.capacityKw ?? '—'}</TableCell>
                  <TableCell>{r.gridDate ?? '—'}</TableCell>
                  <TableCell align="right">{r.meterCount}</TableCell>
                  <TableCell>
                    <StatusChip i18nKey="energy.stations.enabledChip" value={r.enabled ? 'yes' : 'no'} />
                  </TableCell>
                  <TableCell>
                    <Button size="small" onClick={() => openEdit(r)} data-testid="station-edit">
                      {translate('ra.action.edit')}
                    </Button>
                  </TableCell>
                </TableRow>
              )) : <EmptyRow colSpan={10} loading={loading} />}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <Dialog open={open} onClose={() => setOpen(false)} fullWidth maxWidth="sm" data-testid="station-form-dialog">
        <DialogTitle>{form.id ? translate('ra.action.edit') : translate('energy.stations.newStation')}</DialogTitle>
        <DialogContent>
          <Stack gap={2} mt={1}>
            <TextField label={translate('energy.stations.name')} value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} data-testid="station-form-name" required />
            {!form.id && (
              <TextField label={translate('energy.bills.customerNumber')} value={form.customerNumber} onChange={(e) => setForm({ ...form, customerNumber: e.target.value })} data-testid="station-form-customer-number" required />
            )}
            <FormControl fullWidth required data-testid="station-form-landowner">
              <InputLabel>{translate('energy.landowners.name')}</InputLabel>
              <Select
                label={translate('energy.landowners.name')}
                value={form.landownerId}
                onChange={(e) => setForm({ ...form, landownerId: e.target.value })}
              >
                {landowners.map((l) => <MenuItem key={l.id} value={String(l.id)}>{l.name}</MenuItem>)}
              </Select>
            </FormControl>
            <FormControl fullWidth data-testid="station-form-sitetype">
              <InputLabel>{translate('energy.stations.siteType')}</InputLabel>
              <Select
                label={translate('energy.stations.siteType')}
                value={form.siteType}
                onChange={(e) => setForm({ ...form, siteType: e.target.value })}
              >
                <MenuItem value="">—</MenuItem>
                {SITE_TYPES.map((t) => <MenuItem key={t} value={t}>{translate(`energy.siteType.${t}`)}</MenuItem>)}
              </Select>
            </FormControl>
            <FormControl fullWidth data-testid="station-form-spv">
              <InputLabel>{translate('energy.stations.spv')}</InputLabel>
              <Select
                label={translate('energy.stations.spv')}
                value={form.spvPayeeId}
                onChange={(e) => setForm({ ...form, spvPayeeId: e.target.value })}
              >
                <MenuItem value="">—</MenuItem>
                {subsidiaries.map((l) => (
                  <MenuItem key={l.id} value={String(l.id)}>{l.name}{l.code ? `（${l.code}）` : ''}</MenuItem>
                ))}
              </Select>
            </FormControl>
            <TextField label={translate('energy.stations.serviceDistrict')} value={form.serviceDistrict} onChange={(e) => setForm({ ...form, serviceDistrict: e.target.value })} data-testid="station-form-district" />
            <TextField label={translate('energy.stations.capacityKw')} value={form.capacityKw} onChange={(e) => setForm({ ...form, capacityKw: e.target.value })} data-testid="station-form-capacity" />
            <FormControlLabel
              data-testid="station-form-enabled"
              control={(
                <Switch
                  checked={form.enabled}
                  onChange={(e) => setForm({ ...form, enabled: e.target.checked })}
                />
              )}
              label={translate('energy.stations.enabled')}
            />
            <TextField
              label={translate('energy.stations.gridDate')} type="date" value={form.gridDate}
              onChange={(e) => setForm({ ...form, gridDate: e.target.value })} InputLabelProps={{ shrink: true }} data-testid="station-form-griddate"
            />
            <TextField label={translate('energy.stations.serialNo')} value={form.serialNo} onChange={(e) => setForm({ ...form, serialNo: e.target.value })} data-testid="station-form-serialno" />
            <TextField label={translate('energy.stations.address')} value={form.address} onChange={(e) => setForm({ ...form, address: e.target.value })} />
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setOpen(false)}>{translate('ra.action.cancel')}</Button>
          <Button variant="contained" onClick={save} disabled={saving || !form.name || !form.landownerId} data-testid="station-form-save">
            {translate('ra.action.save')}
          </Button>
        </DialogActions>
      </Dialog>
    </EnergyPageFrame>
  );
};

export default StationsPage;
