//! 移植自 `tests/auditEvents.test.ts`（registry 形狀、白名單、diffChanges、truncatePayload）。

use eo_domain::audit_events::*;
use serde_json::{json, Map, Value};

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().cloned().expect("測試資料要是物件")
}

fn is_lower_ident(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(f) if f.is_ascii_lowercase())
        && c.all(|x| x.is_ascii_lowercase() || x.is_ascii_digit() || x == '_')
}

/// 來源：auditEvents.test.ts「every def is complete and uses the documented grammar」
#[test]
fn every_def_is_complete_and_uses_the_grammar() {
    assert!(!ENERGY_AUDIT_EVENTS.is_empty());
    for d in ENERGY_AUDIT_EVENTS {
        // `<domain>.<object>.<action>` 或 `<domain>.<action>`，不用 RBAC 的 `:`
        let parts: Vec<&str> = d.event_type.split('.').collect();
        assert!(parts.len() >= 2 && parts.iter().all(|p| is_lower_ident(p)), "{} grammar", d.event_type);
        assert!(!d.event_type.contains(':'));
        // targetType 是 model 名（camelCase）
        let mut t = d.target_type.chars();
        assert!(
            matches!(t.next(), Some(f) if f.is_ascii_lowercase()) && t.all(|x| x.is_ascii_alphanumeric()),
            "{} targetType",
            d.event_type
        );
        assert!(d.summary_key.starts_with("audit.type."), "{} summaryKey", d.event_type);
    }
}

/// 來源：auditEvents.test.ts「has no duplicate types」
#[test]
fn has_no_duplicate_types() {
    let types = audit_event_types();
    let mut uniq = types.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), types.len());
}

/// 來源：auditEvents.test.ts「declares only types that are actually written (no `chatbotEvent.rawRef` repeat)」
#[test]
fn declares_only_types_that_are_written() {
    let mut expected = vec![
        "allocation.created", "allocation.ended", "allocation.updated", "attachment.category_set",
        "bill.no_contract", "bill.rematched", "contract.created", "contract.updated", "contract.rent_payment_draft_saved",
        "department.created", "department.updated", "department.deleted",
        "department.member_added", "department.member_removed", "department.manager_changed",
        "member.role_changed",
        "flow.started", "flow.candidates_resolved", "flow.task_claimed",
        "flow.task_released", "flow.task_completed", "flow.completed",
        "flow.task_sent_back", "flow.branch_cancelled", "flow.task_reassigned",
        "flow.state_set", "flow.side_effect_applied", "flow.side_effect_released",
        "flow.force_advanced", "flow.terminated",
        "flow.abort_requested", "flow.abort_approved", "flow.abort_rejected",
        "flow.rerun",
        "delegation.created", "delegation.revoked", "lineage.linked",
        "ingest.recognition_requested", "ingest.recognition_completed",
        "ingest.recognition_failed", "ingest.recognition_purged",
        "ingest.recognition_supplied",
        "attachment.uploaded", "attachment.downloaded",
        "attachment.superseded", "attachment.link_removed",
        "flow.attachment_added",
        "bill.matched", "bill.unmatched", "bill.corrected",
        "bill.created_manually",
        "bill.route_gate_added", "bill.route_gate_removed",
        "payout.calculated", "payout.calculation_failed", "payout.superseded",
        "strategy.published",
        "strategy.binding_created", "strategy.binding_switched", "strategy.binding_ended",
        "flow.binding_created", "flow.binding_switched", "flow.binding_ended",
        "station.geocoded", "station.rent_sources_updated", "station.rent_voucher_updated", "station.rent_worksheet_imported",
        "ingest.batch_classified",
    ];
    let mut actual = audit_event_types();
    expected.sort();
    actual.sort();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 67);
}

/// 來源：auditEvents.test.ts「lookup helpers behave」
#[test]
fn lookup_helpers_behave() {
    assert_eq!(get_audit_event_def("department.created").unwrap().target_type, "energyDepartment");
    assert!(get_audit_event_def("nope.nope").is_none());
    assert!(is_known_audit_event_type("member.role_changed"));
    assert!(!is_known_audit_event_type("member.role_changedd"));
}

/// 來源：auditEvents.test.ts「payloadSchema strips keys it did not declare (whitelist, not blacklist)」
#[test]
fn payload_strips_undeclared_keys() {
    let out = sanitize_payload(
        "member.role_changed",
        &json!({ "action": "granted", "roleUniqueName": "energy:cxo", "nationalId": "A123456789" }),
    )
    .unwrap();
    let mut keys: Vec<&String> = out.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, vec!["action", "roleUniqueName"]);
    assert!(out.get("nationalId").is_none());

    // 巢狀物件裡沒宣告的鍵也丟掉。
    let out = sanitize_payload(
        "bill.corrected",
        &json!({ "changes": { "totalAmount": { "from": 1, "to": 2, "secret": "x" } }, "hack": true }),
    )
    .unwrap();
    assert_eq!(out, json!({ "changes": { "totalAmount": { "from": 1, "to": 2 } } }));
}

/// 來源：auditEvents.test.ts「payloadSchema rejects the wrong shape」
#[test]
fn payload_rejects_the_wrong_shape() {
    // action 不在 enum 內
    assert!(sanitize_payload("member.role_changed", &json!({ "action": "exploded", "roleUniqueName": "x" })).is_err());
    // 缺必填
    assert!(sanitize_payload("member.role_changed", &json!({ "action": "granted" })).is_err());
    // 未註冊的 type
    assert!(matches!(sanitize_payload("nope.nope", &json!({})), Err(AuditError::UnknownType(_))));
    // reason 必填且不可為空字串
    assert!(sanitize_payload("flow.terminated", &json!({ "reason": "" })).is_err());
    assert!(sanitize_payload("flow.terminated", &json!({ "reason": "客戶撤件" })).is_ok());
    // nullable 與 optional 是兩件事
    assert!(sanitize_payload("flow.force_advanced", &json!({ "fromNodeKey": null, "toNodeKey": "end", "reason": "r" })).is_ok());
    assert!(sanitize_payload("flow.force_advanced", &json!({ "toNodeKey": "end", "reason": "r" })).is_err());
    assert!(sanitize_payload("flow.task_completed", &json!({ "nodeKey": "a", "outcome": "ok", "reason": null })).is_err());
    // int 與 union
    assert!(sanitize_payload("attachment.downloaded", &json!({ "executionId": 1.5 })).is_err());
    assert!(sanitize_payload(
        "bill.route_gate_removed",
        &json!({ "field": "totalAmount", "before": 138195, "after": null, "gatesBefore": ["a"], "gatesAfter": [],
                 "gatesDelta": { "added": [], "removed": ["a"] } }),
    )
    .is_ok());
    assert!(sanitize_payload("ingest.batch_classified", &json!({ "fileCount": -1 })).is_err());
}

/// 來源：auditEvents.test.ts「mutations are fail-closed; read-ish flow events are best-effort」
#[test]
fn mutations_are_fail_closed() {
    let best_effort = [
        "flow.candidates_resolved", "flow.task_claimed", "flow.task_released",
        "flow.state_set", "lineage.linked", "attachment.downloaded",
        "payout.calculation_failed", "ingest.recognition_purged",
    ];
    for d in ENERGY_AUDIT_EVENTS {
        assert_eq!(d.has_side_effect, !best_effort.contains(&d.event_type), "{} hasSideEffect", d.event_type);
        assert_eq!(is_fail_closed(d.event_type), d.has_side_effect);
    }
    assert!(is_fail_closed("nope.nope"), "不認得的 type 預設 fail-closed");
    assert_eq!(get_audit_event_def("member.role_changed").unwrap().severity, AuditSeverity::Critical);
    assert_eq!(get_audit_event_def("flow.side_effect_released").unwrap().severity, AuditSeverity::Critical);
    assert_eq!(get_audit_event_def("flow.force_advanced").unwrap().severity, AuditSeverity::Critical);
    assert_eq!(get_audit_event_def("department.manager_changed").unwrap().severity, AuditSeverity::Notable);
}

/// 來源：auditEvents.test.ts「only reports fields that actually changed」
#[test]
fn diff_only_reports_changed_fields() {
    let r = diff_changes(
        Some(&obj(json!({ "name": "業務部", "sortOrder": 1, "key": "sales" }))),
        Some(&obj(json!({ "name": "業務二部", "sortOrder": 1, "key": "sales" }))),
        &[],
    );
    assert_eq!(r.changes.keys().collect::<Vec<_>>(), vec!["name"]);
    assert_eq!(r.changes["name"], json!({ "from": "業務部", "to": "業務二部" }));
}

/// 來源：auditEvents.test.ts「treats structurally equal values as unchanged (not object identity)」
#[test]
fn diff_uses_structural_equality() {
    let r = diff_changes(Some(&obj(json!({ "tags": ["a", "b"] }))), Some(&obj(json!({ "tags": ["a", "b"] }))), &[]);
    assert!(r.changes.is_empty());
    // 數值依數值比。
    let r = diff_changes(Some(&obj(json!({ "n": 1 }))), Some(&obj(json!({ "n": 1.0 }))), &[]);
    assert!(r.changes.is_empty());
}

/// 來源：auditEvents.test.ts「reports added / removed keys」
#[test]
fn diff_reports_added_and_removed_keys() {
    let r = diff_changes(Some(&obj(json!({ "a": 1 }))), Some(&obj(json!({ "b": 2 }))), &[]);
    let mut keys: Vec<&String> = r.changes.keys().collect();
    keys.sort();
    assert_eq!(keys, vec!["a", "b"]);
    // undefined 那一側不出現在 JSON 裡。
    assert_eq!(r.changes["a"], json!({ "from": 1 }));
    assert_eq!(r.changes["b"], json!({ "to": 2 }));
}

/// 來源：auditEvents.test.ts「sensitive fields degrade to "changed" — never records before/after values」
#[test]
fn sensitive_fields_degrade_to_changed() {
    let r = diff_changes(
        Some(&obj(json!({ "name": "A", "bankAccount": "1234-5678" }))),
        Some(&obj(json!({ "name": "B", "bankAccount": "9999-0000" }))),
        &["bankAccount"],
    );
    assert_eq!(r.changed_fields, vec!["bankAccount".to_string()]);
    assert_eq!(r.changes.keys().collect::<Vec<_>>(), vec!["name"]);
    let s = serde_json::to_string(&r.changes).unwrap();
    assert!(!s.contains("1234"));
    assert!(!s.contains("9999"));
}

/// 來源：auditEvents.test.ts「handles null/undefined inputs」
#[test]
fn diff_handles_missing_inputs() {
    assert!(diff_changes(None, None, &[]).changes.is_empty());
    assert_eq!(diff_changes(None, Some(&obj(json!({ "a": 1 }))), &[]).changes.keys().collect::<Vec<_>>(), vec!["a"]);
}

/// 來源：auditEvents.test.ts「leaves small payloads untouched (same object)」
#[test]
fn truncate_leaves_small_payloads() {
    let p = json!({ "a": 1, "b": "x" });
    assert_eq!(truncate_payload(p.clone()), p);
}

/// 來源：auditEvents.test.ts「replaces oversized payloads with a fixed-shape marker」
#[test]
fn truncate_replaces_oversized_payloads() {
    let big = json!({ "blob": "x".repeat(PAYLOAD_MAX_BYTES + 100) });
    let out = truncate_payload(big);
    assert_eq!(out["_truncated"], json!(true));
    assert!(out["_originalBytes"].as_u64().unwrap() > PAYLOAD_MAX_BYTES as u64);
    assert!(out["_sample"].as_str().unwrap().chars().count() < 1100);
    assert!(out.get("blob").is_none());
}
