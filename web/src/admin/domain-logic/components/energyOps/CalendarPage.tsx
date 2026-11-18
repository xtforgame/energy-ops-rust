import * as React from 'react';
import {
  Box, Card, CardContent, Typography, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, Tooltip, IconButton, ToggleButton, ToggleButtonGroup, TextField,
  Dialog, DialogTitle, DialogContent, DialogActions, Button, MenuItem, Select, InputLabel, FormControl,
} from '@mui/material';
import RefreshIcon from '@mui/icons-material/Refresh';
import { energyHttp, energyBase } from '@/api/energy';
import { useNotify, useTranslate } from 'react-admin';
import { useSearchParams } from 'react-router-dom';
import { useSelection } from '@/platform/selection/store';
import { apiErrorCode } from '../../../dataProvider/apiError';
import { EnergyPageFrame, EmptyRow, StatusChip, jvTokens } from './shared';

interface CalendarEvent {
  id: number; eventDate: string; kind: string; title: string; status: string;
  landownerId: number | null; landownerName: string | null;
  stationId: number | null; stationName: string | null; occurredOn: string | null;
}

const EVENT_STATUSES = ['pending', 'overdue', 'done', 'cancelled'];

/** `YYYY-MM` → 該月最後一天。不要寫死 31。 */
const monthEnd = (month: string) => {
  const [y, m] = month.split('-').map(Number);
  // `Date.UTC(y, m, 0)` ＝ 第 m 個月的第 0 天 ＝ 上個月的最後一天 ＝ 這個月的最後一天。
  const last = new Date(Date.UTC(y, m, 0)).getUTCDate();
  return `${month}-${String(last).padStart(2, '0')}`;
};

const thisMonth = () => {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`;
};

/**
 * 把 `YYYY-MM` 攤成月曆格子（週一起始）。
 *
 * 全部用 **UTC** 建構日期並用字串比對 —— 不要用本地時區的 `new Date(y, m, d)`
 * 再 `toISOString()`：那會在 UTC+8 把每一天都往前推成前一天，
 * 月曆會整體偏移一格，而且**只有在某些時區才看得出來**。
 */
function buildMonthGrid(month: string): Array<{ date: string | null }> {
  const [y, m] = month.split('-').map(Number);
  if (!y || !m) return [];
  const first = new Date(Date.UTC(y, m - 1, 1));
  const daysInMonth = new Date(Date.UTC(y, m, 0)).getUTCDate();
  // getUTCDay: 0=週日 → 轉成「週一為 0」。
  const lead = (first.getUTCDay() + 6) % 7;
  const cells: Array<{ date: string | null }> = [];
  for (let i = 0; i < lead; i += 1) cells.push({ date: null });
  for (let d = 1; d <= daysInMonth; d += 1) {
    cells.push({ date: `${month}-${String(d).padStart(2, '0')}` });
  }
  while (cells.length % 7 !== 0) cells.push({ date: null });
  return cells;
}

/**
 * P8 行事曆（`FRONTEND.md §2-P8`）—— `energyCalendarEvent` 的**月曆／清單雙檢視**。
 *
 * v1 只有 `installmentDue`（保證金期數到期，`DESIGN.md §1.6`）。
 * `billExpected`（預期收到電費單）要等「多久一張單」這個答案，
 * 欄位已經容得下，不需要改表 —— 補的時候只是多一支產生器。
 *
 * 逾期用**燒橘**（不是紅）—— 逾期是「該催了」，不是「壞了」。
 */
const CalendarPage: React.FC = () => {
  const notify = useNotify();
  const translate = useTranslate();
  const orgId = useSelection()?.organizationId;
  // 卡 E S4：從通知頁點一筆跳過來時帶 `?month=&view=`，用它決定初始檢視；只在第一次 render 讀一次。
  const [searchParams] = useSearchParams();

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [events, setEvents] = React.useState<CalendarEvent[]>([]);
  const [month, setMonth] = React.useState(() => searchParams.get('month') || thisMonth());
  const [view, setView] = React.useState<'month' | 'list'>(() => (searchParams.get('view') === 'list' ? 'list' : 'month'));
  const [editing, setEditing] = React.useState<CalendarEvent | null>(null);
  const [occurredOn, setOccurredOn] = React.useState('');
  const [status, setStatus] = React.useState('pending');
  const [saving, setSaving] = React.useState(false);

  const base = energyBase(orgId);

  const load = React.useCallback(async () => {
    if (!orgId) { setEvents([]); setDenied(false); return; }
    setLoading(true); setDenied(false);
    try {
      // 清單檢視刻意**不限月份**：使用者要的是「接下來有什麼」，
      // 不是「這個月剛好有什麼」。月曆檢視才按月切。
      //
      // 月底**必須算出來**，不可以寫死 `-31`：
      // 二月會變成 `2026-02-31` → Postgres `invalid input syntax for type date`。
      // 而且 `type="month"` 的輸入框在使用者清空／改到一半時 `month` 會是 ''，
      // 那時 `${month}-01` 是 `'-01'` —— 同樣會炸。所以先驗格式再送。
      const validMonth = /^\d{4}-\d{2}$/.test(month);
      const params = (view === 'month' && validMonth)
        ? { from: `${month}-01`, to: monthEnd(month) }
        : {};
      if (view === 'month' && !validMonth) { setEvents([]); setLoading(false); return; }
      const { data } = await energyHttp.get(`${base}/calendar-events`, { params });
      setEvents(Array.isArray(data) ? data : []);
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
    } finally {
      setLoading(false);
    }
  }, [orgId, base, month, view, notify]);

  React.useEffect(() => { load(); }, [load]);

  const openEdit = (ev: CalendarEvent) => {
    setEditing(ev);
    setOccurredOn(ev.occurredOn ?? '');
    setStatus(ev.status);
  };

  const save = async () => {
    if (!editing) return;
    setSaving(true);
    try {
      await energyHttp.patch(`${base}/calendar-events/${editing.id}`, { occurredOn: occurredOn || null, status });
      notify('ra.notification.updated', { type: 'info', messageArgs: { smart_count: 1 } });
      setEditing(null);
      load();
    } catch (e: any) {
      notify(apiErrorCode(e) || e?.message || 'save failed', { type: 'error' });
    } finally {
      setSaving(false);
    }
  };

  const byDate = React.useMemo(() => {
    const m: Record<string, CalendarEvent[]> = {};
    events.forEach((e) => { (m[e.eventDate] = m[e.eventDate] || []).push(e); });
    return m;
  }, [events]);

  const cells = React.useMemo(() => buildMonthGrid(month), [month]);
  const weekdayKeys = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'];

  return (
    <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2} flexWrap="wrap" gap={1}>
        <Typography variant="h6">{translate('energy.calendar.title')}</Typography>
        <Stack direction="row" gap={1} alignItems="center">
          <ToggleButtonGroup
            size="small" exclusive value={view}
            onChange={(_, v) => v && setView(v)}
          >
            <ToggleButton value="month" data-testid="calendar-view-month">
              {translate('energy.calendar.viewMonth')}
            </ToggleButton>
            <ToggleButton value="list" data-testid="calendar-view-list">
              {translate('energy.calendar.viewList')}
            </ToggleButton>
          </ToggleButtonGroup>
          {view === 'month' && (
            <TextField
              size="small" type="month" label={translate('energy.dashboard.month')}
              InputLabelProps={{ shrink: true }}
              value={month} onChange={(e) => setMonth(e.target.value)}
              inputProps={{ 'data-testid': 'calendar-month' }}
            />
          )}
          <Tooltip title={translate('ra.action.refresh')}>
            <IconButton onClick={load} disabled={loading}><RefreshIcon /></IconButton>
          </Tooltip>
        </Stack>
      </Stack>

      <Card>
        <CardContent>
          {view === 'month' ? (
            <Box data-testid="calendar-grid">
              <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(7, 1fr)', gap: 1, mb: 1 }}>
                {weekdayKeys.map((w) => (
                  <Typography key={w} variant="caption" color="text.secondary" align="center">
                    {translate(`energy.calendar.weekday.${w}`)}
                  </Typography>
                ))}
              </Box>
              <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(7, 1fr)', gap: 1 }}>
                {cells.map((c, i) => (
                  <Box
                    // eslint-disable-next-line react/no-array-index-key
                    key={c.date ?? `pad-${i}`}
                    data-testid={c.date ? 'calendar-day' : undefined}
                    data-date={c.date ?? undefined}
                    sx={{
                      minHeight: 84,
                      p: 0.75,
                      border: `1px solid ${c.date ? jvTokens.hairline : 'transparent'}`,
                      borderRadius: 1,
                      background: c.date ? '#fff' : 'transparent',
                    }}
                  >
                    {c.date && (
                      <>
                        <Typography variant="caption" color="text.secondary">
                          {Number(c.date.slice(8))}
                        </Typography>
                        {(byDate[c.date] ?? []).map((ev) => (
                          <Tooltip key={ev.id} title={`${ev.title}${ev.stationName ? ` — ${ev.stationName}` : (ev.landownerName ? ` — ${ev.landownerName}` : '')}`}>
                            <Box
                              data-testid="calendar-event"
                              onClick={() => openEdit(ev)}
                              sx={{
                                mt: 0.5, px: 0.75, py: 0.25, borderRadius: 0.5, fontSize: 11,
                                color: '#fff', cursor: 'pointer',
                                // 逾期燒橘（不是紅）；已完成綠；取消灰；其餘（待處理）主色。
                                background: (ev.status === 'missed' || ev.status === 'overdue') ? jvTokens.warn
                                  : ev.status === 'done' ? jvTokens.ok
                                  : ev.status === 'cancelled' ? jvTokens.muted : jvTokens.primary,
                                whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
                              }}
                            >
                              {ev.title}
                            </Box>
                          </Tooltip>
                        ))}
                      </>
                    )}
                  </Box>
                ))}
              </Box>
            </Box>
          ) : (
            <Table size="small" data-testid="calendar-list">
              <TableHead>
                <TableRow>
                  <TableCell>{translate('energy.events.date')}</TableCell>
                  <TableCell>{translate('energy.events.title')}</TableCell>
                  <TableCell>{translate('energy.contracts.stations')}</TableCell>
                  <TableCell>{translate('energy.landowners.name')}</TableCell>
                  <TableCell>{translate('energy.events.status')}</TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {events.length ? events.map((ev) => (
                  <TableRow key={ev.id} hover sx={{ cursor: 'pointer' }} onClick={() => openEdit(ev)} data-testid="calendar-list-row">
                    <TableCell sx={{ fontVariantNumeric: 'tabular-nums' }}>{ev.eventDate}</TableCell>
                    <TableCell>{ev.title}</TableCell>
                    <TableCell>{ev.stationName || '—'}</TableCell>
                    <TableCell>{ev.landownerName || '—'}</TableCell>
                    <TableCell><StatusChip i18nKey="energy.eventStatus" value={ev.status} /></TableCell>
                  </TableRow>
                )) : <EmptyRow colSpan={5} loading={loading} />}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

      {/* 卡 E S1：記實際發生日／改狀態。填了實際發生日沒另外選狀態，後端自動轉「已完成」。 */}
      <Dialog open={!!editing} onClose={() => setEditing(null)} fullWidth maxWidth="xs" data-testid="calendar-event-dialog">
        <DialogTitle>{editing?.title}</DialogTitle>
        <DialogContent>
          <Stack gap={2} mt={1}>
            <TextField
              label={translate('energy.events.occurredOn')} type="date" value={occurredOn}
              onChange={(e) => setOccurredOn(e.target.value)} InputLabelProps={{ shrink: true }}
              data-testid="calendar-event-occurred"
            />
            <FormControl fullWidth data-testid="calendar-event-status">
              <InputLabel>{translate('energy.events.status')}</InputLabel>
              <Select label={translate('energy.events.status')} value={status} onChange={(e) => setStatus(e.target.value)}>
                {EVENT_STATUSES.map((s) => <MenuItem key={s} value={s}>{translate(`energy.eventStatus.${s}`)}</MenuItem>)}
              </Select>
            </FormControl>
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setEditing(null)}>{translate('ra.action.cancel')}</Button>
          <Button variant="contained" onClick={save} disabled={saving} data-testid="calendar-event-save">
            {translate('ra.action.save')}
          </Button>
        </DialogActions>
      </Dialog>
    </EnergyPageFrame>
  );
};

export default CalendarPage;
