// 平台端點（authn／user／multitenancy／rbac plugin）。新寫的程式一律透過這裡呼叫後端。
import { http } from './http';
import type {
  AddMemberInput, AdminCreateUserInput, AdminPatchUserInput, AdminUserList, CheckResult, CreateGrantInput,
  CreatePermissionInput, CreateRoleInput, EffectivePermissions, GrantQuery, GrantRow, GrantView, Id, Invitation,
  InvitationSummary, InvitePreset, Items, LoginResult, MyOrganization, OrgMember, Organization, Project,
  ProjectMember, RbacPermission, RbacRole, SessionInfo, UpdatePermissionInput, UpdateRoleInput, User, UserRelations,
} from './types';

const V1 = '/api/v1';
const enc = encodeURIComponent;

export const authApi = {
  login: (username: string, password: string) => http.post<LoginResult>(`${V1}/auth/login`, { username, password }),
  logout: () => http.post<{ ok: true }>(`${V1}/auth/logout`),
  session: () => http.get<SessionInfo>(`${V1}/auth/session`),
  register: (input: { name: string; email: string; password: string; invitationToken?: string }) =>
    http.post<LoginResult>(`${V1}/auth/register`, input),
};

export const meApi = {
  organizations: () => http.get<Items<MyOrganization>>(`${V1}/me/organizations`),
  permissions: (scope: { orgId?: Id | null; projectId?: Id | null; resourceIdentifier?: string }) =>
    http.get<EffectivePermissions>(`${V1}/me/permissions`, {
      orgId: scope.orgId ?? undefined,
      projectId: scope.projectId ?? undefined,
      resourceIdentifier: scope.resourceIdentifier,
    }),
  check: (body: { permissions: string[]; orgId?: Id; projectId?: Id; resourceIdentifier?: string }) =>
    http.post<CheckResult>(`${V1}/me/check`, body),
  orgsWithPermission: (permission: string) =>
    http.get<Items<Organization>>(`${V1}/me/orgs-with-permission`, { permission }),
  updateProfile: (input: { name?: string; picture?: string }) => http.patch<User>(`${V1}/me/profile`, input),
  changePassword: (currentPassword: string, newPassword: string) =>
    http.post<{ ok: true }>(`${V1}/me/password`, { currentPassword, newPassword }),
  invitations: () => http.get<Items<Invitation>>(`${V1}/me/invitations`),
  acceptInvitation: (id: Id) => http.post<Invitation>(`${V1}/me/invitations/${id}/accept`),
  declineInvitation: (id: Id) => http.post<{ ok: true }>(`${V1}/me/invitations/${id}/decline`),
};

export const orgApi = {
  create: (input: { uniqueName: string; name: string }) => http.post<Organization>(`${V1}/orgs`, input),
  get: (orgId: Id) => http.get<Organization>(`${V1}/orgs/${orgId}`),
  update: (orgId: Id, input: { name?: string }) => http.patch<Organization>(`${V1}/orgs/${orgId}`, input),
  members: (orgId: Id) => http.get<Items<OrgMember>>(`${V1}/orgs/${orgId}/members`),
  addMember: (orgId: Id, input: AddMemberInput) => http.post<User>(`${V1}/orgs/${orgId}/members`, input),
  removeMember: (orgId: Id, userId: Id) => http.delete(`${V1}/orgs/${orgId}/members/${userId}`),
  projects: (orgId: Id) => http.get<Items<Project>>(`${V1}/orgs/${orgId}/projects`),
  createProject: (orgId: Id, input: { uniqueName: string; name: string; type?: string }) =>
    http.post<Project>(`${V1}/orgs/${orgId}/projects`, input),
  invitations: (orgId: Id) => http.get<Items<Invitation>>(`${V1}/orgs/${orgId}/invitations`),
  invite: (orgId: Id, input: { email: string; inviteRoleKey: string }) =>
    http.post<Invitation>(`${V1}/orgs/${orgId}/invitations`, input),
};

export const projectApi = {
  get: (projectId: Id) => http.get<Project>(`${V1}/projects/${projectId}`),
  rename: (projectId: Id, name: string) => http.patch<Project>(`${V1}/projects/${projectId}`, { name }),
  delete: (projectId: Id) => http.delete(`${V1}/projects/${projectId}`),
  members: (projectId: Id) => http.get<Items<ProjectMember>>(`${V1}/projects/${projectId}/members`),
  addMember: (projectId: Id, userId: Id) => http.post<{ ok: true }>(`${V1}/projects/${projectId}/members`, { userId }),
  removeMember: (projectId: Id, userId: Id) => http.delete(`${V1}/projects/${projectId}/members/${userId}`),
  invitations: (projectId: Id) => http.get<Items<Invitation>>(`${V1}/projects/${projectId}/invitations`),
  invite: (projectId: Id, input: { email: string; inviteRoleKey: string }) =>
    http.post<Invitation>(`${V1}/projects/${projectId}/invitations`, input),
};

export const invitationApi = {
  presets: () => http.get<Items<InvitePreset>>(`${V1}/invite-presets`),
  revoke: (id: Id) => http.post<{ ok: true }>(`${V1}/invitations/${id}/revoke`),
  byToken: (token: string) => http.get<InvitationSummary>(`${V1}/invitations/by-token/${enc(token)}`),
  acceptByToken: (token: string) => http.post<Invitation>(`${V1}/invitations/by-token/${enc(token)}/accept`),
};

/** 組織內 RBAC（`/orgs/{orgId}/rbac/*`）。 */
export const orgRbacApi = {
  roles: (orgId: Id) => http.get<Items<RbacRole>>(`${V1}/orgs/${orgId}/rbac/roles`),
  createRole: (orgId: Id, input: CreateRoleInput) => http.post<RbacRole>(`${V1}/orgs/${orgId}/rbac/roles`, input),
  updateRole: (orgId: Id, roleId: Id, input: UpdateRoleInput) =>
    http.patch<RbacRole>(`${V1}/orgs/${orgId}/rbac/roles/${roleId}`, input),
  deleteRole: (orgId: Id, roleId: Id) => http.delete(`${V1}/orgs/${orgId}/rbac/roles/${roleId}`),
  roleHolders: (orgId: Id, roleId: Id) => http.get<Items<GrantView>>(`${V1}/orgs/${orgId}/rbac/roles/${roleId}/holders`),
  permissions: (orgId: Id) => http.get<Items<RbacPermission>>(`${V1}/orgs/${orgId}/rbac/permissions`),
  createPermission: (orgId: Id, input: CreatePermissionInput) =>
    http.post<RbacPermission>(`${V1}/orgs/${orgId}/rbac/permissions`, input),
  updatePermission: (orgId: Id, permissionId: Id, input: UpdatePermissionInput) =>
    http.patch<RbacPermission>(`${V1}/orgs/${orgId}/rbac/permissions/${permissionId}`, input),
  deletePermission: (orgId: Id, permissionId: Id) => http.delete(`${V1}/orgs/${orgId}/rbac/permissions/${permissionId}`),
  grants: (orgId: Id, q: GrantQuery = {}) => http.get<Items<GrantView>>(`${V1}/orgs/${orgId}/rbac/grants`, { ...q }),
  grant: (orgId: Id, input: CreateGrantInput) => http.post<GrantRow>(`${V1}/orgs/${orgId}/rbac/grants`, input),
  revokeGrant: (orgId: Id, grantId: Id, reason?: string) =>
    http.post<GrantRow>(`${V1}/orgs/${orgId}/rbac/grants/${grantId}/revoke`, reason ? { reason } : {}),
};

/** 超級使用者的全域管理（`/admin/*`）。 */
export const adminApi = {
  roles: () => http.get<Items<RbacRole>>(`${V1}/admin/rbac/roles`),
  createRole: (input: CreateRoleInput) => http.post<RbacRole>(`${V1}/admin/rbac/roles`, input),
  updateRole: (roleId: Id, input: UpdateRoleInput) => http.patch<RbacRole>(`${V1}/admin/rbac/roles/${roleId}`, input),
  deleteRole: (roleId: Id) => http.delete(`${V1}/admin/rbac/roles/${roleId}`),
  permissions: () => http.get<Items<RbacPermission>>(`${V1}/admin/rbac/permissions`),
  createPermission: (input: CreatePermissionInput) => http.post<RbacPermission>(`${V1}/admin/rbac/permissions`, input),
  updatePermission: (permissionId: Id, input: UpdatePermissionInput) =>
    http.patch<RbacPermission>(`${V1}/admin/rbac/permissions/${permissionId}`, input),
  deletePermission: (permissionId: Id) => http.delete(`${V1}/admin/rbac/permissions/${permissionId}`),
  users: (q: { q?: string; page?: number; pageSize?: number } = {}) => http.get<AdminUserList>(`${V1}/admin/users`, q),
  createUser: (input: AdminCreateUserInput) => http.post<User>(`${V1}/admin/users`, input),
  updateUser: (userId: Id, input: AdminPatchUserInput) => http.patch<User>(`${V1}/admin/users/${userId}`, input),
  relations: (userId: Id) => http.get<UserRelations>(`${V1}/admin/users/${userId}/relations`),
};

export const healthApi = {
  health: () => http.get<unknown>('/health'),
};
