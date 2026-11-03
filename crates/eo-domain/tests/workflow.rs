//! 移植自 `tests/workflowSkeleton.test.ts`、`tests/defaultFlow.test.ts`、`tests/routeGateAudit.test.ts`
//! 的純邏輯部分：seed 定義的形狀、分支條件、路線推導。引擎與 DB 的部分屬於 eo-energy。

use eo_domain::conditions::*;
use eo_domain::document_ingest::EnergyDocType;
use eo_domain::route_preview::*;
use eo_domain::workflow_defs::*;
use serde_json::{json, Value};

fn bill(amount: i64) -> Value {
    json!({ "bill": { "totalAmount": amount } })
}

fn sorted(mut v: Vec<&str>) -> Vec<&str> {
    v.sort();
    v
}

/// 來源：workflowSkeleton.test.ts「seeds a published two-gate definition with nodes and transitions」
#[test]
fn minimal_definition_has_two_gates() {
    let d = minimal_bill_approval();
    assert_eq!(d.key, "bill-approval");
    let keys: Vec<&str> = d.nodes.iter().map(|n| n.node_key.as_str()).collect();
    assert_eq!(sorted(keys), vec!["end", "financeApproval", "operatorCheck", "start"]);
    // 預設 false ＝ 發起人不得核自己的單
    for n in d.nodes.iter().filter(|n| n.node_type == NodeType::HumanTask) {
        assert!(!n.allow_self_approval, "{}", n.node_key);
    }
    assert_eq!(minimal_bill_approval_binding().doc_type, None);
}

/// 來源：workflowSkeleton.test.ts「a definition whose node has no outgoing edge fails loudly」（路線那一半）
#[test]
fn dead_end_definition_has_no_next_step() {
    let mut d = WorkflowDefinition::new("deadend", "死路", "energyBill", "");
    d.nodes = vec![NodeDef::new("start", "起點", NodeType::Start)];
    let route = walk_route(&d, &json!({}));
    assert_eq!(route.path, vec!["start"]);
    assert!(select_transitions(&d.transitions, &json!({}), false).is_empty(), "引擎要據此報 no_matching_transition");
}

/// 來源：defaultFlow.test.ts「seeds all seven nodes with their actions」
#[test]
fn default_flow_has_seven_nodes() {
    let d = default_bill_approval();
    let keys: Vec<&str> = d.nodes.iter().map(|n| n.node_key.as_str()).collect();
    assert_eq!(
        sorted(keys),
        sorted(vec!["start", "clerk-review", "manager-approve", "exec-approve", "calc", "finance-release", "payment", "end"])
    );
    // 付款這一關不可回退。
    assert!(!d.node("payment").unwrap().revertable);
    // 計算節點掛著服務。
    assert_eq!(d.node("calc").unwrap().service_key.as_deref(), Some("rating.computePayout"));
    // 中斷核可者＝最高負責人。
    let AssignmentRule::Role { role_unique_names: Some(names) } = &d.abort_approval_rule else {
        panic!("中斷核可規則應為 role");
    };
    assert_eq!(sorted(names.iter().map(String::as_str).collect()), vec!["energy:admin", "energy:cxo"]);
    assert_eq!(default_bill_approval_binding().doc_type, Some(EnergyDocType::ElectricityBill));
}

/// 來源：defaultFlow.test.ts「自核旗標：只有「承辦確認」開，三個**核可**關卡一律關」
#[test]
fn only_clerk_review_allows_self_approval() {
    let d = default_bill_approval();
    assert!(d.node("clerk-review").unwrap().allow_self_approval, "承辦確認：送件者要能改自己被退回的單");
    for gate in ["manager-approve", "exec-approve", "finance-release", "payment"] {
        assert!(!d.node(gate).unwrap().allow_self_approval, "{gate} 是核可關卡，不可以自核");
    }
}

/// 來源：defaultFlow.test.ts「below the threshold (100000) goes straight to calc」（路線那一半）
#[test]
fn below_threshold_skips_exec_gate() {
    let route = walk_route(&default_bill_approval(), &bill(EXEC_APPROVAL_THRESHOLD - 1));
    assert_eq!(
        route.path,
        vec!["start", "clerk-review", "manager-approve", "calc", "finance-release", "payment", "end"]
    );
    assert!(!route.path.contains(&"exec-approve".to_string()), "不經過加簽");
}

/// 來源：defaultFlow.test.ts「at or above the threshold opens the exec gate first (US-8)」（路線那一半）
#[test]
fn at_threshold_opens_exec_gate() {
    let route = walk_route(&default_bill_approval(), &bill(EXEC_APPROVAL_THRESHOLD));
    assert_eq!(route.path[3], "exec-approve");
    let d = default_bill_approval();
    assert_eq!(d.node("exec-approve").unwrap().assignment_rule, AssignmentRule::role(&["energy:cxo"]));
}

/// 來源：routeGateAudit.test.ts「稽核算出的路線 ＝ 校對台預覽算出的路線（同一支 `walkRoute`）」（關卡差異那一半）
#[test]
fn lowering_the_amount_removes_a_gate() {
    let d = default_bill_approval();
    let before = gates_of(&walk_route(&d, &bill(138_195)));
    let after = gates_of(&walk_route(&d, &bill(99_999)));
    assert!(!before.contains(&"calc".to_string()), "calc 不是關卡");
    let delta = gate_delta(&before, &after);
    assert_eq!(delta.removed, vec!["exec-approve"]);
    assert!(delta.added.is_empty());
    assert!(delta.changed);
    assert!(!gate_delta(&before, &before).changed);
}

/// 保證金流程的起點叫 `submit`：路線推導要用 nodeType 找起點。
/// 來源：無對應 TS 測試；補驗 routePreview.ts 的 walkRoute（起點用 nodeType 找）。
#[test]
fn deposit_route_starts_at_its_own_start_node() {
    let d = deposit_request();
    assert_eq!(walk_route(&d, &json!({})).path, vec!["submit", "finance-release", "pay", "end"]);
    assert_eq!(deposit_request_binding().doc_type, Some(EnergyDocType::DepositRequest));
    assert!(d.node("submit").unwrap().allows_subject_edit);
}

/// 用電電費單沒有計算節點 —— 不會對要付出去的錢算分潤（由定義保證）。
/// 來源：無對應 TS 測試；補驗 seedDefinitions.ts 的 UTILITY_BILL／INVOICE_ISSUE。
#[test]
fn utility_bill_has_no_calc_node() {
    let d = utility_bill();
    assert!(d.nodes.iter().all(|n| n.node_type != NodeType::Calculation));
    let route = walk_route(&d, &bill(10_000_000));
    assert_eq!(route.path, vec!["start", "clerk-review", "manager-approve", "finance-release", "payment", "end"]);
    assert_eq!(utility_bill_binding().doc_type, Some(EnergyDocType::ConsumptionBill));
    assert_eq!(invoice_issue_binding().doc_type, Some(EnergyDocType::RenewableSurplus));
}

/// seed 預設值：revert／abort 理由必填、sideEffect 鎖回退、advance 走 transition。
/// 來源：無對應 TS 測試；補驗 seedDefinitions.ts 的 seedWorkflowDefinition 預設值。
#[test]
fn seed_defaults_are_applied() {
    for (def, _) in builtin_definitions() {
        for n in &def.nodes {
            for a in &n.actions {
                if matches!(a.kind, ActionKind::Revert | ActionKind::Abort) {
                    assert!(a.requires_reason, "{}/{}：退回與中斷必填理由", def.key, a.action_key);
                }
                if a.kind == ActionKind::SideEffect {
                    assert!(a.locks_revert && a.side_effect_key.is_some(), "{}/{}", def.key, a.action_key);
                }
                if a.kind == ActionKind::Advance {
                    assert_eq!(a.target_mode, Some(TargetMode::Transition));
                }
            }
            // 付款／開票這類有副作用的關一律不可回退。
            if n.actions.iter().any(|a| a.kind == ActionKind::SideEffect) {
                assert!(!n.revertable, "{}/{}", def.key, n.node_key);
            }
        }
        // 每條邊都指向存在的節點，每份定義都走得到 end。
        for t in &def.transitions {
            assert!(def.node(&t.from_node_key).is_some() && def.node(&t.to_node_key).is_some(), "{}", def.key);
        }
        assert_eq!(walk_route(&def, &json!({})).path.last().map(String::as_str), Some("end"), "{}", def.key);
    }
    assert_eq!(builtin_definitions().len(), 5);
}

/// 定義是資料：序列化成 JSON 再讀回來要一模一樣（條件、指派規則都是原樣的 JSON 形狀）。
/// 來源：無對應 TS 測試；補驗 seedDefinitions.ts 的定義資料形狀。
#[test]
fn definitions_roundtrip_through_json() {
    for (def, binding) in builtin_definitions() {
        let v = serde_json::to_value(&def).unwrap();
        let back: WorkflowDefinition = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(back, def, "{}", def.key);
        let _ = serde_json::to_value(&binding).unwrap();
    }
    let v = serde_json::to_value(default_bill_approval()).unwrap();
    let edge = v["transitions"].as_array().unwrap().iter().find(|t| t["toNodeKey"] == "exec-approve").unwrap();
    assert_eq!(edge["condition"], json!({ "attribute": "bill.totalAmount", "op": "gte", "value": 100000 }));
    assert_eq!(edge["label"], json!("金額 ≥ 100000"));
    let node = v["nodes"].as_array().unwrap().iter().find(|n| n["nodeKey"] == "manager-approve").unwrap();
    assert_eq!(
        node["assignmentRule"],
        json!({ "type": "departmentManagerOfInitiator", "params": { "fallback": "role", "roleUniqueNames": ["energy:cxo"] } })
    );
    let start = v["nodes"].as_array().unwrap().iter().find(|n| n["nodeKey"] == "start").unwrap();
    assert_eq!(start["assignmentRule"], json!({}));
}

// ---------------------------------------------------------------------------
// 條件求值（conditions.ts）
// ---------------------------------------------------------------------------

fn eval(c: Value, attrs: Value) -> bool {
    evaluate_condition(Some(&Condition::from(c)), &attrs)
}

/// 來源：conditions.ts 的 evaluateCondition（空物件＝無條件成立，認不得的形狀 fail-closed）。
#[test]
fn empty_condition_is_true_and_unknown_shape_is_false() {
    assert!(evaluate_condition(None, &json!({})));
    assert!(eval(json!({}), json!({})));
    assert!(eval(Value::Null, json!({})));
    assert!(!eval(json!({ "attr": "bill.totalAmount", "op": "gte" }), json!({})), "打錯字的條件不可變成預設邊");
    assert!(!eval(json!({ "attribute": "x", "op": "between", "value": 1 }), json!({ "x": 1 })));
}

/// 來源：conditions.ts 的 compare（大小比較只對數字，不做隱式轉型）。
#[test]
fn comparisons_do_not_coerce() {
    let a = json!({ "bill": { "totalAmount": 100000, "label": "10" } });
    assert!(eval(json!({ "attribute": "bill.totalAmount", "op": "gte", "value": 100000 }), a.clone()));
    assert!(!eval(json!({ "attribute": "bill.totalAmount", "op": "gt", "value": 100000 }), a.clone()));
    assert!(!eval(json!({ "attribute": "bill.label", "op": "gt", "value": 9 }), a.clone()), "'10' > 9 不成立");
    assert!(eval(json!({ "attribute": "bill.totalAmount", "op": "eq", "value": 100000.0 }), a.clone()));
    assert!(!eval(json!({ "attribute": "bill.label", "op": "eq", "value": 10 }), a.clone()));
    assert!(eval(json!({ "attribute": "bill.label", "op": "in", "value": ["9", "10"] }), a.clone()));
    assert!(!eval(json!({ "attribute": "bill.label", "op": "in", "value": "10" }), a.clone()));
    assert!(eval(json!({ "attribute": "bill.missing", "op": "nin", "value": [1] }), a.clone()));
    assert!(eval(json!({ "attribute": "bill.totalAmount", "op": "exists" }), a.clone()));
    assert!(!eval(json!({ "attribute": "bill.missing.deep", "op": "exists" }), a.clone()));
    // 物件只有同一個實體才 ===，從 JSON 來的永遠不等。
    assert!(!eval(json!({ "attribute": "bill", "op": "eq", "value": { "totalAmount": 100000, "label": "10" } }), a));
}

/// 來源：conditions.ts 的 all／any／not 巢狀。
#[test]
fn nested_all_any_not() {
    let a = json!({ "bill": { "totalAmount": 5 }, "kind": "x" });
    let c = json!({ "all": [
        { "attribute": "bill.totalAmount", "op": "lt", "value": 10 },
        { "any": [ { "attribute": "kind", "op": "eq", "value": "y" }, { "not": { "attribute": "kind", "op": "eq", "value": "y" } } ] },
    ]});
    assert!(eval(c.clone(), a.clone()));
    assert!(eval(json!({ "all": [] }), a.clone()));
    assert!(!eval(json!({ "any": [] }), a.clone()));
    // 條件原樣存回去。
    assert_eq!(Value::from(Condition::from(c.clone())), c);
}

/// 來源：conditions.ts 的 selectTransitions（依 sortOrder 取第一個成立者；parallel 全開）。
#[test]
fn select_transitions_picks_first_by_sort_order() {
    let edges = vec![
        TransitionDef::new("a", "fallback", 10),
        TransitionDef::new("a", "big", 0)
            .when("大額", Condition::leaf("bill.totalAmount", ConditionOp::Gte, json!(100))),
        TransitionDef::new("a", "also", 10),
    ];
    let pick = |amount: i64| select_transitions(&edges, &bill(amount), false).iter().map(|t| t.to_node_key.clone()).collect::<Vec<_>>();
    assert_eq!(pick(100), vec!["big"]);
    assert_eq!(pick(99), vec!["fallback"], "同 sortOrder 保留原順序");
    let all: Vec<&str> = select_transitions(&edges, &bill(100), true).iter().map(|t| t.to_node_key.as_str()).collect();
    assert_eq!(all, vec!["big", "fallback", "also"]);
}

/// 來源：無對應 TS 測試；補驗 conditions.ts 的 readAttribute。
#[test]
fn read_attribute_follows_dot_paths() {
    let a = json!({ "bill": { "lines": [ { "kwh": 3 } ], "no": "AB" } });
    assert_eq!(read_attribute(&a, "bill.lines.0.kwh"), Some(json!(3)));
    assert_eq!(read_attribute(&a, "bill.lines.length"), Some(json!(1)));
    assert_eq!(read_attribute(&a, "bill.no.length"), Some(json!(2)));
    assert_eq!(read_attribute(&a, "bill.none.x"), None);
}
