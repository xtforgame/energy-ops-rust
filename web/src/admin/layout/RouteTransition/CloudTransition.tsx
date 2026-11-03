import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { GlobalStyles, useTheme } from '@mui/material';
import { darken, lighten } from '@mui/material/styles';
import { useReducedMotion } from 'motion/react';
import { useLocation } from 'react-router';
import FrozenLocation from './FrozenLocation';
import { getRouteKey } from './getRouteKey';
import type { CloudCurtain } from './cloudCurtain';
import type { InkBrush } from './inkBrush';
import type { RouteTransitionVariantProps } from './types';

export interface CloudTransitionProps extends RouteTransitionVariantProps {
  /**
   * 整段轉場的時間（毫秒）。
   *
   * ⚠️ 改這個會連帶改變相機的視覺速度（速度 = 2·sweepHalf / duration）：
   * 同樣的行程壓在更短的時間裡就是變快。目前 900ms ↔ sweepHalf 73。
   * 等比例縮放的好處是「被雲遮住的那一小段」的**絕對毫秒數也不會變**
   * （窗口佔比 ∝ 1/sweepHalf，乘上 duration 之後相消）。
   */
  durationMs?: number;
  /**
   * 雲背後鋪什麼。
   *   'theme' — 頁面自己的底色壓暗一點做成淡漸層（不是天空，讀起來像頁面淡入一片霧）。
   *             canvas 不透明 → 換頁必定被遮住；白雲也有了比它暗的底，形狀自然浮出來。
   *   'demo'  — demo 原本的藍天 #1e4877 → #4584b4
   *   'none'  — 什麼都不鋪，只有雲疊在頁面上（⚠️ 遮不滿，換頁會被看見）
   */
  backdrop?: 'theme' | 'demo' | 'none';
  /**
   * 雲幕與頁面的混合模式。
   *
   * 白雲需要比它更暗的底才看得見：黑底上對比最大所以好看，純白底上等於沒有對比。
   * 'auto' 會依主題挑：
   *   亮色主題 → `multiply`（雲的亮部變透明、暗部把頁面壓暗，看起來像雲影掃過）
   *   暗色主題 → `screen`（雲的暗部變透明、亮部提亮，看起來像雲在發光）
   * 有鋪底色時（backdrop ≠ 'none'）canvas 本來就不透明，混合沒有意義，一律 normal。
   */
  blend?: 'auto' | 'normal' | 'multiply' | 'screen';
  /**
   * 不透明度鐘形曲線的飽和倍率：`opacity = smootherstep(clamp(sin(πt)·plateau, 0, 1))`。
   *
   * 飽和點（opacity 到 1）落在 `t/duration = asin(1/plateau)/π`：
   *   plateau 1.5  → 0.232 → 900ms 的轉場只花 209ms 就全滿，**快到讀不出淡入**
   *   plateau 1.15 → 0.336 → 約 300ms 漸入 / 300ms 全滿 / 300ms 漸出
   * 越大 → 全不透明那段越長，但頭尾越突兀。
   * ⚠️ 飽和點必須早於遮罩雲的全遮區間（目前 p=0.48~0.56），否則遮罩雲來的時候
   * 幕還沒全開。
   */
  plateau?: number;
  /**
   * 墨刷 logo 層（疊在雲上）：用 idea/ink-brush-trail.html 的刷毛 shader 沿
   * logoStrokes.ts 的筆畫「畫出」logo，停留一拍，然後尾巴以同樣節奏**順著筆畫離場**。
   * 筆色 = logo 原色。要調形狀開 tools/ink-stroke-editor.html。
   */
  inkLogo?: boolean;
  /** logo 版面尺寸 = min(內容區寬, 高) × inkScale */
  inkScale?: number;
  /**
   * 墨刷全域參數覆寫（墨量 / 毛數 / 筆寬倍率 / 餘韻 —— demo 調墨面板的四個滑桿）。
   * 預設值在 logoStrokes.ts 的 `logoInkParams`（編輯器匯出的 params 區塊貼回那裡），
   * 這個 prop 只在單一頁面要臨時蓋掉時用。
   */
  inkParams?: { ink?: number; bristle?: number; widthScale?: number; lifeMs?: number | null };
}

/**
 * 雲霧過場（第一階段：完整重現 + 淡入淡出）。
 *
 * 場景本身是 `idea/cloud.html` 的原樣重現（見 cloudCurtain.ts），這一層只負責
 * 把它淡入、換頁、淡出：
 *
 *   fadeIn  —— 雲層場景由透明淡入到全不透明；**頁面還是舊的**
 *   hold    —— 完全被蓋住，這時候才解凍 location 換成新頁
 *   fadeOut —— 淡出，露出新頁面
 *
 * 為什麼先做成這樣：雲的「從右到左掃過」是下一步。三個環節（重現、動作、遮蔽時機）
 * 一起改的話，出問題時分不出是哪一段錯 —— 先把「能不能重現」這件事釘死。
 *
 * 「舊頁面要撐到蓋滿才換」靠 FrozenLocation，理由見該檔說明。關鍵是 `covering`
 * 必須在**導覽當下那一個 render** 就成立（用 displayKey 與 routeKey 比對推導），
 * 不能等 effect —— 否則會先閃一格新頁面才被蓋住。
 */
const CloudTransition = ({
  children,
  durationMs = 900,
  plateau = 1.15,
  backdrop = 'theme',
  blend = 'auto',
  inkLogo = true,
  inkScale = 0.5,
  inkParams,
}: CloudTransitionProps) => {
  const { pathname } = useLocation();
  const routeKey = getRouteKey(pathname);
  const reduceMotion = useReducedMotion();
  const theme = useTheme();
  // 'theme' 的兩個色階：頁面底色壓暗（亮色主題）或提亮（暗色主題）一點點。
  // 壓暗幅度要夠白雲讀得出來、又不能像換了一塊背景板 —— 目前 14% / 6%。
  const backdropColors = (() => {
    if (backdrop === 'none') return null;
    if (backdrop === 'demo') return { top: '#1e4877', bottom: '#4584b4' };
    const base = theme.palette.background.default;
    return theme.palette.mode === 'dark'
      ? { top: lighten(base, 0.10), bottom: lighten(base, 0.03) }
      : { top: darken(base, 0.14), bottom: darken(base, 0.06) };
  })();
  // 有底色時 canvas 是不透明的，混合模式會把整片底色也拿去跟頁面混，必須關掉
  const mixBlendMode = (() => {
    if (backdrop !== 'none' || blend === 'normal') return 'normal' as const;
    if (blend === 'multiply' || blend === 'screen') return blend;
    return theme.palette.mode === 'dark' ? ('screen' as const) : ('multiply' as const);
  })();

  // overlay 的 portal 目標：main（與側欄同一個 stacking context）。
  // 初次 render 時 DOM 還沒 commit、querySelector 抓不到，所以 effect 後才定。
  const [portalTarget, setPortalTarget] = useState<Element | null>(null);
  useEffect(() => {
    setPortalTarget(document.querySelector('.RaLayout-contentWithSidebar') || document.body);
  }, []);

  const [displayKey, setDisplayKey] = useState(routeKey);
  // 導覽發生的那一個 render 就會是 true（不經過 effect）
  const covering = displayKey !== routeKey;

  // 讓建立雲幕的 effect 讀得到最新顏色，又不用把顏色放進 deps（放進去會重建整個場景）
  const backdropColorsRef = useRef(backdropColors);
  backdropColorsRef.current = backdropColors;

  // 參數覆寫走 ref + 字串 key：值真的變才重建墨刷，物件 identity 變不算
  const inkParamsRef = useRef(inkParams);
  inkParamsRef.current = inkParams;
  const inkParamsKey = JSON.stringify(inkParams ?? null);

  const hostRef = useRef<HTMLDivElement | null>(null);
  const curtainRef = useRef<CloudCurtain | null>(null);
  const inkRef = useRef<InkBrush | null>(null);
  const routeKeyRef = useRef(routeKey);
  routeKeyRef.current = routeKey;

  // --- 雲幕貼齊 <main class="RaLayout-contentWithSidebar">（側欄 + 內容整片）---
  //     AppBar 在轉場中不該被蓋住；側欄「在範圍內但要疊在雲上」—— 範圍歸雲幕、
  //     層級用下面 GlobalStyles 把 .RaSidebar 抬到 overlay 之上解決。
  //     目標元素可能比視窗高（長頁面），所以要跟視窗做交集裁切。
  const lastFitRef = useRef('');
  const fitToContent = () => {
    const host = hostRef.current;
    const curtain = curtainRef.current;
    const content = document.querySelector('.RaLayout-contentWithSidebar')
      || document.getElementById('main-content');
    if (!host || !content) return;
    const r = content.getBoundingClientRect();
    const left = Math.max(0, r.left);
    const top = Math.max(0, r.top);
    const width = Math.max(0, Math.min(window.innerWidth, r.right) - left);
    const height = Math.max(0, Math.min(window.innerHeight, r.bottom) - top);
    // no-op 防呆：這個函式每幀都會被叫，矩形沒變就什麼都不做
    //（下游 resize 對 canvas 尺寸賦值會清 WebGL buffer，見 cloudCurtain.resize）
    const key = `${left},${top},${width},${height}`;
    if (key === lastFitRef.current) return;
    lastFitRef.current = key;
    host.style.left = `${left}px`;
    host.style.top = `${top}px`;
    host.style.width = `${width}px`;
    host.style.height = `${height}px`;
    curtain?.resize(width, height);
    // logo 置中（略偏上），尺寸跟著內容區
    if (inkRef.current) {
      const size = Math.min(width, height) * inkScale;
      inkRef.current.resize(width, height, {
        x: (width - size) / 2,
        y: (height - size) * 0.44,
        size,
      });
    }
  };

  // --- 建立雲幕（動態 import three；掛載時就先備好，第一次切頁才不會來不及）---
  useEffect(() => {
    if (reduceMotion) return undefined;
    let cancelled = false;
    import('./cloudCurtain')
      .then(({ createCloudCurtain }) => createCloudCurtain({ backdrop: backdropColorsRef.current }))
      .then((curtain) => {
        if (cancelled) { curtain.dispose(); return; }
        curtainRef.current = curtain;
        const { canvas } = curtain;
        canvas.style.width = '100%';
        canvas.style.height = '100%';
        canvas.style.display = 'block';
        hostRef.current?.appendChild(canvas);
        fitToContent();
        // 量測用把手（唯讀）。探針靠它拿遮蔽率，不必去猜像素。
        (window as any).__cloudCurtain = curtain;
      })
      .catch((e) => {
        // three 或貼圖載不起來就退化成沒有轉場，不要讓整個後台掛掉
        // eslint-disable-next-line no-console
        console.warn('[CloudTransition] 雲幕載入失敗，退化為無轉場', e);
      });
    // 墨刷 logo 層（獨立 canvas，疊在雲 canvas 之上）
    if (inkLogo) {
      Promise.all([import('./inkBrush'), import('./logoStrokes')])
        .then(([{ createInkBrush }, { logoStrokes, LOGO_VIEWBOX, logoInkParams }]) => createInkBrush({
          strokes: logoStrokes,
          viewBox: LOGO_VIEWBOX,
          // 時間軸與雲同長：畫上 0..0.42、停留 ..0.58、尾巴追到 1.0（2·0.42+0.16 = 1）
          // ⚠️ 若 lifeMs 覆寫得比自動值長，墨會在 durationMs 被雲的收尾硬切掉
          drawMs: durationMs * 0.42,
          holdMs: durationMs * 0.16,
          ...logoInkParams,
          ...inkParamsRef.current,
        }))
        .then((ib) => {
          if (cancelled) { ib.dispose(); return; }
          inkRef.current = ib;
          ib.canvas.style.position = 'absolute';
          ib.canvas.style.inset = '0';
          ib.canvas.style.width = '100%';
          ib.canvas.style.height = '100%';
          hostRef.current?.appendChild(ib.canvas);
          fitToContent();
          (window as any).__inkBrush = ib;
        })
        .catch((e) => {
          // eslint-disable-next-line no-console
          console.warn('[CloudTransition] 墨刷 logo 層載入失敗，只跑雲', e);
        });
    }
    return () => {
      cancelled = true;
      curtainRef.current?.dispose();
      curtainRef.current = null;
      inkRef.current?.dispose();
      inkRef.current = null;
      delete (window as any).__cloudCurtain;
      delete (window as any).__inkBrush;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reduceMotion, inkLogo, inkParamsKey]);

  // 換主題 → 就地換底色，不重建雲幕
  useEffect(() => {
    curtainRef.current?.setBackdrop(backdropColors);
  }, [backdropColors?.top, backdropColors?.bottom]);

  useEffect(() => {
    window.addEventListener('resize', fitToContent);
    return () => window.removeEventListener('resize', fitToContent);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // --- 時間軸。deps 只有 routeKey：中途 setDisplayKey 造成的重繪不可以打斷迴圈 ---
  const firstRunRef = useRef(true);
  useEffect(() => {
    if (firstRunRef.current) { firstRunRef.current = false; return undefined; }
    if (reduceMotion) { setDisplayKey(routeKeyRef.current); return undefined; }

    const curtain = curtainRef.current;
    const host = hostRef.current;
    if (!curtain || !host) { setDisplayKey(routeKeyRef.current); return undefined; }

    fitToContent();
    curtain.start();

    let raf = 0;
    let swapped = false;
    // 淡出尾段凍結場景（見 tick 內註解）
    const startedAt = performance.now();
    const tick = (now: number) => {
      const t = now - startedAt;
      // 內容區的位置會因為捲動/側欄收合而變，每幀跟一次
      fitToContent();

      // ⛔ 不要用「淡入 / 停留 / 淡出」三段式。那本來就是三個階段，接點再怎麼加
      //    easing，看起來還是會分段。改成**單一條連續的鐘形曲線**：
      //      k = clamp(sin(πt) · plateau, 0, 1)，再過一次 smootherstep
      //    sin 本身處處平滑；乘上 plateau 之後在中段被夾到 1 形成平頂，而
      //    smootherstep 在 k=1 的一階導數為 0，所以連夾住的那個接點也是 C¹。
      const k = Math.min(1, Math.max(0, Math.sin(Math.PI * (t / durationMs)) * plateau));
      const opacity = k * k * k * (k * (k * 6 - 15) + 10);
      host.style.opacity = String(opacity);


      // 相機：單一條等速直線（由左往右，雲相對地往左掠過），**全程不中斷**。
      //
      // 這裡曾經在淡出一開始就凍結場景 —— 因為雲在半透明時持續變化會讓合成亮度非單調
      // （掃動尾段雲變稀 → 畫面先偏亮、雲回來又變暗 → 「結尾閃一下」）。
      // ⭐ `sweepHalf` 從 146 砍到 73 之後那個成因消失了：相機行程只剩一半，根本走不到
      //    雲變稀的尾段。凍結的代價是淡出整段畫面是靜止的，看起來不像「一邊移動一邊消失」。
      // ⚠️ 若之後把 `sweepHalf` 調回大值，要重跑逐幀亮度量測確認閃爍沒有回來
      //    （量合成亮度的最大方向反轉，> 1.5 階就看得見）。
      curtain.setProgress(t / durationMs);

      // 墨刷 logo：畫上 → 停 → 順著筆畫離場（時間軸見 inkBrush.ts 開頭）
      if (inkRef.current) { inkRef.current.setTime(t); inkRef.current.render(); }

      // 換頁點在整段中點：此時相機在雲毯裡、opacity 也在平頂，看不到抽換
      if (!swapped && t >= durationMs / 2) {
        swapped = true;
        setDisplayKey(routeKeyRef.current);
      }

      if (t < durationMs) {
        raf = requestAnimationFrame(tick);
      } else {
        host.style.opacity = '0';
        curtain.stop();
        // 清乾淨墨刷 canvas（超過 totalMs 所有點都死了，再 render 一次就是空白）
        if (inkRef.current) { inkRef.current.setTime(durationMs + 1); inkRef.current.render(); }
      }
    };
    raf = requestAnimationFrame(tick);

    return () => {
      cancelAnimationFrame(raf);
      curtain.stop();
      host.style.opacity = '0';
      // 被新的導覽打斷：確保頁面一定會換過去，不要卡在舊頁
      if (!swapped) setDisplayKey(routeKeyRef.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [routeKey]);

  const overlay = portalTarget ? createPortal(
    <div
      ref={hostRef}
      aria-hidden
      style={{
        position: 'fixed',
        // 實際的 left/top/width/height 由 fitToContent() 逐幀貼齊 #main-content
        left: 0,
        top: 0,
        width: 0,
        height: 0,
        opacity: 0,
        pointerEvents: 'none',
        // ⚠️ z-index 是「layout 的 stacking context 內」的局部值 —— .RaLayout-root
        //    自己有 position:relative; z-index:1，把整個 layout 關在一個 context 裡，
        //    portal 到 body 再高的 z 都到不了側欄上面（實測 1251 蓋不過）。所以 host
        //    portal 進 main、在局部比大小：content(2) < host(3) < 側欄(4) < AppBar(1100)。
        //    position 仍是 fixed（祖先鏈無 transform，實測過；有人加了 transform 這裡會歪）。
        zIndex: 3,
        overflow: 'hidden',
        mixBlendMode,
      }}
    />,
    portalTarget,
  ) : null;

  if (reduceMotion) return <>{children}</>;

  return (
    <>
      {/*
        側欄要疊在雲幕上面：側欄是 docked Drawer、原本沒有 stacking context，
        給 position:relative + zIndex 4（> host 的 3，同一個 stacking context 內）
        就浮上來 —— 雲從選單底下飄過去。
        ⚠️ 選擇器用結構不用 class 名：根節點的 RA class 是 RaSidebar-docked /
        RaSidebar-open（開合會換），沒有裸的 .RaSidebar（實測抓空）。
        ⚠️ 前提是它的祖先鏈沒有別的 stacking context，RA5 目前沒有；
        若哪天選單被雲蓋住了，先檢查這條。
      */}
      <GlobalStyles styles={{ '.RaLayout-contentWithSidebar > .MuiDrawer-docked': { position: 'relative', zIndex: 4 } }} />
      <FrozenLocation frozen={covering}>{children}</FrozenLocation>
      {overlay}
    </>
  );
};

export default CloudTransition;
