import { useEffect, useMemo, useRef, useState } from 'react';
import { Box, Typography } from '@mui/material';
import { Title } from 'react-admin';
import { jvTokens, jvMonoLabel } from '../themes/jvTheme';
import { mapThemes, DEFAULT_MAP_THEME, type MapThemeName } from './scene/palette';
import type { HoverInfo, RegionRef, TaiwanMapHandle } from './scene/createScene';

/**
 * 台灣 3D 地圖（自訂頁）。
 *
 * 版型參考 `src/web/src/pages/futuristic-white-atmos`：**3D 是滿版背景**，
 * 資訊層（標題、讀數、提示）是浮在上面的銳利文字，沒有卡片外框。
 * 對應專案的既有結論「大氣只在背景與容器層、資訊層一律銳利」。
 *
 * ⚠️ 高度不寫死：`#main-content`（`.RaLayout-content`）本身就是
 *    `display:flex; flex-direction:column`、高度已由 appFrame 撐定（實測 915px），
 *    所以這裡只要 `flex:1 + minHeight:0` 就會填滿剩餘空間。
 *    左右的 `mx:-1` 是抵銷 `.RaLayout-content` 的 `padding: 0 8px`，讓畫面真的出血。
 *
 * three.js 場景在 `./scene/`，用動態 import 切成獨立 chunk —— 這頁沒開就不該把
 * three 拉進主 bundle。⚠️ 所以這裡只准 import `scene/palette`（純資料、無 three），
 * 不要 import `scene/glColor` 或任何 `scene/*` 的實作檔。
 *
 * 兩套配色（見 scene/palette.ts）：
 *   'jv'  白底 + 紫（JV 1C）
 *   'lab' futuristic-white 的白色色階 —— 背景物件只是紋理，顏色只剩選取態的主色
 */

const MONO = { ...jvMonoLabel, fontVariantNumeric: 'tabular-nums' as const };

const THEME_LABELS: Record<MapThemeName, string> = { lab: 'MONO', jv: 'VIOLET' };

/** HUD 角落的定位容器：不吃滑鼠事件，滑鼠一律穿到 canvas */
const hud = (extra: Record<string, unknown>) => ({
  position: 'absolute' as const,
  pointerEvents: 'none' as const,
  zIndex: 2,
  ...extra,
});

const TaiwanMapPage = () => {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const [theme, setTheme] = useState<MapThemeName>(DEFAULT_MAP_THEME);
  const [hover, setHover] = useState<HoverInfo | null>(null);
  // painted = 第一幀畫出來了（收掉載入遮罩）；ready = 進場動畫跑完（淡入 HUD）。
  // ⚠️ 這兩件事一定要分開：進場的鏡頭俯衝有好幾秒，遮罩若蓋到 ready 才收，
  //    使用者看到的就是「讀很久」而不是一段動畫（實測 載入 721ms / 動畫 3077ms）。
  const [regions, setRegions] = useState<RegionRef[]>([]);
  const [focusId, setFocusId] = useState<string | null>(null);
  const handleRef = useRef<TaiwanMapHandle | null>(null);
  const [painted, setPainted] = useState(false);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const pal = useMemo(() => mapThemes[theme], [theme]);
  /** 讀數面板：滑鼠優先，沒有 hover 時退回目前特寫的縣市 */
  const readout: HoverInfo | null = useMemo(() => {
    if (hover) return hover;
    const r = regions.find((x) => x.id === focusId);
    return r ? { id: r.id, name: r.name, area: r.area } : null;
  }, [hover, regions, focusId]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return undefined;

    setPainted(false);
    setReady(false);
    setHover(null);
    setRegions([]);
    setFocusId(null);

    let handle: TaiwanMapHandle | null = null;
    let cancelled = false;

    import('./scene/createScene')
      .then(({ createTaiwanMap }) => {
        if (cancelled) return;
        handle = createTaiwanMap(host, {
          theme,
          onHover: setHover,
          onRegions: setRegions,
          onFocusChange: setFocusId,
          onFirstFrame: () => setPainted(true),
          onReady: () => setReady(true),
        });
        handleRef.current = handle;
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      });

    return () => {
      cancelled = true;
      handleRef.current = null;
      handle?.dispose();
    };
  }, [theme]);

  /** 點同一個＝取消特寫，回到背景模式 */
  const toggleFocus = (id: string | null) => {
    const next = id === focusId ? null : id;
    handleRef.current?.focus(next);
    setFocusId(next);
  };

  return (
    <>
      <Title title="台灣行政區 3D 圖層" />
      <Box
        data-focus={focusId ? 'on' : 'off'}
        sx={{
          flex: 1,
          minHeight: 0,
          mx: -1, // 抵銷 .RaLayout-content 的 padding: 0 8px
          position: 'relative',
          overflow: 'hidden',
          backgroundColor: pal.bg,
          // 底噪：跟 3D 地板的網格同一個語彙（lab = rgba(10,10,10,.045) 的 64px 格）
          backgroundImage: `linear-gradient(${pal.grid.fine} 1px, transparent 1px),
                            linear-gradient(90deg, ${pal.grid.fine} 1px, transparent 1px)`,
          backgroundSize: '64px 64px',
          transition: 'background-color .4s ease',

          /* ---- 縣市標籤（labels.ts 產生的 DOM） ---- */
          '& .tw-map-labels': {
            position: 'absolute',
            inset: 0,
            overflow: 'hidden',
            pointerEvents: 'none',
            zIndex: 1,
          },
          '& .tw-map-label': {
            position: 'absolute',
            transform: 'translate(-50%,-50%)',
            fontSize: `${pal.labelSize.base}px`,
            letterSpacing: '.08em',
            whiteSpace: 'nowrap',
            color: pal.sideBottom,
            opacity: 0,
            transition: 'opacity .35s ease, color .2s ease',
            willChange: 'transform',
          },
          // 標籤同樣走景深。文字底下就是背景色、沒有東西會透出來，
          // 所以這一層用 opacity 是安全的（3D 那邊才必須改用顏色退場）
          '& .tw-map-label.on': { opacity: pal.focusDepth.label },
          // 特寫時文字也要「清晰」—— 跟 3D 那邊的 morph 同一個意思，用 CSS transition 就夠平滑
          '&[data-focus="on"] .tw-map-label.on': { opacity: 1, color: jvTokens.caption },
          '& .tw-map-label.on.hot': {
            opacity: 1,
            color: pal.focus,
            fontSize: `${pal.labelSize.hot}px`,
            fontWeight: 600,
          },
          // 被特寫選中的那一個：比 hover 再大一階、更實
          '& .tw-map-label.sel': {
            opacity: 1,
            color: pal.focus,
            fontSize: `${pal.labelSize.sel}px`,
            fontWeight: 700,
            letterSpacing: '.06em',
          },
          '& .tw-map-label .dot': {
            display: 'inline-block',
            width: '4px',
            height: '4px',
            marginRight: '5px',
            backgroundColor: pal.marker,
            transform: 'rotate(45deg) translateY(-1px)',
          },
          '& .tw-map-label.hot .dot': { backgroundColor: pal.focus },
        }}
      >
        {/* data-* 是給驗證探針的斷言真值：canvas 掛在這一層底下，
            頁上還有切頁雲霧與 AI 側欄的 canvas，探針必須認得出是哪一張。 */}
        <Box
          ref={hostRef}
          data-testid="taiwan-map-host"
          data-theme={theme}
          data-ready={ready ? 'true' : 'false'}
          sx={{ position: 'absolute', inset: 0 }}
        />

        {/* 左上：標題 / 中繼資料（原作 html 的 #tl；atmos 版把品牌標也放這） */}
        <Box
          data-testid="taiwan-map-heading"
          sx={hud({
            top: 22,
            left: 26,
            opacity: painted ? 1 : 0,
            transition: 'opacity .8s ease .1s',
          })}
        >
          <Typography sx={{ ...MONO, fontSize: 10, color: jvTokens.faint }}>GEOSPATIAL / 01</Typography>
          <Typography
            sx={{
              fontSize: 19,
              fontWeight: 700,
              letterSpacing: '.06em',
              color: jvTokens.text,
              mt: '4px',
            }}
          >
            台灣行政區 3D 圖層
          </Typography>
          <Typography sx={{ ...MONO, fontSize: 10, color: jvTokens.faint, mt: '5px' }}>
            MERCATOR PROJECTION · TOPOJSON · 22 REGIONS
          </Typography>
        </Box>

        {/* 右上：配色切換（唯一吃滑鼠事件的 HUD） */}
        <Box
          sx={hud({
            top: 22,
            right: 26,
            pointerEvents: 'auto',
            display: 'flex',
            opacity: painted ? 1 : 0,
            transition: 'opacity .8s ease .1s',
          })}
        >
          {(Object.keys(THEME_LABELS) as MapThemeName[]).map((k) => (
            <Box
              key={k}
              component="button"
              type="button"
              onClick={() => setTheme(k)}
              aria-pressed={theme === k}
              sx={{
                ...MONO,
                fontSize: 10,
                px: 1.25,
                py: 0.5,
                cursor: 'pointer',
                border: `1px solid ${theme === k ? jvTokens.primary : jvTokens.border}`,
                marginLeft: '-1px',
                backgroundColor: theme === k ? jvTokens.primary : 'transparent',
                color: theme === k ? '#fff' : jvTokens.muted,
                '&:hover': { borderColor: jvTokens.primary },
              }}
            >
              {THEME_LABELS[k]}
            </Box>
          ))}
        </Box>

        {/* 左側：縣市特寫清單。資訊層 —— 不吃景深，永遠銳利 */}
        {ready && regions.length > 0 && (
          <Box
            data-testid="taiwan-map-regions"
            data-focus={focusId ?? ''}
            sx={hud({
              top: 168,
              left: 26,
              bottom: 148,
              width: 132,
              pointerEvents: 'auto',
              display: 'flex',
              flexDirection: 'column',
              opacity: ready ? 1 : 0,
              transition: 'opacity .8s ease .2s',
            })}
          >
            <Typography sx={{ ...MONO, fontSize: 9.5, color: jvTokens.faint, mb: 0.75 }}>
              REGIONS / {regions.length}
            </Typography>
            <Box sx={{ overflowY: 'auto', minHeight: 0, mr: -1, pr: 1 }}>
              <Box
                component="button"
                type="button"
                onClick={() => toggleFocus(null)}
                sx={{
                  ...MONO,
                  display: 'block',
                  width: '100%',
                  textAlign: 'left',
                  fontSize: 10,
                  px: 1,
                  py: 0.4,
                  mb: 0.5,
                  cursor: 'pointer',
                  border: 'none',
                  borderLeft: `2px solid ${focusId ? 'transparent' : jvTokens.primary}`,
                  backgroundColor: 'transparent',
                  color: focusId ? jvTokens.muted : jvTokens.text,
                  '&:hover': { backgroundColor: jvTokens.hoverBg },
                }}
              >
                OVERVIEW
              </Box>
              {regions.map((r) => {
                const on = r.id === focusId;
                return (
                  <Box
                    key={r.id}
                    component="button"
                    type="button"
                    onClick={() => toggleFocus(r.id)}
                    aria-pressed={on}
                    sx={{
                      display: 'block',
                      width: '100%',
                      textAlign: 'left',
                      fontSize: 12,
                      letterSpacing: '.04em',
                      px: 1,
                      py: 0.4,
                      cursor: 'pointer',
                      border: 'none',
                      borderLeft: `2px solid ${on ? jvTokens.primary : 'transparent'}`,
                      backgroundColor: on ? jvTokens.primary : 'transparent',
                      color: on ? '#fff' : jvTokens.caption,
                      '&:hover': { backgroundColor: on ? jvTokens.primary : jvTokens.hoverBg },
                    }}
                  >
                    {r.name}
                  </Box>
                );
              })}
            </Box>
          </Box>
        )}

        {/* 左下：讀數面板（原作的 #info） */}
        <Box
          data-testid="taiwan-map-readout"
          data-region={readout?.name ?? ''}
          sx={hud({
            left: 26,
            bottom: 26,
            minWidth: 188,
            borderLeft: `2px solid ${pal.focus}`,
            px: 1.75,
            py: 1.25,
            opacity: ready ? 1 : 0,
            transition: 'opacity .8s ease .2s',
          })}
        >
          <Typography sx={{ ...MONO, fontSize: 9.5, color: jvTokens.faint }}>REGION</Typography>
          <Typography
            sx={{ fontSize: 22, fontWeight: 700, letterSpacing: '.04em', color: jvTokens.text, mt: '2px' }}
          >
            {readout?.name ?? '—'}
          </Typography>
          <Typography sx={{ ...MONO, fontSize: 10, color: jvTokens.caption, mt: '5px' }}>
            {readout ? `ID ${readout.id || '—'}　·　AREA ${(readout.area * 100).toFixed(1)}` : 'HOVER TO SCAN'}
          </Typography>
        </Box>

        {/* 右下：操作提示 */}
        <Box
          sx={hud({
            right: 26,
            bottom: 26,
            textAlign: 'right',
            opacity: ready ? 1 : 0,
            transition: 'opacity .8s ease .2s',
          })}
        >
          <Typography sx={{ ...MONO, fontSize: 10, color: jvTokens.faint, lineHeight: 1.9 }}>
            拖曳 · 旋轉　　滾輪 · 縮放
            <br />
            停留 · 縣市抬升
          </Typography>
        </Box>

        {/* 載入 / 錯誤 —— 第一幀畫出來就收掉，把進場動畫讓出來 */}
        {(!painted || error) && (
          <Box
            sx={hud({
              inset: 0,
              display: 'grid',
              placeContent: 'center',
              gap: 1.5,
              justifyItems: 'center',
              backgroundColor: pal.bg,
              zIndex: 3,
              transition: 'opacity .6s ease',
            })}
          >
            {error ? (
              <Typography sx={{ fontSize: 12.5, color: jvTokens.warn, maxWidth: 360, textAlign: 'center' }}>
                3D 場景載入失敗：{error}
              </Typography>
            ) : (
              <>
                <Typography sx={{ ...MONO, fontSize: 10, color: jvTokens.faint }}>INITIALIZING</Typography>
                <Box sx={{ width: 190, height: '1px', backgroundColor: jvTokens.track, overflow: 'hidden' }}>
                  <Box
                    sx={{
                      height: '100%',
                      width: '40%',
                      backgroundColor: pal.focus,
                      animation: 'twMapSweep 1.15s ease-in-out infinite',
                      '@keyframes twMapSweep': {
                        '0%': { transform: 'translateX(-110%)' },
                        '100%': { transform: 'translateX(360%)' },
                      },
                    }}
                  />
                </Box>
              </>
            )}
          </Box>
        )}
      </Box>
    </>
  );
};

export default TaiwanMapPage;
