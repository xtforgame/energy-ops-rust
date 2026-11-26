use eo_domain::rent_cycle::{application, preview, ApplicationEntryInput, Plan, Receipt};
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
    assert_eq!(complete["baseRent"],"1000");
    assert_eq!(complete["sources"]["transfer"]["status"],"received");
    assert_eq!(complete["paymentAllowed"],false);
    assert_eq!(complete["netPayable"],Value::Null);
}
#[test]
fn missing_value_is_not_zero_and_gaps_or_duplicates_block() {
    let p=plan(false);
    let a=receipt(1,"taipower","2026-07-01","2026-09-30","0");
    assert_eq!(run(&p,vec![a.clone()])["baseRent"],"0");
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
    assert_eq!(v["baseRent"],"60");
    assert_eq!(v["sources"]["taipower"]["kwh"],"0");
    assert_eq!(v["sources"]["transfer"]["status"],"notApplicable");
}
#[test]
fn fixed_area_rent_uses_explicit_unit_period() {
    let mut p=plan(false); p.kind="fixedArea".into();p.area=Some("100".into());p.unit_price=Some("12.5".into());
    assert_eq!(run(&p,vec![])["baseRent"],"3750");
    p.unit_period="cycle".into();
    assert_eq!(run(&p,vec![])["baseRent"],"1250");
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

#[test]
fn monthly_management_floor_is_applied_before_quarter_aggregation() {
    let mut p=plan(false);p.share_percent=Some("8".into());
    p.monthly_minimum=Some("8000".into());p.fee_kind=Some("management".into());
    p.commercial_operation_date=Some("2026-01-15".into());
    let rows=vec![receipt(1,"taipower","2026-07-01","2026-07-31","100000"),
        receipt(2,"taipower","2026-08-01","2026-08-31","200000"),
        receipt(3,"taipower","2026-09-01","2026-09-30","0")];
    let v=run(&p,rows);
    assert_eq!(v["baseRent"],"32000");
    assert_eq!(v["monthlyBreakdown"][2]["amountAfterFloor"],"8000");
    assert_eq!(v["feeKind"],"management");
    assert_eq!(v["netPayable"],Value::Null);
}

#[test]
fn monthly_floor_does_not_fill_missing_bills_or_guess_cross_month_allocation() {
    let mut p=plan(false);p.share_percent=Some("8".into());p.monthly_minimum=Some("8000".into());
    p.commercial_operation_date=Some("2026-01-01".into());
    assert_eq!(run(&p,vec![])["baseRent"],Value::Null);
    assert_eq!(run(&p,vec![receipt(1,"taipower","2026-07-01","2026-09-30","300000")])["baseRent"],Value::Null);
    p.commercial_operation_date=None;
    assert!(preview(&p,"2026-07-01",&[]).is_err());
}

#[test]
fn management_is_free_before_operation_but_first_partial_cycle_needs_confirmation() {
    let mut p=plan(false);p.monthly_minimum=Some("8000".into());p.fee_kind=Some("management".into());
    p.commercial_operation_date=Some("2026-10-15".into());
    assert_eq!(run(&p,vec![])["baseRent"],"0");
    assert_eq!(run(&p,vec![])["chargeStatus"],"notStarted");
    assert!(preview(&p,"2026-10-01",&[]).is_err());
}

#[test]
fn first_contract_monthly_eight_percent_uses_gross_income() {
    let mut p=plan(false);p.months=1;p.basis="gross".into();p.share_percent=Some("8".into());
    let mut row=receipt(1,"taipower","2026-07-01","2026-07-31","100000");row.gross=Some("105000".into());
    assert_eq!(run(&p,vec![row])["baseRent"],"8400");
}

// ── S1：R-A fixedAnnual，整組年額，不需面積也不需單據 ──
#[test]
fn fixed_annual_prorates_whole_contract_amount_by_months() {
    let mut p=plan(false);p.kind="fixedAnnual".into();p.annual_amount=Some("240000".into());p.months=6;
    assert_eq!(run(&p,vec![])["baseRent"],"120000");
}

// ── S2/S4：R-C annualTiered，照獅甲公證書 39 列級距表查表；T-12 逐張單據先取整再加總成參考年租金 ──
fn tier_plan(annual_amount:&str,share_percent:&str)->Plan {
    let mut p=plan(false);p.months=12;p.kind="annualTiered".into();
    p.annual_amount=Some(annual_amount.into());p.share_percent=Some(share_percent.into());
    p
}
#[test]
fn annual_tiered_no_adjustment_band_keeps_full_baseline() {
    // 回饋 10%、單張收入 1,000,000 -> 參考年租金 100,000，比值 100%，落在 95~105% 不調整帶，應收基準 100%。
    let p=tier_plan("100000","10");
    let r=receipt(1,"taipower","2026-07-01","2027-06-30","1000000");
    let v=run(&p,vec![r]);
    assert_eq!(v["baseRent"],"100000");
    assert_eq!(v["referenceRent"],"100000");
    assert_eq!(v["tierPercent"],"100");
}
#[test]
fn annual_tiered_below_five_percent_charges_five_percent_of_baseline() {
    // 比值 3%（未滿 5%），獅甲表第(一)項：照經營年租金 5% 計收。
    let p=tier_plan("100000","10");
    let r=receipt(1,"taipower","2026-07-01","2027-06-30","30000");
    let v=run(&p,vec![r]);
    assert_eq!(v["tierPercent"],"5");
    assert_eq!(v["baseRent"],"5000");
}
#[test]
fn annual_tiered_between_105_and_110_rounds_up_to_110_percent() {
    // 比值 107%（逾 105 未滿 110），獅甲表第(二十)項：照 110% 計收，往上取不往下取。
    let p=tier_plan("100000","10");
    let r=receipt(1,"taipower","2026-07-01","2027-06-30","1070000");
    let v=run(&p,vec![r]);
    assert_eq!(v["tierPercent"],"110");
    assert_eq!(v["baseRent"],"110000");
}
#[test]
fn annual_tiered_at_or_above_200_percent_returns_issue_without_amount() {
    // 比值 200% 以上（獅甲表第 39 項）：契約應變更，不自動出金額。
    let p=tier_plan("100000","10");
    let r=receipt(1,"taipower","2026-07-01","2027-06-30","2000000");
    let v=run(&p,vec![r]);
    assert_eq!(v["baseRent"],Value::Null);
    assert!(v["issues"].as_array().unwrap().iter().any(|i|i.as_str().unwrap().contains("契約應變更")));
}

// ── S3/S5：R-D kwhRate，逐張單據＝度數×每度費率×比例，先取整再加總 ──
#[test]
fn kwh_rate_sums_each_bill_rounded_before_adding() {
    let mut p=plan(false);p.kind="kwhRate".into();p.rate_per_kwh=Some("5.2148".into());p.share_percent=Some("18".into());p.months=2;
    let mut a=receipt(1,"taipower","2026-07-01","2026-07-31","0");a.kwh=Some("175196".into());
    let mut b=receipt(2,"taipower","2026-08-01","2026-08-31","0");b.kwh=Some("1000".into());
    // 175196*5.2148*18% = 164450.178...→164450；1000*5.2148*18%=938.664→939；合計 165389。
    let v=run(&p,vec![a,b]);
    assert_eq!(v["baseRent"],"165389");
}

// ── S6：T-11，雙月單整張落在本期照算；跨結算期的單據回新文字 issue，不是舊的「不可直接整筆加總」 ──
#[test]
fn bimonthly_bill_fully_inside_period_is_accepted_but_crossing_period_needs_confirmation() {
    let p=plan(false);
    let bimonthly=receipt(1,"taipower","2026-07-01","2026-08-31","200");
    let last_month=receipt(2,"taipower","2026-09-01","2026-09-30","100");
    let v=run(&p,vec![bimonthly,last_month]);
    assert_eq!(v["baseRent"],"30");
    let crossing=receipt(3,"taipower","2026-09-01","2026-10-15","100");
    let v2=run(&p,vec![crossing]);
    assert_eq!(v2["baseRent"],Value::Null);
    assert!(v2["issues"].as_array().unwrap().iter().any(|i|i.as_str().unwrap().contains("與 PM 11/25 確認不符")));
}

// ── 卡 20261126-0416 S1／S3：`application()` 純函式，合成數字（金額只用來驗加總，不是真資料）──
fn entry(contract_no:&str, tax_id:Option<&str>, invoice_timing:Option<&str>, base_rent:Option<&str>, has_issue:bool) -> ApplicationEntryInput {
    ApplicationEntryInput {
        contract_no: contract_no.into(), rent_calc_code: Some("R-A".into()), kind: "fixedAnnual".into(),
        landowner_names: vec![format!("地主-{contract_no}")],
        landowner_tax_id: tax_id.map(String::from),
        invoice_timing: invoice_timing.map(String::from),
        base_rent: base_rent.map(String::from), has_issue,
    }
}

#[test]
fn two_landowners_same_period_merge_into_one_application_with_two_rows() {
    let company=entry("C-1",Some("12345678"),Some("before"),Some("1000"),false);
    let individual=entry("C-2",Some("A123456789"),None,Some("2000"),false);
    let v=application("測試案場","002","測試公司","2026-07-01","2026-09-30",&[company,individual]);
    assert_eq!(v["summary"],"支付測試案場租金（2026-07-01～2026-09-30）");
    assert_eq!(v["entries"].as_array().unwrap().len(),2);
    assert_eq!(v["total"],"3000");
    assert_eq!(v["state"],"draft");
    assert_eq!(v["paymentAllowed"],false);
}

#[test]
fn voucher_text_covers_company_three_timings_and_individual_and_unknown() {
    let before=entry("C-1",Some("12345678"),Some("before"),Some("100"),false);
    let later=entry("C-2",Some("12345678"),Some("later"),Some("100"),false);
    let unknown_timing=entry("C-3",Some("12345678"),Some("unknown"),Some("100"),false);
    let individual=entry("C-4",Some("A123456789"),None,Some("100"),false);
    let no_tax_id=entry("C-5",None,None,Some("100"),false);
    let v=application("測試案場","待補","待補","2026-07-01","2026-09-30",
        &[before,later,unknown_timing,individual,no_tax_id]);
    let vouchers:Vec<&str>=v["entries"].as_array().unwrap().iter().map(|e|e["voucher"].as_str().unwrap()).collect();
    assert_eq!(vouchers,vec!["有，發票","無，後補","待確認","有，租金計算表","地主類型待確認"]);
}

#[test]
fn any_entry_with_issue_blanks_the_whole_total_and_marks_needs_review() {
    let ok=entry("C-1",Some("12345678"),Some("before"),Some("1000"),false);
    let broken=entry("C-2",Some("A123456789"),None,None,true);
    let v=application("測試案場","002","測試公司","2026-07-01","2026-09-30",&[ok,broken]);
    assert_eq!(v["total"],Value::Null);
    assert_eq!(v["state"],"needsReview");
}
