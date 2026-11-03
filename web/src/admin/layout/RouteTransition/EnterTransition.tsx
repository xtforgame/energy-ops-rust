import { motion, useReducedMotion } from 'motion/react';
import { useLocation } from 'react-router';
import { getRouteKey } from './getRouteKey';
import { containerBase, RouteTransitionVariantProps } from './types';

/**
 * 進場淡入（只有進場，沒有離場）。
 *
 * ⚠️ 為什麼不做離場：RA5 傳給 `<Admin layout>` 的 children 是一個**沒有帶
 * location 的 `<Routes>`**（見 ra-core/src/core/CoreAdminRoutes.tsx）。離場中的
 * 那份仍是活的子樹，router context 一變就會重新解析成新頁面，於是看到的是
 * 「新內容淡出、新內容淡入」。要做離場必須凍結 location context —— 那是
 * CrossfadeTransition 在做的事。這個模式用不到任何 UNSAFE_ API，是最保守的選擇。
 *
 * 版面上刻意不用 position:absolute —— RA 的 Datagrid sticky header 與捲動容器
 * 依賴正常文件流。
 */
const EnterTransition = ({
  children,
  duration = 0.18,
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
    <motion.div
      key={routeKey}
      initial={{ opacity: 0, y: offset }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration, ease: 'easeOut' }}
      style={{ ...containerBase, display: 'flex', flexDirection: 'column' }}
    >
      {children}
    </motion.div>
  );
};

export default EnterTransition;
