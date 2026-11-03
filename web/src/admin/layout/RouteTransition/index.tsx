import * as React from 'react';
import NoTransition from './NoTransition';
import EnterTransition from './EnterTransition';
import CrossfadeTransition from './CrossfadeTransition';
import CloudTransition from './CloudTransition';
import { RouteTransitionMode, RouteTransitionProps } from './types';

const VARIANTS: Record<RouteTransitionMode, React.ComponentType<any>> = {
  none: NoTransition,
  enter: EnterTransition,
  crossfade: CrossfadeTransition,
  cloud: CloudTransition,
};

/**
 * 後台切頁效果的總開關。
 *
 * ⭐ 換效果只要改 `layout/Layout.tsx` 傳進來的 `mode`，不必動這裡。
 *    三個模式的差別、取捨與已知坑見同目錄 README.md。
 *
 * 預設 'none'（硬切）＝ 導入本模組之前的原始行為，連 wrapper div 都不加。
 */
export const RouteTransition = ({
  children,
  mode = 'none',
  ...variantProps
}: RouteTransitionProps & Record<string, any>) => {
  const Variant = VARIANTS[mode] || NoTransition;
  return <Variant {...variantProps}>{children}</Variant>;
};

export { getRouteKey } from './getRouteKey';
export { default as NoTransition } from './NoTransition';
export { default as EnterTransition } from './EnterTransition';
export { default as CrossfadeTransition } from './CrossfadeTransition';
export { default as CloudTransition } from './CloudTransition';
export type { RouteTransitionMode, RouteTransitionProps } from './types';

export default RouteTransition;
