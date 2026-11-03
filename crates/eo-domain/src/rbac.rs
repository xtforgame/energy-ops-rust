//! RBAC 的共用型別：plugin 宣告的 seed、scope、列的種類。
//! 判定邏輯在 eo-platform 的 rbac plugin；這裡只放兩邊都要認得的形狀。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RbacScope {
    Organization,
    Project,
    Resource,
}

impl RbacScope {
    pub fn as_str(self) -> &'static str {
        match self {
            RbacScope::Organization => "organization",
            RbacScope::Project => "project",
            RbacScope::Resource => "resource",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "organization" => Some(RbacScope::Organization),
            "project" => Some(RbacScope::Project),
            "resource" => Some(RbacScope::Resource),
            _ => None,
        }
    }
}

/// permission／role 列的來源。system 與 plugin 列不可經管理 API 修改。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RbacRowType {
    System,
    Plugin,
    UserDefined,
    Synthetic,
}

impl RbacRowType {
    pub fn as_str(self) -> &'static str {
        match self {
            RbacRowType::System => "system",
            RbacRowType::Plugin => "plugin",
            RbacRowType::UserDefined => "user-defined",
            RbacRowType::Synthetic => "synthetic",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeedPermission {
    pub scope: RbacScope,
    /// `<plugin>:<group>:<action>`；(scope, uniqueName) 全域唯一。
    pub unique_name: String,
    pub name: String,
    pub complex: bool,
    /// complex 時包含的 simple permission；不可為空、不可包 complex。
    #[serde(default)]
    pub included: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeedRole {
    pub scope: RbacScope,
    pub unique_name: String,
    pub name: String,
    /// 綁的 permission（simple 或 complex），scope 必須與角色相同。
    pub permissions: Vec<String>,
}

/// 一個 plugin 宣告的 permission 與角色。開機時收集、驗證、冪等寫入，只增不刪。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RbacSeeds {
    pub created_by_plugin: String,
    pub permissions: Vec<SeedPermission>,
    pub roles: Vec<SeedRole>,
    /// resource scope 用的資源型別（`<type>:<id>` 的前綴）。
    #[serde(default)]
    pub resource_types: Vec<String>,
}

impl RbacSeeds {
    pub fn new(plugin: &str) -> Self {
        Self { created_by_plugin: plugin.to_string(), ..Default::default() }
    }

    pub fn simple(mut self, scope: RbacScope, unique_name: &str, name: &str) -> Self {
        self.permissions.push(SeedPermission {
            scope,
            unique_name: unique_name.into(),
            name: name.into(),
            complex: false,
            included: vec![],
        });
        self
    }

    pub fn complex(mut self, scope: RbacScope, unique_name: &str, name: &str, included: &[&str]) -> Self {
        self.permissions.push(SeedPermission {
            scope,
            unique_name: unique_name.into(),
            name: name.into(),
            complex: true,
            included: included.iter().map(|s| s.to_string()).collect(),
        });
        self
    }

    pub fn role(mut self, scope: RbacScope, unique_name: &str, name: &str, permissions: &[&str]) -> Self {
        self.roles.push(SeedRole {
            scope,
            unique_name: unique_name.into(),
            name: name.into(),
            permissions: permissions.iter().map(|s| s.to_string()).collect(),
        });
        self
    }
}

fn perm_key(scope: RbacScope, unique_name: &str) -> String {
    format!("{}:{}", scope.as_str(), unique_name)
}

/// 開機收集到的所有 plugin seed 的一致性檢查（舊 `RbacSeedManager.validateSeeds` 的四條規則）：
/// 1. (scope, uniqueName) 不可被兩個 plugin 宣告；
/// 2. complex 不可為空、不可包 complex，非 complex 不可帶 included；
/// 3. 角色不可為空，綁的 permission 必須在角色的 scope 宣告過。
///
/// 回傳每一條違規的說明（訊息與舊版逐字相同，方便對照）。
pub fn validate_seeds(seeds: &[RbacSeeds]) -> Result<(), Vec<String>> {
    use std::collections::BTreeMap;

    let mut errors = Vec::new();

    let mut perm_claimers: Vec<(String, Vec<&str>)> = Vec::new();
    let mut role_claimers: Vec<(String, Vec<&str>)> = Vec::new();
    for seed in seeds {
        for p in &seed.permissions {
            let key = perm_key(p.scope, &p.unique_name);
            match perm_claimers.iter_mut().find(|(k, _)| *k == key) {
                Some(entry) => entry.1.push(&seed.created_by_plugin),
                None => perm_claimers.push((key, vec![&seed.created_by_plugin])),
            }
        }
        for r in &seed.roles {
            let key = perm_key(r.scope, &r.unique_name);
            match role_claimers.iter_mut().find(|(k, _)| *k == key) {
                Some(entry) => entry.1.push(&seed.created_by_plugin),
                None => role_claimers.push((key, vec![&seed.created_by_plugin])),
            }
        }
    }
    for (key, claimers) in &perm_claimers {
        if claimers.len() > 1 {
            errors.push(format!("Permission {key} claimed by multiple plugins: [{}]", claimers.join(", ")));
        }
    }
    for (key, claimers) in &role_claimers {
        if claimers.len() > 1 {
            errors.push(format!("Role {key} claimed by multiple plugins: [{}]", claimers.join(", ")));
        }
    }

    let all_perms: BTreeMap<String, &SeedPermission> = seeds
        .iter()
        .flat_map(|s| s.permissions.iter())
        .map(|p| (perm_key(p.scope, &p.unique_name), p))
        .collect();
    for p in seeds.iter().flat_map(|s| s.permissions.iter()) {
        let me = perm_key(p.scope, &p.unique_name);
        let has = !p.included.is_empty();
        if !p.complex && has {
            errors.push(format!("Permission {me} is not complex but declares includedPermissionUniqueNames"));
        }
        if p.complex && !has {
            errors.push(format!("Complex permission {me} has empty includedPermissionUniqueNames"));
        }
        for included in &p.included {
            let key = perm_key(p.scope, included);
            match all_perms.get(&key) {
                None => errors.push(format!("Permission {me} references unknown {key}")),
                Some(inc) if inc.complex => errors.push(format!(
                    "Permission {me} (complex) includes another complex permission {key} — nesting is not allowed"
                )),
                Some(_) => {}
            }
        }
    }

    for r in seeds.iter().flat_map(|s| s.roles.iter()) {
        let me = perm_key(r.scope, &r.unique_name);
        if r.permissions.is_empty() {
            errors.push(format!("Role {me} has empty permissionUniqueNames"));
        }
        for name in &r.permissions {
            let key = perm_key(r.scope, name);
            if !all_perms.contains_key(&key) {
                errors.push(format!("Role {me} references unknown permission {key}"));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
