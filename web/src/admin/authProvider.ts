// react-admin 的 authProvider，接新後端的 cookie session（eo_session，httpOnly）。
// - checkAuth：問 /api/v1/auth/session，401 就回登入頁。
// - checkError：401 → 登出；403 → 只是權限不足，維持登入。
// - getPermissions：目前選擇的組織（＋專案）的有效權限 union；超級使用者 ['*']；出錯一律 []（fail-closed）。
import type { AuthProvider } from 'react-admin';
import { authApi, meApi } from '../api/platform';
import { isApiError } from '../api/http';
import type { SessionInfo } from '../api/types';
import { SelectionStore, selectionStore } from '../platform/selection/store';
import { unionPermissions } from '../platform/permissions';

export interface AuthDeps {
  auth: Pick<typeof authApi, 'login' | 'logout' | 'session'>;
  me: Pick<typeof meApi, 'permissions'>;
  selection: Pick<SelectionStore, 'load' | 'getSelection' | 'reset'>;
}

const statusOf = (e: any): number | undefined => e?.status ?? e?.response?.status;

/** 公開路由（不需登入）：接受邀請頁。 */
export const PUBLIC_PATHS = ['/accept-invitation'];

export function isPublicPath(hash: string = typeof window !== 'undefined' ? window.location.hash : ''): boolean {
  const path = hash.replace(/^#/, '').split('?')[0];
  return PUBLIC_PATHS.some((p) => path === p || path.startsWith(`${p}/`));
}

export function createAuthProvider(deps: AuthDeps): AuthProvider {
  let session: SessionInfo | null = null;
  let sessionPromise: Promise<SessionInfo> | null = null;

  const loadSession = (force = false): Promise<SessionInfo> => {
    if (session && !force) return Promise.resolve(session);
    if (!sessionPromise || force) {
      sessionPromise = deps.auth
        .session()
        .then((s) => {
          session = s;
          return s;
        })
        .finally(() => {
          sessionPromise = null;
        });
    }
    return sessionPromise;
  };

  const clear = () => {
    session = null;
    sessionPromise = null;
    deps.selection.reset();
  };

  return {
    login: async ({ username, email, password }) => {
      try {
        await deps.auth.login(String(email ?? username ?? '').trim(), String(password ?? ''));
      } catch (e) {
        clear();
        const reason = isApiError(e) ? e.code : '';
        throw new Error(reason === 'account_disabled' ? 'eo.auth.accountDisabled' : 'ra.auth.sign_in_error');
      }
      await loadSession(true);
      await deps.selection.load(true).catch(() => undefined);
    },
    logout: async () => {
      try {
        await deps.auth.logout();
      } catch {
        // session 已失效也照樣清本機狀態
      }
      clear();
    },
    checkAuth: async () => {
      // 公開頁（接受邀請）不擋：未註冊者要能看到邀請內容
      if (isPublicPath()) return;
      try {
        await loadSession();
      } catch (e) {
        session = null;
        throw new Error('ra.auth.auth_check_error');
      }
    },
    checkError: async (error: any) => {
      const status = statusOf(error);
      if (status === 401) {
        clear();
        throw error;
      }
      // 403：權限不足不等於未登入，留在原頁
    },
    getPermissions: async () => {
      try {
        const s = session ?? (await loadSession());
        if (s.superUser) return ['*'];
        await deps.selection.load();
        const sel = deps.selection.getSelection();
        if (!sel) return [];
        const org = await deps.me.permissions({ orgId: sel.organizationId });
        if (org.superUser) return ['*'];
        let project: string[] = [];
        if (sel.projectId) {
          const p = await deps.me.permissions({ orgId: sel.organizationId, projectId: sel.projectId });
          if (p.superUser) return ['*'];
          project = p.permissions;
        }
        return unionPermissions(org.permissions, project);
      } catch {
        return [];
      }
    },
    getIdentity: async () => {
      // 每次重抓：個人資料改名後頂欄要跟著變
      const s = await loadSession(true);
      return {
        id: s.user.id,
        fullName: s.user.name || s.user.email || '',
        avatar: s.user.picture || '/images/gopher.svg',
        email: s.user.email,
        superUser: s.superUser,
      };
    },
  };
}

const authProvider = createAuthProvider({ auth: authApi, me: meApi, selection: selectionStore });

export default authProvider;
