// 平台 API 的唯一出入口：同網域、帶 cookie session；所有錯誤一律正規化成 ApiError。

/** 後端錯誤的固定形狀 `{code, message, details?}`，加上 HTTP 狀態碼。 */
export class ApiError extends Error {
  readonly status: number;

  readonly code: string;

  readonly details?: unknown;

  constructor(status: number, code: string, message: string, details?: unknown) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
    this.details = details;
  }

  /** 權限不足時後端列出的 permission（任一即可）。 */
  get requiredPermissions(): string[] {
    const d = this.details as { required?: unknown } | undefined;
    return Array.isArray(d?.required) ? (d!.required as string[]) : [];
  }
}

export const isApiError = (e: unknown): e is ApiError => e instanceof ApiError;

/** 把任意回應內容轉成 ApiError：後端形狀優先，其次純文字，最後退回 http_<status>。 */
export function normalizeError(status: number, body: unknown, statusText = ''): ApiError {
  if (body && typeof body === 'object') {
    const b = body as Record<string, unknown>;
    if (typeof b.code === 'string') {
      const message = typeof b.message === 'string' && b.message ? b.message : b.code;
      return new ApiError(status, b.code, message, b.details);
    }
  }
  if (typeof body === 'string' && body.trim()) {
    const text = body.trim();
    if (text.startsWith('{')) {
      try {
        return normalizeError(status, JSON.parse(text), statusText);
      } catch {
        // 不是 JSON，當純文字處理
      }
    }
    return new ApiError(status, `http_${status}`, text.slice(0, 500));
  }
  return new ApiError(status, `http_${status}`, statusText || `HTTP ${status}`);
}

/** 給畫面顯示用：ApiError 取 message，其他錯誤盡量取得可讀字串。 */
export function errorMessage(e: unknown): string {
  if (isApiError(e)) return e.message || e.code;
  if (e instanceof Error) return e.message;
  return String(e ?? 'error');
}

export type Query = Record<string, string | number | boolean | null | undefined>;

export interface RequestOptions {
  query?: Query;
  body?: unknown;
  signal?: AbortSignal;
}

let fetchImpl: typeof fetch = (input, init) => globalThis.fetch(input, init);

/** 測試用：替換底層 fetch。 */
export function setFetch(f: typeof fetch) {
  fetchImpl = f;
}

type UnauthorizedListener = (e: ApiError) => void;
const unauthorizedListeners = new Set<UnauthorizedListener>();

/** 任何呼叫拿到 401 時通知（authProvider 之外的頁面也能觸發回登入頁）。 */
export function onUnauthorized(fn: UnauthorizedListener): () => void {
  unauthorizedListeners.add(fn);
  return () => {
    unauthorizedListeners.delete(fn);
  };
}

/** 其他客戶端（能源的 axios）拿到 401 時也走同一組監聽者。 */
export function notifyUnauthorized(e: ApiError) {
  unauthorizedListeners.forEach((fn) => fn(e));
}

export function buildUrl(path: string, query?: Query): string {
  if (!query) return path;
  const qs = new URLSearchParams();
  for (const [k, v] of Object.entries(query)) {
    if (v !== undefined && v !== null && v !== '') qs.set(k, String(v));
  }
  const s = qs.toString();
  return s ? `${path}${path.includes('?') ? '&' : '?'}${s}` : path;
}

async function readBody(res: Response): Promise<unknown> {
  if (res.status === 204) return undefined;
  const text = await res.text();
  if (!text) return undefined;
  const ct = res.headers.get('content-type') || '';
  if (ct.includes('json') || text.startsWith('{') || text.startsWith('[')) {
    try {
      return JSON.parse(text);
    } catch {
      return text;
    }
  }
  return text;
}

export async function request<T>(method: string, path: string, opts: RequestOptions = {}): Promise<T> {
  const headers: Record<string, string> = { accept: 'application/json' };
  let body: BodyInit | undefined;
  if (opts.body !== undefined) {
    headers['content-type'] = 'application/json';
    body = JSON.stringify(opts.body);
  }
  let res: Response;
  try {
    res = await fetchImpl(buildUrl(path, opts.query), {
      method,
      headers,
      body,
      credentials: 'include',
      signal: opts.signal,
    });
  } catch (e) {
    if ((e as Error)?.name === 'AbortError') throw e;
    throw new ApiError(0, 'network_error', (e as Error)?.message || 'network error');
  }
  const data = await readBody(res);
  if (!res.ok) {
    const err = normalizeError(res.status, data, res.statusText);
    if (err.status === 401) notifyUnauthorized(err);
    throw err;
  }
  return data as T;
}

export const http = {
  get: <T>(path: string, query?: Query, signal?: AbortSignal) => request<T>('GET', path, { query, signal }),
  post: <T>(path: string, body?: unknown) => request<T>('POST', path, { body: body ?? {} }),
  patch: <T>(path: string, body?: unknown) => request<T>('PATCH', path, { body: body ?? {} }),
  delete: <T = void>(path: string) => request<T>('DELETE', path),
};
