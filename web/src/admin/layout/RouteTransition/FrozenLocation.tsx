import * as React from 'react';
import { useContext, useRef } from 'react';
// eslint-disable-next-line camelcase
import { UNSAFE_LocationContext } from 'react-router';

/**
 * ⭐ 交叉溶解的關鍵零件。
 *
 * RA5 傳給 `<Admin layout>` 的 children 是一個**沒有帶 location 的 `<Routes>`**
 * （見 ra-core/src/core/CoreAdminRoutes.tsx）。離場中的那份雖然被 AnimatePresence
 * 留在畫面上，卻仍然是「活的」React 子樹 —— router context 一變，它會重新解析成
 * **新頁面**，於是看到的是「新內容淡出、新內容淡入」，而不是舊溶新。
 *
 * 這裡的作法：**只在需要的那段期間**把 LocationContext 凍在最後一次「未凍結」時的值。
 *   - `frozen === false` → 透傳實際 context，同時把它記進 ref。這一點很重要：
 *     同一個 key 內的導覽（/users/3 → /users/5）不會 remount，若無條件凍結，
 *     那個頁面會從此看不到新的 location（畫面卡在舊記錄）。
 *   - `frozen === true` → 用 ref 裡的舊 context，子樹於是繼續渲染舊路由。
 *
 * 兩個使用者：
 *   - CrossfadeTransition：離場那層 `frozen={!isPresent}`
 *   - CloudTransition：雲還沒蓋滿之前 `frozen={covering}`
 *
 * ⚠️ 用到 react-router 的 `UNSAFE_LocationContext`（內部 API，目前 v7.9.2）。
 * 升 react-router 大版本時要回來確認這個匯出還在；真的沒了就把切頁效果切回
 * 'enter'，那個模式不依賴任何 UNSAFE_ API。
 */
const FrozenLocation = ({
  children,
  frozen,
}: { children: React.ReactNode; frozen: boolean }) => {
  const liveContext = useContext(UNSAFE_LocationContext);
  const lastLiveContext = useRef(liveContext);
  if (!frozen) lastLiveContext.current = liveContext;

  return (
    <UNSAFE_LocationContext.Provider value={frozen ? lastLiveContext.current : liveContext}>
      {children}
    </UNSAFE_LocationContext.Provider>
  );
};

export default FrozenLocation;
