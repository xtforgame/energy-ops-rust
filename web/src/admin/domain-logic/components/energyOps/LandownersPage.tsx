import * as React from 'react';
import {
  Card, CardContent, Typography, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, Tooltip, IconButton, TextField, Chip, Button, Dialog, DialogTitle, DialogContent, DialogActions,
  MenuItem, Select, InputLabel, FormControl,
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
import { landownerRoute } from './routes';

interface LandownerRow {
  id: number; name: string; taxId: string | null; contactName: string | null;
  contactPhone: string | null; status: string; partyType: string; code: string | null; address: string | null;
  stationCount: number; abnormalStationCount: number; contractCount: number;
}

const PARTY_TYPES = ['individual', 'company', 'agency', 'subsidiary', 'taipower_district', 'tianneng'];

interface FormState {
  id?: number;
  name: string; partyType: string; code: string; taxId: string; contactName: string; contactPhone: string;
  contactEmail: string; address: string; bankName: string; bankBranch: string; bankAccount: string; bankAccountName: string;
}

const emptyForm: FormState = {
  name: '', partyType: 'individual', code: '', taxId: '', contactName: '', contactPhone: '', contactEmail: '',
  address: '', bankName: '', bankBranch: '', bankAccount: '', bankAccountName: '',
};

/**
 * P5「受款對象」（卡 20261118-0427-a 前稱「地主」，S3 起加類型與新增／編輯）。
 *
 * 清單而已 —— 詳情在**獨立路由** `#/energyLandowners/:id`（`LandownerDetailPage`）。
 * **不再用 drawer**（2026-08-27 拍板）：drawer 適合「看一眼就關掉」的次要資訊，
 * 而這頁是**入口頁**，使用者會從那裡出發去很多地方 ——
 * 它需要完整的一頁**與可分享的網址**。
 */
const LandownersPage: React.FC = () => {
  const notify = useNotify();
  const translate = useTranslate();
  const navigate = useNavigate();
  const selected = useSelection();
  const orgId = selected?.organizationId;
  const base = energyBase(orgId);

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [rows, setRows] = React.useState<LandownerRow[]>([]);
  const [keyword, setKeyword] = React.useState('');
  const [partyTypeFilter, setPartyTypeFilter] = React.useState('');
  const [open, setOpen] = React.useState(false);
  const [form, setForm] = React.useState<FormState>(emptyForm);
  const [saving, setSaving] = React.useState(false);

  const load = React.useCallback(async () => {
    if (!orgId) { setRows([]); setDenied(false); return; }
    setLoading(true); setDenied(false);
    try {
      const { data } = await energyHttp.get(`${base}/landowners`, {
        params: {
          ...(keyword.trim() ? { q: keyword.trim() } : {}),
          ...(partyTypeFilter ? { partyType: partyTypeFilter } : {}),
        },
      });
      setRows(Array.isArray(data) ? data : []);
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
    } finally {
      setLoading(false);
    }
  }, [orgId, base, keyword, partyTypeFilter, notify]);

  React.useEffect(() => { load(); }, [load]);

  const openCreate = () => { setForm(emptyForm); setOpen(true); };
  const openEdit = (r: LandownerRow) => {
    setForm({
      id: r.id, name: r.name, partyType: r.partyType, code: r.code ?? '', taxId: r.taxId ?? '',
      contactName: r.contactName ?? '', contactPhone: r.contactPhone ?? '', contactEmail: '',
      address: r.address ?? '', bankName: '', bankBranch: '', bankAccount: '', bankAccountName: '',
    });
    setOpen(true);
  };

  const save = async () => {
    if (!orgId) return;
    setSaving(true);
    try {
      const body = { ...form };
      if (form.id) await energyHttp.patch(`${base}/landowners/${form.id}`, body);
      else await energyHttp.post(`${base}/landowners`, body);
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
        <Typography variant="h6">{translate('energy.landowners.title')}</Typography>
        <Stack direction="row" gap={1} alignItems="center">
          <FormControl size="small" sx={{ minWidth: 140 }} data-testid="landowner-partytype-filter">
            <InputLabel>{translate('energy.landowners.partyType')}</InputLabel>
            <Select
              label={translate('energy.landowners.partyType')}
              value={partyTypeFilter}
              onChange={(e) => setPartyTypeFilter(e.target.value)}
            >
              <MenuItem value="">{translate('energy.common.all')}</MenuItem>
              {PARTY_TYPES.map((t) => <MenuItem key={t} value={t}>{translate(`energy.partyType.${t}`)}</MenuItem>)}
            </Select>
          </FormControl>
          <TextField
            size="small"
            label={translate('energy.landowners.search')}
            value={keyword}
            onChange={(e) => setKeyword(e.target.value)}
            inputProps={{ 'data-testid': 'landowner-search' }}
          />
          <Tooltip title={translate('ra.action.refresh')}>
            <IconButton onClick={load} disabled={loading}><RefreshIcon /></IconButton>
          </Tooltip>
          <Button variant="contained" startIcon={<AddIcon />} onClick={openCreate} data-testid="landowner-new">
            {translate('energy.landowners.newLandowner')}
          </Button>
        </Stack>
      </Stack>

      <Card>
        <CardContent>
          <Table size="small" data-testid="landowner-list-table">
            <TableHead>
              <TableRow>
                <TableCell>{translate('energy.landowners.name')}</TableCell>
                <TableCell>{translate('energy.landowners.partyType')}</TableCell>
                <TableCell>{translate('energy.landowners.taxId')}</TableCell>
                <TableCell>{translate('energy.landowners.contact')}</TableCell>
                <TableCell align="right">{translate('energy.landowners.stationCount')}</TableCell>
                <TableCell align="right">{translate('energy.landowners.contractCount')}</TableCell>
                <TableCell>{translate('energy.landowners.status')}</TableCell>
                <TableCell />
              </TableRow>
            </TableHead>
            <TableBody>
              {rows.length ? rows.map((r) => (
                <TableRow key={r.id} hover data-testid="landowner-row">
                  <TableCell
                    sx={{ cursor: 'pointer' }}
                    // 改導到**獨立路由**（`FRONTEND.md §2 P5` 拍板）——
                    // 不再用 drawer：這頁是入口頁，需要完整的一頁**與可分享的網址**。
                    onClick={() => navigate(landownerRoute(r.id))}
                  >
                    <Typography variant="body2" fontWeight={600}>{r.name}</Typography>
                  </TableCell>
                  <TableCell data-testid="landowner-partytype">
                    {translate(`energy.partyType.${r.partyType}`)}{r.code ? `（${r.code}）` : ''}
                  </TableCell>
                  <TableCell>
                    <Typography variant="body2" sx={{ fontVariantNumeric: 'tabular-nums' }}>
                      {r.taxId || '—'}
                    </Typography>
                  </TableCell>
                  <TableCell>
                    <Typography variant="body2">{r.contactName || '—'}</Typography>
                    <Typography variant="caption" color="text.secondary">{r.contactPhone || ''}</Typography>
                  </TableCell>
                  <TableCell align="right">
                    <Stack direction="row" gap={0.5} justifyContent="flex-end" alignItems="center">
                      <Typography variant="body2">{r.stationCount}</Typography>
                      {/* 有異常案場就標**燒橘**（不是紅）—— 清單上要一眼看得出來。 */}
                      {r.abnormalStationCount > 0 && (
                        <Chip
                          size="small"
                          variant="outlined"
                          label={translate('energy.landowners.abnormal', { count: r.abnormalStationCount })}
                          sx={{ color: jvTokens.warn, borderColor: jvTokens.warn }}
                        />
                      )}
                    </Stack>
                  </TableCell>
                  <TableCell align="right">{r.contractCount}</TableCell>
                  <TableCell><StatusChip i18nKey="energy.landownerStatus" value={r.status} /></TableCell>
                  <TableCell>
                    <Button size="small" onClick={() => openEdit(r)} data-testid="landowner-edit">
                      {translate('ra.action.edit')}
                    </Button>
                  </TableCell>
                </TableRow>
              )) : <EmptyRow colSpan={8} loading={loading} />}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <Dialog open={open} onClose={() => setOpen(false)} fullWidth maxWidth="sm" data-testid="landowner-form-dialog">
        <DialogTitle>{form.id ? translate('ra.action.edit') : translate('energy.landowners.newLandowner')}</DialogTitle>
        <DialogContent>
          <Stack gap={2} mt={1}>
            <TextField label={translate('energy.landowners.name')} value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} data-testid="landowner-form-name" required />
            <FormControl fullWidth data-testid="landowner-form-partytype">
              <InputLabel>{translate('energy.landowners.partyType')}</InputLabel>
              <Select
                label={translate('energy.landowners.partyType')}
                value={form.partyType}
                onChange={(e) => setForm({ ...form, partyType: e.target.value })}
              >
                {PARTY_TYPES.map((t) => <MenuItem key={t} value={t}>{translate(`energy.partyType.${t}`)}</MenuItem>)}
              </Select>
            </FormControl>
            {form.partyType === 'subsidiary' && (
              <TextField label={translate('energy.landowners.code')} value={form.code} onChange={(e) => setForm({ ...form, code: e.target.value })} data-testid="landowner-form-code" />
            )}
            <TextField label={translate('energy.landowners.taxId')} value={form.taxId} onChange={(e) => setForm({ ...form, taxId: e.target.value })} data-testid="landowner-form-taxid" />
            <TextField label={translate('energy.landowners.contact')} value={form.contactName} onChange={(e) => setForm({ ...form, contactName: e.target.value })} />
            <TextField label="電話" value={form.contactPhone} onChange={(e) => setForm({ ...form, contactPhone: e.target.value })} />
            <TextField label={translate('energy.landowners.address')} value={form.address} onChange={(e) => setForm({ ...form, address: e.target.value })} />
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setOpen(false)}>{translate('ra.action.cancel')}</Button>
          <Button variant="contained" onClick={save} disabled={saving || !form.name} data-testid="landowner-form-save">
            {translate('ra.action.save')}
          </Button>
        </DialogActions>
      </Dialog>
    </EnergyPageFrame>
  );
};

export default LandownersPage;
