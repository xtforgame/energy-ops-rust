//! 稽核：寫入 `tbl_energy_audit_event` 的唯一入口，與查詢端點。
//!
//! · type 必須在 registry 白名單；payload 過 per-type 白名單（沒宣告的鍵丟棄），超過 32KB 截斷。
//! · 操作者一律由 `OrgAccess` 推導（`Actor::from_access`），不收 body 給的身分。
//! · `record` 寫在呼叫端的交易裡：稽核寫不進去，業務寫入跟著 rollback（fail-closed）。
//! · `record_best_effort` 走獨立連線、失敗只記 log；fail-closed 的 type 拒絕走這條。

mod routes;

pub use routes::AuditPlugin;

use crate::common::{err_d, Ctx, RequestMeta, Tx};
use eo_domain::audit_events::{get_audit_event_def, is_fail_closed, sanitize_payload, truncate_payload, AuditError};
use eo_platform::error::ApiResult;
use eo_platform::rbac::OrgAccess;
use serde_json::{json, Value};
use sqlx::PgPool;

/// 稽核上的操作者快照。欄位私有：只能從已判權的存取或系統身分造出來。
#[derive(Debug, Clone)]
pub struct Actor {
    user_id: Option<i64>,
    label: Option<String>,
    request_ip: Option<String>,
    trace_id: Option<String>,
}

impl Actor {
    pub fn from_access(access: &OrgAccess, meta: &RequestMeta) -> Self {
        let user = access.user();
        Self {
            user_id: user.map(|u| u.id),
            // 寫入當下的名字快照：改名或離職後歷史紀錄仍讀得懂。
            label: user.map(|u| u.name.clone()),
            request_ip: meta.ip.clone(),
            trace_id: meta.trace_id.clone(),
        }
    }

    pub fn system() -> Self {
        Self { user_id: None, label: None, request_ip: None, trace_id: None }
    }

    pub fn user_id(&self) -> Option<i64> {
        self.user_id
    }

    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    fn kind(&self) -> &'static str {
        if self.user_id.is_some() {
            "user"
        } else {
            "system"
        }
    }
}

/// 一筆稽核的內容。`summary` 是伺服器產生的一句人話。
#[derive(Debug, Clone)]
pub struct AuditInput {
    pub event_type: &'static str,
    pub target_id: String,
    pub target_label: Option<String>,
    pub summary: String,
    pub payload: Value,
    pub outcome: &'static str,
    pub on_behalf_of: Option<i64>,
    pub caused_by: Option<i64>,
    /// 只在同一個 type 指向不同 model 時才覆寫；一般讓 registry 當唯一真值。
    pub target_type: Option<&'static str>,
}

impl AuditInput {
    pub fn new(event_type: &'static str, target_id: impl ToString, summary: impl Into<String>) -> Self {
        Self {
            event_type,
            target_id: target_id.to_string(),
            target_label: None,
            summary: summary.into(),
            payload: json!({}),
            outcome: "success",
            on_behalf_of: None,
            caused_by: None,
            target_type: None,
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.target_label = Some(label.into());
        self
    }

    pub fn label_opt(mut self, label: Option<String>) -> Self {
        self.target_label = label;
        self
    }

    pub fn payload(mut self, payload: Value) -> Self {
        self.payload = payload;
        self
    }

    pub fn outcome(mut self, outcome: &'static str) -> Self {
        self.outcome = outcome;
        self
    }

    pub fn on_behalf_of(mut self, user_id: Option<i64>) -> Self {
        self.on_behalf_of = user_id;
        self
    }

    pub fn caused_by(mut self, event_id: Option<i64>) -> Self {
        self.caused_by = event_id;
        self
    }
}

fn audit_error(e: AuditError) -> eo_platform::error::ApiError {
    match e {
        AuditError::UnknownType(t) => err_d(500, "unknown_audit_event_type", json!({ "type": t })),
        AuditError::InvalidPayload { event_type, issues } => {
            err_d(500, "invalid_audit_payload", json!({ "type": event_type, "issues": issues }))
        }
    }
}

const INSERT: &str = "INSERT INTO tbl_energy_audit_event
    (organization_id, project_id, actor_user_id, actor_kind, actor_label, on_behalf_of_user_id, type, target_type,
     target_id, target_label, outcome, summary, payload, caused_by_event_id, trace_id, request_ip, occurred_at)
    VALUES ($1, NULL, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16) RETURNING id";

/// 驗 type 與 payload，回傳要寫進去的 `(targetType, payload)`。
fn prepare(input: &AuditInput) -> ApiResult<(&'static str, Value)> {
    let def = get_audit_event_def(input.event_type)
        .ok_or_else(|| audit_error(AuditError::UnknownType(input.event_type.into())))?;
    // Rust 的 None 序列化成 null；可省略但不可為 null 的欄位遇到 null 視同沒給（JS 的 undefined）。
    let mut payload = input.payload.clone();
    if let Some(m) = payload.as_object_mut() {
        for f in def.payload {
            if f.optional && !f.nullable && m.get(f.key).is_some_and(Value::is_null) {
                m.remove(f.key);
            }
        }
    }
    let clean = sanitize_payload(input.event_type, &payload).map_err(audit_error)?;
    Ok((input.target_type.unwrap_or(def.target_type), truncate_payload(clean)))
}

/// 記一筆稽核（fail-closed：在呼叫端的交易裡，失敗就讓整個交易失敗）。回傳稽核列 id。
pub async fn record(tx: &mut Tx<'_>, ctx: &Ctx, input: AuditInput) -> ApiResult<i64> {
    let (target_type, payload) = prepare(&input)?;
    let a = &ctx.actor;
    let id: i64 = sqlx::query_scalar(INSERT)
        .bind(ctx.org_id())
        .bind(a.user_id)
        .bind(a.kind())
        .bind(a.label.as_deref())
        .bind(input.on_behalf_of)
        .bind(input.event_type)
        .bind(target_type)
        .bind(&input.target_id)
        .bind(input.target_label.as_deref())
        .bind(input.outcome)
        .bind(&input.summary)
        .bind(&payload)
        .bind(input.caused_by)
        .bind(a.trace_id.as_deref())
        .bind(a.request_ip.as_deref())
        .bind(ctx.now())
        .fetch_one(&mut **tx)
        .await?;
    Ok(id)
}

/// best-effort：走獨立連線，寫不進去只記 log。
/// fail-closed 的 type 一律拒絕（避免誤用把保護默默關掉）。
pub async fn record_best_effort(pool: &PgPool, ctx: &Ctx, input: AuditInput) -> ApiResult<Option<i64>> {
    if is_fail_closed(input.event_type) {
        return Err(err_d(500, "audit_type_is_fail_closed", json!({ "type": input.event_type })));
    }
    let attempt = async {
        let mut tx = pool.begin().await?;
        let id = record(&mut tx, ctx, input.clone()).await?;
        tx.commit().await?;
        Ok::<i64, eo_platform::error::ApiError>(id)
    };
    match attempt.await {
        Ok(id) => Ok(Some(id)),
        Err(e) => {
            log::error!("[energy][audit] best-effort record failed: {} {}", input.event_type, e);
            Ok(None)
        }
    }
}

/// 不認得的 type 預設當 fail-closed（稽核的預設方向）。
pub fn fail_closed(event_type: &str) -> bool {
    is_fail_closed(event_type)
}
