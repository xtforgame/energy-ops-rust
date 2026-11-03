import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiError, buildUrl, http, normalizeError, onUnauthorized, request, setFetch } from './http';
import { orgRbacApi, meApi } from './platform';

const json = (status: number, body: unknown, headers: Record<string, string> = { 'content-type': 'application/json' }) =>
  new Response(body === undefined ? null : JSON.stringify(body), { status, headers });

afterEach(() => setFetch((i, init) => globalThis.fetch(i, init)));

describe('normalizeError', () => {
  it('取後端的 {code, message, details}', () => {
    const e = normalizeError(403, { code: 'permission_denied', message: 'permission denied', details: { required: ['platform:rbac:manage'] } });
    expect(e).toBeInstanceOf(ApiError);
    expect(e.status).toBe(403);
    expect(e.code).toBe('permission_denied');
    expect(e.message).toBe('permission denied');
    expect(e.requiredPermissions).toEqual(['platform:rbac:manage']);
  });

  it('message 缺時退回 code', () => {
    expect(normalizeError(400, { code: 'invalid_body' }).message).toBe('invalid_body');
  });

  it('JSON 字串也會解開', () => {
    const e = normalizeError(409, '{"code":"conflict","message":"unique constraint violated"}');
    expect(e.code).toBe('conflict');
  });

  it('純文字回應 → http_<status>，訊息保留', () => {
    const e = normalizeError(502, 'Bad Gateway from proxy');
    expect(e.code).toBe('http_502');
    expect(e.message).toBe('Bad Gateway from proxy');
  });

  it('空回應 → http_<status> 與 statusText', () => {
    const e = normalizeError(500, undefined, 'Internal Server Error');
    expect(e.code).toBe('http_500');
    expect(e.message).toBe('Internal Server Error');
    expect(e.requiredPermissions).toEqual([]);
  });

  it('沒有 code 的物件不當成後端錯誤', () => {
    expect(normalizeError(400, { error: 'x' }).code).toBe('http_400');
  });
});

describe('request', () => {
  it('帶 cookie、JSON body、query；回傳解析後的資料', async () => {
    const fetchMock = vi.fn(async () => json(200, { items: [1] }));
    setFetch(fetchMock as any);
    const r = await request<{ items: number[] }>('POST', '/api/v1/x', { body: { a: 1 }, query: { q: 'z', empty: '', n: null } });
    expect(r.items).toEqual([1]);
    const [url, init] = fetchMock.mock.calls[0] as any;
    expect(url).toBe('/api/v1/x?q=z');
    expect(init.credentials).toBe('include');
    expect(init.headers['content-type']).toBe('application/json');
    expect(init.body).toBe('{"a":1}');
  });

  it('非 2xx 拋出正規化後的 ApiError', async () => {
    setFetch((async () => json(404, { code: 'project_not_found', message: 'project not found' })) as any);
    await expect(http.get('/api/v1/projects/9')).rejects.toMatchObject({ status: 404, code: 'project_not_found' });
  });

  it('204 回 undefined', async () => {
    setFetch((async () => new Response(null, { status: 204 })) as any);
    await expect(http.delete('/api/v1/orgs/1/members/2')).resolves.toBeUndefined();
  });

  it('網路錯誤 → status 0、network_error', async () => {
    setFetch((async () => { throw new TypeError('Failed to fetch'); }) as any);
    await expect(http.get('/api/v1/auth/session')).rejects.toMatchObject({ status: 0, code: 'network_error' });
  });

  it('401 通知 onUnauthorized 的監聽者，403 不會', async () => {
    const seen: number[] = [];
    const off = onUnauthorized((e) => seen.push(e.status));
    setFetch((async () => json(401, { code: 'unauthenticated', message: 'login required' })) as any);
    await expect(http.get('/a')).rejects.toBeInstanceOf(ApiError);
    setFetch((async () => json(403, { code: 'permission_denied', message: 'x' })) as any);
    await expect(http.get('/a')).rejects.toBeInstanceOf(ApiError);
    off();
    expect(seen).toEqual([401]);
  });

  it('端點路徑與 query 名稱照後端（camelCase）', async () => {
    const fetchMock = vi.fn(async () => json(200, { items: [] }));
    setFetch(fetchMock as any);
    await meApi.permissions({ orgId: 3, projectId: 7 });
    await orgRbacApi.grants(3, { includeRevoked: true, userId: 5 });
    await orgRbacApi.revokeGrant(3, 11, '離職');
    const calls = fetchMock.mock.calls as any[];
    expect(calls[0][0]).toBe('/api/v1/me/permissions?orgId=3&projectId=7');
    expect(calls[1][0]).toBe('/api/v1/orgs/3/rbac/grants?includeRevoked=true&userId=5');
    expect(calls[2][0]).toBe('/api/v1/orgs/3/rbac/grants/11/revoke');
    expect(JSON.parse(calls[2][1].body)).toEqual({ reason: '離職' });
  });
});

describe('buildUrl', () => {
  it('已有 ? 時用 & 接', () => {
    expect(buildUrl('/a?x=1', { y: 2 })).toBe('/a?x=1&y=2');
    expect(buildUrl('/a', {})).toBe('/a');
  });
});
