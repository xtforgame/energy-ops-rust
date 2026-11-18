import * as React from 'react';
import {
  Box, Card, CardContent, Typography, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, TextField, Button, Checkbox, IconButton, Tooltip,
} from '@mui/material';
import RefreshIcon from '@mui/icons-material/Refresh';
import AddIcon from '@mui/icons-material/Add';
import DeleteIcon from '@mui/icons-material/Delete';
import { energyHttp, energyBase } from '@/api/energy';
import { useNotify, useTranslate } from 'react-admin';
import { useSelection } from '@/platform/selection/store';
import { apiErrorCode } from '../../../dataProvider/apiError';
import { EnergyPageFrame, EmptyRow } from './shared';

interface RuleRow {
  kind: string; leadDays: number | null; repeatDays: number | null;
  businessDays: boolean; dayOfMonth: number | null; enabled: boolean;
}

interface HolidayRow { id: number; day: string; name: string | null; isWorkday: boolean }

/** 卡 E S3：規則參數全部可留空（顯示「未設定」），以及工作天／假日表的維護。 */
const ReminderSettingsPage: React.FC = () => {
  const notify = useNotify();
  const translate = useTranslate();
  const orgId = useSelection()?.organizationId;
  const base = energyBase(orgId);

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [rules, setRules] = React.useState<RuleRow[]>([]);
  const [savingKind, setSavingKind] = React.useState<string | null>(null);

  const [holidays, setHolidays] = React.useState<HolidayRow[]>([]);
  const [newHoliday, setNewHoliday] = React.useState({ day: '', name: '', isWorkday: false });
  const [addingHoliday, setAddingHoliday] = React.useState(false);

  const load = React.useCallback(async () => {
    if (!orgId) { setRules([]); setHolidays([]); setDenied(false); return; }
    setLoading(true); setDenied(false);
    try {
      const [r, h] = await Promise.all([
        energyHttp.get(`${base}/reminder-rules`),
        energyHttp.get(`${base}/holidays`),
      ]);
      setRules(Array.isArray(r.data) ? r.data : []);
      setHolidays(Array.isArray(h.data) ? h.data : []);
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
    } finally {
      setLoading(false);
    }
  }, [orgId, base, notify]);

  React.useEffect(() => { load(); }, [load]);

  const updateRule = (kind: string, patch: Partial<RuleRow>) => {
    setRules((rs) => rs.map((r) => (r.kind === kind ? { ...r, ...patch } : r)));
  };

  const saveRule = async (row: RuleRow) => {
    setSavingKind(row.kind);
    try {
      await energyHttp.put(`${base}/reminder-rules`, { rules: [row] });
      notify('ra.notification.updated', { type: 'info', messageArgs: { smart_count: 1 } });
      load();
    } catch (e: any) {
      notify(apiErrorCode(e) || e?.message || 'save failed', { type: 'error' });
    } finally {
      setSavingKind(null);
    }
  };

  const addHoliday = async () => {
    if (!newHoliday.day) return;
    setAddingHoliday(true);
    try {
      await energyHttp.post(`${base}/holidays`, newHoliday);
      notify('ra.notification.updated', { type: 'info', messageArgs: { smart_count: 1 } });
      setNewHoliday({ day: '', name: '', isWorkday: false });
      load();
    } catch (e: any) {
      const code = apiErrorCode(e);
      notify(code === 'holiday_exists' ? translate('energy.reminders.holidayExists') : (code || e?.message || 'save failed'), { type: 'warning' });
    } finally {
      setAddingHoliday(false);
    }
  };

  const deleteHoliday = async (id: number) => {
    try {
      await energyHttp.delete(`${base}/holidays/${id}`);
      notify('ra.notification.updated', { type: 'info', messageArgs: { smart_count: 1 } });
      load();
    } catch (e: any) {
      notify(apiErrorCode(e) || e?.message || 'delete failed', { type: 'error' });
    }
  };

  const numField = (v: number | null) => (v === null || v === undefined ? '' : String(v));
  const parseNum = (s: string): number | null => (s.trim() === '' ? null : Number(s));

  return (
    <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2}>
        <Typography variant="h6">{translate('energy.reminders.title')}</Typography>
        <Tooltip title={translate('ra.action.refresh')}>
          <IconButton onClick={load} disabled={loading}><RefreshIcon /></IconButton>
        </Tooltip>
      </Stack>

      <Card sx={{ mb: 2 }}>
        <CardContent>
          <Typography variant="subtitle1" mb={1}>{translate('energy.reminders.rulesTitle')}</Typography>
          <Table size="small" data-testid="reminder-rule-table">
            <TableHead>
              <TableRow>
                <TableCell>{translate('energy.reminders.kind')}</TableCell>
                <TableCell>{translate('energy.reminders.leadDays')}</TableCell>
                <TableCell>{translate('energy.reminders.repeatDays')}</TableCell>
                <TableCell>{translate('energy.reminders.businessDays')}</TableCell>
                <TableCell>{translate('energy.reminders.dayOfMonth')}</TableCell>
                <TableCell>{translate('energy.reminders.enabled')}</TableCell>
                <TableCell />
              </TableRow>
            </TableHead>
            <TableBody>
              {rules.length ? rules.map((r) => {
                const isEmpty = r.leadDays === null && r.repeatDays === null && r.dayOfMonth === null && !r.enabled;
                return (
                  <TableRow key={r.kind} data-testid="reminder-rule-row" data-kind={r.kind}>
                    <TableCell>
                      {translate(`energy.calendarKind.${r.kind}`, { _: r.kind })}
                      {isEmpty && (
                        <Typography variant="caption" color="text.secondary" display="block" data-testid="reminder-rule-not-set">
                          （{translate('energy.reminders.notSet')}）
                        </Typography>
                      )}
                    </TableCell>
                    <TableCell>
                      <TextField
                        size="small" type="number" value={numField(r.leadDays)}
                        onChange={(e) => updateRule(r.kind, { leadDays: parseNum(e.target.value) })}
                        placeholder={translate('energy.reminders.notSet')}
                        inputProps={{ 'data-testid': `reminder-rule-lead-${r.kind}` }}
                        sx={{ width: 90 }}
                      />
                    </TableCell>
                    <TableCell>
                      <TextField
                        size="small" type="number" value={numField(r.repeatDays)}
                        onChange={(e) => updateRule(r.kind, { repeatDays: parseNum(e.target.value) })}
                        placeholder={translate('energy.reminders.notSet')}
                        inputProps={{ 'data-testid': `reminder-rule-repeat-${r.kind}` }}
                        sx={{ width: 90 }}
                      />
                    </TableCell>
                    <TableCell align="center">
                      <Checkbox
                        checked={r.businessDays} onChange={(e) => updateRule(r.kind, { businessDays: e.target.checked })}
                        data-testid={`reminder-rule-business-${r.kind}`}
                      />
                    </TableCell>
                    <TableCell>
                      <TextField
                        size="small" type="number" value={numField(r.dayOfMonth)}
                        onChange={(e) => updateRule(r.kind, { dayOfMonth: parseNum(e.target.value) })}
                        placeholder={translate('energy.reminders.notSet')}
                        inputProps={{ 'data-testid': `reminder-rule-dayofmonth-${r.kind}` }}
                        sx={{ width: 90 }}
                      />
                    </TableCell>
                    <TableCell align="center">
                      <Checkbox
                        checked={r.enabled} onChange={(e) => updateRule(r.kind, { enabled: e.target.checked })}
                        data-testid={`reminder-rule-enabled-${r.kind}`}
                      />
                    </TableCell>
                    <TableCell>
                      <Button
                        size="small" variant="outlined" onClick={() => saveRule(r)} disabled={savingKind === r.kind}
                        data-testid={`reminder-rule-save-${r.kind}`}
                      >
                        {translate('energy.reminders.save')}
                      </Button>
                    </TableCell>
                  </TableRow>
                );
              }) : <EmptyRow colSpan={7} loading={loading} />}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <Card>
        <CardContent>
          <Typography variant="subtitle1" mb={1}>{translate('energy.reminders.holidaysTitle')}</Typography>
          <Table size="small" data-testid="holiday-table">
            <TableHead>
              <TableRow>
                <TableCell>{translate('energy.reminders.day')}</TableCell>
                <TableCell>{translate('energy.reminders.name')}</TableCell>
                <TableCell>{translate('energy.reminders.isWorkday')}</TableCell>
                <TableCell />
              </TableRow>
            </TableHead>
            <TableBody>
              {holidays.map((h) => (
                <TableRow key={h.id} data-testid="holiday-row">
                  <TableCell sx={{ fontVariantNumeric: 'tabular-nums' }}>{h.day}</TableCell>
                  <TableCell>{h.name || '—'}</TableCell>
                  <TableCell>{h.isWorkday ? translate('ra.boolean.true') : translate('ra.boolean.false')}</TableCell>
                  <TableCell>
                    <IconButton size="small" onClick={() => deleteHoliday(h.id)} data-testid="holiday-delete">
                      <DeleteIcon fontSize="small" />
                    </IconButton>
                  </TableCell>
                </TableRow>
              ))}
              <TableRow data-testid="holiday-new-row">
                <TableCell>
                  <TextField
                    size="small" type="date" value={newHoliday.day}
                    onChange={(e) => setNewHoliday({ ...newHoliday, day: e.target.value })}
                    InputLabelProps={{ shrink: true }} inputProps={{ 'data-testid': 'holiday-new-day' }}
                  />
                </TableCell>
                <TableCell>
                  <TextField
                    size="small" value={newHoliday.name}
                    onChange={(e) => setNewHoliday({ ...newHoliday, name: e.target.value })}
                    inputProps={{ 'data-testid': 'holiday-new-name' }}
                  />
                </TableCell>
                <TableCell align="center">
                  <Checkbox
                    checked={newHoliday.isWorkday}
                    onChange={(e) => setNewHoliday({ ...newHoliday, isWorkday: e.target.checked })}
                    data-testid="holiday-new-isworkday"
                  />
                </TableCell>
                <TableCell>
                  <Button size="small" startIcon={<AddIcon />} onClick={addHoliday} disabled={addingHoliday || !newHoliday.day} data-testid="holiday-add">
                    {translate('energy.reminders.add')}
                  </Button>
                </TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </EnergyPageFrame>
  );
};

export default ReminderSettingsPage;
