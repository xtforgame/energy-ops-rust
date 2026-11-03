import * as React from 'react';
import { Box, Typography, Stack, CircularProgress, Alert } from '@mui/material';
import { useTranslate } from 'react-admin';
import type { TaiwanMapHandle } from '../../../taiwan-map/scene/createScene';
import { jvTokens } from './shared';

export interface MapStation {
  id: number;
  name: string;
  status: string;
  /** 後端回的是 DECIMAL **字串**（保精度）—— 這裡才轉 number。 */
  latitude: string | number | null;
  longitude: string | number | null;
}

/**
 * P7 的台灣 3D 圖層 —— **接真實案場**（C5）。
 *
 * 場景是既有資產（`src/tenant-console/src/taiwan-map/`），這裡只是：
 * ① 動態 `import()` 進來（沒開這頁就不該把 three 拉進主 bundle）
 * ② 把 API 回來的案場轉成 `{lat,lng,status}` 傳進去
 *
 * **驗收只能靠真瀏覽器截圖**：WebGL 的內容讀不回 DOM，
 * `rt-tenant-console` 完全驗不到這一層。
 * → 場景會把「實際畫了幾個點」回拋，這裡掛成 `data-*` 讓 crawler 斷言。
 * crawler **不要加 `--use-angle=swiftshader`**：macOS 上會讓 WebGL
 * 建不出 context，畫面全黑但只印 console 錯，很難察覺。
 */
const StationMap: React.FC<{
  stations: MapStation[];
  height?: number;
  /** 點到案場點 → 案場詳情（P11）。 */
  onSelect?: (stationId: number) => void;
}> = ({ stations, height = 520, onSelect }) => {
  const translate = useTranslate();
  const hostRef = React.useRef<HTMLDivElement | null>(null);
  const handleRef = React.useRef<TaiwanMapHandle | null>(null);
  const [loading, setLoading] = React.useState(true);
  const [error, setError] = React.useState<string | null>(null);
  const [plotted, setPlotted] = React.useState<{ count: number; skipped: number } | null>(null);
  // 見下方 `onStationClick` 的註解：callback 不可以進場景的重建依賴。
  const onSelectRef = React.useRef(onSelect);
  onSelectRef.current = onSelect;

  // 只把「有座標的」丟給場景；轉型別在這裡做一次。
  const points = React.useMemo(() => stations
    .map((s) => ({
      id: s.id,
      name: s.name,
      status: s.status,
      latitude: Number(s.latitude),
      longitude: Number(s.longitude),
    }))
    .filter((s) => Number.isFinite(s.latitude) && Number.isFinite(s.longitude)),
  [stations]);

  // 用「內容指紋」當依賴，不是 `points` 本身：
  // 每次 render 都是新陣列 → 場景會被反覆重建（進場動畫一直重播、context 也會堆）。
  const fingerprint = React.useMemo(
    () => points.map((p) => `${p.id}:${p.status}:${p.latitude}:${p.longitude}`).join('|'),
    [points],
  );

  React.useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    let disposed = false;
    let handle: TaiwanMapHandle | null = null;
    setLoading(true); setError(null); setPlotted(null);

    import('../../../taiwan-map/scene/createScene')
      .then(({ createTaiwanMap }) => {
        if (disposed) return;
        handle = createTaiwanMap(host, {
          /**
           * **不指定 theme** —— 用專案預設的 `lab`（MONO，灰）。
           *
           * 這裡我一度改成 `jv`（紫）來「讓標記看得見」，那是**治標而且是錯的**：
           * MONO 才是正式版本，`jv` 只存在於 demo 展示。
           * 真正的病因是**案場層去吃了主題的 `pal.alpha.cone`**（MONO 刻意設成 0），
           * 那個已經在 `stations.ts` 修掉了（`STATION_ALPHA` 自己決定）。
           * 病因修好之後，灰底反而更好 —— 背景去飽和，狀態色（綠／金／燒橘）更跳。
           */
          // 面板要看得清點位 → 鏡頭比滿版背景近（預設 11.5 是給滿版用的）。
          restingDistance: 8,
          stations: points,
          // 案場點可點 → P11（`FRONTEND.md §2 P7` 那張表的第三列）。
          // 走 ref 取最新的 callback，不要把 `onSelect` 放進依賴：
          // 那會讓場景每次 render 都重建（進場動畫一直重播、context 也會堆）。
          onStationClick: (id: number) => { onSelectRef.current?.(id); },
          onStations: (info) => { if (!disposed) setPlotted(info); },
          onFirstFrame: () => { if (!disposed) setLoading(false); },
        });
        handleRef.current = handle;
        // 量測掛點：crawler 要知道「案場點在畫面上的哪裡」才點得到
        // （WebGL 讀不回 DOM）。**只讀**，不拿它驅動點擊。
        (window as any).__taiwanMapHandle = handle;
      })
      .catch((e) => {
        if (disposed) return;
        // 別把 WebGL 建不起來吞掉 —— 那正是「畫面全黑但沒人知道為什麼」的來源。
        setError(e?.message || 'map failed to load');
        setLoading(false);
      });

    return () => {
      disposed = true;
      handle?.dispose();
      handleRef.current = null;
      if ((window as any).__taiwanMapHandle === handle) {
        (window as any).__taiwanMapHandle = null;
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fingerprint]);

  const legendItem = (labelKey: string, c: string) => (
    <Stack direction="row" gap={0.75} alignItems="center">
      <Box sx={{ width: 10, height: 10, borderRadius: '50%', background: c }} />
      <Typography variant="caption" color="text.secondary">{translate(labelKey)}</Typography>
    </Stack>
  );

  return (
    <Box data-testid="station-map">
      {error && <Alert severity="warning" sx={{ mb: 1 }}>{error}</Alert>}
      <Box
        ref={hostRef}
        sx={{
          position: 'relative',
          height,
          border: `1px solid ${jvTokens.hairline}`,
          borderRadius: 1,
          overflow: 'hidden',
        }}
      >
        {loading && !error && (
          <Stack
            alignItems="center" justifyContent="center"
            sx={{ position: 'absolute', inset: 0, zIndex: 1 }}
          >
            <CircularProgress size={22} />
          </Stack>
        )}
      </Box>
      <Stack direction="row" gap={2} mt={1} flexWrap="wrap" alignItems="center">
        {legendItem('energy.stationStatus.operating', jvTokens.ok)}
        {legendItem('energy.stationStatus.construction', jvTokens.gold)}
        {/* 異常是燒橘不是紅。 */}
        {legendItem('energy.stationStatus.abnormal', jvTokens.warn)}
        {plotted && (
          <Typography variant="caption" color="text.secondary">
            {translate('energy.dashboard.plotted', { count: plotted.count })}
            {/* 沒座標的案場要講出來，不要靜靜少幾個點。 */}
            {plotted.skipped > 0
              ? `　${translate('energy.dashboard.noCoords', { count: plotted.skipped })}`
              : ''}
          </Typography>
        )}
      </Stack>
      {/* WebGL 讀不回 DOM —— crawler 只能靠這個掛點斷言「點真的畫出來了」。 */}
      <Box
        data-testid="station-map-info"
        data-plotted={plotted ? String(plotted.count) : ''}
        data-skipped={plotted ? String(plotted.skipped) : ''}
        sx={{ display: 'none' }}
      />
    </Box>
  );
};

export default StationMap;
