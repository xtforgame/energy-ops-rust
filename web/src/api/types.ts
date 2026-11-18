// 後端回應的型別（camelCase，對照 crates/eo-platform 的 Serialize 結構）。

export type Id = number;
export type IsoDate = string;

export interface Items<T> {
  items: T[];
}

export interface User {
  id: Id;
  name: string;
  email: string | null;
  privilege: string | null;
  disabled: boolean;
  picture: string | null;
  createdAt: IsoDate;
}

export interface LoginResult {
  user: User;
  token: string;
  expiresAt: IsoDate;
}

export interface SessionInfo {
  user: User;
  superUser: boolean;
}

export interface Organization {
  id: Id;
  uniqueName: string;
  name: string;
  kind: string; // 'personal' | 'organization' …
  personalUserId: Id | null;
  createdAt: IsoDate;
}

export interface MyProject {
  id: Id;
  organizationId: Id;
  name: string;
  uniqueName: string;
  isDefault: boolean;
  membershipRole: string;
}

export interface MyOrganization {
  id: Id;
  uniqueName: string;
  name: string;
  kind: string;
  membershipRole: string;
  joinedAt: IsoDate;
  projects: MyProject[];
}

export interface Project {
  id: Id;
  organizationId: Id;
  type: string;
  isDefault: boolean;
  uniqueName: string;
  name: string;
  createdAt: IsoDate;
}

export interface EffectivePermissions {
  superUser: boolean;
  privilege: string | null;
  permissions: string[];
  matchedRoles: string[];
}

export interface CheckResult {
  allowed: boolean;
  reason: 'super-user' | 'matched-grant' | 'no-grant' | 'invalid-input';
  matchedRole?: string;
}

export interface OrgMember {
  userId: Id;
  name: string;
  email: string | null;
  disabled: boolean;
  membershipRole: string;
  joinedAt: IsoDate;
  roles: string[];
}

export interface ProjectMember {
  userId: Id;
  name: string;
  email: string | null;
  membershipRole: string;
  joinedAt: IsoDate;
  roles: string[];
}

export type AddMemberInput = { userId: Id } | { email: string; name: string; password: string };

export type InvitationStatus = 'pending' | 'accepted' | 'declined' | 'revoked' | 'expired';

export interface Invitation {
  id: Id;
  scope: 'organization' | 'project';
  organizationId: Id;
  organizationName: string;
  projectId: Id | null;
  projectName: string | null;
  inviterId: Id;
  inviterName: string;
  inviteeId: Id | null;
  inviteeEmail: string;
  inviteRoleKey: string;
  status: InvitationStatus;
  respondedAt: IsoDate | null;
  expiresAt: IsoDate;
  createdAt: IsoDate;
  /** 只在建立時回一次。 */
  token?: string;
}

export interface InvitationSummary {
  id: Id;
  scope: 'organization' | 'project';
  organizationName: string;
  projectName: string | null;
  inviterName: string;
  inviteeEmail: string;
  inviteRoleKey: string;
  status: InvitationStatus;
  expiresAt: IsoDate;
  inviteeRegistered: boolean;
}

export interface InvitePreset {
  key: string;
  scope: 'organization' | 'project';
  labelKey: string;
  grantRoleUniqueNames: string[];
}

export type RbacScope = 'organization' | 'project' | 'resource';

export interface RbacPermission {
  id: Id;
  scope: RbacScope;
  type: string;
  createdByPlugin: string | null;
  uniqueName: string;
  name: string;
  complex: boolean;
  owningOrganizationId: Id | null;
  createdAt: IsoDate;
  included: string[];
}

export interface RbacRole {
  id: Id;
  scope: RbacScope;
  type: string;
  createdByPlugin: string | null;
  uniqueName: string;
  name: string;
  owningOrganizationId: Id | null;
  createdAt: IsoDate;
  permissions: string[];
}

export interface CreateRoleInput {
  scope: RbacScope;
  uniqueName: string;
  name: string;
  permissions?: string[];
}

export interface UpdateRoleInput {
  name?: string;
  permissions?: string[];
}

export interface CreatePermissionInput {
  scope: RbacScope;
  uniqueName: string;
  name: string;
  complex?: boolean;
  included?: string[];
}

export interface UpdatePermissionInput {
  name?: string;
  included?: string[];
}

export interface GrantView {
  id: Id;
  userId: Id;
  userName: string;
  userEmail: string | null;
  roleId: Id;
  roleUniqueName: string;
  roleName: string;
  roleScope: RbacScope;
  organizationId: Id | null;
  projectId: Id | null;
  projectName: string | null;
  resourceIdentifier: string;
  addedByUserId: Id | null;
  revokedByUserId: Id | null;
  revokedAt: IsoDate | null;
  revokedReason: string | null;
  createdAt: IsoDate;
}

export interface GrantRow {
  id: Id;
  userId: Id;
  roleId: Id;
  organizationId: Id | null;
  projectId: Id | null;
  resourceIdentifier: string;
  addedByUserId: Id | null;
  revokedByUserId: Id | null;
  revokedAt: IsoDate | null;
  revokedReason: string | null;
  createdAt: IsoDate;
}

export interface CreateGrantInput {
  userId: Id;
  roleId?: Id;
  roleUniqueName?: string;
  projectId?: Id;
  resourceIdentifier?: string;
}

export interface GrantQuery {
  userId?: Id;
  roleId?: Id;
  projectId?: Id;
  includeRevoked?: boolean;
}

export type Privilege = 'admin' | 'manager' | 'user';

export interface AdminUserList {
  items: User[];
  total: number;
}

export interface AdminCreateUserInput {
  name: string;
  email: string;
  password: string;
  privilege?: Privilege;
}

export interface AdminPatchUserInput {
  name?: string;
  privilege?: Privilege | null;
  disabled?: boolean;
  password?: string;
}

/** 一個帳號的整體關係總覽（`GET /admin/users/:userId/relations`，只准超級使用者看）。 */
export interface RelationOrg { organizationId: Id; organizationName: string; role: string }
export interface RelationProject { projectId: Id; projectName: string; organizationId: Id; organizationName: string }
export interface RelationDepartment { departmentId: Id; path: string; organizationName: string; role: string }
export interface RelationGrant {
  roleUniqueName: string;
  roleName: string;
  scope: string;
  scopeName: string;
  grantedAt: IsoDate;
  grantedBy: string | null;
}
export interface UserRelations {
  organizations: RelationOrg[];
  projects: RelationProject[];
  departments: RelationDepartment[];
  grants: RelationGrant[];
}
