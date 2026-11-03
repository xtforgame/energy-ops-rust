//! 組織治理（部門、角色指派與向上保護、最後一位治理者、成員）、能源 RBAC seed、稽核寫入與查詢、領域表約束。

use crate::support::energy::code_of;
use crate::support::{TestApp, TestUser};
use eo_domain::energy_rbac::{energy_rbac_seeds, ENERGY_COMPLEX_PERMISSIONS, ENERGY_ROLE_DEFS, ENERGY_SIMPLE_PERMISSIONS};
use eo_energy::audit::{self, AuditInput};
use eo_energy::common::Ctx;
use eo_platform::rbac::ScopeRef;
use reqwest::Method;
use serde_json::{json, Value};

struct Fx {
    app: TestApp,
    org: i64,
    admin: TestUser,
    cxo: TestUser,
}

async fn fixture() -> Fx {
    let app = TestApp::new().await;
    let (admin, org) = app.energy_org("org-gov").await;
    let cxo = app.member(org, "cxo", Some("energy:cxo")).await;
    Fx { app, org, admin, cxo }
}

/// 來源：departments.test.ts「creates departments (CXO holds energy:department:manage via energy:govern:org)」
/// 來源：departments.test.ts「rejects a duplicate key inside the same org (409)」
/// 來源：departments.test.ts「rejects an invalid key shape (400, shared zod schema)」
/// 來源：departments.test.ts「assigns a manager and a member; one department can hold several managers」
/// 來源：departments.test.ts「re-assigning an existing member updates the role in place (no duplicate row)」
/// 來源：departments.test.ts「rejects assigning a non-existent user (404)」
/// 來源：departments.test.ts「renames a department; key is immutable (not in the update schema)」
/// 來源：departments.test.ts「removes a department member」
/// 來源：departments.test.ts「cross-tenant: the org is derived from the department row, not from the URL」
/// 來源：departments.test.ts「a user with no energy grant gets 403 (fail-closed)」
/// 來源：departments.test.ts「deletes a department and its membership rows (no orphans)」
#[actix_web::test]
async fn departments_crud_over_http() {
    let f = fixture().await;
    let app = &f.app;
    let alice = app.member(f.org, "Alice", None).await;
    let bob = app.member(f.org, "Bob", None).await;
    let url = |p: &str| app.e(f.org, p);
    let mk = |key: &str, name: &str, sort: i64| json!({ "key": key, "name": name, "sortOrder": sort });
    let sales = expect_status!(app.post(&f.cxo, &url("/departments"), mk("sales", "業務部", 1)).await, 200)["id"].as_i64().unwrap();
    let finance = expect_status!(app.post(&f.cxo, &url("/departments"), mk("finance", "會計部門", 2)).await, 200)["id"].as_i64().unwrap();
    let v = expect_status!(app.post(&f.cxo, &url("/departments"), mk("sales", "業務部（重複）", 0)).await, 409);
    assert_eq!(v["code"], "department_key_taken");
    expect_status!(app.post(&f.cxo, &url("/departments"), mk("業務部", "業務部", 0)).await, 400);

    let set = |d: i64, u: &TestUser, role: &str| (format!("/departments/{d}/members/{}", u.id), json!({ "role": role }));
    for (d, u, r) in [(sales, &alice, "manager"), (sales, &bob, "member"), (finance, &alice, "manager"), (finance, &bob, "manager")] {
        let (p, b) = set(d, u, r);
        expect_status!(TestApp::send(app.req(Some(&f.cxo), Method::PUT, &url(&p)).json(&b)).await, 200);
    }
    let list = expect_status!(app.get(&f.cxo, &url("/departments")).await, 200);
    let items = list["items"].as_array().unwrap();
    let keys: Vec<&str> = items.iter().map(|d| d["key"].as_str().unwrap()).collect();
    assert_eq!(keys, ["sales", "finance"], "sortOrder 排序");
    let s = &items[0];
    assert_eq!(s["members"].as_array().unwrap().len(), 2);
    let member = |d: &Value, u: i64| d["members"].as_array().unwrap().iter().find(|m| m["userId"] == u).unwrap().clone();
    assert_eq!((member(s, alice.id)["role"].clone(), member(s, alice.id)["name"].clone()), (json!("manager"), json!("Alice")));
    assert_eq!(member(s, bob.id)["role"], "member");
    assert_eq!(items[1]["members"].as_array().unwrap().iter().filter(|m| m["role"] == "manager").count(), 2, "一個部門可以有多位主管");

    let (p, b) = set(sales, &bob, "manager");
    expect_status!(TestApp::send(app.req(Some(&f.cxo), Method::PUT, &url(&p)).json(&b)).await, 200);
    let list = expect_status!(app.get(&f.cxo, &url("/departments")).await, 200);
    assert_eq!(list["items"][0]["members"].as_array().unwrap().len(), 2, "原地更新，不重複");
    assert_eq!(member(&list["items"][0], bob.id)["role"], "manager");

    let v = expect_status!(
        TestApp::send(app.req(Some(&f.cxo), Method::PUT, &url(&format!("/departments/{sales}/members/999999999"))).json(&json!({}))).await,
        404
    );
    assert_eq!(v["code"], "user_not_found");

    let v = expect_status!(app.patch(&f.cxo, &url(&format!("/departments/{finance}")), json!({ "name": "財會部", "key": "renamed" })).await, 200);
    assert_eq!((v["name"].clone(), v["key"].clone()), (json!("財會部"), json!("finance")));

    let v = expect_status!(app.delete(&f.cxo, &url(&format!("/departments/{sales}/members/{}", bob.id))).await, 200);
    assert_eq!(v["removed"], 1);

    // 跨組織：拿自己的組織過閘、指向別家的部門 → 403。
    let (outsider, other) = app.energy_org("org-gov-other").await;
    let v = expect_status!(app.patch(&outsider, &app.e(other, &format!("/departments/{sales}")), json!({ "name": "被別家公司改掉" })).await, 403);
    assert_eq!(v["code"], "department_not_in_org");
    let stranger = app.member(f.org, "stranger", None).await;
    expect_status!(app.get(&stranger, &url("/departments")).await, 403);

    let v = expect_status!(app.delete(&f.cxo, &url(&format!("/departments/{finance}"))).await, 200);
    assert_eq!(v["removed"], 1);
    let list = expect_status!(app.get(&f.cxo, &url("/departments")).await, 200);
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    let orphans = app.scalar_i64(&format!("SELECT COUNT(*) FROM mn_energy_department_member WHERE department_id = {finance} AND deleted_at IS NULL")).await;
    assert_eq!(orphans, 0, "成員列跟著軟刪除");
}

async fn holds(app: &TestApp, org: i64, user: &TestUser, role: &str) -> bool {
    app.scalar_i64(&format!(
        "SELECT COUNT(*) FROM tbl_user_rbac_role g JOIN tbl_rbac_role r ON r.id = g.role_id
          WHERE g.user_id = {} AND g.organization_id = {org} AND r.unique_name = '{role}' AND g.deleted_at IS NULL",
        user.id
    ))
    .await
        > 0
}

/// 來源：roleAssignment.test.ts「lists the five named roles in Chinese; no ryxis:/energy: string in the labels」
/// 來源：roleAssignment.test.ts「tells a CXO it cannot assign privileged roles (frontend disables those options)」
/// 來源：roleAssignment.test.ts「CXO assigning energy:operator → 200」
/// 來源：roleAssignment.test.ts「CXO assigning energy:cxo → 403 cannot_assign_privileged_role」
/// 來源：roleAssignment.test.ts「CXO assigning energy:admin → 403 too (both roles are privileged)」
/// 來源：roleAssignment.test.ts「admin assigning energy:cxo → 200」
/// 來源：roleAssignment.test.ts「rejects unknown role names (400) — the endpoint only speaks the five energy roles」
/// 來源：roleAssignment.test.ts「a user without member:manage gets 403 (fail-closed)」
/// 來源：roleAssignment.test.ts「CXO cannot revoke a privileged role either (otherwise it could demote the admin)」
/// 來源：roleAssignment.test.ts「admin can revoke a normal role」
/// 來源：roleAssignment.test.ts「grants never leak across organizations (server-derived org, never client-supplied)」
#[actix_web::test]
async fn role_assignment_with_upward_protection() {
    let f = fixture().await;
    let app = &f.app;
    let url = |p: &str| app.e(f.org, p);
    let t1 = app.member(f.org, "t1", None).await;
    let t2 = app.member(f.org, "t2", None).await;
    let plain = app.member(f.org, "plain", None).await;

    let v = expect_status!(app.get(&f.admin, &url("/assignable-roles")).await, 200);
    let roles = v["items"].as_array().unwrap();
    assert_eq!(roles.len(), 5);
    let mut names: Vec<&str> = roles.iter().map(|r| r["name"].as_str().unwrap()).collect();
    names.sort();
    let mut want = vec!["系統管理員", "高階主管", "業務承辦", "財務", "唯讀"];
    want.sort();
    assert_eq!(names, want);
    for r in roles {
        assert!(!r["name"].as_str().unwrap().contains("energy:") && !r["description"].as_str().unwrap().contains("energy:"));
        assert!(r["id"].is_i64());
    }
    assert_eq!(v["canAssignPrivileged"], true);
    let v = expect_status!(app.get(&f.cxo, &url("/assignable-roles")).await, 200);
    assert_eq!(v["canAssignPrivileged"], false);
    assert_eq!(v["items"].as_array().unwrap().len(), 5, "前端 disable 只是 UX，真正的閘在指派時");

    let assign = |u: &TestUser, role: &str| json!({ "userId": u.id, "roleUniqueName": role });
    expect_status!(app.post(&f.cxo, &url("/role-assignments"), assign(&t1, "energy:operator")).await, 200);
    assert!(holds(app, f.org, &t1, "energy:operator").await);
    for role in ["energy:cxo", "energy:admin"] {
        let v = expect_status!(app.post(&f.cxo, &url("/role-assignments"), assign(&t2, role)).await, 403);
        assert_eq!(v["code"], "cannot_assign_privileged_role");
        assert!(!holds(app, f.org, &t2, role).await);
    }
    expect_status!(app.post(&f.admin, &url("/role-assignments"), assign(&t2, "energy:cxo")).await, 200);
    assert!(holds(app, f.org, &t2, "energy:cxo").await);
    let v = expect_status!(app.post(&f.admin, &url("/role-assignments"), assign(&t1, "platform:org-admin")).await, 400);
    assert_eq!(v["code"], "unknown_role");
    expect_status!(app.post(&plain, &url("/role-assignments"), assign(&t1, "energy:operator")).await, 403);

    let v = expect_status!(app.post(&f.cxo, &url("/role-assignments/revoke"), assign(&f.admin, "energy:admin")).await, 403);
    assert_eq!(v["code"], "cannot_assign_privileged_role");
    assert!(holds(app, f.org, &f.admin, "energy:admin").await);
    let v = expect_status!(app.post(&f.admin, &url("/role-assignments/revoke"), assign(&t1, "energy:operator")).await, 200);
    assert!(v["revoked"].as_i64().unwrap() > 0);
    assert!(!holds(app, f.org, &t1, "energy:operator").await);

    let (_o, other) = app.energy_org("ra-other").await;
    expect_status!(app.post(&f.admin, &app.e(other, "/role-assignments"), assign(&t1, "energy:operator")).await, 403);
}

/// 來源：roleAssignment.test.ts「AcEnergyOps.isLastOrgGovernor sees the lone energy:admin」
/// 來源：roleAssignment.test.ts「revoking the last energy:admin → 400 cannot_remove_last_admin」
/// 來源：roleAssignment.test.ts「tenant-console orgMemberRemove also refuses to remove the last energy:admin」
/// 來源：roleAssignment.test.ts「the guard fires for a caller who IS allowed to manage members」
#[actix_web::test]
async fn last_governor_guard() {
    let app = TestApp::new().await;
    let solo_admin = app.user("solo admin", None).await;
    let org = app.org(&solo_admin, "solo-org").await;
    app.grant_role(org, &solo_admin, "energy:admin").await;
    // 這個組織只有 energy:admin，沒有平台的組織管理員 —— 硬編平台角色的判定會算出「0 個治理者」而放行。
    app.exec_sql(&format!(
        "UPDATE tbl_user_rbac_role SET deleted_at = now() WHERE organization_id = {org} AND role_id = (SELECT id FROM tbl_rbac_role WHERE unique_name = 'platform:org-admin')"
    ))
    .await;
    app.core.rbac.invalidate_all();
    let solo_member = app.member(org, "solo member", None).await;
    let url = |p: &str| app.e(org, p);
    let body = |u: &TestUser, role: &str| json!({ "userId": u.id, "roleUniqueName": role });

    let v = expect_status!(app.post(&solo_admin, &url("/role-assignments/revoke"), body(&solo_admin, "energy:admin")).await, 400);
    assert_eq!(v["code"], "cannot_remove_last_admin");
    assert!(holds(&app, org, &solo_admin, "energy:admin").await);

    // 超級使用者繞過權限閘但不繞過防呆。
    let staff = app.admin().await;
    let v = expect_status!(app.delete(&staff, &url(&format!("/members/{}", solo_admin.id))).await, 400);
    assert_eq!(v["code"], "cannot_remove_last_admin");
    assert!(holds(&app, org, &solo_admin, "energy:admin").await, "整筆 rollback");
    expect_status!(app.delete(&staff, &url(&format!("/members/{}", solo_member.id))).await, 200);
    let member_left = app.scalar_i64(&format!("SELECT COUNT(*) FROM mn_user_organization WHERE user_id = {} AND organization_id = {org} AND deleted_at IS NULL", solo_member.id)).await;
    assert_eq!(member_left, 0);

    // 有第二位治理者時就放行；撤掉他之後又回到「唯一治理者」。
    let second = app.member(org, "second", Some("energy:cxo")).await;
    expect_status!(app.post(&solo_admin, &url("/role-assignments/revoke"), body(&second, "energy:cxo")).await, 200);
    let v = expect_status!(app.post(&solo_admin, &url("/role-assignments/revoke"), body(&solo_admin, "energy:admin")).await, 400);
    assert_eq!(v["code"], "cannot_remove_last_admin");
    // 平台的萬能鑰匙也算治理權（energy:govern:org ∪ platform:rbac:manage）。
    let platform = app.member(org, "platform admin", Some("platform:org-admin")).await;
    expect_status!(app.post(&solo_admin, &url("/role-assignments/revoke"), body(&solo_admin, "energy:admin")).await, 200);
    let _ = platform;
}

/// 能源成員清單：成員＋能源角色＋部門；移除成員會一併清掉本組織的部門成員列並留稽核。
#[actix_web::test]
async fn members_list_and_remove() {
    let f = fixture().await;
    let app = &f.app;
    let op = app.member(f.org, "operator", Some("energy:operator")).await;
    let d = app.department(f.org, "sales", "業務部").await;
    app.dept_member(d, &op, "manager").await;
    let v = expect_status!(app.get(&f.cxo, &app.e(f.org, "/members")).await, 200);
    let m = v["items"].as_array().unwrap().iter().find(|m| m["userId"] == op.id).unwrap().clone();
    assert_eq!(m["energyRoles"][0]["uniqueName"], "energy:operator");
    assert_eq!(m["energyRoles"][0]["name"], "業務承辦");
    assert_eq!((m["departments"][0]["key"].clone(), m["departments"][0]["role"].clone()), (json!("sales"), json!("manager")));
    expect_status!(app.get(&op, &app.e(f.org, "/members")).await, 403);
    expect_status!(app.delete(&f.cxo, &app.e(f.org, &format!("/members/{}", op.id))).await, 200);
    let left = app.scalar_i64(&format!("SELECT COUNT(*) FROM mn_energy_department_member WHERE user_id = {} AND deleted_at IS NULL", op.id)).await;
    assert_eq!(left, 0);
    let revoked: Vec<Value> = sqlx::query_scalar(
        "SELECT payload FROM tbl_energy_audit_event WHERE organization_id = $1 AND type = 'member.role_changed' AND target_id = $2",
    )
    .bind(f.org)
    .bind(op.id.to_string())
    .fetch_all(&app.core.pool)
    .await
    .unwrap();
    assert!(revoked.iter().any(|p| p["action"] == "revoked" && p["roleUniqueName"] == "energy:operator"));
    let v = expect_status!(app.delete(&f.cxo, &app.e(f.org, &format!("/members/{}", f.cxo.id))).await, 400);
    assert_eq!(v["code"], "cannot_remove_self");
}

/// 來源：rbacSeedBoot.test.ts「upserts the five energy roles with type="plugin" / created_by_plugin="energyOps"」
/// 來源：rbacSeedBoot.test.ts「upserts every energy permission at organization scope with the right complex flag」
/// 來源：rbacSeedBoot.test.ts「expands each complex permission into rbacComplexPermission rows」
/// 來源：rbacSeedBoot.test.ts「CXO can govern the org but must NOT be able to assign privileged roles (§2.5.3)」
/// 來源：rbacSeedBoot.test.ts「admin resolves energy:role:assign-privileged (the only real admin/CXO divide)」
/// 來源：rbacSeedBoot.test.ts「viewer resolves energy:flow:view-all (otherwise the read-only role is a blank screen)」
/// 來源：rbacSeedBoot.test.ts「grants do not leak across organizations」
/// 來源：rbacSeedBoot.test.ts「is idempotent: a second boot duplicates neither permissions nor roles」
#[actix_web::test]
async fn energy_rbac_seed_boot() {
    let f = fixture().await;
    let pool = &f.app.core.pool;
    for r in ENERGY_ROLE_DEFS {
        let (t, by, name): (String, Option<String>, String) =
            sqlx::query_as("SELECT type, created_by_plugin, name FROM tbl_rbac_role WHERE scope = 'organization' AND unique_name = $1")
                .bind(r.unique_name)
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!((t.as_str(), by.as_deref(), name.as_str()), ("plugin", Some("energyOps"), r.name));
    }
    for (p, complex) in ENERGY_SIMPLE_PERMISSIONS.iter().map(|p| (p.0, false)).chain(ENERGY_COMPLEX_PERMISSIONS.iter().map(|c| (c.unique_name, true))) {
        let (by, c): (Option<String>, bool) =
            sqlx::query_as("SELECT created_by_plugin, complex FROM tbl_rbac_permission WHERE scope = 'organization' AND unique_name = $1")
                .bind(p)
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!((by.as_deref(), c), (Some("energyOps"), complex), "{p}");
    }
    for c in ENERGY_COMPLEX_PERMISSIONS {
        let mut members: Vec<String> = sqlx::query_scalar(
            "SELECT s.unique_name FROM mn_rbac_complex_permission m
               JOIN tbl_rbac_permission p ON p.id = m.complex_one_id JOIN tbl_rbac_permission s ON s.id = m.simple_one_id
              WHERE p.unique_name = $1 AND p.scope = 'organization'",
        )
        .bind(c.unique_name)
        .fetch_all(pool)
        .await
        .unwrap();
        members.sort();
        let mut want: Vec<String> = c.includes.iter().map(|s| s.to_string()).collect();
        want.sort();
        assert_eq!(members, want, "{}", c.unique_name);
    }
    let viewer = f.app.member(f.org, "viewer", Some("energy:viewer")).await;
    let rbac = &f.app.core.rbac;
    let s = ScopeRef::org(f.org);
    assert!(rbac.has(f.cxo.id, &s, &["energy:member:manage"]).await.unwrap());
    assert!(!rbac.has(f.cxo.id, &s, &["energy:role:assign-privileged"]).await.unwrap());
    assert!(rbac.has(f.admin.id, &s, &["energy:role:assign-privileged"]).await.unwrap());
    assert!(rbac.has(viewer.id, &s, &["energy:flow:view-all"]).await.unwrap());
    assert!(!rbac.has(viewer.id, &s, &["energy:flow:act"]).await.unwrap());
    let (_o, other) = f.app.energy_org("seed-other").await;
    assert!(!rbac.has(f.cxo.id, &ScopeRef::org(other), &["energy:member:manage"]).await.unwrap());

    let count = |t: &'static str| async move {
        sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {t} WHERE created_by_plugin = 'energyOps'")).fetch_one(pool).await.unwrap()
    };
    let (p0, r0) = (count("tbl_rbac_permission").await, count("tbl_rbac_role").await);
    eo_platform::rbac::seed::apply(pool, &[eo_platform::plugins::rbac::platform_seeds(), energy_rbac_seeds()]).await.unwrap();
    assert_eq!((count("tbl_rbac_permission").await, count("tbl_rbac_role").await), (p0, r0), "第二次開機不重複");
    assert_eq!(p0 as usize, ENERGY_SIMPLE_PERMISSIONS.len() + ENERGY_COMPLEX_PERMISSIONS.len());
    assert_eq!(r0 as usize, ENERGY_ROLE_DEFS.len());
}

async fn audit_ctx(app: &TestApp, org: i64) -> Ctx {
    Ctx::system(&app.core, org)
}

/// 來源：auditLogger.test.ts「rejects an unregistered event type (whitelist)」
/// 來源：auditLogger.test.ts「rejects a payload that fails the per-type schema」
/// 來源：auditLogger.test.ts「drops payload keys the schema did not declare (they never reach the DB)」
/// 來源：auditLogger.test.ts「fills targetType from the registry and derives actorKind / occurredAt」
/// 來源：auditLogger.test.ts「records denied outcomes too (拒絕也要留痕)」
/// 來源：auditLogger.test.ts「recordBestEffort refuses to downgrade a fail-closed type」
/// 來源：auditLogger.test.ts「a failing audit write rolls back the business write (fail-closed is real)」
#[actix_web::test]
async fn audit_logger_whitelist_and_fail_closed() {
    let f = fixture().await;
    let app = &f.app;
    let sys = audit_ctx(app, f.org).await;
    let admin = app.ctx(&f.admin, f.org).await;
    let r = in_tx!(app, tx => audit::record(&mut tx, &sys, AuditInput::new("totally.made.up", 1, "x")).await);
    assert_eq!(code_of(r).1, "unknown_audit_event_type");
    let r = in_tx!(app, tx => audit::record(&mut tx, &sys, AuditInput::new("member.role_changed", 1, "x").payload(json!({ "action": "exploded" }))).await);
    assert_eq!(code_of(r).1, "invalid_audit_payload");

    let id = in_tx!(app, tx => audit::record(&mut tx, &admin, AuditInput::new("member.role_changed", f.cxo.id, "測試白名單")
        .payload(json!({ "action": "granted", "roleUniqueName": "energy:viewer", "nationalId": "A123456789" }))).await)
    .unwrap();
    let p: Value = sqlx::query_scalar("SELECT payload FROM tbl_energy_audit_event WHERE id = $1").bind(id).fetch_one(&app.core.pool).await.unwrap();
    let mut keys: Vec<&String> = p.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["action", "roleUniqueName"]);
    assert!(!p.to_string().contains("A123456789"));

    let id = in_tx!(app, tx => audit::record(&mut tx, &sys, AuditInput::new("department.created", 999, "預設值測試").payload(json!({ "departmentKey": "k" }))).await).unwrap();
    let row: (String, String, String, String) =
        sqlx::query_as("SELECT target_type, target_id, actor_kind, outcome FROM tbl_energy_audit_event WHERE id = $1").bind(id).fetch_one(&app.core.pool).await.unwrap();
    assert_eq!(row, ("energyDepartment".into(), "999".into(), "system".into(), "success".into()));

    let id = in_tx!(app, tx => audit::record(&mut tx, &admin, AuditInput::new("member.role_changed", f.cxo.id, "嘗試指派但被擋")
        .outcome("denied").payload(json!({ "action": "granted", "roleUniqueName": "energy:admin" }))).await)
    .unwrap();
    let outcome: String = sqlx::query_scalar("SELECT outcome FROM tbl_energy_audit_event WHERE id = $1").bind(id).fetch_one(&app.core.pool).await.unwrap();
    assert_eq!(outcome, "denied");

    let r = audit::record_best_effort(&app.core.pool, &sys, AuditInput::new("department.created", 1, "x").payload(json!({ "departmentKey": "k" }))).await;
    assert_eq!(code_of(r).1, "audit_type_is_fail_closed");

    // 稽核寫不進去 → 業務寫入跟著消失。
    let mut tx = app.core.pool.begin().await.unwrap();
    sqlx::query("INSERT INTO tbl_energy_department (organization_id, key, name) VALUES ($1, 'rollback-me', '應該要消失的部門')")
        .bind(f.org)
        .execute(&mut *tx)
        .await
        .unwrap();
    let r = audit::record(&mut tx, &sys, AuditInput::new("department.created", 1, "x").payload(json!({ "departmentKey": 123 }))).await;
    assert!(r.is_err());
    drop(tx);
    let n = app.scalar_i64(&format!("SELECT COUNT(*) FROM tbl_energy_department WHERE organization_id = {} AND key = 'rollback-me'", f.org)).await;
    assert_eq!(n, 0, "部門必須跟著稽核一起消失");
}

/// 來源：auditLogger.test.ts「department create / rename / member assign / delete each leave an audit row」
/// 來源：auditLogger.test.ts「actor comes from the session — a forged body.userId is ignored, not honoured」
/// 來源：auditLogger.test.ts「role grant and revoke each leave a member.role_changed row」
/// 來源：auditLogger.test.ts「the audit query endpoint is org-scoped and filterable」
/// 來源：auditLogger.test.ts「audit is org-scoped: another org sees none of these rows」
/// 來源：auditLogger.test.ts「a user without energy:audit:view gets 403」
#[actix_web::test]
async fn audit_trail_of_write_paths_and_query() {
    let f = fixture().await;
    let app = &f.app;
    let url = |p: &str| app.e(f.org, p);
    let alice = app.member(f.org, "alice", None).await;
    let d = expect_status!(app.post(&f.admin, &url("/departments"), json!({ "key": "audit-dept", "name": "稽核測試部" })).await, 200)["id"]
        .as_i64()
        .unwrap();
    expect_status!(app.patch(&f.admin, &url(&format!("/departments/{d}")), json!({ "name": "稽核測試部（改名）" })).await, 200);
    for role in ["member", "manager"] {
        expect_status!(
            TestApp::send(app.req(Some(&f.admin), Method::PUT, &url(&format!("/departments/{d}/members/{}", alice.id))).json(&json!({ "role": role }))).await,
            200
        );
    }
    expect_status!(app.delete(&f.admin, &url(&format!("/departments/{d}"))).await, 200);
    let rows = app.audit_rows(f.org, "energyDepartment", d).await;
    let types: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(types, ["department.created", "department.updated", "department.member_added", "department.manager_changed", "department.deleted"]);
    let renamed = &rows[1].1;
    assert!(renamed["changes"].get("name").is_some());
    assert!(renamed["changes"].get("sortOrder").is_none(), "只含真的變了的欄位");
    assert_eq!((rows[3].1["previousRole"].clone(), rows[3].1["role"].clone()), (json!("member"), json!("manager")));

    // 偽造的 body 身分一律忽略；IP 與 trace 由請求帶入。
    let v = expect_status!(
        TestApp::send(
            app.req(Some(&f.admin), Method::POST, &url("/departments"))
                .header("x-forwarded-for", "203.0.113.7")
                .header("x-request-id", "trace-abc")
                .json(&json!({ "key": "forge", "name": "偽造 actor 測試", "userId": alice.id, "actorId": alice.id }))
        )
        .await,
        200
    );
    let row: (Option<i64>, String, Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT actor_user_id, actor_kind, actor_label, request_ip, trace_id FROM tbl_energy_audit_event
          WHERE target_type = 'energyDepartment' AND target_id = $1",
    )
    .bind(v["id"].as_i64().unwrap().to_string())
    .fetch_one(&app.core.pool)
    .await
    .unwrap();
    assert_eq!(row.0, Some(f.admin.id), "actor 必須是 session 的人");
    assert_eq!(row.1, "user");
    assert!(row.2.is_some(), "actorLabel 是當下快照");
    assert_eq!(row.3.as_deref(), Some("203.0.113.7"));
    assert_eq!(row.4.as_deref(), Some("trace-abc"));

    let ra = |role: &str, reason: &str| json!({ "userId": alice.id, "roleUniqueName": role, "reason": reason });
    expect_status!(app.post(&f.admin, &url("/role-assignments"), ra("energy:operator", "新人報到")).await, 200);
    expect_status!(app.post(&f.admin, &url("/role-assignments/revoke"), ra("energy:operator", "轉調")).await, 200);
    let rows = app.audit_rows(f.org, "user", alice.id).await;
    let granted = rows.iter().find(|r| r.1["action"] == "granted").expect("granted");
    assert!(rows.iter().any(|r| r.1["action"] == "revoked"));
    assert_eq!((granted.1["roleUniqueName"].clone(), granted.1["roleName"].clone(), granted.1["reason"].clone()), (json!("energy:operator"), json!("業務承辦"), json!("新人報到")));
    assert!(granted.1["grantId"].as_i64().unwrap() > 0);

    let v = expect_status!(app.get(&f.admin, &url("/audit-events?type=member.role_changed")).await, 200);
    let items = v["items"].as_array().unwrap();
    assert!(!items.is_empty());
    for e in items {
        assert_eq!((e["type"].clone(), e["severity"].clone(), e["summaryKey"].clone()), (json!("member.role_changed"), json!("critical"), json!("audit.type.member.roleChanged")));
        assert!(e["actorName"].is_string(), "操作者要顯示人名");
    }
    let actors = expect_status!(app.get(&f.admin, &url("/audit-actors")).await, 200);
    assert!(actors["items"].as_array().unwrap().iter().any(|a| a["id"] == f.admin.id));

    let (other_owner, other) = app.energy_org("audit-other").await;
    let v = expect_status!(app.get(&other_owner, &app.e(other, "/audit-events")).await, 200);
    assert!(v["items"].as_array().unwrap().is_empty());
    expect_status!(app.get(&alice, &url("/audit-events")).await, 403);
}

/// 來源：domainModel.test.ts「customerNumber is unique within an org (it is the system-wide join key)」
/// 來源：domainModel.test.ts「the same customerNumber in a DIFFERENT org is fine (tenant isolation)」
/// 來源：domainModel.test.ts「a bill with NO station can be created — reviewStatus=pendingMatch」
/// 來源：domainModel.test.ts「a contract may bind only a landowner (station is nullable by design)」
/// 來源：domainModel.test.ts「installments are unique per (contract, seq) and feed the calendar」
/// 來源：domainModel.test.ts「execution keeps stationKey AND gains a joinable station FK」
/// 來源：domainModel.test.ts「the tenant-boundary FKs really are NOT NULL, but bill.station is NOT」
#[actix_web::test]
async fn domain_model_constraints() {
    let f = fixture().await;
    let app = &f.app;
    let lo = app.landowner(f.org, "王小明").await;
    app.station(f.org, lo, "01-2345-6789", "一號").await;
    let dup = sqlx::query("INSERT INTO tbl_energy_station (organization_id, landowner_id, customer_number, name) VALUES ($1, $2, '01-2345-6789', '重複')")
        .bind(f.org)
        .bind(lo)
        .execute(&app.core.pool)
        .await;
    assert!(dup.is_err(), "同 org 同電號必須被擋");
    let (_o, other) = app.energy_org("dm-other").await;
    let lo2 = app.landowner(other, "別家").await;
    assert!(app.station(other, lo2, "01-2345-6789", "別家一號").await > 0, "別的組織同電號沒問題");

    let orphan = app.bill(f.org, None, "2026-01-01", "2026-01-31", Some("100"), None).await;
    let (st, rs): (Option<i64>, String) = sqlx::query_as("SELECT station_id, review_status FROM tbl_energy_bill WHERE id = $1").bind(orphan).fetch_one(&app.core.pool).await.unwrap();
    assert_eq!((st, rs.as_str()), (None, "pendingMatch"));

    let c = app.contract(f.org, lo, "CT-ONLY-OWNER", "2026-01-01", None).await;
    assert_eq!(app.scalar_i64(&format!("SELECT COUNT(*) FROM mn_energy_contract_station WHERE contract_id = {c}")).await, 0, "合約可以只綁地主");
    app.installment(f.org, c, 1, "2026-03-31", "1000").await;
    let dup = sqlx::query("INSERT INTO tbl_energy_installment (organization_id, contract_id, seq, due_date, amount) VALUES ($1, $2, 1, '2026-04-30', 1)")
        .bind(f.org)
        .bind(c)
        .execute(&app.core.pool)
        .await;
    assert!(dup.is_err(), "同一份合約的第 N 期只能有一列");

    let cols: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT table_name::text, column_name::text, is_nullable::text FROM information_schema.columns
          WHERE table_name IN ('tbl_energy_landowner', 'tbl_energy_station', 'tbl_energy_bill', 'tbl_energy_workflow_execution', 'tbl_energy_contract')",
    )
    .fetch_all(&app.core.pool)
    .await
    .unwrap();
    let null = |t: &str, c: &str| cols.iter().find(|x| x.0 == t && x.1 == c).map(|x| x.2.clone());
    assert!(null("tbl_energy_workflow_execution", "station_key").is_some() && null("tbl_energy_workflow_execution", "station_id").is_some());
    assert_eq!(null("tbl_energy_landowner", "organization_id").as_deref(), Some("NO"));
    assert_eq!(null("tbl_energy_station", "organization_id").as_deref(), Some("NO"));
    assert_eq!(null("tbl_energy_station", "landowner_id").as_deref(), Some("NO"));
    assert_eq!(null("tbl_energy_bill", "organization_id").as_deref(), Some("NO"));
    assert_eq!(null("tbl_energy_bill", "station_id").as_deref(), Some("YES"), "例外件的命脈");
    assert!(null("tbl_energy_contract", "station_id").is_none(), "合約不再直接掛案場");
}
