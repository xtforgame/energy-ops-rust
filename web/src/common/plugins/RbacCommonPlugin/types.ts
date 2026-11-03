/**
 * RBAC core types — shared between server / common / future raadmin5 frontend.
 *
 * See:
 * - docs/features/rbacHardening/SPEC.md §4.2 (RbacSeed shape)
 * - docs/features/rbacHardening/GUIDELINE.md §2 (how plugins declare seeds)
 */

export type RbacScope = 'organization' | 'project' | 'resource';

export type RbacType = 'system' | 'plugin' | 'user-defined' | 'synthetic';

/** Permission stored in tbl_rbac_permission. */
export interface BuiltInPermission {
  scope: RbacScope;
  /**
   * Globally unique within (scope, uniqueName). Format: `<plugin>:<group>:<action>` or `<plugin>:<group>-<rollup>`.
   * See GUIDELINE §2.2 for naming conventions.
   */
  uniqueName: string;
  name: string;
  /** True for "rolled-up" permissions that include a list of simple permissions. */
  complex: boolean;
  /**
   * If complex=true, the simple permissions this complex permission rolls up.
   * MUST all reference simple permissions (complex=false). Nesting is not supported.
   */
  includedPermissionUniqueNames?: string[];
}

/** Role stored in tbl_rbac_role. */
export interface BuiltInRole {
  scope: RbacScope;
  uniqueName: string;
  name: string;
  /** Permissions (simple or complex) bundled into this role. All must share the role's scope. */
  permissionUniqueNames: string[];
}

/**
 * Returned by `Plugin.getRbacSeeds()`. RbacSeedManager collects these from all plugins
 * at server startup and idempotent-upserts into DB.
 */
export interface RbacSeed {
  /** Plugin name; written to rbacPermission.created_by_plugin / rbacRole.created_by_plugin. */
  createdByPlugin: string;
  permissions: BuiltInPermission[];
  roles: BuiltInRole[];
  /**
   * Resource types this plugin uses for resource-scope grants
   * (i.e., the prefix in `<type>:<id>` resourceIdentifier strings).
   * Used by buildResourceIdentifier / parseResourceIdentifier for type safety.
   */
  resourceTypes?: string[];
}
