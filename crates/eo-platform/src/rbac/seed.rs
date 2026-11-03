//! 收集各 plugin 的 seed、驗證、冪等寫入。列只增不刪；plugin 列的 complex 成員與角色權限以宣告為準同步。

use crate::error::{ApiError, ApiResult};
use eo_domain::rbac::{RbacRowType, RbacScope, RbacSeeds};
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::{HashMap, HashSet};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SeedError {
    #[error("permission {scope}:{name} declared by both {a} and {b}")]
    DuplicatePermission { scope: String, name: String, a: String, b: String },
    #[error("role {scope}:{name} declared by both {a} and {b}")]
    DuplicateRole { scope: String, name: String, a: String, b: String },
    #[error("complex permission {0} has no included permissions")]
    EmptyComplex(String),
    #[error("complex permission {complex} includes {included}, which is not a simple permission at the same scope")]
    InvalidInclude { complex: String, included: String },
    #[error("role {role} references {permission}, which is not declared at the same scope")]
    UnknownRolePermission { role: String, permission: String },
}

/// 純驗證：跨 plugin 不重名、complex 非空且只包同 scope 的 simple、角色只綁同 scope 已宣告的 permission。
pub fn validate(seeds: &[RbacSeeds]) -> Result<(), SeedError> {
    let mut perms: HashMap<(RbacScope, &str), (&str, bool)> = HashMap::new();
    for s in seeds {
        for p in &s.permissions {
            if let Some((other, _)) = perms.get(&(p.scope, p.unique_name.as_str())) {
                return Err(SeedError::DuplicatePermission {
                    scope: p.scope.as_str().into(),
                    name: p.unique_name.clone(),
                    a: other.to_string(),
                    b: s.created_by_plugin.clone(),
                });
            }
            perms.insert((p.scope, p.unique_name.as_str()), (s.created_by_plugin.as_str(), p.complex));
        }
    }
    for s in seeds {
        for p in s.permissions.iter().filter(|p| p.complex) {
            if p.included.is_empty() {
                return Err(SeedError::EmptyComplex(p.unique_name.clone()));
            }
            for inc in &p.included {
                match perms.get(&(p.scope, inc.as_str())) {
                    Some((_, false)) => {}
                    _ => {
                        return Err(SeedError::InvalidInclude {
                            complex: p.unique_name.clone(),
                            included: inc.clone(),
                        })
                    }
                }
            }
        }
    }
    let mut roles: HashMap<(RbacScope, &str), &str> = HashMap::new();
    for s in seeds {
        for r in &s.roles {
            if let Some(other) = roles.get(&(r.scope, r.unique_name.as_str())) {
                return Err(SeedError::DuplicateRole {
                    scope: r.scope.as_str().into(),
                    name: r.unique_name.clone(),
                    a: other.to_string(),
                    b: s.created_by_plugin.clone(),
                });
            }
            roles.insert((r.scope, r.unique_name.as_str()), s.created_by_plugin.as_str());
            for p in &r.permissions {
                if !perms.contains_key(&(r.scope, p.as_str())) {
                    return Err(SeedError::UnknownRolePermission { role: r.unique_name.clone(), permission: p.clone() });
                }
            }
        }
    }
    Ok(())
}

async fn upsert_permission(
    tx: &mut Transaction<'_, Postgres>,
    scope: RbacScope,
    unique_name: &str,
    name: &str,
    complex: bool,
    row_type: RbacRowType,
    plugin: &str,
) -> ApiResult<i64> {
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM tbl_rbac_permission WHERE scope = $1 AND unique_name = $2 AND deleted_at IS NULL",
    )
    .bind(scope.as_str())
    .bind(unique_name)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(id) = existing {
        sqlx::query("UPDATE tbl_rbac_permission SET name = $2, complex = $3, type = $4, created_by_plugin = $5 WHERE id = $1")
            .bind(id)
            .bind(name)
            .bind(complex)
            .bind(row_type.as_str())
            .bind(plugin)
            .execute(&mut **tx)
            .await?;
        return Ok(id);
    }
    let id = sqlx::query_scalar(
        "INSERT INTO tbl_rbac_permission (scope, type, created_by_plugin, unique_name, name, complex)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(scope.as_str())
    .bind(row_type.as_str())
    .bind(plugin)
    .bind(unique_name)
    .bind(name)
    .bind(complex)
    .fetch_one(&mut **tx)
    .await?;
    Ok(id)
}

async fn upsert_role(
    tx: &mut Transaction<'_, Postgres>,
    scope: RbacScope,
    unique_name: &str,
    name: &str,
    row_type: RbacRowType,
    plugin: &str,
) -> ApiResult<i64> {
    let existing: Option<i64> =
        sqlx::query_scalar("SELECT id FROM tbl_rbac_role WHERE scope = $1 AND unique_name = $2 AND deleted_at IS NULL")
            .bind(scope.as_str())
            .bind(unique_name)
            .fetch_optional(&mut **tx)
            .await?;
    if let Some(id) = existing {
        sqlx::query("UPDATE tbl_rbac_role SET name = $2, type = $3, created_by_plugin = $4 WHERE id = $1")
            .bind(id)
            .bind(name)
            .bind(row_type.as_str())
            .bind(plugin)
            .execute(&mut **tx)
            .await?;
        return Ok(id);
    }
    let id = sqlx::query_scalar(
        "INSERT INTO tbl_rbac_role (scope, type, created_by_plugin, unique_name, name) VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(scope.as_str())
    .bind(row_type.as_str())
    .bind(plugin)
    .bind(unique_name)
    .bind(name)
    .fetch_one(&mut **tx)
    .await?;
    Ok(id)
}

/// 把連結表同步成 `want`（多的刪、少的補）。
async fn sync_links(
    tx: &mut Transaction<'_, Postgres>,
    table: &str,
    owner_col: &str,
    member_col: &str,
    owner_id: i64,
    want: &HashSet<i64>,
) -> ApiResult<()> {
    let have: Vec<i64> = sqlx::query_scalar(&format!("SELECT {member_col} FROM {table} WHERE {owner_col} = $1"))
        .bind(owner_id)
        .fetch_all(&mut **tx)
        .await?;
    let have: HashSet<i64> = have.into_iter().collect();
    for extra in have.difference(want) {
        sqlx::query(&format!("DELETE FROM {table} WHERE {owner_col} = $1 AND {member_col} = $2"))
            .bind(owner_id)
            .bind(extra)
            .execute(&mut **tx)
            .await?;
    }
    for missing in want.difference(&have) {
        sqlx::query(&format!("INSERT INTO {table} ({owner_col}, {member_col}) VALUES ($1, $2)"))
            .bind(owner_id)
            .bind(missing)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// 驗證後在單一交易內寫入。回傳 (permission 數, 角色數)。
pub async fn apply(pool: &PgPool, seeds: &[RbacSeeds]) -> ApiResult<(usize, usize)> {
    validate(seeds).map_err(|e| ApiError::internal(format!("rbac seed invalid: {e}")))?;
    let mut tx = pool.begin().await?;
    // 多個行程同時開機時排隊。
    sqlx::query("SELECT pg_advisory_xact_lock(727001)").execute(&mut *tx).await?;
    let mut ids: HashMap<(RbacScope, String), i64> = HashMap::new();
    let mut n_perm = 0;
    let mut n_role = 0;
    for s in seeds {
        let row_type = if s.created_by_plugin == "rbac" { RbacRowType::System } else { RbacRowType::Plugin };
        for p in &s.permissions {
            let id = upsert_permission(&mut tx, p.scope, &p.unique_name, &p.name, p.complex, row_type, &s.created_by_plugin)
                .await?;
            ids.insert((p.scope, p.unique_name.clone()), id);
            n_perm += 1;
        }
    }
    for s in seeds {
        for p in s.permissions.iter().filter(|p| p.complex) {
            let cid = ids[&(p.scope, p.unique_name.clone())];
            let want: HashSet<i64> = p.included.iter().map(|i| ids[&(p.scope, i.clone())]).collect();
            sync_links(&mut tx, "mn_rbac_complex_permission", "complex_one_id", "simple_one_id", cid, &want).await?;
        }
    }
    for s in seeds {
        let row_type = if s.created_by_plugin == "rbac" { RbacRowType::System } else { RbacRowType::Plugin };
        for r in &s.roles {
            let rid = upsert_role(&mut tx, r.scope, &r.unique_name, &r.name, row_type, &s.created_by_plugin).await?;
            let want: HashSet<i64> = r.permissions.iter().map(|p| ids[&(r.scope, p.clone())]).collect();
            sync_links(&mut tx, "mn_rbac_role_permission", "role_id", "permission_id", rid, &want).await?;
            n_role += 1;
        }
    }
    tx.commit().await?;
    Ok((n_perm, n_role))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> RbacSeeds {
        RbacSeeds::new("a")
            .simple(RbacScope::Organization, "a:x:view", "x")
            .simple(RbacScope::Organization, "a:x:edit", "x")
            .complex(RbacScope::Organization, "a:x:all", "all", &["a:x:view", "a:x:edit"])
            .role(RbacScope::Organization, "a:admin", "admin", &["a:x:all"])
    }

    /// 來源：az RbacSeedManager.spec.ts「accepts valid seeds」
    #[test]
    fn valid_seed_passes() {
        assert_eq!(validate(&[base()]), Ok(()));
    }

    /// 來源：az RbacSeedManager.spec.ts「rejects cross-plugin duplicate」
    #[test]
    fn duplicate_across_plugins_fails() {
        let b = RbacSeeds::new("b").simple(RbacScope::Organization, "a:x:view", "dup");
        assert!(matches!(validate(&[base(), b]), Err(SeedError::DuplicatePermission { .. })));
    }

    /// 同名不同 scope 不算重複。
    #[test]
    fn same_name_other_scope_is_fine() {
        let b = RbacSeeds::new("b").simple(RbacScope::Project, "a:x:view", "dup");
        assert_eq!(validate(&[base(), b]), Ok(()));
    }

    /// 來源：az RbacSeedManager.spec.ts「rejects empty complex」
    #[test]
    fn empty_complex_fails() {
        let s = RbacSeeds::new("a").complex(RbacScope::Organization, "a:c", "c", &[]);
        assert!(matches!(validate(&[s]), Err(SeedError::EmptyComplex(_))));
    }

    /// 來源：az RbacSeedManager.spec.ts「rejects complex including complex」
    #[test]
    fn complex_including_complex_fails() {
        let s = base().complex(RbacScope::Organization, "a:c2", "c2", &["a:x:all"]);
        assert!(matches!(validate(&[s]), Err(SeedError::InvalidInclude { .. })));
    }

    /// 來源：az RbacSeedManager.spec.ts「rejects role binding permission from another scope」
    #[test]
    fn role_with_other_scope_permission_fails() {
        let s = base().role(RbacScope::Project, "a:p", "p", &["a:x:view"]);
        assert!(matches!(validate(&[s]), Err(SeedError::UnknownRolePermission { .. })));
    }
}
