import type * as React from 'react';

/**
 * 切頁效果。挑法見同目錄 README.md。
 *   none      — 硬切，零轉場（預設，也是導入本模組前的原始行為）
 *   enter     — 新頁淡入 + 微幅上移，永遠只有一層
 *   crossfade — 舊頁溶解成新頁，兩層同時在場
 *   cloud     — three.js 雲霧從右掃到左，蓋滿時換頁（會動態載入 three）
 */
export type RouteTransitionMode = 'none' | 'enter' | 'crossfade' | 'cloud';

export interface RouteTransitionVariantProps {
  children: React.ReactNode;
  /** 轉場時間（秒）。後台資訊密度高，預設刻意壓短。'none' 忽略此值。 */
  duration?: number;
  /** 進場的 y 位移（px）。設 0 就是純淡入／純溶解、不位移。'none' 忽略此值。 */
  offset?: number;
}

export interface RouteTransitionProps extends RouteTransitionVariantProps {
  mode?: RouteTransitionMode;
}

/**
 * wrapper 對版面必須透明：照抄 `.RaLayout-content` 的 flex 設定
 * （ra-ui-materialui/src/layout/Layout.tsx 的 LayoutClasses.content）。
 */
export const containerBase: React.CSSProperties = {
  flexGrow: 1,
  flexBasis: 0,
  minWidth: 0,
};
