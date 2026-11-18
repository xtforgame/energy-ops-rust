import * as React from 'react';
import {
  Box, Card, CardContent, Typography, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, Tooltip, IconButton, ToggleButton, ToggleButtonGroup, Button,
} from '@mui/material';
import RefreshIcon from '@mui/icons-material/Refresh';
import { energyHttp, energyBase } from '@/api/energy';
import { useNotify, useTranslate } from 'react-admin';
import { useNavigate } from 'react-router-dom';
import { useSelection } from '@/platform/selection/store';
import { apiErrorCode } from '../../../dataProvider/apiError';
import { EnergyPageFrame, EmptyRow } from './shared';
import { calendarRoute } from './routes';

interface NotificationRow {
  id: number; eventId: number; kind: string; message: string; notifyOn: string;
  readAt: string | null; eventDate: string | null; eventKind: string | null;
}

/** 卡 E S4：系統內通知（組織內共用，不指定收件人、不寄信）。點一筆跳到行事曆那一天。 */
const NotificationsPage: React.FC = () => {
  const notify = useNotify();
  const translate = useTranslate();
  const navigate = useNavigate();
  const orgId = useSelection()?.organizationId;
  const base = energyBase(orgId);

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [rows, setRows] = React.useState<NotificationRow[]>([]);
  const [filter, setFilter] = React.useState<'unread' | 'all'>('unread');

  const load = React.useCallback(async () => {
    if (!orgId) { setRows([]); setDenied(false); return; }
    setLoading(true); setDenied(false);
    try {
      const { data } = await energyHttp.get(`${base}/notifications`, { params: filter === 'unread' ? { unread: 1 } : {} });
      setRows(Array.isArray(data) ? data : []);
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
    } finally {
      setLoading(false);
    }
  }, [orgId, base, filter, notify]);

  React.useEffect(() => { load(); }, [load]);

  const markRead = async (id: number) => {
    try {
      await energyHttp.post(`${base}/notifications/${id}/read`);
      load();
    } catch (e: any) {
      notify(apiErrorCode(e) || e?.message || 'save failed', { type: 'error' });
    }
  };

  const openEvent = (row: NotificationRow) => {
    if (!row.readAt) markRead(row.id);
    navigate(calendarRoute(row.eventDate));
  };

  return (
    <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2}>
        <Typography variant="h6">{translate('energy.notifications.title')}</Typography>
        <Stack direction="row" gap={1} alignItems="center">
          <ToggleButtonGroup size="small" exclusive value={filter} onChange={(_, v) => v && setFilter(v)}>
            <ToggleButton value="unread" data-testid="notifications-filter-unread">
              {translate('energy.notifications.unreadOnly')}
            </ToggleButton>
            <ToggleButton value="all" data-testid="notifications-filter-all">
              {translate('energy.notifications.all')}
            </ToggleButton>
          </ToggleButtonGroup>
          <Tooltip title={translate('ra.action.refresh')}>
            <IconButton onClick={load} disabled={loading}><RefreshIcon /></IconButton>
          </Tooltip>
        </Stack>
      </Stack>

      <Card>
        <CardContent>
          <Table size="small" data-testid="notification-table">
            <TableHead>
              <TableRow>
                <TableCell>{translate('energy.events.date')}</TableCell>
                <TableCell>{translate('energy.events.title')}</TableCell>
                <TableCell>{translate('energy.notifications.title')}</TableCell>
                <TableCell />
              </TableRow>
            </TableHead>
            <TableBody>
              {rows.length ? rows.map((r) => (
                <TableRow
                  key={r.id} hover sx={{ cursor: 'pointer', fontWeight: r.readAt ? 400 : 700 }}
                  onClick={() => openEvent(r)} data-testid="notification-row" data-read={r.readAt ? '1' : '0'}
                >
                  <TableCell sx={{ fontVariantNumeric: 'tabular-nums' }}>{r.eventDate || '—'}</TableCell>
                  <TableCell sx={{ fontWeight: r.readAt ? 400 : 700 }} data-testid="notification-message">{r.message}</TableCell>
                  <TableCell>{translate(`energy.notifications.kind.${r.kind}`, { _: r.kind })}</TableCell>
                  <TableCell>
                    {!r.readAt && (
                      <Button
                        size="small"
                        onClick={(e) => { e.stopPropagation(); markRead(r.id); }}
                        data-testid="notification-mark-read"
                      >
                        {translate('energy.notifications.markRead')}
                      </Button>
                    )}
                  </TableCell>
                </TableRow>
              )) : <EmptyRow colSpan={4} loading={loading} />}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </EnergyPageFrame>
  );
};

export default NotificationsPage;
