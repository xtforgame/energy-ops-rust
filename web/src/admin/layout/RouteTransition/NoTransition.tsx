import type { RouteTransitionVariantProps } from './types';

/**
 * 零轉場（預設）。
 *
 * 刻意連 wrapper div 都不加，直接把 children 交出去 —— DOM 與導入本模組之前
 * 完全相同，所以選這個模式等於沒有這層存在，不會有任何版面/樣式副作用。
 * 也不 import motion。
 */
const NoTransition = ({ children }: RouteTransitionVariantProps) => <>{children}</>;

export default NoTransition;
