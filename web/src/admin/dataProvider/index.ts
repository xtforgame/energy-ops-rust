// energy 的頁面一律直接用 axios 打 /api/energy-ops/*（自訂頁面，不經 react-admin 的 dataProvider）。
// react-admin 仍需要一個 dataProvider；任何呼叫都代表某處誤用了 RA 的 CRUD，直接回錯誤。
import { DataProvider } from 'react-admin';

const unsupported = (method: string) => () => Promise.reject(new Error(`dataProvider.${method} 不支援（energy 頁面直接呼叫 API）`));

const dataProvider = new Proxy({} as DataProvider, {
  get: (_t, prop) => unsupported(String(prop)),
});

export default dataProvider;
