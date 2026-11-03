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
