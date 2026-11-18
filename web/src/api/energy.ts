// 能源頁面的 API 唯一出入口（第二階段：對新後端 /api/v1/orgs/{orgId}/energy/...）。
// 頁面沿用 axios 相容介面（get/post/patch/put/delete，回 { data }）；新舊回應形狀的差異
// 一律在這裡的轉換表（ENERGY_ROUTES）處理，頁面拿到的仍是 App 5 的形狀。
import axios, { AxiosError, AxiosRequestConfig, AxiosResponse } from 'axios';
import { normalizeError, notifyUnauthorized } from './http';
import { selectionStore } from '../platform/selection/store';

type OrgId = number | string | null | undefined;

/** 新的能源前綴。 */
export const energyPrefix = (orgId: OrgId) => `/api/v1/orgs/${orgId}/energy`;

/** 某組織的能源 API 根路徑；頁面一律以它組路徑。 */
export function energyBase(orgId: OrgId): string {
  return energyPrefix(orgId);
}

/** 沒傳組織時退回目前選定的組織（附件連結等不在頁面 state 裡的地方用）。 */
function orgOrSelected(orgId?: OrgId): OrgId {
  return orgId ?? selectionStore.getSelection()?.organizationId;
}

/** 附件下載網址（<a href> 直接開，靠同網域 cookie）；附件也在組織前綴底下。 */
export function attachmentDownloadUrl(attachmentId: number | string, orgId?: OrgId): string {
  return `${energyPrefix(orgOrSelected(orgId))}/attachments/${attachmentId}/download`;
}

// ───────────── 回應轉換 ─────────────

type Adapt = (data: any) => any;

/** 新後端清單一律 `{ items }`；頁面沿用 App 5 的陣列。已經是陣列就原樣。 */
export const unwrapItems: Adapt = (data) => {
  if (Array.isArray(data)) return data;
  if (data && Array.isArray(data.items)) return data.items;
  return data;
};

const num = (v: unknown) => (v === null || v === undefined || v === '' ? v : Number(v));

/** 一條路由：方法＋能源前綴之後的路徑樣式＋（可選）回應轉換。 */
export interface EnergyRoute {
  method: 'GET' | 'POST' | 'PATCH' | 'PUT' | 'DELETE';
  /** 前綴之後的路徑，`:x` 是路徑參數。 */
  path: string;
  adapt?: Adapt;
  /** 收件那一組（附件／電費單／收件批次／辨識）：後端另一條分支在做，尚未上線。 */
  intake?: boolean;
}

/** 能源 API 的完整清單（契約 docs/API-ENERGY.md）。沒有 adapt 的原樣交給頁面。 */
export const ENERGY_ROUTES: EnergyRoute[] = [
  // 組織治理
  { method: 'GET', path: '/departments', adapt: unwrapItems },
  { method: 'POST', path: '/departments' },
  { method: 'PATCH', path: '/departments/:departmentId' },
  { method: 'DELETE', path: '/departments/:departmentId' },
  { method: 'PUT', path: '/departments/:departmentId/members/:userId' },
  { method: 'DELETE', path: '/departments/:departmentId/members/:userId' },
  // 成員挑選器沿用舊的 `{ id, name }`：補上 id（= userId）
  { method: 'GET', path: '/members', adapt: (d) => (unwrapItems(d) ?? []).map((m: any) => ({ id: m.userId, ...m })) },
  { method: 'DELETE', path: '/members/:userId' },
  { method: 'GET', path: '/assignable-roles' },
  { method: 'POST', path: '/role-assignments' },
  { method: 'POST', path: '/role-assignments/revoke' },
  { method: 'GET', path: '/audit-events', adapt: unwrapItems },
  { method: 'GET', path: '/audit-actors', adapt: unwrapItems },
  // 儀表板、工作台
  { method: 'GET', path: '/dashboard' },
  { method: 'GET', path: '/calendar-events', adapt: unwrapItems },
  { method: 'GET', path: '/me/landing' },
  { method: 'GET', path: '/workbench' },
  // 流程（清單頁讀 rows；新後端叫 items）
  { method: 'GET', path: '/executions', adapt: (d) => (d && Array.isArray(d.items) && !d.rows ? { ...d, rows: d.items } : d) },
  { method: 'POST', path: '/executions' },
  { method: 'GET', path: '/executions/:executionIdOrNo' },
  { method: 'GET', path: '/executions/:executionId/actions' },
  { method: 'GET', path: '/executions/:executionId/graph' },
  { method: 'GET', path: '/executions/:executionId/revertable' },
  { method: 'GET', path: '/executions/:executionId/lineage', adapt: unwrapItems },
  { method: 'POST', path: '/executions/:executionId/abort' },
  { method: 'POST', path: '/executions/:executionId/force-advance' },
  { method: 'POST', path: '/executions/:executionId/terminate' },
  { method: 'POST', path: '/tasks/:taskId/claim' },
  { method: 'POST', path: '/tasks/:taskId/release' },
  { method: 'POST', path: '/tasks/:taskId/actions' },
  { method: 'POST', path: '/tasks/:taskId/abort-decision' },
  { method: 'POST', path: '/tasks/:taskId/reassign' },
  { method: 'POST', path: '/side-effects/:sideEffectId/release' },
  { method: 'GET', path: '/delegations', adapt: unwrapItems },
  { method: 'POST', path: '/delegations' },
  { method: 'POST', path: '/delegations/:delegationId/revoke' },
  { method: 'POST', path: '/lineage' },
  { method: 'GET', path: '/workflows' },
  { method: 'GET', path: '/workflows/:definitionId/graph' },
  { method: 'GET', path: '/workflows/:definitionId/nodes/:nodeKey/tasks', adapt: unwrapItems },
  { method: 'POST', path: '/setup/install-defaults' },
  // 附件
  { method: 'GET', path: '/executions/:executionId/attachments', adapt: unwrapItems, intake: true },
  { method: 'POST', path: '/executions/:executionId/attachments', intake: true },
  { method: 'GET', path: '/attachments/:attachmentId/download', intake: true },
  { method: 'POST', path: '/attachment-links/:linkId/remove', intake: true },
  { method: 'POST', path: '/uploads/sign', intake: true },
  // 電費單與收件
  { method: 'POST', path: '/bills/ingest', intake: true },
  { method: 'GET', path: '/bills', adapt: unwrapItems, intake: true },
  { method: 'GET', path: '/bills/unmatched', adapt: unwrapItems, intake: true },
  { method: 'GET', path: '/bills/:billId', intake: true },
  { method: 'PATCH', path: '/bills/:billId', intake: true },
  { method: 'GET', path: '/bills/:billId/branch-preview', intake: true },
  { method: 'GET', path: '/bills/:billId/payouts', adapt: unwrapItems, intake: true },
  { method: 'POST', path: '/bills/:billId/submit', intake: true },
  { method: 'POST', path: '/bills/:billId/match', intake: true },
  { method: 'POST', path: '/ingest-batches', intake: true },
  { method: 'GET', path: '/ingest-batches', adapt: unwrapItems, intake: true },
  { method: 'GET', path: '/ingest-batches/:batchId', intake: true },
  { method: 'POST', path: '/ingest-batches/:batchId/attachments', intake: true },
  { method: 'POST', path: '/ingest-batches/:batchId/recognize', intake: true },
  { method: 'PATCH', path: '/ingest-items/:itemId', intake: true },
  { method: 'POST', path: '/ingest-items/:itemId/recognition', intake: true },
  { method: 'POST', path: '/ingest-items/:itemId/submit', intake: true },
  { method: 'POST', path: '/ingest-items/:itemId/discard', intake: true },
  { method: 'GET', path: '/recognition/status', intake: true },
  // 主檔與綁定
  { method: 'GET', path: '/stations', adapt: unwrapItems },
  { method: 'POST', path: '/stations' },
  { method: 'GET', path: '/station-list', adapt: unwrapItems },
  { method: 'PATCH', path: '/stations/:stationId' },
  { method: 'GET', path: '/unbound-stations', adapt: unwrapItems },
  { method: 'GET', path: '/stations/:stationId/overview' },
  { method: 'GET', path: '/stations/:stationId/meters', adapt: unwrapItems },
  { method: 'POST', path: '/stations/:stationId/meters', adapt: unwrapItems },
  { method: 'PATCH', path: '/meters/:meterId' },
  { method: 'GET', path: '/stations/:stationId/allocations', adapt: unwrapItems },
  { method: 'POST', path: '/stations/:stationId/allocations', adapt: unwrapItems },
  { method: 'PATCH', path: '/allocations/:allocationId' },
  { method: 'GET', path: '/stations/:stationId/allocation-history', adapt: unwrapItems },
  // 建議座標的頁面直接 toFixed：確保是數字
  {
    method: 'GET',
    path: '/stations/:stationId/geocode',
    adapt: (d) => (d?.result
      ? { ...d, result: { ...d.result, latitude: num(d.result.latitude), longitude: num(d.result.longitude) } }
      : d),
  },
  { method: 'POST', path: '/stations/:stationId/geocode' },
  { method: 'GET', path: '/stations/:stationId/bindings' },
  { method: 'GET', path: '/stations/:stationId/bindings/preview' },
  { method: 'POST', path: '/stations/:stationId/bindings' },
  { method: 'POST', path: '/stations/:stationId/bindings/switch' },
  { method: 'POST', path: '/stations/:stationId/bindings/end' },
  { method: 'GET', path: '/binding-options', adapt: unwrapItems },
  { method: 'GET', path: '/landowners', adapt: unwrapItems },
  { method: 'POST', path: '/landowners' },
  { method: 'PATCH', path: '/landowners/:landownerId' },
  { method: 'GET', path: '/landowners/:landownerId' },
  { method: 'GET', path: '/landowners/:landownerId/overview' },
];

const ENERGY_URL = /^\/api\/v1\/orgs\/([^/?#]+)\/energy(\/[^?#]*)?/;

const compiled = ENERGY_ROUTES.map((r) => ({
  route: r,
  re: new RegExp(`^${r.path.replace(/:[A-Za-z]+/g, '[^/]+')}/?$`),
  // 靜態段越多越優先（/bills/unmatched 要贏過 /bills/:billId）
  score: r.path.split('/').filter((s) => s && !s.startsWith(':')).length,
}));

/** 找出 url 對應的能源路由；不是能源路徑或不在清單上回 null。 */
export function matchEnergyRoute(method: string, url: string): { orgId: string; path: string; route: EnergyRoute } | null {
  const m = ENERGY_URL.exec(url);
  if (!m) return null;
  const path = m[2] || '/';
  const upper = method.toUpperCase();
  const hit = compiled
    .filter((c) => c.route.method === upper && c.re.test(path))
    .sort((a, b) => b.score - a.score)[0];
  return hit ? { orgId: decodeURIComponent(m[1]), path, route: hit.route } : null;
}

/** 依路由轉換回應；不在清單上的原樣回傳。 */
export function adaptEnergyResponse(method: string, url: string, data: unknown): unknown {
  const hit = matchEnergyRoute(method, url);
  return hit?.route.adapt ? hit.route.adapt(data) : data;
}

// ───────────── 客戶端 ─────────────

const client = axios.create({ withCredentials: true });

/** 測試用：替換 axios 的底層 adapter。 */
export function setEnergyAdapter(adapter: AxiosRequestConfig['adapter'] | undefined) {
  client.defaults.adapter = adapter ?? axios.defaults.adapter;
}

client.interceptors.response.use(
  (res: AxiosResponse) => {
    // blob 下載不轉換
    if (res.config.responseType === 'blob' || !res.config.url) return res;
    res.data = adaptEnergyResponse(res.config.method || 'get', res.config.url, res.data);
    return res;
  },
  (error: AxiosError) => {
    const status = error.response?.status;
    if (status === 401) notifyUnauthorized(normalizeError(401, error.response?.data, error.response?.statusText));
    return Promise.reject(error);
  },
);

/** axios 相容的能源 API 客戶端。 */
export const energyHttp = {
  get: <T = any>(url: string, config?: AxiosRequestConfig) => client.get<T>(url, config),
  post: <T = any>(url: string, data?: unknown, config?: AxiosRequestConfig) => client.post<T>(url, data, config),
  patch: <T = any>(url: string, data?: unknown, config?: AxiosRequestConfig) => client.patch<T>(url, data, config),
  put: <T = any>(url: string, data?: unknown, config?: AxiosRequestConfig) => client.put<T>(url, data, config),
  delete: <T = any>(url: string, config?: AxiosRequestConfig) => client.delete<T>(url, config),
};

// ───────────── 上傳 ─────────────

export type UploadTarget =
  | { target: 'batch'; organizationId: number | string; batchId: number | string }
  | { target: 'execution'; executionId: number | string; organizationId?: number | string };

/** 原檔型別：瀏覽器給空字串時依副檔名補。 */
export const mimeOf = (file: File) => {
  if (file.type) return file.type;
  const ext = (file.name.split('.').pop() || '').toLowerCase();
  return ({
    pdf: 'application/pdf', jpg: 'image/jpeg', jpeg: 'image/jpeg', png: 'image/png', heic: 'image/heic',
  } as Record<string, string>)[ext] || 'application/octet-stream';
};

/** 直傳簽名端點有沒有上線：第一次問過就記住（整頁生命週期）。 */
let signSupport: 'unknown' | 'yes' | 'no' = 'unknown';

/** 測試用。 */
export function resetUploadSignSupport() {
  signSupport = 'unknown';
}

/** 端點不存在（還沒上線）才退回 multipart；其他錯誤（權限、檔太大）照常丟出。 */
function signUnavailable(e: unknown): boolean {
  const res = (e as AxiosError)?.response;
  if (!res) return false;
  if (res.status === 501 || res.status === 405) return true;
  if (res.status !== 404) return false;
  const code = (res.data as any)?.code;
  return !code || code === 'route_not_found' || code === 'not_implemented';
}

let putImpl: typeof fetch = (input, init) => globalThis.fetch(input, init);

/** 測試用：替換直傳 PUT 用的 fetch。 */
export function setUploadFetch(f: typeof fetch | undefined) {
  putImpl = f ?? ((input, init) => globalThis.fetch(input, init));
}

function orgOfEndpoint(endpoint: string, target: UploadTarget): OrgId {
  return ENERGY_URL.exec(endpoint)?.[1] ?? target.organizationId ?? orgOrSelected();
}

function multipart(endpoint: string, file: File, mimeType: string, target: UploadTarget, fields: Record<string, unknown>) {
  const form = new FormData();
  form.append('file', file, file.name);
  form.append('originalFilename', file.name);
  form.append('mimeType', mimeType);
  form.append('target', target.target);
  for (const [k, v] of Object.entries(fields)) {
    if (v !== undefined && v !== null) form.append(k, typeof v === 'object' ? JSON.stringify(v) : String(v));
  }
  return client.post(endpoint, form);
}

/**
 * 上傳附件到 endpoint（批次或流程單的附件端點）。
 * 有 `uploads/sign` 就走「簽名 → PUT 直傳物件儲存 → 送 JSON {stagingKey,…}」；
 * 端點不存在（404 route_not_found／501）就退回 multipart 直送。回傳原端點的 axios 回應。
 */
export async function uploadViaStorage(
  endpoint: string,
  file: File,
  target: UploadTarget,
  fields: Record<string, unknown> = {},
) {
  const mimeType = mimeOf(file);
  if (signSupport !== 'no') {
    const orgId = orgOfEndpoint(endpoint, target);
    const signBody = target.target === 'batch'
      ? { target: 'batch', batchId: Number(target.batchId) }
      : { target: 'execution', executionId: Number(target.executionId) };
    let signed: any;
    try {
      ({ data: signed } = await client.post(`${energyPrefix(orgId)}/uploads/sign`, {
        ...signBody, mimeType, size: file.size, originalFilename: file.name,
      }));
      signSupport = 'yes';
    } catch (e) {
      if (!signUnavailable(e)) throw e;
      signSupport = 'no';
    }
    if (signed) {
      const url = signed.signedUrl ?? signed.uploadUrl ?? signed.url;
      const put = await putImpl(url, {
        method: signed.method ?? 'PUT',
        headers: { 'content-type': mimeType, ...(signed.headers ?? {}) },
        body: file,
      });
      if (!put.ok) {
        throw Object.assign(new Error(`storage_upload_failed_${put.status}`), {
          response: { status: put.status, data: { code: 'storage_upload_failed', message: `storage upload failed (${put.status})` } },
        });
      }
      return client.post(endpoint, { stagingKey: signed.stagingKey, originalFilename: file.name, mimeType, ...fields });
    }
  }
  return multipart(endpoint, file, mimeType, target, fields);
}

// ───────────── 辨識服務狀態 ─────────────

/** on：可用；off：沒設定（服務未開）；down：有設定但連不上；unknown：端點未上線或讀不到。 */
export type RecognitionState = 'on' | 'off' | 'down' | 'unknown';

/** 把 `GET E/recognition/status` 的回應讀成狀態。 */
export function recognitionStateOf(data: any): RecognitionState {
  if (!data || typeof data !== 'object') return 'unknown';
  if (data.configured === false || data.enabled === false || data.status === 'not_configured') return 'off';
  if (data.available === false || data.reachable === false || data.ok === false || data.status === 'unavailable') return 'down';
  if (data.configured === true || data.available === true || data.status === 'ok') return 'on';
  return 'unknown';
}

export async function fetchRecognitionState(orgId: OrgId): Promise<RecognitionState> {
  if (!orgId) return 'unknown';
  try {
    const { data } = await energyHttp.get(`${energyPrefix(orgId)}/recognition/status`);
    return recognitionStateOf(data);
  } catch {
    return 'unknown';
  }
}

// ───────────── 初始化 ─────────────

export interface InstallDefaultsReport {
  definitions: Array<{ id: number; key: string; name: string; version: number }>;
  strategies: Array<{ id: number; key: string; name: string }>;
  departments: Array<{ id: number; key: string; name: string }>;
}

/** 替組織安裝內建流程定義、預設策略與部門（冪等）。 */
export async function installEnergyDefaults(orgId: OrgId): Promise<InstallDefaultsReport> {
  const { data } = await energyHttp.post<InstallDefaultsReport>(`${energyPrefix(orgId)}/setup/install-defaults`);
  return data;
}

// ───────────── 成員挑選器 ─────────────

/** 部門頁的人員下拉：先問能源成員清單，沒權限就退回平台成員清單；都拿不到回空陣列。 */
export async function fetchMemberOptions(orgId: OrgId): Promise<Array<{ id: number; name: string | null; email?: string | null }>> {
  if (!orgId) return [];
  try {
    const { data } = await energyHttp.get(`${energyPrefix(orgId)}/members`);
    return Array.isArray(data) ? data : [];
  } catch {
    try {
      const { data } = await energyHttp.get(`/api/v1/orgs/${orgId}/members`);
      return (unwrapItems(data) ?? []).map((m: any) => ({ id: m.userId ?? m.id, name: m.name, email: m.email }));
    } catch {
      return [];
    }
  }
}
