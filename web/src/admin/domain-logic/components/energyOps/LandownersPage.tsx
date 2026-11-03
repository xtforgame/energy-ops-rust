import * as React from 'react';
import {
  Card, CardContent, Typography, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, Tooltip, IconButton, TextField, Chip,
} from '@mui/material';
import RefreshIcon from '@mui/icons-material/Refresh';
import { energyHttp, energyBase } from '@/api/energyLegacy';
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
  contactPhone: string | null; status: string;
  stationCount: number; abnormalStationCount: number; contractCount: number;
}

/**
 * P5 地主 / 發電站（`FRONTEND.md §2-P5`、`REQUIREMENTS.md §五-3`）。
 *
 * 清單而已 —— 詳情在**獨立路由** `#/energyLandowners/:id`（`LandownerDetailPage`）。
 * **不再用 drawer**（2026-08-27 拍板）：drawer 適合「看一眼就關掉」的次要資訊，
 * 而地主頁是**入口頁**，使用者會從那裡出發去很多地方 ——
 * 它需要完整的一頁**與可分享的網址**。
 *
 * 本頁唯讀：主檔在 v1 由 D 層的 Excel 遷移灌入。
 */
const LandownersPage: React.FC = () => {
  const notify = useNotify();
  const translate = useTranslate();
  const navigate = useNavigate();
  const selected = useSelection();
  const orgId = selected?.organizationId;

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [rows, setRows] = React.useState<LandownerRow[]>([]);
  const [keyword, setKeyword] = React.useState('');

  const load = React.useCallback(async () => {
    if (!orgId) { setRows([]); setDenied(false); return; }
    setLoading(true); setDenied(false);
    try {
      const { data } = await energyHttp.get(`${energyBase(orgId)}/landowners`, {
        params: keyword.trim() ? { q: keyword.trim() } : {},
      });
      setRows(Array.isArray(data) ? data : []);
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
    } finally {
      setLoading(false);
    }
  }, [orgId, keyword, notify]);

  React.useEffect(() => { load(); }, [load]);

  return (
    <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2}>
        <Typography variant="h6">{translate('energy.landowners.title')}</Typography>
        <Stack direction="row" gap={1} alignItems="center">
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
        </Stack>
      </Stack>

      <Card>
        <CardContent>
          <Table size="small">
            <TableHead>
              <TableRow>
                <TableCell>{translate('energy.landowners.name')}</TableCell>
                <TableCell>{translate('energy.landowners.taxId')}</TableCell>
                <TableCell>{translate('energy.landowners.contact')}</TableCell>
                <TableCell align="right">{translate('energy.landowners.stationCount')}</TableCell>
                <TableCell align="right">{translate('energy.landowners.contractCount')}</TableCell>
                <TableCell>{translate('energy.landowners.status')}</TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {rows.length ? rows.map((r) => (
                <TableRow
                  key={r.id}
                  hover
                  sx={{ cursor: 'pointer' }}
                  data-testid="landowner-row"
                  // 改導到**獨立路由**（`FRONTEND.md §2 P5` 拍板）——
                  // 不再用 drawer：地主頁是入口頁，需要完整的一頁**與可分享的網址**。
                  onClick={() => navigate(landownerRoute(r.id))}
                >
                  <TableCell><Typography variant="body2" fontWeight={600}>{r.name}</Typography></TableCell>
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
                </TableRow>
              )) : <EmptyRow colSpan={6} loading={loading} />}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

    </EnergyPageFrame>
  );
};

export default LandownersPage;
