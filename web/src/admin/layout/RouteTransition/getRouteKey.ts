/**
 * 把 pathname 收斂成「值得重播動畫」的 key。
 *
 * RA5 的 hash router pathname 形如：
 *   /                → dashboard
 *   /users           → users
 *   /users/create    → users/create
 *   /users/3         → users/edit
 *   /users/3/show    → users/show
 *
 * 記錄 id 刻意不進 key：在同一個資源裡換筆記錄（/users/3 → /users/5）不重播動畫，
 * 否則連續編輯時畫面會一直閃。query string 不在 pathname 裡，所以換 filter/分頁
 * 也不會觸發動畫。
 */
// eslint-disable-next-line import/prefer-default-export
export const getRouteKey = (pathname: string): string => {
  const [resource, second, third] = pathname.split('/').filter(Boolean);
  if (!resource) return 'dashboard';
  if (!second) return resource;
  return `${resource}/${second === 'create' ? 'create' : third || 'edit'}`;
};
