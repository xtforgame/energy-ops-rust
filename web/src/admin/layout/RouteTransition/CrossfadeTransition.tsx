import * as React from 'react';
import { GlobalStyles } from '@mui/material';
import {
  AnimatePresence, motion, useIsPresent, useReducedMotion,
} from 'motion/react';
import { useLocation } from 'react-router';
import FrozenLocation from './FrozenLocation';
import { getRouteKey } from './getRouteKey';
import { containerBase, RouteTransitionVariantProps } from './types';

interface LayerProps extends Required<Pick<RouteTransitionVariantProps, 'duration' | 'offset'>> {
  children: React.ReactNode;
}

/**
 * 一層頁面。進場與離場的兩層會**同時**存在於同一個 grid cell（`gridArea: 1 / 1`）
 * 而互相疊合 —— 這是交叉溶解的前提。
 *
 * 刻意不用 position:absolute：絕對定位會讓這一層脫離文件流，長列表撐不出高度、
 * 頁面就捲不到底（實測 1400x380 的儀表板，grid 版本層高 5272 = 內容 5272，
 * 可捲到底；absolute 版本會被裁掉）。
 */
const Layer = ({ children, duration, offset }: LayerProps) => {
  const isPresent = useIsPresent();

  return (
    <motion.div
      initial={{ opacity: 0, y: offset }}
      animate={{ opacity: 1, y: 0, transition: { duration, ease: 'easeOut' } }}
      exit={{ opacity: 0, transition: { duration: duration * 0.8, ease: 'easeIn' } }}
      style={{
        gridArea: '1 / 1',
        display: 'flex',
        flexDirection: 'column',
        minWidth: 0,
        // 離場那層只是殘影：不吃點擊，也要壓在進場那層底下
        pointerEvents: isPresent ? 'auto' : 'none',
        zIndex: isPresent ? 1 : 0,
      }}
    >
      <FrozenLocation frozen={!isPresent}>{children}</FrozenLocation>
    </motion.div>
  );
};

/**
 * 交叉溶解：舊畫面溶解成新畫面，兩層同時在場。
 *
 * ⚠️ 已知取捨（挑這個模式前先讀）：
 * 1. 疊合期間（預設 ~130ms）**兩份 RA 頁面同時掛載** —— 兩份 react-query 訂閱、
 *    兩份 mount effect。若某頁在 mount/unmount 時寫全域 store，理論上會互相蓋。
 *    把 duration 壓短可以縮小這個窗口。
 * 2. 尾端有一格殘影：離場動畫結束到 React 移除節點之間有一幀，舊層的 inline
 *    opacity 被 motion 清掉而 revert 回 1。試過 transitionEnd 與寫死 base opacity
 *    都無效（motion 對正在動畫的屬性會忽略後來的靜態值）。實測影響是那一幀舊內容
 *    只貢獻約 6%（壓在 0.94 的新層底下），前一幀是 0.6% —— 8ms 的 5% 亮度跳動，
 *    且 rAF 取樣在 paint 之前，未必真的被畫出來。判斷不值得為它加 hack。
 * 3. 依賴 react-router 的 UNSAFE_LocationContext，見 FrozenLocation.tsx。
 */
const CrossfadeTransition = ({
  children,
  duration = 0.2,
  offset = 6,
}: RouteTransitionVariantProps) => {
  const { pathname } = useLocation();
  const reduceMotion = useReducedMotion();
  const routeKey = getRouteKey(pathname);

  if (reduceMotion) {
    return (
      <div style={{ ...containerBase, display: 'flex', flexDirection: 'column' }}>
        {children}
      </div>
    );
  }

  return (
    <>
      {/*
        RA 的 <Title> 是用 createPortal 打進 AppBar 的 #react-admin-title
        （ra-ui-materialui/src/layout/Title.tsx）。疊層期間兩個頁面各塞一個 span
        進去，標題會變成兩頁串接（實測看到「我的邀請專案」）。portal 的內容跑到
        我們的層外面，靠 opacity 蓋不到，只能在落點上處理：後掛的那個（＝進場頁）
        永遠是最後一個子節點，所以只顯示 :last-child。
        平時 portal 裡本來就只有一個子節點，這條規則不會有作用。
      */}
      <GlobalStyles styles={{ '#react-admin-title > *:not(:last-child)': { display: 'none' } }} />
      <div
        style={{
          ...containerBase,
          display: 'grid',
          gridTemplateColumns: 'minmax(0, 1fr)',
          gridTemplateRows: 'minmax(0, 1fr)',
        }}
      >
        <AnimatePresence>
          <Layer key={routeKey} duration={duration} offset={offset}>
            {children}
          </Layer>
        </AnimatePresence>
      </div>
    </>
  );
};

export default CrossfadeTransition;
