//! 測試佈景：每條測試一個全新的資料庫與物件前綴，起一個真的 HTTP server（隨機埠），用 reqwest 打。
//! 需要 docker compose 的 Postgres 與 MinIO（`docker compose up -d`）。

#![allow(dead_code)]

#[macro_use]
pub mod energy;

use actix_web::{App, HttpServer};
use chrono::{TimeZone, Utc};
use eo_platform::clock::ManualClock;
use eo_platform::config::{Config, S3Config};
use eo_platform::core::{Core, CoreRef};
use rand::Rng;
use reqwest::{Method, StatusCode};
use serde_json::Value;
use std::sync::Arc;

pub const ADMIN_DB_URL_DEFAULT: &str = "postgres://energy:energy@127.0.0.1:56432/postgres";

fn env_or(k: &str, d: &str) -> String {
    std::env::var(k).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| d.to_string())
}

pub struct TestUser {
    pub id: i64,
    pub email: String,
    pub token: String,
}

pub struct TestApp {
    pub base: String,
    pub core: CoreRef,
    pub clock: Arc<ManualClock>,
    pub http: reqwest::Client,
    pub db_name: String,
}

impl TestApp {
    pub async fn new() -> Self {
        Self::with_config(|_| {}).await
    }

    /// `tweak` 可以改設定（例如接上假的辨識服務）。
    pub async fn with_config(tweak: impl FnOnce(&mut Config)) -> Self {
        let admin_url = env_or("EO_TEST_ADMIN_DATABASE_URL", ADMIN_DB_URL_DEFAULT);
        let suffix: String = {
            let mut r = rand::rng();
            (0..10).map(|_| (b'a' + r.random_range(0..26)) as char).collect()
        };
        let db_name = format!("eo_t_{suffix}");
        let admin = sqlx::PgPool::connect(&admin_url).await.expect("test postgres not reachable (docker compose up -d)");
        sqlx::query(&format!("CREATE DATABASE {db_name}")).execute(&admin).await.unwrap();
        admin.close().await;
        let (base_url, _) = admin_url.rsplit_once('/').unwrap();
        let mut config = Config {
            database_url: format!("{base_url}/{db_name}"),
            http_host: "127.0.0.1".into(),
            http_port: 0,
            s3: S3Config {
                endpoint: env_or("EO_TEST_S3_ENDPOINT", "http://127.0.0.1:56900"),
                region: "us-east-1".into(),
                bucket: env_or("EO_TEST_S3_BUCKET", "energy-files-test"),
                access_key: env_or("EO_TEST_S3_ACCESS_KEY", "energyminio"),
                secret_key: env_or("EO_TEST_S3_SECRET_KEY", "energyminio-secret"),
                key_prefix: format!("t-{suffix}"),
            },
            web_dir: None,
            tpr_url: None,
            tpr_api_key: None,
            cookie_secure: false,
            bootstrap_admin: None,
            demo_seed: false,
        };
        tweak(&mut config);
        let clock = Arc::new(ManualClock::new(Utc.with_ymd_and_hms(2026, 11, 4, 2, 0, 0).unwrap()));
        let core = Arc::new(Core::new(config, clock.clone()).unwrap());
        let system = energy_ops::start(&core).await.expect("startup");
        let (s, c) = (Arc::clone(&system), Arc::clone(&core));
        let server = HttpServer::new(move || App::new().configure(|cfg| energy_ops::configure_app(cfg, &s, &c)))
            .workers(1)
            .bind(("127.0.0.1", 0))
            .unwrap();
        let port = server.addrs()[0].port();
        actix_web::rt::spawn(server.run());
        Self {
            base: format!("http://127.0.0.1:{port}"),
            core,
            clock,
            http: reqwest::Client::new(),
            db_name,
        }
    }

    /// 直接在 DB 建使用者（含個人組織）並開一個 session。
    pub async fn user(&self, name: &str, privilege: Option<&str>) -> TestUser {
        let email = format!("{}@example.com", name.to_lowercase().replace(' ', "."));
        let mut tx = self.core.pool.begin().await.unwrap();
        let u = eo_platform::tenancy::create_user(&mut tx, name, &email, "password123", privilege).await.unwrap();
        tx.commit().await.unwrap();
        let s = eo_platform::auth::create_session(&self.core, u.id, None, None, None).await.unwrap();
        TestUser { id: u.id, email, token: s.token }
    }

    pub async fn admin(&self) -> TestUser {
        self.user("Root Admin", Some("admin")).await
    }

    /// 建組織（以 owner 身分），回傳組織 id。
    pub async fn org(&self, owner: &TestUser, unique_name: &str) -> i64 {
        let mut tx = self.core.pool.begin().await.unwrap();
        let o = eo_platform::tenancy::create_org(&mut tx, unique_name, unique_name, owner.id).await.unwrap();
        tx.commit().await.unwrap();
        self.core.rbac.invalidate_user(owner.id);
        o.id
    }

    /// 直接加成員。
    pub async fn add_member(&self, org: i64, user: &TestUser) {
        let mut tx = self.core.pool.begin().await.unwrap();
        eo_platform::tenancy::add_org_member(&mut tx, user.id, org, "member").await.unwrap();
        tx.commit().await.unwrap();
    }

    /// 直接授權（組織層，依角色 uniqueName）。
    pub async fn grant_role(&self, org: i64, user: &TestUser, role_unique_name: &str) {
        let mut tx = self.core.pool.begin().await.unwrap();
        let r = eo_platform::rbac::grant::role_by_unique_name(&mut tx, eo_domain::rbac::RbacScope::Organization, role_unique_name)
            .await
            .unwrap();
        eo_platform::rbac::grant::grant(
            &mut tx,
            &eo_platform::rbac::grant::GrantInput {
                user_id: user.id,
                role_id: r.id,
                organization_id: Some(org),
                project_id: None,
                resource_identifier: None,
            },
            None,
        )
        .await
        .unwrap();
        self.core.rbac.commit(tx, &[user.id]).await.unwrap();
    }

    pub fn req(&self, user: Option<&TestUser>, method: Method, path: &str) -> reqwest::RequestBuilder {
        let mut r = self.http.request(method, format!("{}{}", self.base, path));
        if let Some(u) = user {
            r = r.bearer_auth(&u.token);
        }
        r
    }

    pub async fn send(rb: reqwest::RequestBuilder) -> (StatusCode, Value) {
        let resp = rb.send().await.unwrap();
        let status = resp.status();
        let text = resp.text().await.unwrap();
        let v = if text.is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap_or(Value::String(text)) };
        (status, v)
    }

    pub async fn get(&self, user: &TestUser, path: &str) -> (StatusCode, Value) {
        Self::send(self.req(Some(user), Method::GET, path)).await
    }

    pub async fn post(&self, user: &TestUser, path: &str, body: Value) -> (StatusCode, Value) {
        Self::send(self.req(Some(user), Method::POST, path).json(&body)).await
    }

    pub async fn patch(&self, user: &TestUser, path: &str, body: Value) -> (StatusCode, Value) {
        Self::send(self.req(Some(user), Method::PATCH, path).json(&body)).await
    }

    pub async fn delete(&self, user: &TestUser, path: &str) -> (StatusCode, Value) {
        Self::send(self.req(Some(user), Method::DELETE, path)).await
    }
}

/// 斷言狀態碼，失敗時印出回應。
#[macro_export]
macro_rules! expect_status {
    ($resp:expr, $code:expr) => {{
        let (s, v) = $resp;
        assert_eq!(s.as_u16(), $code, "unexpected status, body: {v}");
        v
    }};
}
