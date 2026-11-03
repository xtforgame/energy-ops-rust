import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '../api/http';
import type { EffectivePermissions, SessionInfo } from '../api/types';
import type { Selection } from '../platform/selection/store';
import { AuthDeps, createAuthProvider, isPublicPath } from './authProvider';

const user = { id: 1, name: 'Ann', email: 'ann@example.com', privilege: null, disabled: false, picture: null, createdAt: '' };
const session = (superUser = false): SessionInfo => ({ user, superUser });
const eff = (permissions: string[], superUser = false): EffectivePermissions => ({ superUser, privilege: null, permissions, matchedRoles: [] });
const sel = (projectId: number | null = null): Selection => ({
  organizationId: 10, organizationName: 'Acme', organizationKind: 'organization', projectId, projectName: projectId ? 'P' : null, projectIsDefault: false,
});

function setup(over: Partial<{ session: () => Promise<SessionInfo>; perms: AuthDeps['me']['permissions']; selection: Selection | null }> = {}) {
  const deps: AuthDeps = {
    auth: {
      login: vi.fn(async () => ({ user, token: 't', expiresAt: '' })),
      logout: vi.fn(async () => ({ ok: true as const })),
      session: vi.fn(over.session ?? (async () => session())),
    },
    me: { permissions: vi.fn(over.perms ?? (async () => eff(['platform:member:manage']))) },
    selection: {
      load: vi.fn(async () => ({}) as any),
      getSelection: vi.fn(() => (over.selection === undefined ? sel() : over.selection)),
      reset: vi.fn(),
    },
  };
  return { deps, ap: createAuthProvider(deps) };
}

beforeEach(() => {
  window.location.hash = '';
});

describe('checkAuth', () => {
  it('session 有效就通過', async () => {
    const { ap } = setup();
    await expect(ap.checkAuth({})).resolves.toBeUndefined();
  });

  it('session 401 → 拒絕（回登入頁）', async () => {
    const { ap } = setup({ session: async () => { throw new ApiError(401, 'unauthenticated', 'login required'); } });
    await expect(ap.checkAuth({})).rejects.toBeTruthy();
  });

  it('公開頁（接受邀請）不檢查', async () => {
    window.location.hash = '#/accept-invitation?token=abc';
    const { ap, deps } = setup({ session: async () => { throw new ApiError(401, 'unauthenticated', 'x'); } });
    await expect(ap.checkAuth({})).resolves.toBeUndefined();
    expect(deps.auth.session).not.toHaveBeenCalled();
  });
});

describe('checkError', () => {
  it('401 → 拒絕（登出），清掉選擇', async () => {
    const { ap, deps } = setup();
    await expect(ap.checkError(new ApiError(401, 'unauthenticated', 'x'))).rejects.toBeTruthy();
    expect(deps.selection.reset).toHaveBeenCalled();
  });

  it('axios 形狀的 401 也算', async () => {
    const { ap } = setup();
    await expect(ap.checkError({ response: { status: 401 } })).rejects.toBeTruthy();
  });

  it('403 → 維持登入', async () => {
    const { ap, deps } = setup();
    await expect(ap.checkError(new ApiError(403, 'permission_denied', 'x'))).resolves.toBeUndefined();
    expect(deps.selection.reset).not.toHaveBeenCalled();
  });

  it('其他錯誤（500）不登出', async () => {
    const { ap } = setup();
    await expect(ap.checkError(new ApiError(500, 'internal', 'x'))).resolves.toBeUndefined();
  });
});

describe('getPermissions', () => {
  it('超級使用者 → ["*"]，不必問權限', async () => {
    const { ap, deps } = setup({ session: async () => session(true) });
    await expect(ap.getPermissions({})).resolves.toEqual(['*']);
    expect(deps.me.permissions).not.toHaveBeenCalled();
  });

  it('只有組織 → 組織的權限', async () => {
    const { ap, deps } = setup();
    await expect(ap.getPermissions({})).resolves.toEqual(['platform:member:manage']);
    expect(deps.me.permissions).toHaveBeenCalledWith({ orgId: 10 });
  });

  it('選了專案 → 組織與專案的 union（去重）', async () => {
    const perms = vi.fn(async (q: { orgId?: number | null; projectId?: number | null }) => (q.projectId
      ? eff(['platform:member:manage', 'platform:project:manage'])
      : eff(['platform:member:manage', 'energy:flow:view'])));
    const { ap } = setup({ perms: perms as any, selection: sel(77) });
    await expect(ap.getPermissions({})).resolves.toEqual(['platform:member:manage', 'energy:flow:view', 'platform:project:manage']);
    expect(perms).toHaveBeenCalledWith({ orgId: 10, projectId: 77 });
  });

  it('後端回 superUser:true → ["*"]', async () => {
    const { ap } = setup({ perms: async () => eff(['*'], true) });
    await expect(ap.getPermissions({})).resolves.toEqual(['*']);
  });

  it('沒有選擇 → []', async () => {
    const { ap } = setup({ selection: null });
    await expect(ap.getPermissions({})).resolves.toEqual([]);
  });

  it('權限 API 出錯 → []（fail-closed）', async () => {
    const { ap } = setup({ perms: async () => { throw new ApiError(500, 'internal', 'x'); } });
    await expect(ap.getPermissions({})).resolves.toEqual([]);
  });

  it('專案權限出錯也整個 fail-closed，不只給組織那一半', async () => {
    const perms = vi.fn(async (q: { projectId?: number | null }) => {
      if (q.projectId) throw new ApiError(403, 'permission_denied', 'x');
      return eff(['platform:rbac:manage']);
    });
    const { ap } = setup({ perms: perms as any, selection: sel(5) });
    await expect(ap.getPermissions({})).resolves.toEqual([]);
  });

  it('session 失效 → []', async () => {
    const { ap } = setup({ session: async () => { throw new ApiError(401, 'unauthenticated', 'x'); } });
    await expect(ap.getPermissions({})).resolves.toEqual([]);
  });
});

describe('login / logout', () => {
  it('登入失敗 → 拋 ra.auth.sign_in_error', async () => {
    const { ap, deps } = setup();
    (deps.auth.login as any).mockRejectedValueOnce(new ApiError(401, 'unauthenticated', 'x'));
    await expect(ap.login({ email: 'a', password: 'b' })).rejects.toThrow('ra.auth.sign_in_error');
  });

  it('停用帳號有專屬訊息', async () => {
    const { ap, deps } = setup();
    (deps.auth.login as any).mockRejectedValueOnce(new ApiError(403, 'account_disabled', 'x'));
    await expect(ap.login({ email: 'a', password: 'b' })).rejects.toThrow('eo.auth.accountDisabled');
  });

  it('登入成功：用 username 欄位、重抓 session 與組織', async () => {
    const { ap, deps } = setup();
    await ap.login({ username: ' ann@example.com ', password: 'pw' });
    expect(deps.auth.login).toHaveBeenCalledWith('ann@example.com', 'pw');
    expect(deps.selection.load).toHaveBeenCalledWith(true);
  });

  it('登出：API 失敗也清本機狀態', async () => {
    const { ap, deps } = setup();
    (deps.auth.logout as any).mockRejectedValueOnce(new ApiError(401, 'unauthenticated', 'x'));
    await expect(ap.logout({})).resolves.toBeUndefined();
    expect(deps.selection.reset).toHaveBeenCalled();
  });
});

describe('isPublicPath', () => {
  it('只認 /accept-invitation', () => {
    expect(isPublicPath('#/accept-invitation?token=x')).toBe(true);
    expect(isPublicPath('#/accept-invitations')).toBe(false);
    expect(isPublicPath('#/organizationMembers')).toBe(false);
  });
});
