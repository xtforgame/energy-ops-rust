import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AxiosRequestConfig, AxiosResponse } from 'axios';
import {
  ENERGY_ROUTES, adaptEnergyResponse, attachmentDownloadUrl, energyBase, energyHttp, fetchMemberOptions,
  fetchRecognitionState, installEnergyDefaults, matchEnergyRoute, recognitionStateOf, resetUploadSignSupport,
  setEnergyAdapter, setUploadFetch, uploadViaStorage,
} from './energy';
import { onUnauthorized } from './http';
import { selectionStore } from '../platform/selection/store';

interface Call { method: string; url: string; data: unknown; headers: Record<string, unknown> }

type Reply = { status: number; data?: unknown };

/** 假後端：依「方法 路徑」回應，記下每一次呼叫。 */
function fakeBackend(handler: (method: string, url: string, body: unknown) => Reply) {
  const calls: Call[] = [];
  setEnergyAdapter(async (config: AxiosRequestConfig) => {
    const method = (config.method || 'get').toUpperCase();
    const url = config.url || '';
    let body: unknown = config.data;
    if (typeof body === 'string') {
      try { body = JSON.parse(body); } catch { /* 原樣 */ }
    }
    calls.push({ method, url, data: body, headers: { ...(config.headers as any) } });
    const r = handler(method, url, body);
    const res: AxiosResponse = {
      data: r.data, status: r.status, statusText: String(r.status), headers: {}, config: config as any,
    };
    if (r.status >= 400) {
      const err: any = new Error(`Request failed with status code ${r.status}`);
      err.response = res;
      err.config = config;
      err.isAxiosError = true;
      throw err;
    }
    return res;
  });
  return calls;
}

afterEach(() => {
  setEnergyAdapter(undefined);
  setUploadFetch(undefined);
  resetUploadSignSupport();
  vi.restoreAllMocks();
});

const SAMPLE: Record<string, string> = {
  departmentId: '3', userId: '10', executionIdOrNo: '7', executionId: '7', taskId: '11', sideEffectId: '5',
  delegationId: '4', definitionId: '1', nodeKey: 'clerk-review', attachmentId: '21', linkId: '22', billId: '31',
  batchId: '41', itemId: '51', stationId: '2', landownerId: '9',
};
const concrete = (path: string) => path.replace(/:([A-Za-z]+)/g, (_, k) => SAMPLE[k] ?? 'x');

describe('路由表：每個端點都在組織前綴下，回應轉成頁面要的形狀', () => {
  it('energyBase 是新前綴', () => {
    expect(energyBase(15)).toBe('/api/v1/orgs/15/energy');
  });

  it('清單共有契約上的全部端點（無重複）', () => {
    const keys = ENERGY_ROUTES.map((r) => `${r.method} ${r.path}`);
    expect(new Set(keys).size).toBe(keys.length);
    // 11/19: master already had 108 routes; application export adds one.
    expect(keys.length).toBe(113);
  });

  for (const route of ENERGY_ROUTES) {
    const path = concrete(route.path);
    it(`${route.method} E${route.path}`, async () => {
      const backend = route.adapt ? { items: [{ id: 1, userId: 1 }], total: 1 } : { ok: true, id: 1 };
      const calls = fakeBackend(() => ({ status: 200, data: backend }));
      const url = `${energyBase(15)}${path}`;
      const m = route.method.toLowerCase() as 'get' | 'post' | 'patch' | 'put' | 'delete';
      const res = m === 'get' || m === 'delete'
        ? await energyHttp[m](url)
        : await energyHttp[m](url, { a: 1 });
      expect(calls).toHaveLength(1);
      expect(calls[0].method).toBe(route.method);
      expect(calls[0].url).toBe(`/api/v1/orgs/15/energy${path}`);
      // 比對到的是自己這一條（不是別條較寬的樣式）
      expect(matchEnergyRoute(route.method, url)?.route).toBe(route);
      expect(res.data).toEqual(route.adapt ? route.adapt(backend) : backend);
    });
  }
});

describe('個別轉換', () => {
  const E = '/api/v1/orgs/15/energy';

  it('清單 {items} → 陣列；已是陣列原樣', () => {
    expect(adaptEnergyResponse('get', `${E}/departments`, { items: [{ id: 1 }] })).toEqual([{ id: 1 }]);
    expect(adaptEnergyResponse('get', `${E}/departments`, [{ id: 2 }])).toEqual([{ id: 2 }]);
    expect(adaptEnergyResponse('get', `${E}/audit-events?limit=300&outcome=denied`, { items: [] })).toEqual([]);
    expect(adaptEnergyResponse('get', `${E}/executions/7/lineage`, { items: [{ id: 3 }] })).toEqual([{ id: 3 }]);
  });

  it('流程單清單：items 另外掛成 rows，total 保留', () => {
    const out: any = adaptEnergyResponse('get', `${E}/executions?scope=all&pageSize=200`, {
      items: [{ id: 1 }], total: 1, tab: 'running', scope: 'all', page: 1, pageSize: 200,
    });
    expect(out.rows).toEqual([{ id: 1 }]);
    expect(out.total).toBe(1);
  });

  it('流程單詳情（同樣以 /executions/ 開頭）不被當成清單', () => {
    const detail = { id: 7, tasks: [] };
    expect(adaptEnergyResponse('get', `${E}/executions/7`, detail)).toBe(detail);
    expect(adaptEnergyResponse('get', `${E}/executions/BA-2026-0001`, detail)).toBe(detail);
  });

  it('成員清單：補 id = userId（部門頁挑選器讀 id）', () => {
    expect(adaptEnergyResponse('get', `${E}/members`, {
      items: [{ userId: 10, name: '乙', email: 'b@x', energyRoles: [], departments: [] }],
    })).toEqual([{ id: 10, userId: 10, name: '乙', email: 'b@x', energyRoles: [], departments: [] }]);
  });

  it('建議座標轉成數字（頁面直接 toFixed）', () => {
    const out: any = adaptEnergyResponse('get', `${E}/stations/2/geocode`, {
      usable: true, result: { latitude: '22.5', longitude: '120.25', precision: 'rooftop' },
    });
    expect(out.result.latitude).toBe(22.5);
    expect(out.result.longitude).toBe(120.25);
    const none = { usable: false, reason: 'not_found', result: null };
    expect(adaptEnergyResponse('get', `${E}/stations/2/geocode`, none)).toBe(none);
  });

  it('靜態段優先：/bills/unmatched 不被當成 /bills/:billId', () => {
    expect(matchEnergyRoute('GET', `${E}/bills/unmatched`)?.route.path).toBe('/bills/unmatched');
    expect(matchEnergyRoute('GET', `${E}/bills/31`)?.route.path).toBe('/bills/:billId');
    expect(matchEnergyRoute('GET', `${E}/executions/7/attachments`)?.route.path).toBe('/executions/:executionId/attachments');
  });

  it('非能源路徑、清單外的路徑原樣', () => {
    const d = { items: [1] };
    expect(adaptEnergyResponse('get', '/api/v1/orgs/15/members', d)).toBe(d);
    expect(adaptEnergyResponse('get', `${E}/no-such-thing`, d)).toBe(d);
    expect(matchEnergyRoute('GET', '/api/energy-ops/organizations/15/departments')).toBeNull();
  });

  it('blob 下載不經轉換', async () => {
    const blob = new Blob(['%PDF']);
    fakeBackend(() => ({ status: 200, data: blob }));
    const res = await energyHttp.get(attachmentDownloadUrl(21, 15), { responseType: 'blob' });
    expect(res.data).toBe(blob);
  });
});

describe('附件網址', () => {
  it('帶組織時用該組織', () => {
    expect(attachmentDownloadUrl(21, 15)).toBe('/api/v1/orgs/15/energy/attachments/21/download');
  });

  it('沒帶組織時用目前選定的組織', () => {
    vi.spyOn(selectionStore, 'getSelection').mockReturnValue({
      organizationId: 42, organizationName: 'x', organizationKind: 'team', projectId: null, projectName: null, projectIsDefault: false,
    });
    expect(attachmentDownloadUrl(5)).toBe('/api/v1/orgs/42/energy/attachments/5/download');
  });
});

describe('401 接到登入流程', () => {
  it('能源呼叫 401 → 通知 onUnauthorized；403 不會；錯誤照樣丟給頁面', async () => {
    const seen: number[] = [];
    const off = onUnauthorized((e) => seen.push(e.status));
    fakeBackend((_m, url) => (url.endsWith('/workbench')
      ? { status: 401, data: { code: 'unauthenticated', message: 'login required' } }
      : { status: 403, data: { code: 'permission_denied', message: 'no' } }));
    await expect(energyHttp.get(`${energyBase(15)}/workbench`)).rejects.toMatchObject({ response: { status: 401 } });
    await expect(energyHttp.get(`${energyBase(15)}/dashboard`)).rejects.toMatchObject({ response: { status: 403 } });
    off();
    expect(seen).toEqual([401]);
  });
});

describe('上傳：有簽名端點走直傳，沒有就 multipart', () => {
  const file = () => new File(['hello'], 'bill.pdf', { type: '' });
  const batchEndpoint = `${energyBase(15)}/ingest-batches/41/attachments`;
  const execEndpoint = `${energyBase(15)}/executions/7/attachments`;
  let puts: Array<{ url: string; init: RequestInit }>;

  beforeEach(() => {
    puts = [];
    setUploadFetch((async (url: any, init: any) => {
      puts.push({ url: String(url), init });
      return new Response(null, { status: 200 });
    }) as typeof fetch);
  });

  it('sign 可用：簽名 → PUT 到簽名網址 → 原端點送 JSON {stagingKey, originalFilename, mimeType, ...fields}', async () => {
    const calls = fakeBackend((_m, url) => (url.endsWith('/uploads/sign')
      ? { status: 200, data: { stagingKey: 'stg/1/abc', signedUrl: 'http://s3.local/put?sig=1' } }
      : { status: 200, data: { id: 99 } }));
    const res = await uploadViaStorage(execEndpoint, file(), { target: 'execution', executionId: 7 }, { role: 'added' });
    expect(res.data).toEqual({ id: 99 });
    expect(calls.map((c) => `${c.method} ${c.url}`)).toEqual([
      'POST /api/v1/orgs/15/energy/uploads/sign',
      `POST ${execEndpoint}`,
    ]);
    expect(calls[0].data).toEqual({
      target: 'execution', executionId: 7, mimeType: 'application/pdf', size: 5, originalFilename: 'bill.pdf',
    });
    expect(puts).toHaveLength(1);
    expect(puts[0].url).toBe('http://s3.local/put?sig=1');
    expect(puts[0].init.method).toBe('PUT');
    expect((puts[0].init.headers as any)['content-type']).toBe('application/pdf');
    expect(calls[1].data).toEqual({ stagingKey: 'stg/1/abc', originalFilename: 'bill.pdf', mimeType: 'application/pdf', role: 'added' });
  });

  it('批次的 sign body 帶 batchId', async () => {
    const calls = fakeBackend((_m, url) => (url.endsWith('/uploads/sign')
      ? { status: 200, data: { stagingKey: 'k', signedUrl: 'http://s3/x' } }
      : { status: 200, data: {} }));
    await uploadViaStorage(batchEndpoint, file(), { target: 'batch', organizationId: 15, batchId: 41 });
    expect(calls[0].data).toMatchObject({ target: 'batch', batchId: 41 });
  });

  it('sign 404 route_not_found → multipart 直送，之後不再問 sign', async () => {
    const calls = fakeBackend((_m, url) => (url.endsWith('/uploads/sign')
      ? { status: 404, data: { code: 'route_not_found', message: 'no such API route' } }
      : { status: 200, data: { id: 1 } }));
    await uploadViaStorage(batchEndpoint, file(), { target: 'batch', organizationId: 15, batchId: 41 });
    await uploadViaStorage(batchEndpoint, file(), { target: 'batch', organizationId: 15, batchId: 41 });
    expect(calls.map((c) => `${c.method} ${c.url}`)).toEqual([
      'POST /api/v1/orgs/15/energy/uploads/sign',
      `POST ${batchEndpoint}`,
      `POST ${batchEndpoint}`,
    ]);
    const form = calls[1].data as FormData;
    expect(form).toBeInstanceOf(FormData);
    expect((form.get('file') as File).name).toBe('bill.pdf');
    expect(form.get('originalFilename')).toBe('bill.pdf');
    expect(form.get('mimeType')).toBe('application/pdf');
    expect(form.get('target')).toBe('batch');
    expect(puts).toHaveLength(0);
  });

  it('sign 501 → multipart', async () => {
    const calls = fakeBackend((_m, url) => (url.endsWith('/uploads/sign')
      ? { status: 501, data: { code: 'not_implemented', message: 'presign off' } }
      : { status: 200, data: {} }));
    await uploadViaStorage(execEndpoint, file(), { target: 'execution', executionId: 7 }, { role: 'source' });
    expect(calls[1].data).toBeInstanceOf(FormData);
    expect((calls[1].data as FormData).get('role')).toBe('source');
  });

  it('sign 回業務錯誤（403、404 batch_not_found、413）→ 照樣丟出，不偷偷退回 multipart', async () => {
    for (const [status, code] of [[403, 'permission_denied'], [404, 'batch_not_found'], [413, 'file_too_large']] as const) {
      resetUploadSignSupport();
      const calls = fakeBackend(() => ({ status, data: { code, message: code } }));
      await expect(uploadViaStorage(batchEndpoint, file(), { target: 'batch', organizationId: 15, batchId: 41 }))
        .rejects.toMatchObject({ response: { status } });
      expect(calls).toHaveLength(1);
    }
  });

  it('PUT 到物件儲存失敗 → storage_upload_failed，不送原端點', async () => {
    setUploadFetch((async () => new Response(null, { status: 403 })) as unknown as typeof fetch);
    const calls = fakeBackend(() => ({ status: 200, data: { stagingKey: 'k', signedUrl: 'http://s3/x' } }));
    await expect(uploadViaStorage(execEndpoint, file(), { target: 'execution', executionId: 7 }))
      .rejects.toMatchObject({ response: { status: 403, data: { code: 'storage_upload_failed' } } });
    expect(calls).toHaveLength(1);
  });
});

describe('辨識服務狀態', () => {
  it('回應讀成狀態', () => {
    expect(recognitionStateOf({ configured: false })).toBe('off');
    expect(recognitionStateOf({ enabled: false })).toBe('off');
    expect(recognitionStateOf({ status: 'not_configured' })).toBe('off');
    expect(recognitionStateOf({ configured: true, available: false })).toBe('down');
    expect(recognitionStateOf({ configured: true, available: true })).toBe('on');
    expect(recognitionStateOf(null)).toBe('unknown');
  });

  it('打 E/recognition/status；端點未上線（404）當 unknown，不顯示服務未開', async () => {
    const calls = fakeBackend(() => ({ status: 200, data: { configured: false } }));
    expect(await fetchRecognitionState(15)).toBe('off');
    expect(calls[0].url).toBe('/api/v1/orgs/15/energy/recognition/status');
    fakeBackend(() => ({ status: 404, data: { code: 'route_not_found', message: 'x' } }));
    expect(await fetchRecognitionState(15)).toBe('unknown');
  });
});

describe('初始化能源設定、成員挑選器', () => {
  it('install-defaults 打對路徑並回報告', async () => {
    const report = { definitions: [{ id: 1, key: 'bill-approval', name: 'x', version: 1 }], strategies: [], departments: [] };
    const calls = fakeBackend(() => ({ status: 200, data: report }));
    expect(await installEnergyDefaults(15)).toEqual(report);
    expect(`${calls[0].method} ${calls[0].url}`).toBe('POST /api/v1/orgs/15/energy/setup/install-defaults');
  });

  it('成員挑選器先用 E/members', async () => {
    const calls = fakeBackend(() => ({ status: 200, data: { items: [{ userId: 10, name: '乙', email: 'b@x' }] } }));
    expect(await fetchMemberOptions(15)).toEqual([{ id: 10, userId: 10, name: '乙', email: 'b@x' }]);
    expect(calls.map((c) => c.url)).toEqual(['/api/v1/orgs/15/energy/members']);
  });

  it('E/members 沒權限 → 退回平台成員清單', async () => {
    const calls = fakeBackend((_m, url) => (url.includes('/energy/')
      ? { status: 403, data: { code: 'permission_denied', message: 'no' } }
      : { status: 200, data: { items: [{ userId: 3, name: '丙', email: 'c@x', roles: [] }] } }));
    expect(await fetchMemberOptions(15)).toEqual([{ id: 3, name: '丙', email: 'c@x' }]);
    expect(calls.map((c) => c.url)).toEqual(['/api/v1/orgs/15/energy/members', '/api/v1/orgs/15/members']);
  });
});

describe('頁面不再打舊路徑', () => {
  it('src/admin 底下沒有 /api/energy-ops、/api/tenant-console', () => {
    const files = import.meta.glob('../admin/**/*.{ts,tsx}', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
    expect(Object.keys(files).length).toBeGreaterThan(20);
    const offenders = Object.entries(files)
      .filter(([, src]) => /['"`]\/api\/(energy-ops|tenant-console)\//.test(src))
      .map(([f]) => f);
    expect(offenders).toEqual([]);
  });
});
