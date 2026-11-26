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
    /// fixedAnnual：整組年額；annualTiered：年度基準（經營年租金）。
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub annual_amount:Option<String>,
    /// kwhRate：每度費率。
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub rate_per_kwh:Option<String>,
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
        "fixedAnnual"=>{
            if number(p.annual_amount.as_deref())?<=Decimal::ZERO {return Err("整組年額須大於零".into());}
        },
        "annualTiered"=>{
            if number(p.annual_amount.as_deref())?<=Decimal::ZERO {return Err("年度基準（經營年租金）須大於零".into());}
            let rate=number(p.share_percent.as_deref())?;
            if rate<Decimal::ZERO || rate>Decimal::from(100) {return Err("售電回饋百分比須介於 0 與 100".into());}
            if p.months!=12 {return Err("年度基準級距僅支援 12 個月付款週期".into());}
        },
        "kwhRate"=>{
            if number(p.rate_per_kwh.as_deref())?<Decimal::ZERO {return Err("每度費率不得為負".into());}
            let rate=number(p.share_percent.as_deref())?;
            if rate<Decimal::ZERO || rate>Decimal::from(100) {return Err("比例須介於 0 與 100".into());}
        },
        _=>return Err("未知租金計算方式".into()),
    }
    Ok(())
}
/// 獅甲公證書第五條第二項逐列級距表（每 5 個百分點一階，原文「未滿 X%／逾 A% 未滿 B%」者照 B% 計收；
/// 95%～105% 不調整照 100% 收，在此以門檻 105→100 表示）。找第一個 ratio < 門檻 的列，回傳對應百分比；
/// 都不成立（ratio ≥ 200）回 None，由呼叫端轉成「依契約應變更經營年租金，不自動計算」的 issue。
const SCHOOL_ROOF_TIERS:&[(i64,i64)]=&[
    (5,5),(10,10),(15,15),(20,20),(25,25),(30,30),(35,35),(40,40),(45,45),(50,50),
    (55,55),(60,60),(65,65),(70,70),(75,75),(80,80),(85,85),(90,90),(95,95),
    (105,100),
    (110,110),(115,115),(120,120),(125,125),(130,130),(135,135),(140,140),(145,145),(150,150),
    (155,155),(160,160),(165,165),(170,170),(175,175),(180,180),(185,185),(190,190),(195,195),(200,200),
];
fn tier_percent(ratio:Decimal)->Option<Decimal> {
    for (threshold,percent) in SCHOOL_ROOF_TIERS {
        if ratio<Decimal::from(*threshold) {return Some(Decimal::from(*percent));}
    }
    None
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
                "issues":[],"baseRent":Decimal::ZERO.to_string(),"lines":[],"referenceRent":null,"ratioPercent":null,"tierPercent":null,"monthlyBreakdown":[],
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
    let line_kind:Option<&str>=match p.kind.as_str() {"revenueShare"|"annualTiered"=>Some("income"),"kwhRate"=>Some("kwh"),_=>None};
    let mut sources=serde_json::Map::new();let mut revenue=Decimal::ZERO;
    let mut lines=Vec::<Value>::new();let mut line_total=Decimal::ZERO;
    for source in ["taipower","transfer"] {
        let selected:Vec<_>=rows.iter().filter(|r|r.source==source).collect();
        let required=["revenueShare","annualTiered","kwhRate"].contains(&p.kind.as_str()) || p.transfer;
        let applicable=source=="taipower" || p.transfer;
        let label=if source=="taipower" {"台電"}else{"轉供"};
        let before=issues.len();let mut amount=Decimal::ZERO;let mut kwh=Decimal::ZERO;let mut ranges=vec![];
        let mut source_line_total=Decimal::ZERO;
        for r in &selected {
            match (day(&r.start),day(&r.end)) {
                (Ok(a),Ok(b)) if a<=b && a>=from && b<=to=>ranges.push((a,b)),
                (Ok(a),Ok(b)) if a<=b=>issues.push(format!("{label}單據 {} 計費期間跨結算期，與 PM 11/25 確認不符，請人工確認",r.bill_id)),
                _=>issues.push(format!("{label}單據 {} 期間不完整或無法判讀",r.bill_id)),
            }
            let value=if p.basis=="untaxed" {&r.untaxed}else{&r.gross};
            match number(value.as_deref()) {
                Ok(n)=>{
                    amount+=n;
                    if line_kind==Some("income") {
                        let share=number(p.share_percent.as_deref())?;
                        let raw=n.checked_mul(share).and_then(|x|x.checked_div(Decimal::from(100))).ok_or("金額超出範圍")?;
                        let rounded=raw.round_dp_with_strategy(0,RoundingStrategy::MidpointAwayFromZero);
                        source_line_total+=rounded;
                        lines.push(json!({"billId":r.bill_id,"source":source,"basis":n.normalize().to_string(),"lineAmount":rounded.to_string()}));
                    }
                },
                Err(_)=>issues.push(format!("{label}單據 {} 缺少或無法判讀選定收入基底",r.bill_id)),
            }
            match number(r.kwh.as_deref()) {
                Ok(n) if n>=Decimal::ZERO=>{
                    kwh+=n;
                    if line_kind==Some("kwh") {
                        let rate=number(p.rate_per_kwh.as_deref())?;let share=number(p.share_percent.as_deref())?;
                        let raw=n.checked_mul(rate).and_then(|x|x.checked_mul(share)).and_then(|x|x.checked_div(Decimal::from(100))).ok_or("金額超出範圍")?;
                        let rounded=raw.round_dp_with_strategy(0,RoundingStrategy::MidpointAwayFromZero);
                        source_line_total+=rounded;
                        lines.push(json!({"billId":r.bill_id,"source":source,"basis":n.normalize().to_string(),"lineAmount":rounded.to_string()}));
                    }
                },
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
        if complete && applicable {revenue+=amount;line_total+=source_line_total;}
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
                let proportional_raw=income.checked_mul(number(p.share_percent.as_deref())?).and_then(|n|n.checked_div(Decimal::from(100))).ok_or("金額超出範圍")?;
                let proportional=proportional_raw.round_dp_with_strategy(0,RoundingStrategy::MidpointAwayFromZero);
                let floor_rounded=floor.round_dp_with_strategy(0,RoundingStrategy::MidpointAwayFromZero);
                let amount=proportional.max(floor_rounded);floor_total+=amount;
                monthly.push(json!({"month":month.format("%Y-%m").to_string(),"income":income.normalize().to_string(),
                    "proportionalAmount":proportional.to_string(),"minimum":floor_rounded.to_string(),
                    "amountAfterFloor":amount.to_string()}));
            }
        }
    }
    // T-12：所有 kind 的結果都以逐筆（單據、月、期）先取整再加總為準，這裡只再做一次「整數元」格式化，不再依 roundingPlaces 取小數。
    let mut reference_rent:Option<Decimal>=None;let mut ratio_percent:Option<Decimal>=None;let mut tier_percent_out:Option<Decimal>=None;
    let base=if !issues.is_empty() {None}
    else if p.kind=="annualTiered" {
        reference_rent=Some(line_total);
        let baseline=number(p.annual_amount.as_deref())?;
        let ratio=if baseline>Decimal::ZERO {line_total.checked_mul(Decimal::from(100)).and_then(|n|n.checked_div(baseline)).ok_or("計算金額超出範圍")?}else{Decimal::ZERO};
        ratio_percent=Some(ratio);
        match tier_percent(ratio) {
            Some(tier)=>{
                tier_percent_out=Some(tier);
                Some(baseline.checked_mul(tier).and_then(|n|n.checked_div(Decimal::from(100))).ok_or("計算金額超出範圍")?
                    .round_dp_with_strategy(0,RoundingStrategy::MidpointAwayFromZero))
            },
            None=>{issues.push("依契約應變更經營年租金，不自動計算".into());None},
        }
    } else {
        let raw=match p.kind.as_str() {
            "fixedArea"=>number(p.area.as_deref())?.checked_mul(number(p.unit_price.as_deref())?)
                .and_then(|n|n.checked_mul(Decimal::from(if p.unit_period=="month" {p.months}else{1}))).ok_or("計算金額超出範圍")?,
            "fixedAnnual"=>number(p.annual_amount.as_deref())?.checked_mul(Decimal::from(p.months)).and_then(|n|n.checked_div(Decimal::from(12))).ok_or("計算金額超出範圍")?,
            "kwhRate"=>line_total,
            _/*revenueShare*/=>if p.monthly_minimum.is_some() {floor_total}else{line_total},
        };
        Some(raw.round_dp_with_strategy(0,RoundingStrategy::MidpointAwayFromZero))
    };
    let base=if issues.is_empty() {base.map(|v|v.to_string())}else{None};
    Ok(json!({"schemaVersion":"rent-cycle-draft/v1","state":"draft","periodStart":from.to_string(),"periodEnd":to.to_string(),
        "plan":p,"receipts":rows,"sources":sources,"issues":issues,"baseRent":base,"lines":lines,
        "referenceRent":reference_rent.map(|v|v.to_string()),
        // 比值只取 2 位小數給畫面顯示；級距判斷(tier_percent)用的是取位前的原始比值，避免邊界值判錯。
        "ratioPercent":ratio_percent.map(|v|format!("{:.2}",v.round_dp_with_strategy(2,RoundingStrategy::MidpointAwayFromZero))),
        "tierPercent":tier_percent_out.map(|v|v.to_string()),
        "monthlyBreakdown":monthly,"feeKind":p.fee_kind.as_deref().unwrap_or("rent"),"chargeStatus":"calculating",
        "taxStatus":"pendingConfirmation","netPayable":null,"paymentAllowed":false,
        "invoiceDirection":"receiveLandlordInvoice","note":"此為租金基礎試算；稅費、扣繳與正式付款待範例確認，不產生付款或變更審核狀態。"}))
}

/// 一份合約（可能掛多位地主，不分攤金額，整份一列：決定 1）在合併申請裡的一列輸入。
/// `has_issue` 含「試算有 issue」與「base_rent 算不出來」兩種情況，由呼叫端（`eo-energy` 的
/// `station_application`）判定，這裡只負責組字串與加總，不碰資料庫。
#[derive(Debug, Clone)]
pub struct ApplicationEntryInput {
    pub contract_no: String,
    pub rent_calc_code: Option<String>,
    pub kind: String,
    /// 地主姓名全部列出（決定 3：多地主不拆列、不拆金額）。
    pub landowner_names: Vec<String>,
    /// 多地主時只取第一位的 tax_id 判斷類型／憑證文字（決定 3 的簡化：驗收例都只有一位地主）。
    pub landowner_tax_id: Option<String>,
    /// 存草稿時選的憑證安排（"before"／"later"／"unknown"），只有公司地主會用到。
    pub invoice_timing: Option<String>,
    pub base_rent: Option<String>,
    pub has_issue: bool,
}

/// tax_id 是 8 位數字＝公司（統編），有值但不是 8 位數字＝個人，空的＝類型待確認（決定 2）。
fn landowner_kind(tax_id: &Option<String>) -> &'static str {
    match tax_id.as_deref() {
        None => "unknown",
        Some(t) if t.trim().is_empty() => "unknown",
        Some(t) if t.len() == 8 && t.chars().all(|c| c.is_ascii_digit()) => "company",
        Some(_) => "individual",
    }
}

/// 租金計算方式代號對人看的中文名（跟 RentCycle.tsx 的下拉選單同一套文字）：申請裡的「算法」欄
/// 與費用說明不印程式代號，commander 代看畫面時糾正的產品缺陷。
fn kind_label(kind: &str) -> String {
    match kind {
        "fixedArea" => "面積 × 固定單價",
        "revenueShare" => "電費收入 × 比例",
        "fixedAnnual" => "整組年額（不分面積）",
        "annualTiered" => "年度基準＋級距調整",
        "kwhRate" => "度數 × 每度費率 × 比例",
        other => other,
    }
    .to_string()
}

/// T-8 欄位規則：公司依憑證安排三選一文字；個人固定「有，租金計算表」；類型待確認時單獨一句。
fn voucher_text(tax_id: &Option<String>, invoice_timing: &Option<String>) -> String {
    match landowner_kind(tax_id) {
        "unknown" => "地主類型待確認".into(),
        "company" => match invoice_timing.as_deref() {
            Some("before") => "有，發票".into(),
            Some("later") => "無，後補".into(),
            _ => "待確認".into(),
        },
        _ => "有，租金計算表".into(),
    }
}

/// 同案場同一期的租金付款申請（T-8：多地主合一張）。純函式，不讀資料庫、不寫入；
/// `company_code`／`company_name` 由呼叫端先讀案場 `easyflowDefaults`、沒有就傳「待補」。
/// 合計只加沒有 issue 的列；任何一列有 issue 或算不出 baseRent，整張 `total` 回 null、`state` 為
/// `"needsReview"`（決定 4 的延伸：試算不完整時申請本身也不能給出一個誤導的合計數字）。
pub fn application(
    station_name: &str,
    company_code: &str,
    company_name: &str,
    period_start: &str,
    period_end: &str,
    entries: &[ApplicationEntryInput],
) -> Value {
    let mut total = Decimal::ZERO;
    let mut needs_review = false;
    let lines: Vec<Value> = entries
        .iter()
        .map(|e| {
            if e.has_issue || e.base_rent.is_none() {
                needs_review = true;
            } else if let Some(amt) = &e.base_rent {
                if let Ok(d) = Decimal::from_str(amt) {
                    total += d;
                }
            }
            json!({
                "contractNo": e.contract_no, "rentCalcCode": e.rent_calc_code, "kind": kind_label(&e.kind),
                "landownerNames": e.landowner_names, "landownerType": landowner_kind(&e.landowner_tax_id),
                "voucher": voucher_text(&e.landowner_tax_id, &e.invoice_timing),
                "baseRent": e.base_rent, "hasIssue": e.has_issue,
            })
        })
        .collect();
    json!({
        "summary": format!("支付{}租金（{period_start}～{period_end}）",if entries.len()==1&&landowner_kind(&entries[0].landowner_tax_id)=="company" {entries[0].landowner_names.join("、")}else if station_name.ends_with("案場"){station_name.to_string()}else{format!("{station_name}案場")}),
        "periodStart": period_start, "periodEnd": period_end,
        "companyCode": company_code, "companyName": company_name,
        "feeDescription": entries.iter().map(|e| format!("{}：{}，繳費期限待補",e.contract_no,kind_label(&e.kind))).collect::<Vec<_>>().join("；"),
        "entries": lines,
        "total": if needs_review {Value::Null} else {json!(total.to_string())},
        "state": if needs_review {"needsReview"} else {"draft"},
        "paymentAllowed": false,
        "note": "預覽草稿・尚未送出、未付款",
    })
}
