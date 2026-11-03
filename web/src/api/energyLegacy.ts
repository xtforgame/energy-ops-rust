// 能源頁面的「舊」API 呼叫全部經過這裡（第一階段：路徑維持 App 5 的 /api/energy-ops/*、尚未對到新後端）。
// 第二階段只改這個檔：energyBase／rewriteLegacyPath／attachmentDownloadUrl／uploadViaStorage
// 換成 /api/v1/orgs/{orgId}/energy/... 即可，呼叫端不必動。
// 呼叫端的介面刻意與 axios 相同（get/post/patch/delete，回 { data }），舊程式只要把 `axios.` 換成 `energyHttp.`。
import axios, { AxiosRequestConfig } from 'axios';

type OrgId = number | string | null | undefined;

/** 舊路徑 → 新路徑的對照點。第一階段原樣回傳。 */
export function rewriteLegacyPath(url: string): string {
  return url;
}

/** 某組織的能源 API 根路徑（舊：/api/energy-ops/organizations/{orgId}）。 */
export function energyBase(orgId: OrgId): string {
  return `/api/energy-ops/organizations/${orgId}`;
}

/** 不分組織的能源 API 根路徑（附件、流程單附件）。 */
export const ENERGY_ROOT = '/api/energy-ops';

/** 附件下載網址（<a href> 直接開，靠同網域 cookie）。 */
export function attachmentDownloadUrl(attachmentId: number | string): string {
  return rewriteLegacyPath(`${ENERGY_ROOT}/attachments/${attachmentId}/download`);
}

/** 舊成員 API（部門頁要成員清單；第二階段改用平台的 /api/v1/orgs/{orgId}/members）。 */
export function legacyTenantConsoleBase(orgId: OrgId): string {
  return `/api/tenant-console/organizations/${orgId}`;
}

const client = axios.create({ withCredentials: true });

client.interceptors.request.use((config) => {
  if (config.url) config.url = rewriteLegacyPath(config.url);
  return config;
});

/** axios 相容的能源 API 客戶端。 */
export const energyHttp = {
  get: <T = any>(url: string, config?: AxiosRequestConfig) => client.get<T>(url, config),
  post: <T = any>(url: string, data?: unknown, config?: AxiosRequestConfig) => client.post<T>(url, data, config),
  patch: <T = any>(url: string, data?: unknown, config?: AxiosRequestConfig) => client.patch<T>(url, data, config),
  put: <T = any>(url: string, data?: unknown, config?: AxiosRequestConfig) => client.put<T>(url, data, config),
  delete: <T = any>(url: string, config?: AxiosRequestConfig) => client.delete<T>(url, config),
};

export type UploadTarget =
  | { target: 'batch'; organizationId: number | string; batchId: number | string }
  | { target: 'execution'; executionId: number | string };

/** 原檔型別：瀏覽器給空字串時依副檔名補。 */
const mimeOf = (file: File) => {
  if (file.type) return file.type;
  const ext = (file.name.split('.').pop() || '').toLowerCase();
  return ({
    pdf: 'application/pdf', jpg: 'image/jpeg', jpeg: 'image/jpeg', png: 'image/png', heic: 'image/heic',
  } as Record<string, string>)[ext] || 'application/octet-stream';
};

/**
 * 上傳附件。App 5 走「簽名網址直傳 Supabase Storage → 再送 JSON」；新後端有自己的物件儲存，
 * 第一階段先改回 multipart 直送原端點（欄位名 file），第二階段依新 API 調整。
 */
export async function uploadViaStorage(
  endpoint: string,
  file: File,
  target: UploadTarget,
  fields: Record<string, unknown> = {},
) {
  const form = new FormData();
  form.append('file', file, file.name);
  form.append('originalFilename', file.name);
  form.append('mimeType', mimeOf(file));
  form.append('target', target.target);
  for (const [k, v] of Object.entries(fields)) {
    if (v !== undefined && v !== null) form.append(k, typeof v === 'object' ? JSON.stringify(v) : String(v));
  }
  return client.post(endpoint, form);
}
