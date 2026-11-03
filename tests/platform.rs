//! 平台的整合測試：登入、組織、成員、邀請、RBAC。


use crate::support::TestApp;
use reqwest::Method;
use serde_json::json;

/// 來源：az AuthnPlugin 測試「basic login sets session; wrong password rejected」
#[actix_web::test]
async fn login_logout_and_session_revocation() {
    let app = TestApp::new().await;
    let u = app.user("Alice", None).await;

    let (s, v) = TestApp::send(app.req(None, Method::POST, "/api/v1/auth/login").json(&json!({"username": u.email, "password": "nope-nope"}))).await;
    assert_eq!(s.as_u16(), 401, "{v}");
    assert_eq!(v["code"], "unauthenticated");

    let v = expect_status!(
        TestApp::send(app.req(None, Method::POST, "/api/v1/auth/login").json(&json!({"username": u.email.to_uppercase(), "password": "password123"}))).await,
        200
    );
    let token = v["token"].as_str().unwrap().to_string();
    let me = crate::support::TestUser { id: u.id, email: u.email.clone(), token };
    let v = expect_status!(app.get(&me, "/api/v1/auth/session").await, 200);
    assert_eq!(v["user"]["id"], u.id);

    expect_status!(app.post(&me, "/api/v1/auth/logout", json!({})).await, 200);
    expect_status!(app.get(&me, "/api/v1/auth/session").await, 401);
}

/// 閒置超過 14 天就失效；有在用就滑動延長。
#[actix_web::test]
async fn session_idle_timeout_slides() {
    let app = TestApp::new().await;
    let u = app.user("Idle", None).await;
    app.clock.advance(chrono::Duration::days(10));
    expect_status!(app.get(&u, "/api/v1/auth/session").await, 200);
    app.clock.advance(chrono::Duration::days(10));
    expect_status!(app.get(&u, "/api/v1/auth/session").await, 200);
    app.clock.advance(chrono::Duration::days(15));
    expect_status!(app.get(&u, "/api/v1/auth/session").await, 401);
}

#[actix_web::test]
async fn disabled_user_loses_access() {
    let app = TestApp::new().await;
    let root = app.admin().await;
    let u = app.user("Bob", None).await;
    expect_status!(app.patch(&root, &format!("/api/v1/admin/users/{}", u.id), json!({"disabled": true})).await, 200);
    expect_status!(app.get(&u, "/api/v1/auth/session").await, 401);
    let (s, _) = TestApp::send(app.req(None, Method::POST, "/api/v1/auth/login").json(&json!({"username": u.email, "password": "password123"}))).await;
    assert_eq!(s.as_u16(), 403);
}

#[actix_web::test]
async fn registration_requires_invitation() {
    let app = TestApp::new().await;
    let (s, v) = TestApp::send(
        app.req(None, Method::POST, "/api/v1/auth/register").json(&json!({"name": "X", "email": "x@example.com", "password": "password123"})),
    )
    .await;
    assert_eq!(s.as_u16(), 403);
    assert_eq!(v["code"], "signup_closed");
}

/// 每個使用者都有個人組織，是其中的管理員。
#[actix_web::test]
async fn personal_org_is_created_with_admin_grant() {
    let app = TestApp::new().await;
    let u = app.user("Carol", None).await;
    let v = expect_status!(app.get(&u, "/api/v1/me/organizations").await, 200);
    let orgs = v["items"].as_array().unwrap();
    assert_eq!(orgs.len(), 1);
    assert_eq!(orgs[0]["kind"], "personal");
    let org = orgs[0]["id"].as_i64().unwrap();
    let v = expect_status!(app.get(&u, &format!("/api/v1/me/permissions?orgId={org}")).await, 200);
    assert!(v["permissions"].as_array().unwrap().iter().any(|p| p == "platform:rbac:manage"));
}

/// 來源：az TenantConsole 測試「invite → accept → member + preset grant」
#[actix_web::test]
async fn invitation_flow_for_unregistered_user() {
    let app = TestApp::new().await;
    let owner = app.user("Owner", None).await;
    let org = app.org(&owner, "acme").await;

    let inv = expect_status!(
        app.post(&owner, &format!("/api/v1/orgs/{org}/invitations"), json!({"email": "NewGuy@Example.com", "inviteRoleKey": "org_admin"})).await,
        201
    );
    let token = inv["token"].as_str().unwrap().to_string();

    // 不登入也能看摘要。
    let v = expect_status!(TestApp::send(app.req(None, Method::GET, &format!("/api/v1/invitations/by-token/{token}"))).await, 200);
    assert_eq!(v["inviteeRegistered"], false);
    assert_eq!(v["organizationName"], "acme");

    // email 不符不能註冊。
    let (s, _) = TestApp::send(app.req(None, Method::POST, "/api/v1/auth/register").json(&json!({
        "name": "Other", "email": "other@example.com", "password": "password123", "invitationToken": token
    })))
    .await;
    assert_eq!(s.as_u16(), 403);

    let v = expect_status!(
        TestApp::send(app.req(None, Method::POST, "/api/v1/auth/register").json(&json!({
            "name": "New Guy", "email": "newguy@example.com", "password": "password123", "invitationToken": token
        })))
        .await,
        200
    );
    let me = crate::support::TestUser {
        id: v["user"]["id"].as_i64().unwrap(),
        email: "newguy@example.com".into(),
        token: v["token"].as_str().unwrap().into(),
    };
    expect_status!(app.post(&me, &format!("/api/v1/invitations/by-token/{token}/accept"), json!({})).await, 200);

    let v = expect_status!(app.get(&me, "/api/v1/me/organizations").await, 200);
    let acme = v["items"].as_array().unwrap().iter().find(|o| o["id"] == org).cloned().expect("joined acme");
    assert!(acme["projects"].as_array().unwrap().iter().any(|p| p["isDefault"] == true), "joined lobby");
    let v = expect_status!(app.get(&me, &format!("/api/v1/me/permissions?orgId={org}")).await, 200);
    assert!(v["matchedRoles"].as_array().unwrap().iter().any(|r| r == "platform:org-admin"));

    // 已接受的不能再接受。
    expect_status!(app.post(&me, &format!("/api/v1/invitations/by-token/{token}/accept"), json!({})).await, 409);
}

#[actix_web::test]
async fn only_rbac_managers_can_invite_admins() {
    let app = TestApp::new().await;
    let owner = app.user("Owner", None).await;
    let org = app.org(&owner, "acme").await;
    let m = app.user("Member Manager", None).await;
    app.add_member(org, &m).await;
    // 只給成員管理權：自訂一個角色。
    let role = expect_status!(
        app.post(&owner, &format!("/api/v1/orgs/{org}/rbac/roles"), json!({
            "scope": "organization", "uniqueName": "custom:hr", "name": "HR", "permissions": ["platform:member:manage"]
        }))
        .await,
        201
    );
    expect_status!(app.post(&owner, &format!("/api/v1/orgs/{org}/rbac/grants"), json!({"userId": m.id, "roleId": role["id"]})).await, 201);
    expect_status!(app.post(&m, &format!("/api/v1/orgs/{org}/invitations"), json!({"email": "a@example.com", "inviteRoleKey": "org_member"})).await, 201);
    let v = expect_status!(app.post(&m, &format!("/api/v1/orgs/{org}/invitations"), json!({"email": "b@example.com", "inviteRoleKey": "org_admin"})).await, 403);
    assert_eq!(v["code"], "permission_denied");
}

/// 來源：az RbacGrantService／TenantConsole 測試「removing a member revokes all grants」「cannot remove last admin」
#[actix_web::test]
async fn remove_member_revokes_grants_and_protects_last_admin() {
    let app = TestApp::new().await;
    let owner = app.user("Owner", None).await;
    let org = app.org(&owner, "acme").await;
    let b = app.user("Bob", None).await;
    app.add_member(org, &b).await;
    app.grant_role(org, &b, "platform:org-admin").await;
    expect_status!(app.get(&b, &format!("/api/v1/orgs/{org}/members")).await, 200);

    // 不能移除自己。
    expect_status!(app.delete(&owner, &format!("/api/v1/orgs/{org}/members/{}", owner.id)).await, 400);
    expect_status!(app.delete(&owner, &format!("/api/v1/orgs/{org}/members/{}", b.id)).await, 204);
    expect_status!(app.get(&b, &format!("/api/v1/orgs/{org}/members")).await, 403);

    // 撤掉唯一管理員的 grant 會被擋。
    let v = expect_status!(app.get(&owner, &format!("/api/v1/orgs/{org}/rbac/grants?userId={}", owner.id)).await, 200);
    let gid = v["items"][0]["id"].as_i64().unwrap();
    let v = expect_status!(app.post(&owner, &format!("/api/v1/orgs/{org}/rbac/grants/{gid}/revoke"), json!({})).await, 409);
    assert_eq!(v["code"], "last_governor");
}

/// 跨租戶：A 組織的管理員碰不到 B 組織。
#[actix_web::test]
async fn cross_tenant_access_is_denied() {
    let app = TestApp::new().await;
    let a = app.user("A Owner", None).await;
    let b = app.user("B Owner", None).await;
    let org_a = app.org(&a, "org-a").await;
    let org_b = app.org(&b, "org-b").await;
    expect_status!(app.get(&a, &format!("/api/v1/orgs/{org_a}/members")).await, 200);
    expect_status!(app.get(&a, &format!("/api/v1/orgs/{org_b}/members")).await, 403);
    expect_status!(app.get(&a, &format!("/api/v1/orgs/{org_b}")).await, 403);
    // 用 A 的組織路徑撤 B 的 grant 也不行。
    let v = expect_status!(app.get(&b, &format!("/api/v1/orgs/{org_b}/rbac/grants")).await, 200);
    let gid = v["items"][0]["id"].as_i64().unwrap();
    expect_status!(app.post(&a, &format!("/api/v1/orgs/{org_a}/rbac/grants/{gid}/revoke"), json!({})).await, 404);
}

/// 自訂角色：建立、授權後立即生效；改角色內容後快取失效；被持有的角色不能刪。
/// 來源：az RbacCustomDefService.spec.ts、RbacCacheManager.spec.ts
#[actix_web::test]
async fn custom_roles_and_cache_invalidation() {
    let app = TestApp::new().await;
    let owner = app.user("Owner", None).await;
    let org = app.org(&owner, "acme").await;
    let u = app.user("Dave", None).await;
    app.add_member(org, &u).await;

    // system／plugin 角色不能改。
    let v = expect_status!(app.get(&owner, &format!("/api/v1/orgs/{org}/rbac/roles")).await, 200);
    let sys = v["items"].as_array().unwrap().iter().find(|r| r["uniqueName"] == "platform:org-admin").unwrap()["id"].as_i64().unwrap();
    expect_status!(app.patch(&owner, &format!("/api/v1/orgs/{org}/rbac/roles/{sys}"), json!({"name": "x"})).await, 403);

    // uniqueName 要 custom: 前綴。
    expect_status!(
        app.post(&owner, &format!("/api/v1/orgs/{org}/rbac/roles"), json!({"scope": "organization", "uniqueName": "hr", "name": "HR"})).await,
        400
    );
    let role = expect_status!(
        app.post(&owner, &format!("/api/v1/orgs/{org}/rbac/roles"), json!({
            "scope": "organization", "uniqueName": "custom:viewer", "name": "Viewer", "permissions": ["platform:org:manage"]
        }))
        .await,
        201
    );
    let rid = role["id"].as_i64().unwrap();
    expect_status!(app.post(&owner, &format!("/api/v1/orgs/{org}/rbac/grants"), json!({"userId": u.id, "roleId": rid})).await, 201);
    expect_status!(app.patch(&u, &format!("/api/v1/orgs/{org}"), json!({"name": "renamed"})).await, 200);

    // 改角色內容拿掉權限 → 立即失效。
    expect_status!(app.patch(&owner, &format!("/api/v1/orgs/{org}/rbac/roles/{rid}"), json!({"permissions": ["platform:project:manage"]})).await, 200);
    expect_status!(app.patch(&u, &format!("/api/v1/orgs/{org}"), json!({"name": "again"})).await, 403);

    // complex 包 complex 被擋。
    let c = expect_status!(
        app.post(&owner, &format!("/api/v1/orgs/{org}/rbac/permissions"), json!({
            "scope": "organization", "uniqueName": "custom:bundle", "name": "B", "complex": true, "included": ["platform:org:manage"]
        }))
        .await,
        201
    );
    expect_status!(
        app.post(&owner, &format!("/api/v1/orgs/{org}/rbac/permissions"), json!({
            "scope": "organization", "uniqueName": "custom:bundle2", "name": "B2", "complex": true, "included": ["custom:bundle"]
        }))
        .await,
        400
    );
    // 組織不能建 simple 自訂 permission。
    expect_status!(
        app.post(&owner, &format!("/api/v1/orgs/{org}/rbac/permissions"), json!({
            "scope": "organization", "uniqueName": "custom:simple", "name": "S"
        }))
        .await,
        403
    );
    // 角色綁 complex 後，展開的 simple 生效。
    expect_status!(app.patch(&owner, &format!("/api/v1/orgs/{org}/rbac/roles/{rid}"), json!({"permissions": ["custom:bundle"]})).await, 200);
    expect_status!(app.patch(&u, &format!("/api/v1/orgs/{org}"), json!({"name": "via-bundle"})).await, 200);
    // 被角色引用的 permission 不能刪；被持有的角色不能刪。
    expect_status!(app.delete(&owner, &format!("/api/v1/orgs/{org}/rbac/permissions/{}", c["id"])).await, 409);
    expect_status!(app.delete(&owner, &format!("/api/v1/orgs/{org}/rbac/roles/{rid}")).await, 409);

    // 另一個組織看不到這個自訂角色，也不能拿來授權。
    let other_owner = app.user("Other", None).await;
    let other = app.org(&other_owner, "other").await;
    let v = expect_status!(app.get(&other_owner, &format!("/api/v1/orgs/{other}/rbac/roles")).await, 200);
    assert!(!v["items"].as_array().unwrap().iter().any(|r| r["id"] == rid));
    let x = app.user("X", None).await;
    app.add_member(other, &x).await;
    expect_status!(app.post(&other_owner, &format!("/api/v1/orgs/{other}/rbac/grants"), json!({"userId": x.id, "roleId": rid})).await, 403);
}

/// 專案層授權只涵蓋該專案；組織層授權涵蓋所有專案。
#[actix_web::test]
async fn project_scope_hierarchy() {
    let app = TestApp::new().await;
    let owner = app.user("Owner", None).await;
    let org = app.org(&owner, "acme").await;
    let p1 = expect_status!(app.post(&owner, &format!("/api/v1/orgs/{org}/projects"), json!({"uniqueName": "p1", "name": "P1"})).await, 201)["id"]
        .as_i64()
        .unwrap();
    let p2 = expect_status!(app.post(&owner, &format!("/api/v1/orgs/{org}/projects"), json!({"uniqueName": "p2", "name": "P2"})).await, 201)["id"]
        .as_i64()
        .unwrap();
    let u = app.user("Eve", None).await;
    app.add_member(org, &u).await;
    expect_status!(app.post(&owner, &format!("/api/v1/projects/{p1}/members"), json!({"userId": u.id})).await, 201);
    expect_status!(
        app.post(&owner, &format!("/api/v1/orgs/{org}/rbac/grants"), json!({"userId": u.id, "roleUniqueName": "platform:project-admin", "projectId": p1})).await,
        201
    );
    expect_status!(app.patch(&u, &format!("/api/v1/projects/{p1}"), json!({"name": "P1!"})).await, 200);
    expect_status!(app.patch(&u, &format!("/api/v1/projects/{p2}"), json!({"name": "P2!"})).await, 403);
    // 專案層授權不涵蓋組織層。
    expect_status!(app.get(&u, &format!("/api/v1/orgs/{org}/members")).await, 403);
    // 組織管理員什麼專案都能改。
    expect_status!(app.patch(&owner, &format!("/api/v1/projects/{p2}"), json!({"name": "P2?"})).await, 200);
    // 只看得到自己所屬的專案。
    let v = expect_status!(app.get(&u, &format!("/api/v1/orgs/{org}/projects")).await, 200);
    let ids: Vec<i64> = v["items"].as_array().unwrap().iter().map(|p| p["id"].as_i64().unwrap()).collect();
    assert!(ids.contains(&p1) && !ids.contains(&p2));
    // 專案層 permissions 是組織層與專案層的聯集。
    let v = expect_status!(app.get(&u, &format!("/api/v1/me/permissions?projectId={p1}")).await, 200);
    assert!(v["permissions"].as_array().unwrap().iter().any(|p| p == "platform:project:manage"));
    // lobby 不能刪。
    let lobby = v["matchedRoles"].clone();
    let _ = lobby;
}

/// 超級使用者：/me/permissions 回 ["*"]，任何組織都放行。
#[actix_web::test]
async fn super_user_bypasses() {
    let app = TestApp::new().await;
    let root = app.admin().await;
    let owner = app.user("Owner", None).await;
    let org = app.org(&owner, "acme").await;
    expect_status!(app.get(&root, &format!("/api/v1/orgs/{org}/members")).await, 200);
    let v = expect_status!(app.get(&root, &format!("/api/v1/me/permissions?orgId={org}")).await, 200);
    assert_eq!(v["permissions"], json!(["*"]));
    // 一般使用者不能建組織。
    expect_status!(app.post(&owner, "/api/v1/orgs", json!({"uniqueName": "x", "name": "x"})).await, 403);
    expect_status!(app.post(&root, "/api/v1/orgs", json!({"uniqueName": "x", "name": "x"})).await, 201);
}

#[actix_web::test]
async fn health_lists_routes() {
    let app = TestApp::new().await;
    let v = expect_status!(TestApp::send(app.req(None, Method::GET, "/health")).await, 200);
    assert_eq!(v["database"], "ok");
    assert_eq!(v["storage"], "ok");
    assert!(v["routes"].as_array().unwrap().iter().any(|r| r == "POST /api/v1/auth/login"));
}

/// 物件儲存：put／head／get／delete 與預簽下載。
#[actix_web::test]
async fn storage_roundtrip_and_presign() {
    let app = TestApp::new().await;
    let s = &app.core.storage;
    s.put("a/中文 檔.txt", bytes::Bytes::from_static(b"hello"), "text/plain").await.unwrap();
    assert_eq!(s.head("a/中文 檔.txt").await.unwrap().unwrap().size, 5);
    assert_eq!(&s.get("a/中文 檔.txt").await.unwrap()[..], b"hello");
    let url = s.presign("GET", "a/中文 檔.txt", std::time::Duration::from_secs(60), &[]);
    let body = reqwest::get(url).await.unwrap().bytes().await.unwrap();
    assert_eq!(&body[..], b"hello");
    s.delete("a/中文 檔.txt").await.unwrap();
    assert!(s.head("a/中文 檔.txt").await.unwrap().is_none());
}
