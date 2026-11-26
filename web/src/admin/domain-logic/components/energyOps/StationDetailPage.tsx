import RentPaymentPreview from './RentPaymentPreview';
import * as React from 'react';
import {
  Box, Card, CardContent, Typography, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, Chip, IconButton, Button, Alert, TextField, Dialog, DialogTitle, DialogContent, DialogActions,
  MenuItem, Select, InputLabel, FormControl,
} from '@mui/material';
import ArrowBackIcon from '@mui/icons-material/ArrowBack';
import RefreshIcon from '@mui/icons-material/Refresh';
import AddIcon from '@mui/icons-material/Add';
import { energyHttp, energyBase } from '@/api/energy';
import { useNotify, useTranslate } from 'react-admin';
import { useParams, useNavigate } from 'react-router-dom';
import { useSelection } from '@/platform/selection/store';
import { apiErrorCode } from '../../../dataProvider/apiError';
import {
  EnergyPageFrame, EmptyRow, StatusChip, Money, Period, jvTokens,
} from './shared';
import { billRoute, executionListRoute, landownerRoute, myWorkRoute } from './routes';
// 綁定的三個具名動作（`FRONTEND.md §2.84`）——這一頁是動作的家。
import BindingActions, { type BindingKind } from './BindingActions';
import RentCycle from './RentCycle';
import RentApplication from './RentApplication';
import EasyFlowDefaultsSection from './EasyFlowDefaultsSection';

/** 卡 20261118-0427-a 時區修正：`toISOString()`／`Date` 的 `.slice()` 都是 UTC，顯示給人看要轉本地時間。
 *  後端存的 UTC 不動（`occurredAt` 原樣送 PATCH／顯示其他地方），這裡只管「轉成人看的字串」這一層。 */
function pad2(n: number): string {
  return String(n).padStart(2, '0');
}

/** 本地日期 `YYYY-MM-DD`（預設今天；給「結束」分配的迄日預設值用）。 */
function localDate(d: Date = new Date()): string {
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
}

/** 本地日期時間 `YYYY-MM-DD HH:mm:ss`（給異動紀錄的時間戳顯示用）。 */
function localDateTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return String(iso);
  return `${localDate(d)} ${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`;
}

/** 卡 20261118-0427-a S2：電號區（一對多，有效期重疊回 409 `meter_overlap`）。 */
const MetersSection: React.FC<{ orgId?: number | null; stationId: number; onDone?: () => void }> = ({ orgId, stationId, onDone }) => {
  const notify = useNotify();
  const translate = useTranslate();
  const base = energyBase(orgId);
  const [meters, setMeters] = React.useState<any[]>([]);
  const [open, setOpen] = React.useState(false);
  const [form, setForm] = React.useState({
    customerNumber: '', usage: 'wholesale', validFrom: '', note: '', taipowerContractNo: '', feedInRate: '',
  });
  const [saving, setSaving] = React.useState(false);

  const load = React.useCallback(async () => {
    if (!orgId) return;
    try {
      const { data } = await energyHttp.get(`${base}/stations/${stationId}/meters`);
      setMeters(Array.isArray(data) ? data : []);
    } catch { /* 詳情頁其他區塊照常顯示 */ }
  }, [orgId, base, stationId]);
  React.useEffect(() => { load(); }, [load]);

  const save = async () => {
    setSaving(true);
    try {
      await energyHttp.post(`${base}/stations/${stationId}/meters`, form);
      notify('ra.notification.updated', { type: 'info', messageArgs: { smart_count: 1 } });
      setOpen(false);
      setForm({ customerNumber: '', usage: 'wholesale', validFrom: '', note: '', taipowerContractNo: '', feedInRate: '' });
      load();
      // 補電號之後有可能自動補上工作流綁定（卡 20261125-2334），讓外層重抓綁定區才看得到。
      onDone?.();
    } catch (e: any) {
      if (apiErrorCode(e) === 'meter_overlap') {
        notify(translate('energy.meters.overlap'), { type: 'warning' });
      } else {
        notify(apiErrorCode(e) || e?.message || 'save failed', { type: 'error' });
      }
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card sx={{ mb: 2 }}>
      <CardContent>
        <Stack direction="row" alignItems="center" justifyContent="space-between" mb={1}>
          <Typography variant="subtitle1">{translate('energy.meters.title')}</Typography>
          <Button size="small" startIcon={<AddIcon />} onClick={() => setOpen(true)} data-testid="meter-add">
            {translate('energy.meters.add')}
          </Button>
        </Stack>
        <Table size="small" data-testid="meter-list">
          <TableHead>
            <TableRow>
              <TableCell>{translate('energy.bills.customerNumber')}</TableCell>
              <TableCell>{translate('energy.meters.usage')}</TableCell>
              <TableCell>{translate('energy.meters.taipowerContractNo')}</TableCell>
              <TableCell align="right">{translate('energy.meters.feedInRate')}</TableCell>
              <TableCell>{translate('energy.stations.validFrom')}</TableCell>
              <TableCell>{translate('energy.stations.validTo')}</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {meters.length ? meters.map((m) => (
              <TableRow key={m.id} data-testid="meter-row">
                <TableCell sx={{ fontVariantNumeric: 'tabular-nums' }}>{m.customerNumber}</TableCell>
                <TableCell>{translate(`energy.meters.usageType.${m.usage}`)}</TableCell>
                <TableCell>{m.taipowerContractNo ?? '—'}</TableCell>
                <TableCell align="right">{m.feedInRate ?? '—'}</TableCell>
                <TableCell>{m.validFrom ?? '—'}</TableCell>
                <TableCell>{m.validTo ?? translate('energy.stations.openEnded')}</TableCell>
              </TableRow>
            )) : <EmptyRow colSpan={6} />}
          </TableBody>
        </Table>
      </CardContent>
      <Dialog open={open} onClose={() => setOpen(false)} fullWidth maxWidth="xs" data-testid="meter-form-dialog">
        <DialogTitle>{translate('energy.meters.add')}</DialogTitle>
        <DialogContent>
          <Stack gap={2} mt={1}>
            <TextField
              label={translate('energy.bills.customerNumber')} value={form.customerNumber}
              onChange={(e) => setForm({ ...form, customerNumber: e.target.value })} data-testid="meter-form-customer-number" required
            />
            <FormControl fullWidth data-testid="meter-form-usage">
              <InputLabel>{translate('energy.meters.usage')}</InputLabel>
              <Select
                label={translate('energy.meters.usage')} value={form.usage}
                onChange={(e) => setForm({ ...form, usage: e.target.value })}
              >
                <MenuItem value="wholesale">{translate('energy.meters.usageType.wholesale')}</MenuItem>
                <MenuItem value="consumption">{translate('energy.meters.usageType.consumption')}</MenuItem>
              </Select>
            </FormControl>
            <TextField
              label={translate('energy.meters.taipowerContractNo')} value={form.taipowerContractNo}
              onChange={(e) => setForm({ ...form, taipowerContractNo: e.target.value })}
              data-testid="meter-form-contract-no"
            />
            <TextField
              label={translate('energy.meters.feedInRate')} value={form.feedInRate}
              onChange={(e) => setForm({ ...form, feedInRate: e.target.value })}
              data-testid="meter-form-feedinrate"
            />
            <TextField
              label={translate('energy.stations.validFrom')} type="date" value={form.validFrom}
              onChange={(e) => setForm({ ...form, validFrom: e.target.value })} InputLabelProps={{ shrink: true }}
              data-testid="meter-form-validfrom"
            />
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setOpen(false)}>{translate('ra.action.cancel')}</Button>
          <Button variant="contained" onClick={save} disabled={saving || !form.customerNumber} data-testid="meter-form-save">
            {translate('ra.action.save')}
          </Button>
        </DialogActions>
      </Dialog>
    </Card>
  );
};

/** 卡 20261118-0427-a S4：地主分配區（比例／固定金額／依土地面積）＋異動紀錄。 */
const AllocationsSection: React.FC<{ orgId?: number | null; stationId: number }> = ({ orgId, stationId }) => {
  const notify = useNotify();
  const translate = useTranslate();
  const base = energyBase(orgId);
  const [allocations, setAllocations] = React.useState<any[]>([]);
  const [history, setHistory] = React.useState<any[]>([]);
  const [landowners, setLandowners] = React.useState<any[]>([]);
  const [open, setOpen] = React.useState(false);
  const [form, setForm] = React.useState({ landownerId: '', method: 'ratio', ratio: '', fixedAmount: '', landArea: '', validFrom: '' });
  const [editId, setEditId] = React.useState<number | null>(null);
  const [endTarget, setEndTarget] = React.useState<any>(null);
  const [endReason, setEndReason] = React.useState('');
  const [saving, setSaving] = React.useState(false);

  const load = React.useCallback(async () => {
    if (!orgId) return;
    try {
      const [a, h, l] = await Promise.all([
        energyHttp.get(`${base}/stations/${stationId}/allocations`),
        energyHttp.get(`${base}/stations/${stationId}/allocation-history`),
        energyHttp.get(`${base}/landowners`),
      ]);
      setAllocations(Array.isArray(a.data) ? a.data : []);
      setHistory(Array.isArray(h.data) ? h.data : []);
      setLandowners(Array.isArray(l.data) ? l.data : []);
    } catch { /* 詳情頁其他區塊照常顯示 */ }
  }, [orgId, base, stationId]);
  React.useEffect(() => { load(); }, [load]);

  const save = async () => {
    setSaving(true);
    try {
      // landownerId 是 MUI Select 的字串值，後端要的是整數，這裡轉型（卡 A smoke 揪出：
      // 漏轉時後端 `as_i64()` 對 JSON 字串回 None，一律 400 landowner_id_required）。
      const body = { ...form, landownerId: Number(form.landownerId) };
      if (editId) await energyHttp.patch(`${base}/allocations/${editId}`, body);
      else await energyHttp.post(`${base}/stations/${stationId}/allocations`, body);
      notify('ra.notification.updated', { type: 'info', messageArgs: { smart_count: 1 } });
      setOpen(false);
      setEditId(null);
      setForm({ landownerId: '', method: 'ratio', ratio: '', fixedAmount: '', landArea: '', validFrom: '' });
      load();
    } catch (e: any) {
      notify(apiErrorCode(e) || e?.message || 'save failed', { type: 'error' });
    } finally {
      setSaving(false);
    }
  };

  const openEdit = (a: any) => {
    setEditId(a.id);
    setForm({
      landownerId: String(a.landownerId), method: a.method, ratio: a.ratio ?? '',
      fixedAmount: a.fixedAmount ?? '', landArea: a.landArea ?? '', validFrom: a.validFrom ?? '',
    });
    setOpen(true);
  };

  const endAllocation = async () => {
    if (!endTarget) return;
    setSaving(true);
    try {
      const today = localDate();
      await energyHttp.patch(`${base}/allocations/${endTarget.id}`, { validTo: today, changeReason: endReason });
      notify('ra.notification.updated', { type: 'info', messageArgs: { smart_count: 1 } });
      setEndTarget(null);
      setEndReason('');
      load();
    } catch (e: any) {
      notify(apiErrorCode(e) || e?.message || 'end failed', { type: 'error' });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card sx={{ mb: 2 }}>
      <CardContent>
        <Stack direction="row" alignItems="center" justifyContent="space-between" mb={1}>
          <Typography variant="subtitle1">{translate('energy.allocations.title')}</Typography>
          <Button
            size="small" startIcon={<AddIcon />} data-testid="allocation-add"
            onClick={() => {
              setEditId(null);
              setForm({ landownerId: '', method: 'ratio', ratio: '', fixedAmount: '', landArea: '', validFrom: '' });
              setOpen(true);
            }}
          >
            {translate('energy.allocations.add')}
          </Button>
        </Stack>
        <Table size="small" data-testid="allocation-list">
          <TableHead>
            <TableRow>
              <TableCell>{translate('energy.landowners.name')}</TableCell>
              <TableCell>{translate('energy.allocations.method')}</TableCell>
              <TableCell align="right">{translate('energy.allocations.value')}</TableCell>
              <TableCell>{translate('energy.stations.validFrom')}</TableCell>
              <TableCell>{translate('energy.stations.validTo')}</TableCell>
              <TableCell />
            </TableRow>
          </TableHead>
          <TableBody>
            {allocations.length ? allocations.map((a) => (
              <TableRow key={a.id} data-testid="allocation-row">
                <TableCell>{landowners.find((l) => l.id === a.landownerId)?.name ?? a.landownerId}</TableCell>
                <TableCell>{translate(`energy.allocations.methodType.${a.method}`)}</TableCell>
                <TableCell align="right">
                  {a.method === 'ratio' ? `${a.ratio}%` : (a.method === 'fixed' ? a.fixedAmount : a.landArea)}
                </TableCell>
                <TableCell>{a.validFrom ?? '—'}</TableCell>
                <TableCell>{a.validTo ?? translate('energy.stations.openEnded')}</TableCell>
                <TableCell>
                  {a.active && (
                    <>
                      <Button size="small" onClick={() => openEdit(a)} data-testid="allocation-edit">
                        {translate('ra.action.edit')}
                      </Button>
                      <Button size="small" onClick={() => setEndTarget(a)} data-testid="allocation-end">
                        {translate('energy.allocations.end')}
                      </Button>
                    </>
                  )}
                </TableCell>
              </TableRow>
            )) : <EmptyRow colSpan={6} />}
          </TableBody>
        </Table>

        <Typography variant="subtitle2" mt={2} mb={0.5}>{translate('energy.allocations.history')}</Typography>
        <Stack gap={0.5} data-testid="allocation-history">
          {history.length ? history.map((h) => (
            <Typography key={h.id} variant="body2" data-testid="allocation-history-row">
              {localDateTime(h.occurredAt)}　{h.summary}
              <Typography component="span" variant="caption" color="text.secondary" data-testid="allocation-history-actor">
                （{translate('energy.allocations.by', { who: h.actorLabel ?? '—' })}）
              </Typography>
            </Typography>
          )) : <Typography variant="body2" color="text.secondary">{translate('energy.common.empty')}</Typography>}
        </Stack>
      </CardContent>

      <Dialog open={open} onClose={() => setOpen(false)} fullWidth maxWidth="xs" data-testid="allocation-form-dialog">
        <DialogTitle>{translate('energy.allocations.add')}</DialogTitle>
        <DialogContent>
          <Stack gap={2} mt={1}>
            <FormControl fullWidth required data-testid="allocation-form-landowner">
              <InputLabel>{translate('energy.landowners.name')}</InputLabel>
              <Select
                label={translate('energy.landowners.name')} value={form.landownerId}
                onChange={(e) => setForm({ ...form, landownerId: e.target.value })}
              >
                {landowners.map((l) => <MenuItem key={l.id} value={String(l.id)}>{l.name}</MenuItem>)}
              </Select>
            </FormControl>
            <FormControl fullWidth data-testid="allocation-form-method">
              <InputLabel>{translate('energy.allocations.method')}</InputLabel>
              <Select
                label={translate('energy.allocations.method')} value={form.method}
                onChange={(e) => setForm({ ...form, method: e.target.value })}
              >
                <MenuItem value="ratio">{translate('energy.allocations.methodType.ratio')}</MenuItem>
                <MenuItem value="fixed">{translate('energy.allocations.methodType.fixed')}</MenuItem>
                <MenuItem value="land_area">{translate('energy.allocations.methodType.land_area')}</MenuItem>
              </Select>
            </FormControl>
            {form.method === 'ratio' && (
              <TextField label={translate('energy.allocations.ratio')} value={form.ratio} onChange={(e) => setForm({ ...form, ratio: e.target.value })} data-testid="allocation-form-ratio" />
            )}
            {form.method === 'fixed' && (
              <TextField label={translate('energy.allocations.fixedAmount')} value={form.fixedAmount} onChange={(e) => setForm({ ...form, fixedAmount: e.target.value })} data-testid="allocation-form-fixedamount" />
            )}
            {form.method === 'land_area' && (
              <TextField label={translate('energy.allocations.landArea')} value={form.landArea} onChange={(e) => setForm({ ...form, landArea: e.target.value })} data-testid="allocation-form-landarea" />
            )}
            <TextField
              label={translate('energy.stations.validFrom')} type="date" value={form.validFrom}
              onChange={(e) => setForm({ ...form, validFrom: e.target.value })} InputLabelProps={{ shrink: true }}
            />
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setOpen(false)}>{translate('ra.action.cancel')}</Button>
          <Button variant="contained" onClick={save} disabled={saving || !form.landownerId} data-testid="allocation-form-save">
            {translate('ra.action.save')}
          </Button>
        </DialogActions>
      </Dialog>

      <Dialog open={!!endTarget} onClose={() => setEndTarget(null)} fullWidth maxWidth="xs" data-testid="allocation-end-dialog">
        <DialogTitle>{translate('energy.allocations.end')}</DialogTitle>
        <DialogContent>
          <TextField
            fullWidth sx={{ mt: 1 }} label={translate('energy.allocations.changeReason')} value={endReason}
            onChange={(e) => setEndReason(e.target.value)} data-testid="allocation-end-reason" required
          />
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setEndTarget(null)}>{translate('ra.action.cancel')}</Button>
          <Button variant="contained" onClick={endAllocation} disabled={saving || !endReason} data-testid="allocation-end-confirm">
            {translate('ra.action.save')}
          </Button>
        </DialogActions>
      </Dialog>
    </Card>
  );
};

interface AxisSeg {
  id: number;
  validFrom: string;
  validTo: string | null;
  /** 現在生效的那一段 —— **後端算的**，前端不比日期。 */
  active: boolean;
  name?: string | null;
  version?: number | null;
  type?: string | null;
  definitionId?: number | null;
  strategyId?: number | null;
}

/**
 * P11 案場詳情（`FRONTEND.md §2 P11`）。
 *
 * **它回答的問題**：這個案場**適用哪版流程、哪版策略**，最近幾張單怎麼了？
 *
 * **兩條效期軸就是這頁存在的理由**。
 * **必須看得出「現在生效的是哪一段」** ——
 * 光列一堆日期區間看不出重點（規格原文）。
 * 所以生效那段用實心主色 ＋「生效中」標籤，其餘淡化。
 *
 * `active` 由**後端**算（`stationOverview`）——
 * 前端自己比日期，比錯了會指向錯的版本而且完全不會報錯。
 */
const StationDetailPage: React.FC = () => {
  const notify = useNotify();
  const translate = useTranslate();
  const navigate = useNavigate();
  const { id } = useParams();
  const orgId = useSelection()?.organizationId;

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [data, setData] = React.useState<any>(null);
  /** 綁定段 ＋ **後端說的** `canManage`（前端不自己推權限）。 */
  const [bindings, setBindings] = React.useState<any>(null);

  const base = energyBase(orgId);

  const load = React.useCallback(async () => {
    if (!orgId || !id) return;
    setLoading(true); setDenied(false);
    try {
      const { data: d } = await energyHttp.get(`${base}/stations/${id}/overview`);
      setData(d);
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
    } finally { setLoading(false); }
  }, [orgId, id, base, notify]);

  /** 綁定段（含 `editable`：已結束的段唯讀）。 */
  const loadBindings = React.useCallback(async () => {
    if (!orgId || !id) return;
    try {
      const { data: b } = await energyHttp.get(`${base}/stations/${id}/bindings`);
      setBindings(b);
    } catch { setBindings(null); }
  }, [orgId, id, base]);

  React.useEffect(() => { load(); }, [load]);
  React.useEffect(() => { loadBindings(); }, [loadBindings]);

  /**
   * 一條效期軸。
   * 生效中的那一段**視覺上要跳出來** —— 這是這頁唯一真正重要的資訊。
   */
  const renderAxis = (titleKey: string, segs: AxisSeg[], testid: string, kind: BindingKind) => {
    // 綁定段用**綁定端點**回來的那一份（它帶 `editable`／`isCurrent`）——
    // 不要拿 overview 的 `active` 來推：那是兩份真值。
    const bSegs: any[] = bindings?.[kind] ?? [];
    const current = bSegs.find((x) => x.isCurrent) ?? null;
    /**
     * 綁定分「線」：一條線 ＝ (適用單別, 流程 key)。
     * 每條線各自有自己的現行段 —— 整個案場只算一段的話，
     * 第二條流程會看起來像過期的歷史，而它其實正在跑。
     */
    const laneMap = new Map<string, { docType: string | null; targetKey: string; current: any }>();
    bSegs.forEach((seg) => {
      const key = `${seg.docType ?? ''}|${seg.targetKey ?? ''}`;
      const prev = laneMap.get(key);
      laneMap.set(key, {
        docType: seg.docType ?? null,
        targetKey: String(seg.targetKey ?? ''),
        current: seg.isCurrent ? seg : (prev?.current ?? null),
      });
    });
    const lanes = [...laneMap.values()];
    return (
    <Card sx={{ mb: 2 }}>
      <CardContent>
        <Typography variant="subtitle1" gutterBottom>{translate(titleKey)}</Typography>
        {segs.length ? (
          <Stack direction="row" gap={1} flexWrap="wrap" data-testid={testid}>
            {segs.map((seg) => (
              <Box
                key={seg.id}
                data-testid="axis-segment"
                data-active={seg.active ? '1' : '0'}
                sx={{
                  flex: '1 1 200px',
                  minWidth: 200,
                  p: 1.25,
                  borderRadius: 1,
                  // 生效中＝實心主色；其餘淡化（不是同一種灰）。
                  border: `2px solid ${seg.active ? jvTokens.primary : jvTokens.border}`,
                  background: seg.active ? jvTokens.hoverBg : 'transparent',
                  opacity: seg.active ? 1 : 0.65,
                }}
              >
                <Stack direction="row" alignItems="center" gap={0.75} mb={0.5}>
                  <Typography
                    variant="body2"
                    sx={{ fontWeight: seg.active ? 700 : 500, color: seg.active ? jvTokens.primaryDark : 'text.secondary' }}
                  >
                    {seg.name ?? '—'}
                    {seg.version != null ? ` v${seg.version}` : ''}
                  </Typography>
                  {seg.active && (
                    <Chip
                      size="small"
                      data-testid="axis-active-badge"
                      label={translate('energy.stations.activeNow')}
                      sx={{ background: jvTokens.primary, color: '#fff', fontWeight: 600, height: 20 }}
                    />
                  )}
                </Stack>
                <Typography variant="caption" color="text.secondary" sx={{ fontVariantNumeric: 'tabular-nums' }}>
                  {String(seg.validFrom).slice(0, 10)}
                  {' ~ '}
                  {seg.validTo ? String(seg.validTo).slice(0, 10) : translate('energy.stations.openEnded')}
                </Typography>
                {/* 每一段都可點 → 流程版本（P9 是 C7c；先連到定義本身）。 */}
                {seg.definitionId != null && (
                  <Button
                    size="small"
                    sx={{ p: 0, minWidth: 0, mt: 0.5 }}
                    data-testid="axis-open-workflow"
                    onClick={() => navigate(myWorkRoute())}
                  >
                    {translate('energy.stations.viewWorkflow')}
                  </Button>
                )}
              </Box>
            ))}
          </Stack>
        ) : (
          /* `FRONTEND.md §2.85.1`：空軸不是留白 —— 要說出**後果**與**誰能修**。 */
          <Stack gap={0.5} data-testid="axis-unbound" data-axis={testid}>
            <Typography variant="body2" sx={{ color: jvTokens.warn, fontWeight: 600 }}>
              {translate('energy.stations.noBinding')}
            </Typography>
            <Typography variant="caption" color="text.secondary">
              {translate('energy.stations.noBindingHint')}
            </Typography>
            {/* 指出誰能綁：管理員在 P9 流程總覽。 */}
            <Button
              size="small"
              sx={{ p: 0, minWidth: 0, alignSelf: 'flex-start' }}
              data-testid="axis-open-workflows"
              onClick={() => navigate('/energyWorkflowConsole')}
            >
              {translate('energy.stations.gotoWorkflows')}
            </Button>
          </Stack>
        )}

        {/**
          * **一座案場現在可以綁多條流程**（躉購·有餘電要核可 ＋ 開發票）。
          *
          * 所以動作不是「這個案場的那一段綁定」，而是**每條線各一組**：
          * 一條線 ＝ (適用單別, 流程)。少了這個分組，換版與解除都只能猜
          * 要動哪一條 —— 而解除是 critical 動作。
          */}
        {kind === 'workflow' ? (
          <Stack gap={1} mt={1} data-testid="binding-lanes" data-count={String(lanes.length)}>
            {lanes.map((lane) => (
              <Stack
                key={`${lane.docType ?? ''}|${lane.targetKey}`}
                direction="row"
                alignItems="center"
                gap={1}
                flexWrap="wrap"
                data-testid="binding-lane"
                data-doc-type={lane.docType ?? ''}
                data-workflow={lane.targetKey}
              >
                <Chip
                  size="small"
                  data-testid="lane-doc-type"
                  label={translate(`energy.docType.${lane.docType ?? 'anyDocType'}`, { _: lane.docType ?? '' })}
                  sx={{ borderColor: jvTokens.border, color: jvTokens.caption }}
                  variant="outlined"
                />
                <Typography variant="body2" sx={{ fontWeight: 600 }}>
                  {lane.current
                    ? `${lane.current.name ?? lane.targetKey} v${lane.current.version}`
                    : translate('energy.stations.noBinding')}
                </Typography>
                <BindingActions
                  orgId={orgId}
                  stationId={Number(id)}
                  kind={kind}
                  docType={lane.docType}
                  targetKey={lane.targetKey}
                  hasCurrent={!!lane.current}
                  currentLabel={lane.current ? `${lane.current.name} v${lane.current.version}` : null}
                  canManage={!!bindings?.canManage?.[kind]}
                  onDone={() => { loadBindings(); load(); }}
                />
              </Stack>
            ))}
            {/* 綁**新的一條**：選單別 ＋ 選版本。既有的線在上面各自處理。 */}
            <BindingActions
              orgId={orgId}
              stationId={Number(id)}
              kind={kind}
              allowDocTypeSelect
              hasCurrent={false}
              canManage={!!bindings?.canManage?.[kind]}
              onDone={() => { loadBindings(); load(); }}
            />
          </Stack>
        ) : (
          /* 計價策略不分單別：一座案場的錢怎麼算，與收到哪一種單無關。 */
          <BindingActions
            orgId={orgId}
            stationId={Number(id)}
            kind={kind}
            hasCurrent={!!current}
            currentLabel={current ? `${current.name} v${current.version}` : null}
            canManage={!!bindings?.canManage?.[kind]}
            onDone={() => { loadBindings(); load(); }}
          />
        )}
      </CardContent>
    </Card>
    );
  };

  if (!data && !loading) {
    return (
      <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
        <Alert severity="info">{translate('energy.stations.notFound')}</Alert>
      </EnergyPageFrame>
    );
  }

  return (
    <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2}>
        <Stack direction="row" alignItems="center" gap={1}>
          <IconButton onClick={() => navigate('/energyLandownerConsole')}><ArrowBackIcon /></IconButton>
          <Typography variant="h6" data-testid="station-name">{data?.name}</Typography>
          <StatusChip i18nKey="energy.stationStatus" value={data?.status} />
        </Stack>
        <IconButton onClick={load} disabled={loading}><RefreshIcon /></IconButton>
      </Stack>

      <Card sx={{ mb: 2 }}>
        <CardContent>
          <Stack direction="row" gap={4} flexWrap="wrap">
            <Box>
              <Typography variant="caption" color="text.secondary">
                {translate('energy.bills.customerNumber')}
              </Typography>
              <Typography variant="body2" sx={{ fontVariantNumeric: 'tabular-nums' }}>
                {data?.customerNumber}
              </Typography>
            </Box>
            <Box>
              <Typography variant="caption" color="text.secondary">
                {translate('energy.landowners.name')}
              </Typography>
              {/* 跨頁：案場 → 地主（P5）。 */}
              <Button
                size="small"
                sx={{ p: 0, minWidth: 0, display: 'block' }}
                data-testid="station-open-landowner"
                onClick={() => (data?.landowner
                  ? navigate(landownerRoute(data.landowner.id)) : undefined)}
              >
                {data?.landowner?.name ?? '—'}
              </Button>
            </Box>
            <Box>
              <Typography variant="caption" color="text.secondary">
                {translate('energy.dashboard.coords')}
              </Typography>
              <Typography variant="body2" sx={{ fontVariantNumeric: 'tabular-nums' }}>
                {data?.latitude && data?.longitude
                  ? `${Number(data.latitude).toFixed(4)}, ${Number(data.longitude).toFixed(4)}` : '—'}
              </Typography>
            </Box>
          </Stack>
        </CardContent>
      </Card>

      {/* 卡 20261118-0427-a S2／S4：電號區、地主分配區（各自抓自己的資料，不擋其他區塊）。 */}
      {id && <RentPaymentPreview base={base} stationId={Number(id)} />}
      {/* 卡 20261126-0416 S1：同案場同期多地主合一張租金付款申請。 */}
      {id && <RentApplication base={base} stationId={Number(id)} />}
      {id && <MetersSection orgId={orgId} stationId={Number(id)} onDone={() => { loadBindings(); load(); }} />}
      {id && <AllocationsSection orgId={orgId} stationId={Number(id)} />}
      {/* 卡 20261126-0214：使用者裁定畫面放案場層（地主分配區後面）。 */}
      {id && <EasyFlowDefaultsSection orgId={orgId} stationId={Number(id)} />}

      {/* 兩條效期軸 */}
      {renderAxis('energy.stations.workflowAxis', data?.workflowAxis ?? [], 'workflow-axis', 'workflow')}
      {/* **策略軸也要有** —— 綁定缺口有兩份，只補流程那一份等於只做一半。 */}
      {renderAxis('energy.stations.strategyAxis', data?.strategyAxis ?? [], 'strategy-axis', 'rating')}

      {/* 卡 E S5：合約時間軸（簽約、起、迄、續約提醒、生效中）。 */}
      <Card sx={{ mb: 2 }}>
        <CardContent>
          <Typography variant="subtitle1" mb={1}>{translate('energy.stations.contractTimeline')}</Typography>
          <Table size="small" data-testid="contract-timeline">
            <TableHead>
              <TableRow>
                <TableCell>{translate('energy.contracts.no')}</TableCell>
                <TableCell>{translate('energy.contracts.signedOn')}</TableCell>
                <TableCell>{translate('energy.stations.validFrom')}</TableCell>
                <TableCell>{translate('energy.stations.validTo')}</TableCell>
                <TableCell>{translate('energy.contracts.renewalRemindOn')}</TableCell>
                <TableCell />
              </TableRow>
            </TableHead>
            <TableBody>
              {(data?.contracts ?? []).length ? data.contracts.map((c: any) => (
                <TableRow key={c.id} data-testid="contract-timeline-row">
                  <TableCell><Typography variant="body2" fontWeight={600}>{c.contractNo}</Typography></TableCell>
                  <TableCell>{c.signedOn || '—'}</TableCell>
                  <TableCell>{c.validFrom}</TableCell>
                  <TableCell>{c.validTo || translate('energy.stations.openEnded')}</TableCell>
                  <TableCell>{c.renewalRemindOn || '—'}</TableCell>
                  <TableCell>
                    {c.active && (
                      <Chip
                        size="small" data-testid="contract-timeline-active"
                        label={translate('energy.stations.activeNow')}
                        sx={{ background: jvTokens.primary, color: '#fff', fontWeight: 600, height: 20 }}
                      />
                    )}
                    <RentCycle base={base} contractId={c.id} stationId={Number(id)} rentCalcCode={c.rentCalcCode} />
                  </TableCell>
                </TableRow>
              )) : <EmptyRow colSpan={6} />}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      {/* 卡 E S5：未來事件（本案場或掛在本案場合約上的事件，今天起 20 筆）。 */}
      <Card sx={{ mb: 2 }}>
        <CardContent>
          <Typography variant="subtitle1" mb={1}>{translate('energy.stations.upcomingEvents')}</Typography>
          <Table size="small" data-testid="station-upcoming-events">
            <TableHead>
              <TableRow>
                <TableCell>{translate('energy.events.date')}</TableCell>
                <TableCell>{translate('energy.events.title')}</TableCell>
                <TableCell>{translate('energy.events.status')}</TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {(data?.upcomingEvents ?? []).length ? data.upcomingEvents.map((e: any) => (
                <TableRow key={e.id} data-testid="station-upcoming-event-row">
                  <TableCell sx={{ fontVariantNumeric: 'tabular-nums' }}>{e.eventDate}</TableCell>
                  <TableCell>{e.title}</TableCell>
                  <TableCell><StatusChip i18nKey="energy.eventStatus" value={e.status} /></TableCell>
                </TableRow>
              )) : <EmptyRow colSpan={3} />}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <Card>
        <CardContent>
          <Stack direction="row" alignItems="center" justifyContent="space-between" mb={1}>
            <Typography variant="subtitle1">
              {translate('energy.landowners.tabBills')}（{data?.recentBills?.length ?? 0}）
            </Typography>
            {/* 「最近的單」只是**最近的** —— 不能讓人以為這個案場就這幾張。
                「看全部」帶著 `stationId` 去 P2（那裡才看得到保證金那類）。 */}
            <Button
              size="small"
              data-testid="station-all-executions"
              onClick={() => navigate(executionListRoute({ status: 'running', stationId: id }))}
            >
              {translate('energy.workflows.seeAll')}
            </Button>
          </Stack>
          <Table size="small">
            <TableHead>
              <TableRow>
                <TableCell>{translate('energy.bills.period')}</TableCell>
                <TableCell align="right">{translate('energy.bills.amount')}</TableCell>
                <TableCell>{translate('energy.bills.reviewStatus')}</TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {(data?.recentBills ?? []).length ? data.recentBills.map((b: any) => (
                <TableRow
                  key={b.id}
                  hover
                  sx={{ cursor: 'pointer' }}
                  data-testid="station-bill"
                  onClick={() => navigate(billRoute(b.id))}
                >
                  <TableCell><Period from={b.billingPeriodStart} to={b.billingPeriodEnd} /></TableCell>
                  <TableCell align="right"><Money value={b.totalAmount} /></TableCell>
                  <TableCell><StatusChip i18nKey="energy.reviewStatus" value={b.reviewStatus} /></TableCell>
                </TableRow>
              )) : <EmptyRow colSpan={3} />}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </EnergyPageFrame>
  );
};

export default StationDetailPage;
