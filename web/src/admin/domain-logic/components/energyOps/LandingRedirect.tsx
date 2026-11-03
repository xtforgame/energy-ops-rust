import * as React from 'react';
import { energyHttp, energyBase } from '@/api/energy';
import { useNavigate, useLocation } from 'react-router-dom';
import { useSelection } from '@/platform/selection/store';

/**
 * 登入後導到「這個角色該做事的那一頁」（`FRONTEND.md §1.3`）。
 *
 * **落點由後端決定**（`GET .../me/landing`）——
 * 角色與**部門身分**是後端才知道的事實（「部門主管」是組織事實不是 RBAC 角色）。
 * 前端自己推就會有第二套規則，兩套遲早不一致。
 *
 * **只在登入後的第一次導向**：
 * 使用者手動導去別頁時**絕不強制跳回** —— 那會讓人沒辦法待在他想看的頁面上，
 * 而且症狀是「點什麼都會彈回去」，比沒有落點還糟。
 *
 * 三道閘（缺一就會變成「一直跳回去」）：
 * 1. 只在**根路徑**（`#/`，RA 登入後的預設位置）才導 —— 已經在別頁就不動
 * 2. 每個 session **只導一次**（`sessionStorage` 記號）
 * 3. 組織還沒選好（`organizationId` 為空）就先不動，等下一次 render
 */
const LandingRedirect: React.FC = () => {
  const navigate = useNavigate();
  const location = useLocation();
  const orgId = useSelection()?.organizationId;
  const done = React.useRef(false);

  React.useEffect(() => {
    if (done.current) return;
    // 閘 ③：組織還沒選好 —— 這時打 API 會落在錯的 org 上。
    if (!orgId) return;
    // 閘 ①：只在根路徑導。使用者已經在別頁（不管是手動輸網址還是點選單）就不碰。
    const path = location.pathname === '/' ? '/' : location.pathname;
    if (path !== '/') { done.current = true; return; }

    const key = `energy-landing-done:${orgId}`;
    // 閘 ②：這個 session 導過就不再導。
    if (sessionStorage.getItem(key)) { done.current = true; return; }

    let alive = true;
    energyHttp.get(`${energyBase(orgId)}/me/landing`)
      .then(({ data }) => {
        if (!alive || !data?.route) return;
        sessionStorage.setItem(key, data.route);
        done.current = true;
        // `replace` —— 不要留在瀏覽器歷史裡，否則使用者按上一頁會跳回根路徑再被導一次。
        navigate(`/${data.route}`, { replace: true });
      })
      .catch(() => {
        // 拿不到落點就**什麼都不做**（留在預設頁）——
        // 不要退回一個猜的頁面：猜錯比不導更煩。
        done.current = true;
      });
    return () => { alive = false; };
  }, [orgId, location.pathname, navigate]);

  return null;
};

export default LandingRedirect;
