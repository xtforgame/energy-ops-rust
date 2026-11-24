//! 移植自 `tests/getRbacSeeds.test.ts` 的常數那一半（seed 結構與 validateSeeds 四條規則）。
//! 「再對 migration 寫出的資料庫驗同一件事」那一半要 DB，屬於 eo-platform／eo-energy。

use eo_domain::energy_rbac::*;
use eo_domain::rbac::{validate_seeds, RbacScope, RbacSeeds, SeedPermission, SeedRole};

fn seed() -> RbacSeeds {
    energy_rbac_seeds()
}

fn perm<'a>(s: &'a RbacSeeds, name: &str) -> &'a SeedPermission {
    s.permissions.iter().find(|p| p.unique_name == name).unwrap_or_else(|| panic!("{name}"))
}

fn role<'a>(s: &'a RbacSeeds, name: &str) -> &'a SeedRole {
    s.roles.iter().find(|r| r.unique_name == name).unwrap_or_else(|| panic!("{name}"))
}

/// 角色展開成 simple（complex 換成成員）。
fn grants(s: &RbacSeeds, role_name: &str) -> Vec<String> {
    role(s, role_name)
        .permissions
        .iter()
        .flat_map(|pu| {
            let p = perm(s, pu);
            if p.complex {
                p.included.clone()
            } else {
                vec![pu.clone()]
            }
        })
        .collect()
}

/// 來源：getRbacSeeds.test.ts「is self-consistent (passes RbacSeedManager.validateSeeds)」
#[test]
fn is_self_consistent() {
    assert_eq!(validate_seeds(&[seed()]), Ok(()));
    let s = seed();
    assert_eq!(s.permissions.iter().filter(|p| !p.complex).count(), 18);
    assert_eq!(s.permissions.iter().filter(|p| p.complex).count(), 5);
    assert_eq!(s.roles.len(), 5);
}

/// validateSeeds 的四條規則真的會擋（否則上一條是空跑）。
/// 來源：無對應 TS 測試；補驗 tests/_rbacOracle.ts 的 validateSeeds 四條規則。
#[test]
fn validate_seeds_catches_each_rule() {
    // 1. 兩個 plugin 宣告同一個 (scope, uniqueName)
    let other = RbacSeeds::new("other").simple(RbacScope::Organization, "energy:audit:view", "x");
    let errs = validate_seeds(&[seed(), other]).unwrap_err();
    assert!(errs.iter().any(|e| e.contains("claimed by multiple plugins: [energyOps, other]")), "{errs:?}");
    // 2. 空 complex、complex 包 complex、引用不存在的
    let bad = RbacSeeds::new("bad")
        .simple(RbacScope::Organization, "bad:a:b", "a")
        .complex(RbacScope::Organization, "bad:empty", "e", &[])
        .complex(RbacScope::Organization, "bad:nest", "n", &["bad:empty"])
        .complex(RbacScope::Organization, "bad:ghost", "g", &["bad:none"])
        // 3. 空角色、角色引用別的 scope
        .role(RbacScope::Organization, "bad:role-empty", "r", &[])
        .role(RbacScope::Project, "bad:role-scope", "r", &["bad:a:b"]);
    let errs = validate_seeds(&[bad]).unwrap_err();
    for needle in [
        "Complex permission organization:bad:empty has empty includedPermissionUniqueNames",
        "includes another complex permission organization:bad:empty",
        "references unknown organization:bad:none",
        "Role organization:bad:role-empty has empty permissionUniqueNames",
        "Role project:bad:role-scope references unknown permission project:bad:a:b",
    ] {
        assert!(errs.iter().any(|e| e.contains(needle)), "缺：{needle}\n{errs:?}");
    }
}

/// 來源：getRbacSeeds.test.ts「declares createdByPlugin = "energyOps"」
#[test]
fn declares_created_by_plugin() {
    assert_eq!(seed().created_by_plugin, ENERGY_OPS_PLUGIN_NAME);
    assert_eq!(seed().created_by_plugin, "energyOps");
}

/// 來源：getRbacSeeds.test.ts「declares everything at organization scope only (validateSeeds rule #3)」
#[test]
fn organization_scope_only() {
    let s = seed();
    assert!(s.permissions.iter().all(|p| p.scope == RbacScope::Organization));
    assert!(s.roles.iter().all(|r| r.scope == RbacScope::Organization));
}

/// 來源：getRbacSeeds.test.ts「prefixes every permission / role with "energy:" and gives each a Chinese display name」
#[test]
fn energy_prefix_and_display_names() {
    let s = seed();
    for p in &s.permissions {
        assert!(p.unique_name.starts_with("energy:"));
        assert!(p.unique_name.split(':').count() >= 2, "{} grammar", p.unique_name);
        assert!(!p.name.is_empty());
        assert!(!p.name.contains("energy:"), "{} 顯示名不可洩漏 uniqueName", p.unique_name);
    }
    for r in &s.roles {
        assert!(r.unique_name.starts_with("energy:"));
        assert!(!r.name.is_empty());
        assert!(!r.name.contains("energy:") && !r.name.contains("ryxis:"));
    }
}

/// 來源：getRbacSeeds.test.ts「does NOT declare energy:ops:view / energy:ops:edit (empty complex would fail seed validation)」
#[test]
fn does_not_declare_ops_view_edit() {
    let s = seed();
    assert!(!s.permissions.iter().any(|p| p.unique_name == "energy:ops:view" || p.unique_name == "energy:ops:edit"));
}

/// 來源：getRbacSeeds.test.ts「every complex permission is non-empty and includes only simple permissions」
#[test]
fn complex_permissions_are_non_empty_and_flat() {
    let s = seed();
    let complex: Vec<&SeedPermission> = s.permissions.iter().filter(|p| p.complex).collect();
    assert_eq!(complex.len(), 5, "should declare 5 complex permissions");
    for c in complex {
        assert!(!c.included.is_empty(), "{} must not be empty", c.unique_name);
        for name in &c.included {
            assert!(!perm(&s, name).complex, "{} includes {name} — must be a declared simple", c.unique_name);
        }
    }
}

/// 來源：getRbacSeeds.test.ts「declares the five energy roles with the right display names」
#[test]
fn five_roles_with_display_names() {
    let s = seed();
    let mut names: Vec<&str> = s.roles.iter().map(|r| r.unique_name.as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["energy:admin", "energy:cxo", "energy:finance", "energy:operator", "energy:viewer"]);
    assert_eq!(role(&s, "energy:admin").name, "系統管理員");
    assert_eq!(role(&s, "energy:cxo").name, "高階主管");
    assert_eq!(role(&s, "energy:operator").name, "業務承辦");
    assert_eq!(role(&s, "energy:finance").name, "財務");
    assert_eq!(role(&s, "energy:viewer").name, "唯讀");
}

/// 來源：getRbacSeeds.test.ts「every role permission exists at the role scope (validateSeeds rule #3 precondition)」
#[test]
fn role_permissions_exist_at_role_scope() {
    let s = seed();
    for r in &s.roles {
        assert!(!r.permissions.is_empty(), "{} must not be empty", r.unique_name);
        for pu in &r.permissions {
            assert!(
                s.permissions.iter().any(|p| p.scope == r.scope && &p.unique_name == pu),
                "{} needs {pu}@{:?}",
                r.unique_name,
                r.scope
            );
        }
    }
}

/// 來源：getRbacSeeds.test.ts「only energy:admin holds energy:role:assign-privileged (§2.5.3 upward protection)」
#[test]
fn only_admin_assigns_privileged_roles() {
    let s = seed();
    assert!(grants(&s, "energy:admin").contains(&"energy:role:assign-privileged".to_string()));
    for other in ["energy:cxo", "energy:operator", "energy:finance", "energy:viewer"] {
        assert!(!grants(&s, other).contains(&"energy:role:assign-privileged".to_string()), "{other}");
    }
    // 對外提供的展開函式答案相同。
    assert!(expand_role_permissions("energy:admin").contains(&"energy:role:assign-privileged"));
}

/// 來源：getRbacSeeds.test.ts「energy:viewer resolves to energy:flow:view-all (otherwise the read-only role sees a blank screen)」
#[test]
fn viewer_sees_everything_but_cannot_act() {
    let g = grants(&seed(), "energy:viewer");
    assert!(g.contains(&"energy:flow:view-all".to_string()));
    assert!(g.contains(&"energy:audit:view".to_string()));
    assert!(!g.contains(&"energy:flow:act".to_string()));
}

/// Current seeds additionally reserve master-data management for admin; no CXO privilege escalation.
#[test]
fn admin_also_owns_master_data_management() {
    let s = seed();
    let admin = &role(&s, "energy:admin").permissions;
    let cxo = &role(&s, "energy:cxo").permissions;
    let admin_only: Vec<&String> = admin.iter().filter(|p| !cxo.contains(p)).collect();
    let cxo_only: Vec<&String> = cxo.iter().filter(|p| !admin.contains(p)).collect();
    assert_eq!(admin_only, vec!["energy:govern:system", "energy:master:manage"]);
    assert!(cxo_only.is_empty());
    assert!(admin.contains(&"energy:flow:abort-approve".to_string()));
    assert!(cxo.contains(&"energy:flow:abort-approve".to_string()));
}

/// 來源：getRbacSeeds.test.ts「PRIVILEGED_ROLES lists roles that actually exist」
#[test]
fn privileged_roles_exist() {
    let mut p = PRIVILEGED_ROLES.to_vec();
    p.sort();
    assert_eq!(p, vec!["energy:admin", "energy:cxo"]);
    for r in PRIVILEGED_ROLES {
        assert!(is_energy_role(r), "{r} must be a declared role");
        assert!(is_privileged_energy_role(r));
    }
    assert!(!is_privileged_energy_role("energy:operator"));
    assert!(!is_energy_role("ryxis:org-admin"));
}

/// 來源：getRbacSeeds.test.ts「ORG_GOVERNANCE_PERMISSION is a declared complex held by admin + CXO only (§2.5.4-I7)」
#[test]
fn org_governance_is_held_by_admin_and_cxo() {
    let s = seed();
    assert!(perm(&s, ORG_GOVERNANCE_PERMISSION).complex);
    let mut holders: Vec<&str> = s
        .roles
        .iter()
        .filter(|r| r.permissions.iter().any(|p| p == ORG_GOVERNANCE_PERMISSION))
        .map(|r| r.unique_name.as_str())
        .collect();
    holders.sort();
    assert_eq!(holders, vec!["energy:admin", "energy:cxo"]);
}

/// 來源：getRbacSeeds.test.ts「ENERGY_ROLE_DEFS powers the frontend dropdown (label + description, no uniqueName leak)」
#[test]
fn role_defs_power_the_dropdown() {
    assert_eq!(ENERGY_ROLE_DEFS.len(), 5);
    for d in &ENERGY_ROLE_DEFS {
        assert!(!d.description.is_empty());
        assert!(!d.description.contains("energy:") && !d.description.contains("ryxis:"));
        assert_eq!(get_energy_role_def(d.unique_name), Some(d));
    }
    assert_eq!(get_energy_role_def("energy:nope"), None);
}

/// 總鑰匙在這個 port 改名（由 platform crate seed，不在 energyOps 的 seed 裡）。
/// 來源：無對應 TS 測試；補驗 本 port 的總鑰匙改名（PLATFORM_RBAC_MANAGE）。
#[test]
fn platform_master_key_is_renamed() {
    assert_eq!(PLATFORM_RBAC_MANAGE, "platform:rbac:manage");
    assert!(!seed().permissions.iter().any(|p| p.unique_name == PLATFORM_RBAC_MANAGE));
}
