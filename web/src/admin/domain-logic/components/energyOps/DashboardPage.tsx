import * as React from 'react';
import {
  Box, Card, CardContent, Typography, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, Tooltip, IconButton, TextField, Button, Chip, Alert,
  Dialog, DialogTitle, DialogContent, DialogActions,
} from '@mui/material';
import RefreshIcon from '@mui/icons-material/Refresh';
import MyLocationIcon from '@mui/icons-material/MyLocation';
import { energyHttp, energyBase } from '@/api/energy';
import { useNotify, useTranslate } from 'react-admin';
import { useNavigate } from 'react-router-dom';
import { useSelection } from '@/platform/selection/store';
import { apiErrorCode } from '../../../dataProvider/apiError';
import {
  EnergyPageFrame, EmptyRow, StatusChip, jvTokens,
} from './shared';
import { billListRoute, executionListRoute, myWorkRoute, stationRoute } from './routes';
import StationMap from './StationMap';

interface Dashboard {
  month: string;
  runningCount: number;
  stuckDays: number;
  stuckCount: number;
  /** 已放行未付 —— 目的地是工作台的會計分區。 */
  releasedUnpaidCount: number;
  unmatchedCount: number;
  /** S2：有案場、但這段計費期間沒有生效中的合約 —— 跟「待指認案場」是兩種各自的例外。 */
  noContractCount: number;
  stationTotal: number;
  stationByStatus: Record<string, number>;
}
interface StationRow {
  id: number; name: string; customerNumber: string; status: string;
  address: string | null; latitude: string | null; longitude: string | null;
  /** `manual` 的列不可被自動 geocode 覆蓋（`DESIGN.md §1.2.1`）。 */
  geocodePrecision: string | null; geocodeSource: string | null; geocodedAt: string | null;
  landownerName: string | null;
}
interface GeocodeSuggestion {
  stationId: number;
  address: string | null;
  result: {
    latitude: number; longitude: number; precision: string; source: string; matchedAddress?: string;
  } | null;
  usable: boolean;
  reason?: 'no_address' | 'not_found' | 'too_coarse';
}

/** 這個月（`YYYY-MM`）。用本地時間，不是 UTC —— 使用者看的是自己的月份。 */
const thisMonth = () => {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`;
};

/**
 * P7 Dashboard（`FRONTEND.md §2-P7`、`REQUIREMENTS.md §五-1`）：
 * **幾件卡關**、案場狀態（G4 起不再有「本月應付」——通過不再逐張算應付，租金改走週期試算）。
 *
 * 「卡關」的判定在**後端**（`buildDashboard`）：還在跑、而且最舊的待辦已經等超過
 * `stuckDays` 天。前端不自己重算一套 —— 兩邊算法遲早會不一樣。
 * 門檻是**參數**（`DESIGN.md §0`）：後端回 `stuckDays`，畫面照著寫「超過 N 天」，
 * 不寫死一個數字在文案裡。
 *
 * **台灣 3D 圖層已接真實案場**（C5）：`scene/stations.ts` ＋ `geo.ts` 的
 * `projectLonLat`（與縣市邊界同一個投影）。沒有座標的案場**不會**被標在 (0,0)，
 * 而是算進「N 座沒有座標」的提示裡。
 *
 * 「由地址推估」按鈕（C5 第 4 步）：**不在建檔時自動呼叫**，
 * 也**不直接寫入** —— 先回建議值讓人看過再採用（`DESIGN.md §1.2.1`）。
 */
const DashboardPage: React.FC = () => {
  const notify = useNotify();
  const translate = useTranslate();
  const navigate = useNavigate();
  const orgId = useSelection()?.organizationId;

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [data, setData] = React.useState<Dashboard | null>(null);
  const [stations, setStations] = React.useState<StationRow[]>([]);
  const [month, setMonth] = React.useState(thisMonth());
  const [geoTarget, setGeoTarget] = React.useState<StationRow | null>(null);
  const [geo, setGeo] = React.useState<GeocodeSuggestion | null>(null);
  const [geoBusy, setGeoBusy] = React.useState(false);

  const base = energyBase(orgId);

  const load = React.useCallback(async () => {
    if (!orgId) { setData(null); setDenied(false); return; }
    setLoading(true); setDenied(false);
    try {
      const { data: d } = await energyHttp.get(`${base}/dashboard`, { params: { month } });
      setData(d);
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
    } finally {
      setLoading(false);
    }
    try {
      const { data: s } = await energyHttp.get(`${base}/station-list`);
      setStations(Array.isArray(s) ? s : []);
    } catch { setStations([]); }
  }, [orgId, base, month, notify]);

  React.useEffect(() => { load(); }, [load]);

  // ---- 由地址推估座標（C5 第 4 步）----
  const suggest = async (station: StationRow) => {
    setGeoTarget(station); setGeo(null); setGeoBusy(true);
    try {
      const { data } = await energyHttp.get(`${base}/stations/${station.id}/geocode`);
      setGeo(data);
    } catch (e: any) {
      notify(apiErrorCode(e) || 'geocode failed', { type: 'error' });
      setGeoTarget(null);
    } finally { setGeoBusy(false); }
  };

  const applyGeocode = async () => {
    if (!geoTarget) return;
    try {
      await energyHttp.post(`${base}/stations/${geoTarget.id}/geocode`);
      notify('energy.dashboard.geocodeApplied', { type: 'success' });
      setGeoTarget(null); setGeo(null);
      load();
    } catch (e: any) {
      notify(apiErrorCode(e) || 'apply failed', { type: 'error' });
    }
  };

  /** KPI 卡。`hot` ＝ 需要有人處理的數字，用**燒橘**（不是紅）。 */
  const kpi = (labelKey: string, value: React.ReactNode, hint?: string, hot?: boolean, onClick?: () => void) => (
    <Card
      sx={{ flex: '1 1 200px', minWidth: 200, cursor: onClick ? 'pointer' : undefined }}
      onClick={onClick}
      data-testid="kpi-card"
      data-kpi={labelKey}
    >
      <CardContent>
        <Typography variant="caption" color="text.secondary">{translate(labelKey)}</Typography>
        <Typography
          variant="h5"
          sx={{ mt: 0.5, fontVariantNumeric: 'tabular-nums', color: hot ? jvTokens.warn : jvTokens.primaryDark }}
        >
          {value}
        </Typography>
        {hint && <Typography variant="caption" color="text.secondary">{hint}</Typography>}
      </CardContent>
    </Card>
  );

  return (
    <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2}>
        <Typography variant="h6">{translate('energy.dashboard.title')}</Typography>
        <Stack direction="row" gap={1} alignItems="center">
          <TextField
            size="small" type="month" label={translate('energy.dashboard.month')}
            InputLabelProps={{ shrink: true }}
            value={month} onChange={(e) => setMonth(e.target.value)}
            inputProps={{ 'data-testid': 'dashboard-month' }}
          />
          <Tooltip title={translate('ra.action.refresh')}>
            <IconButton onClick={load} disabled={loading}><RefreshIcon /></IconButton>
          </Tooltip>
        </Stack>
      </Stack>

      <Stack direction="row" gap={2} flexWrap="wrap" mb={2}>
        {/* **每一個數字都要有目的地**（`FRONTEND.md §2 P7`）——
            不能點的數字等於一張海報：使用者看到「3 件卡關」卻得自己回選單再篩一次。 */}
        {kpi(
          'energy.dashboard.running',
          data?.runningCount ?? 0,
          undefined,
          false,
          // 「進行中幾件」的去處是 P9：那裡看得到**它們卡在哪一關**。
          () => navigate('/energyWorkflowConsole'),
        )}
        {kpi(
          'energy.dashboard.stuck',
          data?.stuckCount ?? 0,
          // 門檻來自後端，不在文案裡寫死天數。
          translate('energy.dashboard.stuckHint', { days: data?.stuckDays ?? 0 }),
          (data?.stuckCount ?? 0) > 0,
          // 去 **P12 流程單（執行）**，而且**帶著同一個門檻**過去 ——
          // 導到工作台是錯的：工作台只顯示「指派給我的」，
          // 卡關的那幾件多半**不在我手上**（那正是它們卡住的原因）。
          () => navigate(executionListRoute({ status: 'running', waitingDaysMin: data?.stuckDays ?? 3 })),
        )}
        {kpi(
          'energy.dashboard.releasedUnpaid',
          data?.releasedUnpaidCount ?? 0,
          translate('energy.dashboard.releasedUnpaidHint'),
          (data?.releasedUnpaidCount ?? 0) > 0,
          // 目的地是**工作台的會計分區**（`FRONTEND.md §2 P7`）。
          () => navigate(myWorkRoute()),
        )}
        {kpi(
          'energy.dashboard.unmatched',
          data?.unmatchedCount ?? 0,
          translate('energy.dashboard.unmatchedHint'),
          (data?.unmatchedCount ?? 0) > 0,
          () => navigate(billListRoute({ tab: 'pendingMatch' })),
        )}
        {kpi(
          'energy.dashboard.noContract',
          data?.noContractCount ?? 0,
          translate('energy.dashboard.noContractHint'),
          (data?.noContractCount ?? 0) > 0,
          () => navigate(billListRoute({ tab: 'pendingContract' })),
        )}
      </Stack>

      <Card>
        <CardContent>
          <Stack direction="row" alignItems="baseline" gap={2} mb={1}>
            <Typography variant="subtitle1">
              {translate('energy.dashboard.stations')}（{data?.stationTotal ?? 0}）
            </Typography>
            {/* 狀態分布：一眼看得出有沒有異常。 */}
            <Stack direction="row" gap={1}>
              {Object.entries(data?.stationByStatus ?? {}).map(([k, v]) => (
                <Stack key={k} direction="row" gap={0.5} alignItems="center">
                  <StatusChip i18nKey="energy.stationStatus" value={k} />
                  <Typography variant="body2">{v}</Typography>
                </Stack>
              ))}
            </Stack>
          </Stack>

          {/* C5：真實案場標在地圖上。投影走 `geo.ts` 的 `projectLonLat`
              —— 與縣市邊界**同一個**麥卡托投影。 */}
          <Box mb={2}>
            {/* 案場點現在**點得動**了（C7d 在場景加了 raycast picking）。
                但下面那張案場表**保留**：它是 accessible 的路徑
                （鍵盤／讀屏／WebGL 建不起來時都還走得通），
                不要因為地圖點得動了就把它拿掉。 */}
            <StationMap
              stations={stations}
              onSelect={(id) => navigate(stationRoute(id))}
            />
          </Box>

          <Table size="small">
            <TableHead>
              <TableRow>
                <TableCell>{translate('energy.stations.name')}</TableCell>
                <TableCell>{translate('energy.bills.customerNumber')}</TableCell>
                <TableCell>{translate('energy.landowners.name')}</TableCell>
                <TableCell>{translate('energy.dashboard.coords')}</TableCell>
                <TableCell>{translate('energy.dashboard.coordSource')}</TableCell>
                <TableCell>{translate('energy.stations.status')}</TableCell>
                <TableCell align="right" />
              </TableRow>
            </TableHead>
            <TableBody>
              {stations.length ? stations.map((s) => (
                <TableRow
                  key={s.id}
                  hover
                  sx={{ cursor: 'pointer' }}
                  data-testid="station-row"
                  data-station={s.id}
                  // 跨頁：Dashboard 的案場 → **P11 案場詳情**（`FRONTEND.md §2 P7`）。
                  onClick={() => navigate(stationRoute(s.id))}
                >
                  <TableCell>{s.name}</TableCell>
                  <TableCell sx={{ fontVariantNumeric: 'tabular-nums' }}>{s.customerNumber}</TableCell>
                  <TableCell>{s.landownerName || '—'}</TableCell>
                  <TableCell>
                    <Typography variant="caption" color="text.secondary" sx={{ fontVariantNumeric: 'tabular-nums' }}>
                      {s.latitude && s.longitude
                        ? `${Number(s.latitude).toFixed(4)}, ${Number(s.longitude).toFixed(4)}`
                        : '—'}
                    </Typography>
                  </TableCell>
                  <TableCell>
                    {s.geocodeSource ? (
                      <Chip
                        size="small"
                        variant="outlined"
                        data-testid="coord-source"
                        label={translate(`energy.dashboard.source.${s.geocodeSource}`, { _: s.geocodeSource })}
                        // `manual` 用主色標出來：它是第一等公民，不是退路。
                        sx={s.geocodeSource === 'manual'
                          ? { color: jvTokens.primary, borderColor: jvTokens.primary }
                          : { color: jvTokens.muted, borderColor: jvTokens.border }}
                      />
                    ) : (
                      <Typography variant="caption" color="text.secondary">—</Typography>
                    )}
                  </TableCell>
                  <TableCell><StatusChip i18nKey="energy.stationStatus" value={s.status} /></TableCell>
                  <TableCell align="right">
                    {/* 只有「沒座標」的案場才給推估按鈕 ——
                        有 manual 座標的列連按鈕都不該出現（後端也會 409 擋）。 */}
                    {!s.latitude && s.address ? (
                      <Button
                        size="small"
                        startIcon={<MyLocationIcon />}
                        data-testid="geocode-btn"
                        // 整列現在可點（→ P11）——不 stopPropagation 的話
                        // 按「由地址推估」會**同時**把使用者導走，對話框一開就被關掉。
                        onClick={(e) => { e.stopPropagation(); suggest(s); }}
                      >
                        {translate('energy.dashboard.geocode')}
                      </Button>
                    ) : null}
                  </TableCell>
                </TableRow>
              )) : <EmptyRow colSpan={7} loading={loading} />}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      {/* 推估結果 —— 先看再採用，不自動寫入。 */}
      <Dialog open={!!geoTarget} onClose={() => setGeoTarget(null)} fullWidth maxWidth="xs">
        <DialogTitle>{translate('energy.dashboard.geocodeTitle')}</DialogTitle>
        <DialogContent>
          <Stack gap={1.5} mt={1} data-testid="geocode-dialog">
            <Typography variant="body2" color="text.secondary">
              {geoTarget?.name}　{geo?.address || geoTarget?.address || ''}
            </Typography>
            {geoBusy && <Typography variant="body2">{translate('energy.dashboard.geocodeBusy')}</Typography>}
            {!geoBusy && geo?.usable && geo.result && (
              <>
                <Typography variant="body2" sx={{ fontVariantNumeric: 'tabular-nums' }} data-testid="geocode-coords">
                  {geo.result.latitude.toFixed(6)}, {geo.result.longitude.toFixed(6)}
                </Typography>
                <Typography variant="caption" color="text.secondary">
                  {translate(`energy.dashboard.precision.${geo.result.precision}`, { _: geo.result.precision })}
                  {geo.result.matchedAddress ? `　${geo.result.matchedAddress}` : ''}
                </Typography>
              </>
            )}
            {!geoBusy && !geo?.usable && (
              // 查不到／精度不足時**不填座標**，只給一句人看得懂的話。
              <Alert
                severity="warning"
                data-testid="geocode-blocked"
                sx={{
                  color: jvTokens.warn, borderColor: jvTokens.warn,
                  border: '1px solid', backgroundColor: 'transparent',
                }}
              >
                {translate(`energy.dashboard.geocodeReason.${geo?.reason ?? 'not_found'}`)}
              </Alert>
            )}
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setGeoTarget(null)}>{translate('ra.action.cancel')}</Button>
          <Button
            variant="contained"
            disabled={!geo?.usable}
            data-testid="geocode-apply"
            onClick={applyGeocode}
          >
            {translate('energy.dashboard.geocodeApply')}
          </Button>
        </DialogActions>
      </Dialog>

      <Box mt={2} />
    </EnergyPageFrame>
  );
};

export default DashboardPage;
