//! RBAC：判定、快取、型別閘、授權寫入、seed、自訂角色。
//!
//! 判定語意照 az `RbacPermissionResolver`：
//! 1. 超級使用者（`tbl_user.privilege` ∈ {admin, manager}，每次讀 DB、不進快取）直接放行；
//! 2. 其餘看使用者所有有效 grant：上層 grant 涵蓋下層請求（org ⊇ project ⊇ resource），反之不行；
//! 3. 角色的 permission 展開 complex 一層；請求的 permission 清單是 OR。
//!
//! 型別閘：handler 拿到 `OrgAccess`／`ProjectAccess`／`ResourceAccess` 的唯一路是這裡的 `authorize_*`，
//! 所有 org 範圍的資料存取都要收它，所以漏判權或漏 org 條件編不過。

pub mod custom;
pub mod grant;
pub mod seed;

use crate::auth::{is_super_privilege, CurrentUser};
use crate::error::{ApiError, ApiResult};
use eo_domain::rbac::RbacScope;
use serde::Serialize;
use sqlx::PgPool;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// 舊版每個閘都 OR 上的萬能鑰匙：持有它＝可以管理該組織的 RBAC。
pub const PLATFORM_RBAC_MANAGE: &str = "platform:rbac:manage";
pub const PLATFORM_ORG_MANAGE: &str = "platform:org:manage";
pub const PLATFORM_MEMBER_MANAGE: &str = "platform:member:manage";
pub const PLATFORM_PROJECT_MANAGE: &str = "platform:project:manage";
pub const PLATFORM_ORG_ADMIN_ROLE: &str = "platform:org-admin";
pub const PLATFORM_PROJECT_ADMIN_ROLE: &str = "platform:project-admin";

/// 誰在做這件事：登入的人，或系統自己（背景任務）。
#[derive(Debug, Clone)]
pub enum Actor {
    User(CurrentUser),
    System,
}

impl Actor {
    pub fn user(&self) -> Option<&CurrentUser> {
        match self {
            Actor::User(u) => Some(u),
            Actor::System => None,
        }
    }
    pub fn user_id(&self) -> Option<i64> {
        self.user().map(|u| u.id)
    }
}

/// 已在某組織通過判權。欄位私有：只有本模組造得出來。
#[derive(Debug, Clone)]
pub struct OrgAccess {
    actor: Actor,
    org_id: i64,
    super_user: bool,
}

impl OrgAccess {
    pub fn org_id(&self) -> i64 {
        self.org_id
    }
    pub fn actor(&self) -> &Actor {
        &self.actor
    }
    pub fn user(&self) -> Option<&CurrentUser> {
        self.actor.user()
    }
    pub fn user_id(&self) -> Option<i64> {
        self.actor.user_id()
    }
    /// 需要「一定是人」的動作（例如認領任務）用這個。
    pub fn require_user(&self) -> ApiResult<&CurrentUser> {
        self.actor.user().ok_or_else(|| ApiError::forbidden("user_required", "this action requires a user"))
    }
    pub fn is_super_user(&self) -> bool {
        self.super_user
    }
}

#[derive(Debug, Clone)]
pub struct ProjectAccess {
    org: OrgAccess,
    project_id: i64,
}

impl ProjectAccess {
    pub fn org(&self) -> &OrgAccess {
        &self.org
    }
    pub fn org_id(&self) -> i64 {
        self.org.org_id
    }
    pub fn project_id(&self) -> i64 {
        self.project_id
    }
    pub fn user(&self) -> Option<&CurrentUser> {
        self.org.user()
    }
}

#[derive(Debug, Clone)]
pub struct ResourceAccess {
    org: OrgAccess,
    project_id: Option<i64>,
    resource_identifier: String,
}

impl ResourceAccess {
    pub fn org(&self) -> &OrgAccess {
        &self.org
    }
    pub fn project_id(&self) -> Option<i64> {
        self.project_id
    }
    pub fn resource_identifier(&self) -> &str {
        &self.resource_identifier
    }
}

/// 一次判定的範圍。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScopeRef {
    pub scope: RbacScope,
    pub org: Option<i64>,
    pub project: Option<i64>,
    pub resource: Option<String>,
}

impl ScopeRef {
    pub fn org(org: i64) -> Self {
        Self { scope: RbacScope::Organization, org: Some(org), project: None, resource: None }
    }
    pub fn project(org: i64, project: i64) -> Self {
        Self { scope: RbacScope::Project, org: Some(org), project: Some(project), resource: None }
    }
    pub fn resource(org: i64, project: Option<i64>, resource: &str) -> Self {
        Self { scope: RbacScope::Resource, org: Some(org), project, resource: Some(resource.to_string()) }
    }

    fn valid(&self) -> bool {
        match self.scope {
            RbacScope::Organization => self.org.is_some(),
            RbacScope::Project => self.project.is_some(),
            RbacScope::Resource => self.resource.as_deref().map(|r| !r.is_empty()).unwrap_or(false),
        }
    }
}

/// 使用者的一筆有效 grant 與展開後的 permission。
#[derive(Debug, Clone)]
pub struct GrantEntry {
    pub grant_id: i64,
    pub role_id: i64,
    pub role_unique_name: String,
    pub org: Option<i64>,
    pub project: Option<i64>,
    pub resource: String,
    pub permissions: HashSet<String>,
}

/// grant 是否涵蓋請求：上層涵蓋下層，下層不涵蓋上層。
pub fn grant_matches(g: &GrantEntry, req: &ScopeRef) -> bool {
    let req_org = req.org.unwrap_or(0);
    let req_proj = req.project.unwrap_or(0);
    if !g.resource.is_empty() {
        if req.scope != RbacScope::Resource {
            return false;
        }
        if Some(g.resource.as_str()) != req.resource.as_deref() {
            return false;
        }
        if let Some(o) = g.org {
            if o != req_org {
                return false;
            }
        }
        if let Some(p) = g.project {
            if p != req_proj {
                return false;
            }
        }
        return true;
    }
    if let Some(p) = g.project {
        if req.scope == RbacScope::Organization {
            return false;
        }
        if p != req_proj {
            return false;
        }
        if let Some(o) = g.org {
            if o != req_org {
                return false;
            }
        }
        return true;
    }
    if let Some(o) = g.org {
        return o == req_org;
    }
    false
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CheckReason {
    SuperUser,
    MatchedGrant,
    NoGrant,
    InvalidInput,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub allowed: bool,
    pub reason: CheckReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_role: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectivePermissions {
    pub super_user: bool,
    pub privilege: Option<String>,
    /// 超級使用者回 `["*"]`。
    pub permissions: Vec<String>,
    pub matched_roles: Vec<String>,
}

/// 每個使用者的 grant 快取。`generation` 防止「讀到 commit 前的狀態、在失效之後才寫回」。
#[derive(Default)]
struct GrantCache {
    entries: HashMap<i64, Arc<Vec<GrantEntry>>>,
    generations: HashMap<i64, u64>,
    global_generation: u64,
}

pub struct Rbac {
    pool: PgPool,
    cache: Mutex<GrantCache>,
}

#[derive(sqlx::FromRow)]
struct GrantPermRow {
    grant_id: i64,
    role_id: i64,
    role_unique_name: String,
    organization_id: Option<i64>,
    project_id: Option<i64>,
    resource_identifier: String,
    perm: Option<String>,
    simple: Option<String>,
}

impl Rbac {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, cache: Mutex::new(GrantCache::default()) }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// 某使用者的授權變了（commit 之後呼叫）。
    pub fn invalidate_user(&self, user_id: i64) {
        let mut c = self.cache.lock().unwrap();
        c.entries.remove(&user_id);
        *c.generations.entry(user_id).or_insert(0) += 1;
    }

    /// 角色或 permission 定義變了：全部失效。
    pub fn invalidate_all(&self) {
        let mut c = self.cache.lock().unwrap();
        c.entries.clear();
        c.global_generation += 1;
    }

    fn generation(&self, user_id: i64) -> (u64, u64) {
        let c = self.cache.lock().unwrap();
        (c.generations.get(&user_id).copied().unwrap_or(0), c.global_generation)
    }

    pub async fn load_grants(&self, user_id: i64) -> ApiResult<Arc<Vec<GrantEntry>>> {
        if let Some(e) = self.cache.lock().unwrap().entries.get(&user_id) {
            return Ok(Arc::clone(e));
        }
        let gen_before = self.generation(user_id);
        let rows: Vec<GrantPermRow> = sqlx::query_as(
            "SELECT g.id AS grant_id, g.role_id, r.unique_name AS role_unique_name,
                    g.organization_id, g.project_id, g.resource_identifier,
                    p.unique_name AS perm, sp.unique_name AS simple
               FROM tbl_user_rbac_role g
               JOIN tbl_rbac_role r ON r.id = g.role_id AND r.deleted_at IS NULL
               LEFT JOIN mn_rbac_role_permission rp ON rp.role_id = r.id
               LEFT JOIN tbl_rbac_permission p ON p.id = rp.permission_id AND p.deleted_at IS NULL
               LEFT JOIN mn_rbac_complex_permission cp ON cp.complex_one_id = p.id AND p.complex
               LEFT JOIN tbl_rbac_permission sp ON sp.id = cp.simple_one_id AND sp.deleted_at IS NULL
              WHERE g.user_id = $1 AND g.deleted_at IS NULL
              ORDER BY g.id",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        let mut by_grant: Vec<GrantEntry> = Vec::new();
        for r in rows {
            if by_grant.last().map(|g| g.grant_id) != Some(r.grant_id) {
                by_grant.push(GrantEntry {
                    grant_id: r.grant_id,
                    role_id: r.role_id,
                    role_unique_name: r.role_unique_name.clone(),
                    org: r.organization_id,
                    project: r.project_id,
                    resource: r.resource_identifier.clone(),
                    permissions: HashSet::new(),
                });
            }
            let g = by_grant.last_mut().unwrap();
            if let Some(p) = r.perm {
                g.permissions.insert(p);
            }
            if let Some(s) = r.simple {
                g.permissions.insert(s);
            }
        }
        let arc = Arc::new(by_grant);
        let mut c = self.cache.lock().unwrap();
        let gen_now = (c.generations.get(&user_id).copied().unwrap_or(0), c.global_generation);
        if gen_now == gen_before {
            c.entries.insert(user_id, Arc::clone(&arc));
        }
        Ok(arc)
    }

    /// 讀 DB 的 privilege（不用 session 裡那份，避免權限變了還沿用舊值）。
    pub async fn is_super_user(&self, user_id: i64) -> ApiResult<bool> {
        let p: Option<Option<String>> =
            sqlx::query_scalar("SELECT privilege FROM tbl_user WHERE id = $1 AND deleted_at IS NULL")
                .bind(user_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(p.flatten().as_deref().map(|p| is_super_privilege(Some(p))).unwrap_or(false))
    }

    pub async fn check(&self, user_id: i64, req: &ScopeRef, permissions: &[&str]) -> ApiResult<CheckResult> {
        if permissions.is_empty() || !req.valid() {
            return Ok(CheckResult { allowed: false, reason: CheckReason::InvalidInput, matched_role: None });
        }
        if self.is_super_user(user_id).await? {
            return Ok(CheckResult { allowed: true, reason: CheckReason::SuperUser, matched_role: None });
        }
        let grants = self.load_grants(user_id).await?;
        for g in grants.iter() {
            if !grant_matches(g, req) {
                continue;
            }
            if permissions.iter().any(|p| g.permissions.contains(*p)) {
                return Ok(CheckResult {
                    allowed: true,
                    reason: CheckReason::MatchedGrant,
                    matched_role: Some(g.role_unique_name.clone()),
                });
            }
        }
        Ok(CheckResult { allowed: false, reason: CheckReason::NoGrant, matched_role: None })
    }

    /// 不拿型別閘、只想知道「有沒有」時用（例如決定要不要顯示某按鈕）。
    pub async fn has(&self, user_id: i64, req: &ScopeRef, permissions: &[&str]) -> ApiResult<bool> {
        Ok(self.check(user_id, req, permissions).await?.allowed)
    }

    /// 組織層判權。`permissions` 任一即可；`platform:rbac:manage` 一律視為也可以。
    pub async fn authorize_org(&self, user: &CurrentUser, org_id: i64, permissions: &[&str]) -> ApiResult<OrgAccess> {
        let mut perms: Vec<&str> = permissions.to_vec();
        if !perms.contains(&PLATFORM_RBAC_MANAGE) {
            perms.push(PLATFORM_RBAC_MANAGE);
        }
        let r = self.check(user.id, &ScopeRef::org(org_id), &perms).await?;
        if !r.allowed {
            return Err(ApiError::permission_denied(permissions));
        }
        Ok(OrgAccess { actor: Actor::User(user.clone()), org_id, super_user: r.reason == CheckReason::SuperUser })
    }

    /// 只要求「是這個組織的成員」（或超級使用者），不要求特定 permission。
    pub async fn authorize_member(&self, user: &CurrentUser, org_id: i64) -> ApiResult<OrgAccess> {
        if self.is_super_user(user.id).await? {
            return Ok(OrgAccess { actor: Actor::User(user.clone()), org_id, super_user: true });
        }
        let member: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM mn_user_organization WHERE user_id = $1 AND organization_id = $2 AND deleted_at IS NULL)",
        )
        .bind(user.id)
        .bind(org_id)
        .fetch_one(&self.pool)
        .await?;
        if !member {
            return Err(ApiError::forbidden("not_a_member", "not a member of this organization"));
        }
        Ok(OrgAccess { actor: Actor::User(user.clone()), org_id, super_user: false })
    }

    /// 專案層判權：專案所屬的組織一律由 server 推導，不信任請求帶的 org。
    pub async fn authorize_project(
        &self,
        user: &CurrentUser,
        project_id: i64,
        permissions: &[&str],
    ) -> ApiResult<ProjectAccess> {
        let org_id: i64 =
            sqlx::query_scalar("SELECT organization_id FROM tbl_project WHERE id = $1 AND deleted_at IS NULL")
                .bind(project_id)
                .fetch_optional(&self.pool)
                .await?
                .ok_or_else(|| ApiError::not_found("project_not_found", "project not found"))?;
        let mut perms: Vec<&str> = permissions.to_vec();
        if !perms.contains(&PLATFORM_RBAC_MANAGE) {
            perms.push(PLATFORM_RBAC_MANAGE);
        }
        let r = self.check(user.id, &ScopeRef::project(org_id, project_id), &perms).await?;
        if !r.allowed {
            return Err(ApiError::permission_denied(permissions));
        }
        Ok(ProjectAccess {
            org: OrgAccess { actor: Actor::User(user.clone()), org_id, super_user: r.reason == CheckReason::SuperUser },
            project_id,
        })
    }

    /// 資源層判權。資源所屬的組織／專案由呼叫端從資源本身推導後傳入。
    pub async fn authorize_resource(
        &self,
        user: &CurrentUser,
        org_id: i64,
        project_id: Option<i64>,
        resource_identifier: &str,
        permissions: &[&str],
    ) -> ApiResult<ResourceAccess> {
        let mut perms: Vec<&str> = permissions.to_vec();
        if !perms.contains(&PLATFORM_RBAC_MANAGE) {
            perms.push(PLATFORM_RBAC_MANAGE);
        }
        let r = self.check(user.id, &ScopeRef::resource(org_id, project_id, resource_identifier), &perms).await?;
        if !r.allowed {
            return Err(ApiError::permission_denied(permissions));
        }
        Ok(ResourceAccess {
            org: OrgAccess { actor: Actor::User(user.clone()), org_id, super_user: r.reason == CheckReason::SuperUser },
            project_id,
            resource_identifier: resource_identifier.to_string(),
        })
    }

    /// 背景任務以系統身分在某組織做事（例如辨識對帳）。
    pub fn system_access(&self, org_id: i64) -> OrgAccess {
        OrgAccess { actor: Actor::System, org_id, super_user: true }
    }

    /// 在某範圍內實際持有的 permission（前端選單與頁面用）。
    pub async fn effective_permissions(&self, user_id: i64, req: &ScopeRef) -> ApiResult<EffectivePermissions> {
        let privilege: Option<String> =
            sqlx::query_scalar("SELECT privilege FROM tbl_user WHERE id = $1 AND deleted_at IS NULL")
                .bind(user_id)
                .fetch_optional(&self.pool)
                .await?
                .flatten();
        if is_super_privilege(privilege.as_deref()) {
            return Ok(EffectivePermissions {
                super_user: true,
                privilege,
                permissions: vec!["*".into()],
                matched_roles: vec![],
            });
        }
        let grants = self.load_grants(user_id).await?;
        let mut perms = BTreeSet::new();
        let mut roles = BTreeSet::new();
        for g in grants.iter().filter(|g| grant_matches(g, req)) {
            perms.extend(g.permissions.iter().cloned());
            roles.insert(g.role_unique_name.clone());
        }
        Ok(EffectivePermissions {
            super_user: false,
            privilege,
            permissions: perms.into_iter().collect(),
            matched_roles: roles.into_iter().collect(),
        })
    }

    /// 在組織內持有某些角色（組織層 grant）的使用者，id 升序。流程候選人解析用。
    pub async fn users_with_roles(&self, org_id: i64, role_unique_names: &[String]) -> ApiResult<Vec<i64>> {
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT DISTINCT g.user_id
               FROM tbl_user_rbac_role g
               JOIN tbl_rbac_role r ON r.id = g.role_id AND r.deleted_at IS NULL
               JOIN tbl_user u ON u.id = g.user_id AND u.deleted_at IS NULL AND NOT u.disabled
              WHERE g.deleted_at IS NULL AND g.organization_id = $1 AND g.project_id IS NULL
                AND g.resource_identifier = '' AND r.unique_name = ANY($2)
              ORDER BY g.user_id",
        )
        .bind(org_id)
        .bind(role_unique_names)
        .fetch_all(&self.pool)
        .await?;
        Ok(ids)
    }

    /// 在組織內（組織層 grant）持有某 permission 的使用者，id 升序。不含超級使用者。
    pub async fn users_with_permission(&self, org_id: i64, permission: &str) -> ApiResult<Vec<i64>> {
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT DISTINCT g.user_id
               FROM tbl_user_rbac_role g
               JOIN tbl_rbac_role r ON r.id = g.role_id AND r.deleted_at IS NULL
               JOIN mn_rbac_role_permission rp ON rp.role_id = r.id
               JOIN tbl_rbac_permission p ON p.id = rp.permission_id AND p.deleted_at IS NULL
               LEFT JOIN mn_rbac_complex_permission cp ON cp.complex_one_id = p.id AND p.complex
               LEFT JOIN tbl_rbac_permission sp ON sp.id = cp.simple_one_id AND sp.deleted_at IS NULL
               JOIN tbl_user u ON u.id = g.user_id AND u.deleted_at IS NULL AND NOT u.disabled
              WHERE g.deleted_at IS NULL AND g.organization_id = $1 AND g.project_id IS NULL
                AND g.resource_identifier = '' AND (p.unique_name = $2 OR sp.unique_name = $2)
              ORDER BY g.user_id",
        )
        .bind(org_id)
        .bind(permission)
        .fetch_all(&self.pool)
        .await?;
        Ok(ids)
    }
}

pub type RbacRef = Arc<Rbac>;

#[cfg(test)]
mod tests {
    use super::*;

    fn g(org: Option<i64>, project: Option<i64>, resource: &str) -> GrantEntry {
        GrantEntry {
            grant_id: 1,
            role_id: 1,
            role_unique_name: "r".into(),
            org,
            project,
            resource: resource.into(),
            permissions: HashSet::new(),
        }
    }

    /// 來源：az RbacPermissionResolver.spec.ts「org grant covers project and resource requests」
    #[test]
    fn org_grant_covers_lower_scopes() {
        let grant = g(Some(1), None, "");
        assert!(grant_matches(&grant, &ScopeRef::org(1)));
        assert!(grant_matches(&grant, &ScopeRef::project(1, 5)));
        assert!(grant_matches(&grant, &ScopeRef::resource(1, Some(5), "x:1")));
        assert!(!grant_matches(&grant, &ScopeRef::org(2)));
    }

    /// 來源：az RbacPermissionResolver.spec.ts「project grant does not cover org request」
    #[test]
    fn project_grant_covers_project_and_resource_only() {
        let grant = g(Some(1), Some(5), "");
        assert!(!grant_matches(&grant, &ScopeRef::org(1)));
        assert!(grant_matches(&grant, &ScopeRef::project(1, 5)));
        assert!(!grant_matches(&grant, &ScopeRef::project(1, 6)));
        assert!(!grant_matches(&grant, &ScopeRef::project(2, 5)));
        assert!(grant_matches(&grant, &ScopeRef::resource(1, Some(5), "x:1")));
    }

    /// 來源：az RbacPermissionResolver.spec.ts「resource grant only matches identical identifier」
    #[test]
    fn resource_grant_matches_exact_only() {
        let grant = g(Some(1), None, "doc:9");
        assert!(grant_matches(&grant, &ScopeRef::resource(1, None, "doc:9")));
        assert!(!grant_matches(&grant, &ScopeRef::resource(1, None, "doc:8")));
        assert!(!grant_matches(&grant, &ScopeRef::project(1, 5)));
        assert!(!grant_matches(&grant, &ScopeRef::org(1)));
    }

    #[test]
    fn grant_without_scope_matches_nothing() {
        let grant = g(None, None, "");
        assert!(!grant_matches(&grant, &ScopeRef::org(1)));
    }
}
