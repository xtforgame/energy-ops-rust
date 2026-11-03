import { describe, expect, it } from 'vitest';
import {
  canAccessResource, canSeeInMenu, hasAnyPermission, RESOURCE_PERMISSION_MAP, unionPermissions,
} from './permissions';

describe('canAccessResource', () => {
  it('尚未載入（undefined/null）→ 先顯示', () => {
    expect(canAccessResource('rbacRoles', undefined)).toBe(true);
    expect(canAccessResource('rbacRoles', null)).toBe(true);
  });

  it('已載入但為空 → 有閘的一律封鎖', () => {
    expect(canAccessResource('rbacRoles', [])).toBe(false);
    expect(canAccessResource('organizationMembers', [])).toBe(false);
    expect(canAccessResource('energyBillConsole', [])).toBe(false);
  });

  it('沒列在表上的 resource → 顯示', () => {
    expect(canAccessResource('somethingElse', [])).toBe(true);
  });

  it('超級使用者全部通過，含 super-only 頁', () => {
    expect(canAccessResource('adminUsers', ['*'])).toBe(true);
    expect(canAccessResource('rbacGrants', ['*'])).toBe(true);
  });

  it('super-only 頁：任何一般權限都進不去', () => {
    const everything = Object.values(RESOURCE_PERMISSION_MAP).flat();
    expect(canAccessResource('adminUsers', everything)).toBe(false);
    expect(canAccessResource('adminRbac', everything)).toBe(false);
  });

  it('平台頁各自的閘', () => {
    expect(canAccessResource('rbacRoles', ['platform:rbac:manage'])).toBe(true);
    expect(canAccessResource('rbacRoles', ['platform:member:manage'])).toBe(false);
    expect(canAccessResource('organizationMembers', ['energy:member:manage'])).toBe(true);
    expect(canAccessResource('projects', ['platform:project:manage'])).toBe(true);
    expect(canAccessResource('organizationSettings', ['platform:org:manage'])).toBe(true);
    expect(canAccessResource('organizationSettings', ['platform:rbac:manage'])).toBe(false);
  });

  it('能源頁：ryxis:rbac:manage 已換成 platform:rbac:manage', () => {
    expect(canAccessResource('energyAuditConsole', ['platform:rbac:manage'])).toBe(true);
    expect(canAccessResource('energyAuditConsole', ['ryxis:rbac:manage'])).toBe(false);
    expect(canAccessResource('energyAuditConsole', ['energy:audit:view'])).toBe(true);
    for (const list of Object.values(RESOURCE_PERMISSION_MAP)) expect(list).not.toContain('ryxis:rbac:manage');
  });
});

describe('canSeeInMenu', () => {
  it('流程單：路由給 flow:view，選單要 view-all', () => {
    expect(canAccessResource('energyExecutionConsole', ['energy:flow:view'])).toBe(true);
    expect(canSeeInMenu('energyExecutionConsole', ['energy:flow:view'])).toBe(false);
    expect(canSeeInMenu('energyExecutionConsole', ['energy:flow:view-all'])).toBe(true);
  });

  it('其他項目跟路由閘相同', () => {
    expect(canSeeInMenu('rbacRoles', ['platform:rbac:manage'])).toBe(true);
    expect(canSeeInMenu('rbacRoles', [])).toBe(false);
    expect(canSeeInMenu('rbacRoles', undefined)).toBe(true);
  });
});

describe('hasAnyPermission / unionPermissions', () => {
  it('hasAnyPermission', () => {
    expect(hasAnyPermission(null, ['a'])).toBe(false);
    expect(hasAnyPermission(['b', 'a'], ['a'])).toBe(true);
    expect(hasAnyPermission(['*'], ['x'])).toBe(true);
  });

  it('unionPermissions 去重保序，遇 * 回 ["*"]', () => {
    expect(unionPermissions(['a', 'b'], ['b', 'c'], undefined)).toEqual(['a', 'b', 'c']);
    expect(unionPermissions(['a'], ['*'])).toEqual(['*']);
    expect(unionPermissions()).toEqual([]);
  });
});
