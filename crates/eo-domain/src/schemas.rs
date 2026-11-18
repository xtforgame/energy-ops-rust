//! API 請求 body 的形狀與檢查（來源：`common/schemas.ts`）。
//!
//! 反序列化管型別（未宣告的鍵忽略＝zod 的 strip），`validate()` 管長度、範圍、格式。
//! DB 層沒有 NOT NULL，`key`／`name` 的必填是在這裡強制的。
//! 「沒給」與「給 null」意義不同的欄位用 `Option<Option<T>>`：外層 None ＝ 不改，`Some(None)` ＝ 清空。

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::document_ingest::{EnergyDocType, BILL_DOC_TYPES};
use crate::js_compat::utf16_len;
use crate::serde_util::double_option;
use crate::taipower_canonical::is_iso_day;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum SchemaError {
    #[error("body 形狀不對：{0}")]
    Shape(String),
    #[error("body 驗證失敗：{0:?}")]
    Invalid(Vec<String>),
}

/// 有額外規則的 body。
pub trait Validate {
    fn validate(&self) -> Result<(), Vec<String>>;
}

/// 反序列化 ＋ 驗證（中介層的 `validateBody`：失敗就 400）。
pub fn parse_body<T: DeserializeOwned + Validate>(body: &Value) -> Result<T, SchemaError> {
    let v: T = serde_json::from_value(body.clone()).map_err(|e| SchemaError::Shape(e.to_string()))?;
    v.validate().map_err(SchemaError::Invalid)?;
    Ok(v)
}

#[derive(Default)]
struct Errs(Vec<String>);

impl Errs {
    fn len(&mut self, name: &str, s: &str, min: usize, max: usize) {
        let n = utf16_len(s);
        if n < min || n > max {
            self.0.push(format!("{name}: 長度須介於 {min}–{max}"));
        }
    }
    fn max(&mut self, name: &str, s: Option<&str>, max: usize) {
        if let Some(s) = s {
            self.len(name, s, 0, max);
        }
    }
    fn date(&mut self, name: &str, s: Option<&str>) {
        if s.is_some_and(|s| !is_iso_day(s)) {
            self.0.push(format!("{name}: 日期格式須為 YYYY-MM-DD"));
        }
    }
    fn positive(&mut self, name: &str, n: i64) {
        if n <= 0 {
            self.0.push(format!("{name}: 須為正整數"));
        }
    }
    fn done(self) -> Result<(), Vec<String>> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(self.0)
        }
    }
}

/// 部門在組織內的穩定識別碼：小寫英數與連字號，首字不可是連字號。
pub fn is_valid_department_key(key: &str) -> bool {
    let n = utf16_len(key);
    (1..=80).contains(&n)
        && key.bytes().next().is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && key.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DepartmentMemberRole {
    Manager,
    #[default]
    Member,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDepartmentInput {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub sort_order: Option<i64>,
}

impl Validate for CreateDepartmentInput {
    fn validate(&self) -> Result<(), Vec<String>> {
        let mut e = Errs::default();
        if !is_valid_department_key(&self.key) {
            e.0.push("key: key 只能用小寫英數與連字號".into());
        }
        e.len("name", &self.name, 1, 200);
        e.done()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDepartmentInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub sort_order: Option<i64>,
}

impl Validate for UpdateDepartmentInput {
    fn validate(&self) -> Result<(), Vec<String>> {
        let mut e = Errs::default();
        if let Some(n) = &self.name {
            e.len("name", n, 1, 200);
        }
        if self.name.is_none() && self.sort_order.is_none() {
            e.0.push("至少要給一個要更新的欄位".into());
        }
        e.done()
    }
}

/// 使用者 id：正整數或非空字串。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UserIdInput {
    Id(i64),
    Text(String),
}

impl UserIdInput {
    fn check(&self, e: &mut Errs) {
        match self {
            UserIdInput::Id(n) => e.positive("userId", *n),
            UserIdInput::Text(s) => e.len("userId", s, 1, usize::MAX),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetDepartmentMemberInput {
    pub user_id: UserIdInput,
    #[serde(default)]
    pub role: DepartmentMemberRole,
}

impl Validate for SetDepartmentMemberInput {
    fn validate(&self) -> Result<(), Vec<String>> {
        let mut e = Errs::default();
        self.user_id.check(&mut e);
        e.done()
    }
}

/// 角色指派／撤銷：用 uniqueName 不用 roleId —— id 隨環境變，uniqueName 才是穩定真值。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleAssignmentInput {
    pub user_id: UserIdInput,
    pub role_unique_name: String,
    #[serde(default)]
    pub reason: Option<String>,
}

impl Validate for RoleAssignmentInput {
    fn validate(&self) -> Result<(), Vec<String>> {
        let mut e = Errs::default();
        self.user_id.check(&mut e);
        e.len("roleUniqueName", &self.role_unique_name, 1, usize::MAX);
        e.max("reason", self.reason.as_deref(), 500);
        e.done()
    }
}

/// subject id：字串或數字。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SubjectIdInput {
    Number(f64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartExecutionInput {
    pub station_key: String,
    pub workflow_key: String,
    pub subject_type: String,
    pub subject_id: SubjectIdInput,
    /// 業務日期，不是今天：補登五月的單要用五月生效的那份約。做成必填就是逼呼叫端想清楚。
    pub business_date: String,
    /// 分支條件求值用的單據屬性快照。
    #[serde(default)]
    pub attributes: Option<Map<String, Value>>,
}

impl Validate for StartExecutionInput {
    fn validate(&self) -> Result<(), Vec<String>> {
        let mut e = Errs::default();
        e.len("stationKey", &self.station_key, 1, 120);
        e.len("workflowKey", &self.workflow_key, 1, 120);
        e.len("subjectType", &self.subject_type, 1, 60);
        if let SubjectIdInput::Text(s) = &self.subject_id {
            e.len("subjectId", s, 1, 120);
        }
        e.date("businessDate", Some(&self.business_date));
        e.done()
    }
}

/// 單一入口的 body。送的是 actionKey（定義側 action 表的一列），不是 kind。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformActionInput {
    pub action_key: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub payload: Option<Map<String, Value>>,
    /// sideEffect 的備註（匯款單號等）。人輸入的，系統無從驗證。
    #[serde(default)]
    pub note: Option<String>,
    /// `explicitNode` 時的目標（需目標節點 allowJumpTarget）。
    #[serde(default)]
    pub target_node_key: Option<String>,
}

impl Validate for PerformActionInput {
    fn validate(&self) -> Result<(), Vec<String>> {
        let mut e = Errs::default();
        e.len("actionKey", &self.action_key, 1, 120);
        e.max("reason", self.reason.as_deref(), 2000);
        e.max("note", self.note.as_deref(), 500);
        e.max("targetNodeKey", self.target_node_key.as_deref(), 120);
        e.done()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReleaseSideEffectInput {
    /// 必填 —— 這是全系統最需要被看見的一筆稽核。
    pub reason: String,
}

impl Validate for ReleaseSideEffectInput {
    fn validate(&self) -> Result<(), Vec<String>> {
        let mut e = Errs::default();
        e.len("reason", &self.reason, 1, 2000);
        e.done()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecideAbortInput {
    pub approved: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

impl Validate for DecideAbortInput {
    fn validate(&self) -> Result<(), Vec<String>> {
        let mut e = Errs::default();
        e.max("reason", self.reason.as_deref(), 2000);
        e.done()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDelegationInput {
    pub delegator_user_id: i64,
    pub delegate_user_id: i64,
    pub valid_from: String,
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<Option<String>>,
    #[serde(default)]
    pub reason: Option<String>,
}

impl Validate for CreateDelegationInput {
    fn validate(&self) -> Result<(), Vec<String>> {
        let mut e = Errs::default();
        e.positive("delegatorUserId", self.delegator_user_id);
        e.positive("delegateUserId", self.delegate_user_id);
        e.date("validFrom", Some(&self.valid_from));
        e.date("validTo", self.valid_to.as_ref().and_then(|v| v.as_deref()));
        e.max("reason", self.reason.as_deref(), 500);
        e.done()
    }
}

/// 血緣關係的標籤 —— 只是標籤，不觸發任何引擎行為。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RelationKind {
    Derived,
    Reversal,
    Correction,
    Merged,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkLineageInput {
    pub upstream_execution_id: i64,
    pub downstream_execution_id: i64,
    #[serde(default)]
    pub relation_kind: Option<RelationKind>,
    #[serde(default)]
    pub note: Option<String>,
}

impl Validate for LinkLineageInput {
    fn validate(&self) -> Result<(), Vec<String>> {
        let mut e = Errs::default();
        e.positive("upstreamExecutionId", self.upstream_execution_id);
        e.positive("downstreamExecutionId", self.downstream_execution_id);
        e.max("note", self.note.as_deref(), 500);
        e.done()
    }
}

/// 吃的是「OCR 整理後的 JSON」—— 不 pin shape，任何值都收。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestBillInput {
    #[serde(default)]
    pub ocr_json: Value,
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub source_attachment_id: Option<Option<i64>>,
}

impl Validate for IngestBillInput {
    fn validate(&self) -> Result<(), Vec<String>> {
        let mut e = Errs::default();
        if let Some(Some(id)) = self.source_attachment_id {
            e.positive("sourceAttachmentId", id);
        }
        e.done()
    }
}

/// 人工指認：把例外件配到一個發電站。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchStationInput {
    pub station_id: i64,
}

impl Validate for MatchStationInput {
    fn validate(&self) -> Result<(), Vec<String>> {
        let mut e = Errs::default();
        e.positive("stationId", self.station_id);
        e.done()
    }
}

/// 校對：只改最終值。刻意沒有 `ocrRaw`／`ocrExtracted` —— 辨識值不可被前端覆蓋。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectBillInput {
    /// 電號。改它會重新配案場 —— 它是這張單走哪一條流程的依據。
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub customer_number_raw: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub billing_period_start: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub billing_period_end: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub total_amount: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub generation_kwh: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub issued_at: Option<Option<String>>,
    /// 繳費期限：高壓用電單才有。
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub due_date: Option<Option<String>>,
    /// 單別 —— 決定這張單要開哪些流程。值域＝四種細分 ＋ 兩個舊粗分，不另寫一份名單。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_type: Option<EnergyDocType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark_reviewed: Option<bool>,
    /// 通知單號（既有 `document_no`，卡 B 起改成可校對）。
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub document_no: Option<Option<String>>,
    /// 電費單明細 19 欄（卡 B）—— 欄位表驅動，鍵是 `taipower_extractor::BILL_DETAIL_FIELDS` 的 camel 名。
    /// 不逐欄手寫成 27 個具名欄位：用同一張表驗證、同一支函式寫回 `detail_fields` jsonb。
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub detail: Map<String, Value>,
}

/// 校對台可以指定的單別。
pub fn correctable_doc_types() -> Vec<EnergyDocType> {
    let mut v = BILL_DOC_TYPES.to_vec();
    v.extend([EnergyDocType::ElectricityBill, EnergyDocType::ConsumptionBill]);
    v
}

impl Validate for CorrectBillInput {
    fn validate(&self) -> Result<(), Vec<String>> {
        let mut e = Errs::default();
        let inner = |v: &Option<Option<String>>| v.as_ref().and_then(|x| x.as_deref()).map(str::to_string);
        e.max("customerNumberRaw", inner(&self.customer_number_raw).as_deref(), 120);
        e.date("billingPeriodStart", inner(&self.billing_period_start).as_deref());
        e.date("billingPeriodEnd", inner(&self.billing_period_end).as_deref());
        e.date("issuedAt", inner(&self.issued_at).as_deref());
        e.date("dueDate", inner(&self.due_date).as_deref());
        if let Some(t) = self.doc_type {
            if !correctable_doc_types().contains(&t) {
                e.0.push(format!("docType: 不可指定為 {}", t.as_str()));
            }
        }
        e.done()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 來源：無對應 TS 測試；補驗 schemas.ts 的 departmentKeySchema／createDepartmentSchema／updateDepartmentSchema。
    #[test]
    fn department_key_rules() {
        assert!(is_valid_department_key("sales-2"));
        assert!(!is_valid_department_key("-sales"));
        assert!(!is_valid_department_key("Sales"));
        assert!(!is_valid_department_key(""));
        let ok: CreateDepartmentInput = parse_body(&json!({ "key": "sales", "name": "業務部", "extra": 1 })).unwrap();
        assert_eq!(ok.sort_order, None);
        assert!(parse_body::<UpdateDepartmentInput>(&json!({})).is_err());
    }

    /// 來源：無對應 TS 測試；補驗 schemas.ts 的 startExecutionSchema。
    #[test]
    fn start_execution_requires_business_date() {
        let body = json!({
            "stationKey": "S1", "workflowKey": "bill-approval", "subjectType": "energyBill",
            "subjectId": 12, "businessDate": "2026-05-31",
        });
        assert!(parse_body::<StartExecutionInput>(&body).is_ok());
        let mut bad = body.clone();
        bad["businessDate"] = json!("2026/05/31");
        assert!(matches!(parse_body::<StartExecutionInput>(&bad), Err(SchemaError::Invalid(_))));
        bad.as_object_mut().unwrap().remove("businessDate");
        assert!(matches!(parse_body::<StartExecutionInput>(&bad), Err(SchemaError::Shape(_))));
    }

    /// 來源：無對應 TS 測試；補驗 schemas.ts 的 correctBillSchema。
    #[test]
    fn correct_bill_tells_null_from_absent() {
        let c: CorrectBillInput = parse_body(&json!({ "totalAmount": null, "docType": "renewableSurplus" })).unwrap();
        assert_eq!(c.total_amount, Some(None));
        assert_eq!(c.generation_kwh, None);
        assert!(parse_body::<CorrectBillInput>(&json!({ "docType": "depositRequest" })).is_err());
        assert!(parse_body::<CorrectBillInput>(&json!({ "dueDate": "2026-13" })).is_err());
    }

    /// 來源：無對應 TS 測試；補驗 schemas.ts 的 setDepartmentMemberSchema。
    #[test]
    fn member_role_defaults_to_member() {
        let m: SetDepartmentMemberInput = parse_body(&json!({ "userId": "7" })).unwrap();
        assert_eq!(m.role, DepartmentMemberRole::Member);
        assert!(parse_body::<SetDepartmentMemberInput>(&json!({ "userId": 0 })).is_err());
    }
}
