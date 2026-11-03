// 任何 API（平台或能源）回 401 → session 已失效，登出並回登入頁；同一波多個 401 只處理一次。
import * as React from 'react';
import { useLogout } from 'react-admin';
import { onUnauthorized } from '../../api/http';

const UnauthorizedWatcher: React.FC = () => {
  const logout = useLogout();
  const pending = React.useRef(false);
  React.useEffect(() => onUnauthorized(() => {
    if (pending.current) return;
    pending.current = true;
    Promise.resolve(logout(undefined, '/login', true)).finally(() => {
      pending.current = false;
    });
  }), [logout]);
  return null;
};

export default UnauthorizedWatcher;
