use eo_domain::rent_cycle::{preview, Plan, Receipt};
use serde_json::{json, Value};

fn plan(transfer: bool) -> Plan {
    serde_json::from_value(json!({"stationId":1,"anchor":"2026-07-01","months":3,
        "transfer":transfer,"kind":"revenueShare","basis":"untaxed","sharePercent":"10",
        "area":null,"unitPrice":null,"unitPeriod":"month","roundingPlaces":2})).unwrap()
}
fn receipt(id:i64, source:&str, start:&str,end:&str,amount:&str) -> Receipt {
    serde_json::from_value(json!({"billId":id,"source":source,"start":start,"end":end,
        "untaxed":amount,"gross":null,"kwh":"0"})).unwrap()
}
fn run(p:&Plan, rows:Vec<Receipt>) -> Value { preview(p,"2026-07-01",&rows).unwrap() }

#[test]
fn quarterly_requires_both_sources_even_when_zero() {
    let p=plan(true);
    let a=receipt(1,"taipower","2026-07-01","2026-09-30","10000");
    let missing=run(&p,vec![a.clone()]);
    assert_eq!(missing["baseRent"],Value::Null);
    assert_eq!(missing["sources"]["transfer"]["status"],"missing");
    let complete=run(&p,vec![a,receipt(2,"transfer","2026-07-01","2026-09-30","0")]);
    assert_eq!(complete["baseRent"],"1000.00");
    assert_eq!(complete["sources"]["transfer"]["status"],"received");
    assert_eq!(complete["paymentAllowed"],false);
    assert_eq!(complete["netPayable"],Value::Null);
}
#[test]
fn missing_value_is_not_zero_and_gaps_or_duplicates_block() {
    let p=plan(false);
    let a=receipt(1,"taipower","2026-07-01","2026-09-30","0");
    assert_eq!(run(&p,vec![a.clone()])["baseRent"],"0.00");
    let mut missing=a.clone(); missing.untaxed=None;
    assert_eq!(run(&p,vec![missing])["baseRent"],Value::Null);
    assert_eq!(run(&p,vec![a.clone(),a])["baseRent"],Value::Null);
    assert_eq!(run(&p,vec![receipt(1,"taipower","2026-08-01","2026-09-30","500")])["baseRent"],Value::Null);
}
#[test]
fn monthly_inputs_sum_without_net_bill_total_and_keep_kwh() {
    let rows=vec![receipt(1,"taipower","2026-07-01","2026-07-31","100"),
        receipt(2,"taipower","2026-08-01","2026-08-31","200"),
        receipt(3,"taipower","2026-09-01","2026-09-30","300")];
    let v=run(&plan(false),rows);
    assert_eq!(v["baseRent"],"60.00");
    assert_eq!(v["sources"]["taipower"]["kwh"],"0");
    assert_eq!(v["sources"]["transfer"]["status"],"notApplicable");
}
#[test]
fn fixed_area_rent_uses_explicit_unit_period() {
    let mut p=plan(false); p.kind="fixedArea".into();p.area=Some("100".into());p.unit_price=Some("12.5".into());
    assert_eq!(run(&p,vec![])["baseRent"],"3750.00");
    p.unit_period="cycle".into();
    assert_eq!(run(&p,vec![])["baseRent"],"1250.00");
    p.transfer=true;
    assert_eq!(run(&p,vec![])["baseRent"],Value::Null);
}
#[test]
fn cycle_alignment_and_invalid_money_are_rejected() {
    let p=plan(false);
    assert!(preview(&p,"2026-08-01",&[]).is_err());
    let mut bad=p;bad.share_percent=Some("NaN".into());
    assert!(preview(&bad,"2026-07-01",&[]).is_err());
}

#[test]
fn overlapping_periods_and_cross_cycle_bills_are_not_silently_prorated() {
    let p=plan(false);
    let a=receipt(1,"taipower","2026-07-01","2026-08-31","100");
    let b=receipt(2,"taipower","2026-08-01","2026-09-30","100");
    assert_eq!(run(&p,vec![a,b])["baseRent"],Value::Null);
    assert_eq!(run(&p,vec![receipt(3,"taipower","2026-06-01","2026-09-30","100")])["baseRent"],Value::Null);
    let mut r=receipt(4,"taipower","2026-07-01","2026-09-30","100");r.kwh=None;
    assert_eq!(run(&p,vec![r])["baseRent"],Value::Null);
}
