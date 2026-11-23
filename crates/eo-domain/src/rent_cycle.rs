//! Landlord rent cycle drafts. Never authorizes payment or guesses tax treatment.
use chrono::{Datelike, Days, Months, NaiveDate};
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub struct Plan {
    pub station_id:i64,
    pub anchor:String,
    pub months:u32,
    pub transfer:bool,
    pub kind:String,
    pub basis:String,
    pub share_percent:Option<String>,
    pub area:Option<String>,
    pub unit_price:Option<String>,
    pub unit_period:String,
    pub rounding_places:u32,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub fee_kind:Option<String>,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub monthly_minimum:Option<String>,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub commercial_operation_date:Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub bill_id:i64,
    pub source:String,
    pub start:String,
    pub end:String,
    pub untaxed:Option<String>,
    pub gross:Option<String>,
    pub kwh:Option<String>,
}
fn day(s:&str)->Result<NaiveDate,String> {
    NaiveDate::parse_from_str(s,"%Y-%m-%d").map_err(|_|"日期格式必須為 YYYY-MM-DD".into())
}
fn number(s:Option<&str>)->Result<Decimal,String> {
    let d=Decimal::from_str(s.ok_or("缺少數值")?).map_err(|_|"數值格式不正確")?;
    // Bound inputs to avoid overflow and unreasonable accidental identifiers as amounts.
    if d.abs()>Decimal::from(1_000_000_000_000i64) { return Err("數值超出支援範圍".into()); }
    Ok(d)
}
pub fn validate(p:&Plan)->Result<(),String> {
    if p.fee_kind.as_deref().is_some_and(|k| !["rent","management"].contains(&k)) {return Err("費用性質須為租金或管理費".into());}
    if let Some(date)=&p.commercial_operation_date {day(date)?;}
    if let Some(floor)=&p.monthly_minimum {
        if p.kind!="revenueShare" || number(Some(floor))?<Decimal::ZERO {return Err("每月保底僅適用比例計算，且不得為負數".into());}
        if p.commercial_operation_date.is_none() {return Err("設定每月保底前，請確認正式併聯發電日期".into());}
    }
    if p.station_id<=0 || !(1..=12).contains(&p.months) || p.rounding_places>2 {return Err("案場、付款週期或小數位數不正確".into());}
    if day(&p.anchor)?.day()!=1 {return Err("週期起點目前支援每月一日；不完整月份需另行確認".into());}
    if !["untaxed","gross"].contains(&p.basis.as_str()) {return Err("請選擇含稅或未稅收入基底".into());}
    if !["month","cycle"].contains(&p.unit_period.as_str()) {return Err("請指定單價為每月或每期".into());}
    match p.kind.as_str() {
        "fixedArea"=>{
            if number(p.area.as_deref())?<=Decimal::ZERO || number(p.unit_price.as_deref())?<Decimal::ZERO {return Err("面積須大於零，單價不得為負".into());}
        },
        "revenueShare"=>{
            let rate=number(p.share_percent.as_deref())?;
            if rate<Decimal::ZERO || rate>Decimal::from(100) {return Err("比例須介於 0 與 100".into());}
        },
        _=>return Err("未知租金計算方式".into()),
    }
    Ok(())
}
pub fn bounds(p:&Plan,start:&str)->Result<(NaiveDate,NaiveDate),String> {
    validate(p)?;
    let start=day(start)?;let anchor=day(&p.anchor)?;
    let diff=(start.year()-anchor.year())*12+start.month() as i32-anchor.month() as i32;
    if start.day()!=1 || diff<0 || diff%p.months as i32!=0 {return Err("計算起日不符合合約付款週期起點".into());}
    let end=start.checked_add_months(Months::new(p.months)).and_then(|d|d.checked_sub_days(Days::new(1))).ok_or("日期超出範圍")?;
    Ok((start,end))
}
pub fn preview(p:&Plan,start:&str,rows:&[Receipt])->Result<Value,String> {
    let (from,to)=bounds(p,start)?;
    if rows.len()>500 {return Err("單期單據超過 500 張，請縮小範圍".into());}
    if let Some(date)=&p.commercial_operation_date {
        let operation=day(date)?;
        if operation>to {
            return Ok(json!({"schemaVersion":"rent-cycle-draft/v1","state":"draft","periodStart":from.to_string(),"periodEnd":to.to_string(),
                "plan":p,"receipts":rows,"sources":{"taipower":{"status":"notRequired","amount":null,"kwh":null,"billIds":[]},
                "transfer":{"status":if p.transfer {"notRequired"}else{"notApplicable"},"amount":null,"kwh":null,"billIds":[]}},
                "issues":[],"baseRent":format!("{:.*}",p.rounding_places as usize,Decimal::ZERO),"monthlyBreakdown":[],
                "feeKind":p.fee_kind.as_deref().unwrap_or("rent"),"chargeStatus":"notStarted",
                "taxStatus":"pendingConfirmation","netPayable":null,"paymentAllowed":false,"invoiceDirection":"receiveLandlordInvoice",
                "note":"整期在正式併聯發電日前，不計費；不是零收入單據，也不啟動付款。"}));
        }
        if operation>from {return Err("首次併聯落在本期內；首月及不完整週期計費方式待確認，不自動按日分攤或套整月保底".into());}
    }
    let mut issues=Vec::<String>::new();let mut ids=HashSet::new();
    for r in rows {
        if !ids.insert(r.bill_id) {issues.push(format!("單據 {} 重複選取",r.bill_id));}
        if !["taipower","transfer"].contains(&r.source.as_str()) {issues.push(format!("單據 {} 尚未指定來源",r.bill_id));}
        if !p.transfer && r.source=="transfer" {issues.push("非轉供案場卻選入轉供單據，請確認設定".into());}
    }
    let mut sources=serde_json::Map::new();let mut revenue=Decimal::ZERO;
    for source in ["taipower","transfer"] {
        let selected:Vec<_>=rows.iter().filter(|r|r.source==source).collect();
        let required=p.kind=="revenueShare" || p.transfer;
        let applicable=source=="taipower" || p.transfer;
        let label=if source=="taipower" {"台電"}else{"轉供"};
        let before=issues.len();let mut amount=Decimal::ZERO;let mut kwh=Decimal::ZERO;let mut ranges=vec![];
        for r in &selected {
            match (day(&r.start),day(&r.end)) {
                (Ok(a),Ok(b)) if a<=b && a>=from && b<=to=>ranges.push((a,b)),
                _=>issues.push(format!("{label}單據 {} 期間不完整或跨出付款週期；不可直接整筆加總",r.bill_id)),
            }
            let value=if p.basis=="untaxed" {&r.untaxed}else{&r.gross};
            match number(value.as_deref()) {
                Ok(n)=>amount+=n,
                Err(_)=>issues.push(format!("{label}單據 {} 缺少或無法判讀選定收入基底",r.bill_id)),
            }
            match number(r.kwh.as_deref()) {
                Ok(n) if n>=Decimal::ZERO=>kwh+=n,
                _=>issues.push(format!("{label}單據 {} 缺少或無法判讀度數",r.bill_id)),
            }
        }
        ranges.sort();let mut cursor=from;
        if applicable && required {
            for (a,b) in &ranges {
                if *a!=cursor {issues.push(format!("{label}單據期間有缺漏或重疊，請確認所有電號與帳期"));}
                cursor=b.checked_add_days(Days::new(1)).ok_or("日期超出範圍")?;
            }
            if cursor<=to {issues.push(format!("{label}尚未收齊本週期單據；金額為零也必須有單據"));}
        }
        let complete=issues.len()==before && !selected.is_empty();
        let status=if !applicable {"notApplicable"}else if selected.is_empty() && !required {"notRequired"}else if selected.is_empty() {"missing"}else if complete {"received"}else{"needsReview"};
        sources.insert(source.into(),json!({"status":status,"amount":if complete {Some(amount.normalize().to_string())}else{None},
            "kwh":if complete {Some(kwh.normalize().to_string())}else{None},"billIds":selected.iter().map(|r|r.bill_id).collect::<Vec<_>>()}));
        if complete && applicable {revenue+=amount;}
    }
    let mut monthly=Vec::new();let mut floor_total=Decimal::ZERO;
    if let Some(floor)=&p.monthly_minimum {
        let floor=number(Some(floor))?;
        for r in rows {
            if let (Ok(a),Ok(b))=(day(&r.start),day(&r.end)) {
                if (a.year(),a.month())!=(b.year(),b.month()) {issues.push(format!("單據 {} 跨月；每月保底需月別收入，不得將整季總額平均分攤",r.bill_id));}
            }
        }
        if issues.is_empty() {
            for offset in 0..p.months {
                let month=from.checked_add_months(Months::new(offset)).ok_or("日期超出範圍")?;
                let mut income=Decimal::ZERO;
                for r in rows {
                    let date=day(&r.start)?;
                    if (date.year(),date.month())==(month.year(),month.month()) {
                        income+=number(if p.basis=="gross" {r.gross.as_deref()}else{r.untaxed.as_deref()})?;
                    }
                }
                let proportional=income.checked_mul(number(p.share_percent.as_deref())?).and_then(|n|n.checked_div(Decimal::from(100))).ok_or("金額超出範圍")?;
                let amount=proportional.max(floor);floor_total+=amount;
                monthly.push(json!({"month":month.format("%Y-%m").to_string(),"income":income.normalize().to_string(),
                    "proportionalAmount":proportional.normalize().to_string(),"minimum":floor.normalize().to_string(),
                    "amountAfterFloor":amount.normalize().to_string()}));
            }
        }
    }
    let base=if issues.is_empty() {
        let raw=if p.kind=="fixedArea" {
            number(p.area.as_deref())?.checked_mul(number(p.unit_price.as_deref())?)
                .and_then(|n|n.checked_mul(Decimal::from(if p.unit_period=="month" {p.months}else{1})))
        }else if p.monthly_minimum.is_some() {Some(floor_total)}
        else{revenue.checked_mul(number(p.share_percent.as_deref())?).and_then(|n|n.checked_div(Decimal::from(100)))}.ok_or("計算金額超出範圍")?;
        Some(format!("{:.*}",p.rounding_places as usize,raw.round_dp_with_strategy(p.rounding_places,RoundingStrategy::MidpointAwayFromZero)))
    }else{None};
    Ok(json!({"schemaVersion":"rent-cycle-draft/v1","state":"draft","periodStart":from.to_string(),"periodEnd":to.to_string(),
        "plan":p,"receipts":rows,"sources":sources,"issues":issues,"baseRent":base,
        "monthlyBreakdown":monthly,"feeKind":p.fee_kind.as_deref().unwrap_or("rent"),"chargeStatus":"calculating",
        "taxStatus":"pendingConfirmation","netPayable":null,"paymentAllowed":false,
        "invoiceDirection":"receiveLandlordInvoice","note":"此為租金基礎試算；稅費、扣繳與正式付款待範例確認，不產生付款或變更審核狀態。"}))
}
