// 前端的權限閘（只決定選單與路由要不要出現；後端才是真相）。
// permissions 是 authProvider.getPermissions 在目前選擇的組織（＋專案）下的 union；超級使用者是 ['*']。

export const PLATFORM_RBAC_MANAGE = 'platform:rbac:manage';
export const PLATFORM_ORG_MANAGE = 'platform:org:manage';
export const PLATFORM_MEMBER_MANAGE = 'platform:member:manage';
export const PLATFORM_PROJECT_MANAGE = 'platform:project:manage';
/** 只有超級使用者（['*']）會通過的哨兵；不是後端的 permission。 */
export const SUPER_USER_ONLY = '__super_user__';

const RBAC = PLATFORM_RBAC_MANAGE;

/** resource → 需要的 permission（任一即可）。沒列到的 resource 一律顯示。 */
export const RESOURCE_PERMISSION_MAP: Record<string, string[]> = {
  // ── 平台 ──
  organizationMembers: [PLATFORM_MEMBER_MANAGE, 'energy:member:manage'],
  organizationInvitations: [PLATFORM_MEMBER_MANAGE],
  organizationSettings: [PLATFORM_ORG_MANAGE],
  projects: [PLATFORM_PROJECT_MANAGE],
  projectMembers: [PLATFORM_MEMBER_MANAGE],
  rbacRoles: [RBAC],
  rbacPermissions: [RBAC],
  rbacGrants: [RBAC],
  adminUsers: [SUPER_USER_ONLY],
  adminRbac: [SUPER_USER_ONLY],
  // ── 能源（沿用 App 5 的閘，ryxis:rbac:manage 換成 platform:rbac:manage）──
  departments: [RBAC, 'energy:department:manage'],
  energyIngestConsole: [RBAC, 'energy:flow:act', 'energy:flow:view', 'energy:flow:view-all'],
  energyMyWorkConsole: [RBAC, 'energy:flow:view', 'energy:flow:view-all'],
  energyBillConsole: [RBAC, 'energy:flow:view', 'energy:flow:view-all'],
  // 路由比選單寬：參與者要能開自己的流程單詳情（選單另見 MENU_PERMISSION_MAP）。
  energyExecutionConsole: [RBAC, 'energy:flow:view', 'energy:flow:view-all'],
  energyLandownerConsole: [RBAC, 'energy:flow:view', 'energy:flow:view-all', 'energy:master:manage'],
  energyStationConsole: [RBAC, 'energy:flow:view', 'energy:flow:view-all', 'energy:master:manage'],
  energyContractConsole: [RBAC, 'energy:flow:view', 'energy:flow:view-all', 'energy:master:manage'],
  energyWorkflowConsole: [RBAC, 'energy:flow:view', 'energy:flow:view-all'],
  energyAuditConsole: [RBAC, 'energy:audit:view'],
  energyDashboardConsole: [RBAC, 'energy:flow:view', 'energy:flow:view-all'],
  energyCalendarConsole: [RBAC, 'energy:flow:view', 'energy:flow:view-all'],
};

/** 選單專用、比路由嚴的閘。 */
export const MENU_PERMISSION_MAP: Record<string, string[]> = {
  energyExecutionConsole: [RBAC, 'energy:flow:view-all'],
};

export const isSuperUser = (permissions?: string[] | null) => !!permissions?.includes('*');

/** 持有其中任一個 permission（超級使用者一律通過）。 */
export function hasAnyPermission(permissions: string[] | null | undefined, required: string[]): boolean {
  if (!permissions) return false;
  if (permissions.includes('*')) return true;
  return required.some((p) => p !== SUPER_USER_ONLY && permissions.includes(p));
}

function gate(map: Record<string, string[]>, name: string, permissions?: string[] | null): boolean | undefined {
  if (permissions === undefined || permissions === null) return undefined;
  if (permissions.includes('*')) return true;
  const required = map[name];
  if (!required) return undefined;
  return hasAnyPermission(permissions, required);
}

/**
 * 能不能進這個 resource。
 * permissions 尚未載入（undefined/null）→ 先顯示（避免閃爍）；已載入的 [] → 依表封鎖。
 */
export function canAccessResource(name: string, permissions?: string[] | null): boolean {
  return gate(RESOURCE_PERMISSION_MAP, name, permissions) ?? true;
}

/** 選單上看不看得到：預設等於 canAccessResource，MENU_PERMISSION_MAP 列到的走較嚴的閘。 */
export function canSeeInMenu(name: string, permissions?: string[] | null): boolean {
  if (permissions === undefined || permissions === null) return true;
  if (MENU_PERMISSION_MAP[name]) return gate(MENU_PERMISSION_MAP, name, permissions) ?? true;
  return canAccessResource(name, permissions);
}

/** 多個 scope 的有效權限合併（去重、保序）；任一為 ['*'] 就回 ['*']。 */
export function unionPermissions(...lists: Array<string[] | null | undefined>): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const list of lists) {
    for (const p of list || []) {
      if (p === '*') return ['*'];
      if (!seen.has(p)) {
        seen.add(p);
        out.push(p);
      }
    }
  }
  return out;
}
