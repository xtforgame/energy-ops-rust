//! 核心資料大表匯入正式資料（卡 20261126-2300）。`POST E/imports/core-table`：multipart
//! `file`＋`dryRun=1|0`＋`planHash`。撞到的以大表為準直接覆蓋；認不出是哪一筆的整列不寫、
//! 列進待補。照 `station_import.rs` 的寫法：永遠先把這一批寫進同一個交易，dryRun 或
//! `planHash` 跟重算的不符就整批 rollback（不進 repo、不留痕），兩者都通過才 commit。
//!
//! 「撞到怎麼認、覆蓋哪些欄」照 DESIGN.md 的表：案場比① 電號 key ② project_code ③ 唯一同名；
//! 地主比唯一同名；合約比 `contract_no` = 資料識別碼。三個案場比對條件（含電號已被別的案場
//! 佔用）或同名地主不只一個，整列不寫、列進待補（`認不出是哪一筆`）。
//!
//! 陷阱（commander 核規模時指出）：`update_contract` 整列 UPDATE，沒帶的欄位會被清空，所以
//! 覆蓋合約前先讀 `ContractRow` 整列，把 billCodes／rentPayCode／stage／note／contractType／
//! paymentFrequency 原樣帶回 body。案場的 `project_code` 不是 `update_station` 能直接設的值
//! （那支函式是從 gridDate/siteType/spv/serialNo 現算出來的），這裡改用只動 name、project_code
//! 兩欄的窄 SQL UPDATE，其餘欄位完全不碰。

use super::contracts;
use super::meters;
use super::allocations;
use super::{create_landowner, create_station, station_in_org, StationFull};
use crate::audit::{self, AuditInput};
use crate::common::{day, day_opt, err, err_d, Ctx, RequestMeta, Tx};
use crate::gates;
use actix_web::{web, HttpRequest, HttpResponse};
use calamine::{open_workbook_from_rs, Data, Reader, Xlsx};
use chrono::NaiveDate;
use eo_domain::business_key::normalize_customer_number;
use eo_platform::auth::CurrentUser;
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use rust_decimal::Decimal;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Cursor;

type Core = web::Data<CoreRef>;

const SHEET_NAME: &str = "核心資料大表";

fn cell_to_string(d: &Data) -> String {
    match d {
        Data::Empty => String::new(),
        Data::String(s) => s.trim().to_string(),
        Data::Float(f) => {
            if f.fract() == 0.0 {
                format!("{}", *f as i64)
            } else {
                f.to_string()
            }
        }
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        other => other.to_string().trim().to_string(),
    }
}

/// 一格裡用「/」「、」或換行分隔多個名字；去重保留第一次出現的順序。
fn split_names(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in s.split(['/', '、', '\n', '\r']) {
        let p = part.trim();
        if !p.is_empty() && !out.iter().any(|x| x == p) {
            out.push(p.to_string());
        }
    }
    out
}

/// 大表的日期一律是 `YYYY-MM-DDT00:00:00.000Z`，取前 10 碼。
fn parse_sheet_date(s: &str) -> Option<NaiveDate> {
    let d: String = s.chars().take(10).collect();
    NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()
}

/// 公式類型 → 租金算法代號（使用者 2026-11-26 拍板）。比例＋保底／原表未載公式／公式類型
/// 待確認這三種：代號空著，列進待補。
fn rent_calc_code_of(formula_type: &str) -> Option<&'static str> {
    match formula_type {
        "收入比例" => Some("R-B"),
        "固定租金" => Some("R-A"),
        "年度基準＋調整" => Some("R-C"),
        "面積計租" => Some("R-A"),
        "不需付租金（業主自建案）" => Some("R-0"),
        _ => None,
    }
}

struct CoreRow {
    row_no: usize,
    meter_raw: Option<String>,
    station_name: String,
    landowner_names: Vec<String>,
    lessor_names: Vec<String>,
    formula_type: Option<String>,
    signed_on: Option<NaiveDate>,
    lease_valid_to: Option<NaiveDate>,
    rule_valid_from: Option<NaiveDate>,
    rule_valid_to: Option<NaiveDate>,
    station_status: String,
    project_code: Option<String>,
    data_id: String,
    /// 「公司／SPV」欄（S5）：只對既有子公司，不新建。
    subsidiary_name: Option<String>,
}

/// 這一列「電號能用來比對案場」的正規化鍵：11 碼數字，而且這個案場不標「共用電號，待綁案場」
/// （共用電號的列不用電號比對，照 DESIGN 的規則）。
fn meter_match_key(row: &CoreRow) -> Option<String> {
    if row.station_status == "共用電號，待綁案場" {
        return None;
    }
    let raw = row.meter_raw.as_deref()?;
    let key = normalize_customer_number(Some(raw))?;
    (key.len() == 11 && key.chars().all(|c| c.is_ascii_digit())).then_some(key)
}

fn read_core_rows(bytes: &[u8]) -> ApiResult<Vec<CoreRow>> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut wb = open_workbook_from_rs::<Xlsx<_>, _>(cursor).map_err(|e| err_d(400, "bad_excel", json!({ "message": e.to_string() })))?;
    let range = wb
        .worksheet_range(SHEET_NAME)
        .map_err(|_| err_d(400, "bad_excel", json!({ "message": format!("找不到「{SHEET_NAME}」頁") })))?;
    // `range.start()` 是這個範圍左上角的實際列號（0-indexed）：前面幾列整列沒有任何儲存格時，
    // calamine 算出來的 range 會直接從有資料的那一列開始，`.rows()` 的 index 0 不等於試算表第 1 列。
    let start_row = range.start().map(|(r, _)| r).unwrap_or(0);
    let grid: Vec<Vec<String>> = range.rows().map(|row| row.iter().map(cell_to_string).collect()).collect();
    // 表頭不假設固定在第幾列（文件上寫第 6 列，但那是試算表絕對列號，不是 grid 的 index）：
    // 找 grid 裡第一列同時有「案場」「資料識別碼」兩欄的，當表頭。
    let header_idx = grid
        .iter()
        .position(|row| row.iter().any(|c| c.trim() == "案場") && row.iter().any(|c| c.trim() == "資料識別碼"))
        .ok_or_else(|| err_d(400, "bad_excel", json!({ "message": "找不到表頭（案場／資料識別碼）" })))?;
    let header = grid[header_idx].clone();
    let idx = |name: &str| header.iter().position(|h| h.trim() == name);
    let need = |name: &str| idx(name).ok_or_else(|| err_d(400, "missing_column", json!({ "column": name })));
    let c_meter = need("電號")?;
    let c_station = need("案場")?;
    let c_subsidiary = idx("公司／SPV");
    let c_landowner = need("地主（土地所有權人）")?;
    let c_lessor = need("出租方／租金對象")?;
    let c_formula = need("公式類型")?;
    let c_signed = need("租約簽署日")?;
    let c_lease_to = need("租約到期日")?;
    let c_rule_from = need("規則適用起日")?;
    let c_rule_to = need("規則適用迄日")?;
    let c_status = need("案場對應狀態")?;
    let c_project = need("專案編號")?;
    let c_data_id = need("資料識別碼")?;

    let mut out = Vec::new();
    for (i, row) in grid.iter().enumerate() {
        if i <= header_idx {
            continue;
        }
        let row_no = start_row as usize + i + 1; // 試算表的絕對列號（1-indexed）
        let get = |c: usize| row.get(c).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let Some(station_name) = get(c_station) else { continue }; // 整列全空就跳過
        let Some(data_id) = get(c_data_id) else { continue };
        out.push(CoreRow {
            row_no,
            meter_raw: get(c_meter),
            station_name,
            landowner_names: get(c_landowner).map(|s| split_names(&s)).unwrap_or_default(),
            lessor_names: get(c_lessor).map(|s| split_names(&s)).unwrap_or_default(),
            formula_type: get(c_formula),
            signed_on: get(c_signed).and_then(|s| parse_sheet_date(&s)),
            lease_valid_to: get(c_lease_to).and_then(|s| parse_sheet_date(&s)),
            rule_valid_from: get(c_rule_from).and_then(|s| parse_sheet_date(&s)),
            rule_valid_to: get(c_rule_to).and_then(|s| parse_sheet_date(&s)),
            station_status: get(c_status).unwrap_or_default(),
            project_code: get(c_project),
            data_id,
            subsidiary_name: c_subsidiary.and_then(|c| get(c)),
        });
    }
    Ok(out)
}

const ASSET_SHEET_NAME: &str = "0. 資產及義務項目清單";

struct AssetRow {
    region: Option<String>,
    capacity_raw: Option<String>,
}

enum AssetEntry {
    Unique(AssetRow),
    Duplicate,
}

/// S6：資產管理清單（可選的第二個檔）。表頭不寫死在第 3 列——跟 `read_core_rows` 一樣動態找，
/// 前次 calamine 列號偏移的教訓。回 `project_code -> AssetEntry`；`#N/A`／空的專案編號略過；
/// 同一個專案編號出現兩次就是 `Duplicate`（那個專案編號列進待補，不拿來補案場）。
fn read_asset_rows(bytes: &[u8]) -> ApiResult<HashMap<String, AssetEntry>> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut wb = open_workbook_from_rs::<Xlsx<_>, _>(cursor).map_err(|e| err_d(400, "bad_excel", json!({ "message": e.to_string() })))?;
    let range = wb
        .worksheet_range(ASSET_SHEET_NAME)
        .map_err(|_| err_d(400, "bad_excel", json!({ "message": format!("找不到「{ASSET_SHEET_NAME}」頁") })))?;
    let grid: Vec<Vec<String>> = range.rows().map(|row| row.iter().map(cell_to_string).collect()).collect();
    let header_idx = grid
        .iter()
        .position(|row| row.iter().any(|c| c.trim() == "專案編號") && row.iter().any(|c| c.trim() == "設置容量kWp"))
        .ok_or_else(|| err_d(400, "bad_excel", json!({ "message": "找不到資產清單表頭（專案編號／設置容量kWp）" })))?;
    let header = &grid[header_idx];
    let idx = |name: &str| header.iter().position(|h| h.trim() == name);
    let need = |name: &str| idx(name).ok_or_else(|| err_d(400, "missing_column", json!({ "column": name })));
    let c_project = need("專案編號")?;
    let c_region = idx("設置區域");
    let c_capacity = need("設置容量kWp")?;

    let mut grouped: HashMap<String, Vec<AssetRow>> = HashMap::new();
    for row in grid.iter().skip(header_idx + 1) {
        let get = |c: usize| row.get(c).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let Some(pc) = get(c_project) else { continue };
        if pc == "#N/A" {
            continue;
        }
        grouped.entry(pc).or_default().push(AssetRow { region: c_region.and_then(|c| get(c)), capacity_raw: get(c_capacity) });
    }
    Ok(grouped
        .into_iter()
        .map(|(pc, mut rows)| if rows.len() == 1 { (pc, AssetEntry::Unique(rows.pop().unwrap())) } else { (pc, AssetEntry::Duplicate) })
        .collect())
}

enum StationMatch {
    New,
    Found(i64),
    Ambiguous,
}

/// 依序比：①電號 key ②`project_code` ③唯一同名。電號／project_code 都有時要互相同意，
/// 不同意（含電號已經被別的案場佔用）就是歧義；都沒有才退到比名稱。
async fn match_station(tx: &mut Tx<'_>, org: i64, row: &CoreRow) -> ApiResult<StationMatch> {
    let meter_key = meter_match_key(row);
    let meter_owners: Vec<i64> = match &meter_key {
        Some(k) => sqlx::query_scalar(
            "SELECT DISTINCT station_id FROM tbl_energy_station_meter WHERE organization_id = $1 AND customer_number_key = $2 AND deleted_at IS NULL",
        )
        .bind(org)
        .bind(k)
        .fetch_all(&mut **tx)
        .await?,
        None => vec![],
    };
    let pc_owners: Vec<i64> = match &row.project_code {
        Some(pc) => sqlx::query_scalar("SELECT id FROM tbl_energy_station WHERE organization_id = $1 AND project_code = $2 AND deleted_at IS NULL")
            .bind(org)
            .bind(pc)
            .fetch_all(&mut **tx)
            .await?,
        None => vec![],
    };
    if meter_key.is_some() || row.project_code.is_some() {
        if meter_owners.len() > 1 || pc_owners.len() > 1 {
            return Ok(StationMatch::Ambiguous);
        }
        let a = meter_owners.first().copied();
        let b = pc_owners.first().copied();
        return Ok(match (a, b) {
            (Some(x), Some(y)) if x != y => StationMatch::Ambiguous,
            (Some(x), _) => StationMatch::Found(x),
            (None, Some(y)) => StationMatch::Found(y),
            (None, None) => StationMatch::New,
        });
    }
    let name_owners: Vec<i64> = sqlx::query_scalar("SELECT id FROM tbl_energy_station WHERE organization_id = $1 AND name = $2 AND deleted_at IS NULL")
        .bind(org)
        .bind(&row.station_name)
        .fetch_all(&mut **tx)
        .await?;
    Ok(match name_owners.len() {
        0 => StationMatch::New,
        1 => StationMatch::Found(name_owners[0]),
        _ => StationMatch::Ambiguous,
    })
}

enum NameMatch {
    New,
    One(i64),
    Ambiguous,
}

async fn match_landowner(tx: &mut Tx<'_>, org: i64, name: &str) -> ApiResult<NameMatch> {
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM tbl_energy_landowner WHERE organization_id = $1 AND btrim(name) = $2 AND deleted_at IS NULL",
    )
    .bind(org)
    .bind(name)
    .fetch_all(&mut **tx)
    .await?;
    Ok(match ids.len() {
        0 => NameMatch::New,
        1 => NameMatch::One(ids[0]),
        _ => NameMatch::Ambiguous,
    })
}

enum SpvMatch {
    None,
    One(i64),
    Ambiguous,
}

/// 「公司／SPV」欄（S5）：只對既有子公司，不新建（跟 station_import.rs:98-119 的
/// find_or_create_subsidiary 不同）。先比名字，比不到再比代碼（子公司簡稱），兩邊都要唯一。
async fn match_subsidiary(tx: &mut Tx<'_>, org: i64, name: &str) -> ApiResult<SpvMatch> {
    let by_name: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM tbl_energy_landowner WHERE organization_id = $1 AND party_type = 'subsidiary' AND btrim(name) = $2 AND deleted_at IS NULL",
    )
    .bind(org)
    .bind(name)
    .fetch_all(&mut **tx)
    .await?;
    if !by_name.is_empty() {
        return Ok(match by_name.len() {
            1 => SpvMatch::One(by_name[0]),
            _ => SpvMatch::Ambiguous,
        });
    }
    let by_code: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM tbl_energy_landowner WHERE organization_id = $1 AND party_type = 'subsidiary' AND code = $2 AND deleted_at IS NULL",
    )
    .bind(org)
    .bind(name)
    .fetch_all(&mut **tx)
    .await?;
    Ok(match by_code.len() {
        0 => SpvMatch::None,
        1 => SpvMatch::One(by_code[0]),
        _ => SpvMatch::Ambiguous,
    })
}

/// 租金試算設定（`rentCycleDraft.kind`）跟新代號是否還合——規則照 `rent_cycle.rs:52-58`。
fn rent_kind_ok(code: &str, kind: &str) -> bool {
    match code {
        "R-A" => kind == "fixedArea" || kind == "fixedAnnual",
        "R-B" => kind == "revenueShare",
        "R-C" => kind == "annualTiered",
        "R-D" => kind == "kwhRate",
        _ => false,
    }
}

#[derive(Default)]
struct Counts {
    created: i64,
    updated: i64,
    unchanged: i64,
    pending: i64,
}

impl Counts {
    fn json(&self) -> Value {
        json!({ "created": self.created, "updated": self.updated, "unchanged": self.unchanged, "pending": self.pending })
    }
}

#[derive(Default)]
struct Summary {
    stations: Counts,
    landowners: Counts,
    contracts: Counts,
}

/// 整批跑一次：永遠執行建立／覆蓋（照 `station_import.rs` 的 dryRun 回滾寫法，由呼叫端決定
/// commit 還是 rollback）。回 `(summary, rows, planHash)`。
async fn run_import(
    tx: &mut Tx<'_>,
    ctx: &Ctx,
    rows: &[CoreRow],
    asset_map: &Option<HashMap<String, AssetEntry>>,
) -> ApiResult<(Value, Vec<Value>, String)> {
    let org = ctx.org_id();
    let mut summary = Summary::default();
    let mut row_results = Vec::new();
    // 這次會碰到的既有資料的指紋：寫入前讀到的值（原值），不是寫完之後的值。
    let mut hash_snapshots: Vec<Value> = Vec::new();

    for row in rows {
        let mut pending: Vec<String> = Vec::new();
        if row.station_status == "共用電號，待綁案場" {
            pending.push("共用電號".into());
        }
        if row.station_status == "電號缺漏" {
            pending.push("電號缺漏".into());
        }

        // ── Phase A：只讀、不寫，先把「認不出是哪一筆」判完 ──
        let station_match = match_station(tx, org, row).await?;
        if matches!(station_match, StationMatch::Ambiguous) {
            pending.push("認不出是哪一筆".into());
            summary.stations.pending += 1;
            summary.landowners.pending += 1;
            summary.contracts.pending += 1;
            row_results.push(json!({
                "row": row.row_no, "dataId": row.data_id, "station": row.station_name,
                "skipped": true, "pending": pending,
            }));
            continue;
        }
        let mut all_names: Vec<String> = row.landowner_names.clone();
        for n in &row.lessor_names {
            if !all_names.iter().any(|x| x == n) {
                all_names.push(n.clone());
            }
        }
        let mut name_match: HashMap<String, NameMatch> = HashMap::new();
        let mut ambiguous_landowner = false;
        for name in &all_names {
            let m = match_landowner(tx, org, name).await?;
            if matches!(m, NameMatch::Ambiguous) {
                ambiguous_landowner = true;
            }
            name_match.insert(name.clone(), m);
        }
        if ambiguous_landowner {
            pending.push("認不出是哪一筆".into());
            summary.stations.pending += 1;
            summary.landowners.pending += 1;
            summary.contracts.pending += 1;
            row_results.push(json!({
                "row": row.row_no, "dataId": row.data_id, "station": row.station_name,
                "skipped": true, "pending": pending,
            }));
            continue;
        }

        // ── Phase B：確定不歧義，開始寫 ──
        // 公司／SPV（S5）：只對既有子公司，不新建；對不到或同名不只一個，不動那欄、列進待補。
        let spv_id: Option<i64> = match &row.subsidiary_name {
            Some(name) => match match_subsidiary(tx, org, name).await? {
                SpvMatch::One(id) => Some(id),
                SpvMatch::None | SpvMatch::Ambiguous => {
                    pending.push("子公司對不到".into());
                    None
                }
            },
            None => None,
        };
        // 區域／容量（S6）：第二個檔「資產管理清單」可選；沒選就是 None，region_new/capacity_new
        // 一律是 None，案場那段照舊，跟現在一模一樣。
        let (region_new, capacity_new): (Option<String>, Option<Decimal>) = match (asset_map, &row.project_code) {
            (Some(map), Some(pc)) => match map.get(pc) {
                Some(AssetEntry::Duplicate) => {
                    pending.push("資產清單專案編號重複".into());
                    (None, None)
                }
                Some(AssetEntry::Unique(a)) => {
                    let region = a.region.clone();
                    let capacity = match &a.capacity_raw {
                        None => None,
                        Some(s) => match s.parse::<Decimal>() {
                            Ok(d) => Some(d),
                            Err(_) => {
                                pending.push("容量不是數字".into());
                                None
                            }
                        },
                    };
                    (region, capacity)
                }
                None => (None, None),
            },
            _ => (None, None),
        };
        let (station_id, station_action, station_changes) = match station_match {
            StationMatch::New => {
                let mut body = json!({ "name": row.station_name });
                if let Some(sid) = spv_id {
                    body["spvPayeeId"] = json!(sid);
                }
                if let Some(r) = &region_new {
                    body["region"] = json!(r);
                }
                if let Some(c) = capacity_new {
                    body["capacityKw"] = json!(c.to_string());
                }
                let id = create_station(tx, ctx, &body).await?;
                if let Some(pc) = &row.project_code {
                    sqlx::query("UPDATE tbl_energy_station SET project_code = $2 WHERE id = $1 AND organization_id = $3")
                        .bind(id)
                        .bind(pc)
                        .bind(org)
                        .execute(&mut **tx)
                        .await?;
                }
                if let Some(raw) = &row.meter_raw {
                    if meter_match_key(row).is_some() {
                        sqlx::query(
                            "INSERT INTO tbl_energy_station_meter (organization_id, station_id, customer_number, usage) VALUES ($1, $2, $3, 'wholesale')",
                        )
                        .bind(org)
                        .bind(id)
                        .bind(raw)
                        .execute(&mut **tx)
                        .await?;
                    }
                }
                (id, "create", json!({}))
            }
            StationMatch::Found(id) => {
                let existing: StationFull = station_in_org(tx, &ctx.access, id).await?;
                hash_snapshots.push(json!({
                    "station": id, "name": existing.name, "projectCode": existing.project_code, "spvPayeeId": existing.spv_payee_id,
                    "region": existing.region, "capacityKw": existing.capacity_kw.map(|d| d.to_string()),
                }));
                let mut changes = Map::new();
                if existing.name != row.station_name {
                    changes.insert("name".into(), json!({ "from": existing.name, "to": row.station_name }));
                }
                if let Some(pc) = &row.project_code {
                    if existing.project_code.as_deref() != Some(pc.as_str()) {
                        changes.insert("projectCode".into(), json!({ "from": existing.project_code, "to": pc }));
                    }
                }
                if let Some(sid) = spv_id {
                    if existing.spv_payee_id != Some(sid) {
                        changes.insert("spvPayeeId".into(), json!({ "from": existing.spv_payee_id, "to": sid }));
                    }
                }
                if let Some(r) = &region_new {
                    if existing.region.as_deref() != Some(r.as_str()) {
                        changes.insert("region".into(), json!({ "from": existing.region, "to": r }));
                    }
                }
                if let Some(c) = capacity_new {
                    if existing.capacity_kw != Some(c) {
                        changes.insert("capacityKw".into(), json!({ "from": existing.capacity_kw.map(|d| d.to_string()), "to": c.to_string() }));
                    }
                }
                if !changes.is_empty() {
                    sqlx::query(
                        "UPDATE tbl_energy_station SET name = $2, project_code = COALESCE($3, project_code), spv_payee_id = COALESCE($4, spv_payee_id),
                           region = COALESCE($5, region), capacity_kw = COALESCE($6, capacity_kw)
                           WHERE id = $1 AND organization_id = $7 AND deleted_at IS NULL",
                    )
                        .bind(id)
                        .bind(&row.station_name)
                        .bind(&row.project_code)
                        .bind(spv_id)
                        .bind(&region_new)
                        .bind(capacity_new)
                        .bind(org)
                        .execute(&mut **tx)
                        .await?;
                    audit::record(
                        tx,
                        ctx,
                        AuditInput::new("station.core_table_updated", id, format!("核心大表匯入更新案場「{}」", row.station_name))
                            .label(row.station_name.clone())
                            .payload(json!({ "changes": Value::Object(changes.clone()) })),
                    )
                    .await?;
                }
                // 主電號：電號能用、跟既有的不同，才換（能到這裡表示 match_station 已經確認沒被別的案場佔用）。
                if let Some(raw) = &row.meter_raw {
                    if let Some(key) = meter_match_key(row) {
                        let current: Option<(i64, String, Option<String>, Option<Decimal>, Option<Decimal>, Option<NaiveDate>, Option<NaiveDate>, Option<String>)> =
                            sqlx::query_as(
                                "SELECT id, customer_number, taipower_contract_no, feed_in_rate, green_rate, valid_from, valid_to, note
                                   FROM tbl_energy_station_meter WHERE organization_id = $1 AND station_id = $2 AND usage = 'wholesale' AND deleted_at IS NULL
                                   ORDER BY id ASC LIMIT 1",
                            )
                            .bind(org)
                            .bind(id)
                            .fetch_optional(&mut **tx)
                            .await?;
                        match current {
                            None => {
                                sqlx::query(
                                    "INSERT INTO tbl_energy_station_meter (organization_id, station_id, customer_number, usage) VALUES ($1, $2, $3, 'wholesale')",
                                )
                                .bind(org)
                                .bind(id)
                                .bind(raw)
                                .execute(&mut **tx)
                                .await?;
                                audit::record(
                                    tx, ctx,
                                    AuditInput::new("meter.core_table_updated", id, format!("核心大表匯入新增主電號「{raw}」"))
                                        .payload(json!({ "changes": { "customerNumber": { "from": null, "to": raw } } })),
                                ).await.ok();
                            }
                            Some((meter_id, cur_num, tpc, fir, gr, vf, vt, note)) => {
                                if normalize_customer_number(Some(&cur_num)).as_deref() != Some(key.as_str()) {
                                    hash_snapshots.push(json!({ "meter": meter_id, "customerNumber": cur_num }));
                                    let body = json!({
                                        "customerNumber": raw, "taipowerContractNo": tpc,
                                        "feedInRate": fir.map(|d| d.to_string()), "greenRate": gr.map(|d| d.to_string()),
                                        "validFrom": day_opt(vf), "validTo": day_opt(vt), "note": note,
                                    });
                                    meters::update_meter(tx, ctx, meter_id, &body).await?;
                                    audit::record(
                                        tx, ctx,
                                        AuditInput::new("meter.core_table_updated", meter_id, "核心大表匯入更新主電號")
                                            .payload(json!({ "changes": { "customerNumber": { "from": cur_num, "to": raw } } })),
                                    ).await.ok();
                                }
                            }
                        }
                    }
                }
                let action = if changes.is_empty() { "unchanged" } else { "update" };
                (id, action, Value::Object(changes))
            }
            StationMatch::Ambiguous => unreachable!("已經在 Phase A 擋掉"),
        };
        match station_action {
            "create" => summary.stations.created += 1,
            "update" => summary.stations.updated += 1,
            _ => summary.stations.unchanged += 1,
        }

        // ── 地主：地主欄＋出租方欄的名字都建成地主 ──
        let mut name_to_id: HashMap<String, i64> = HashMap::new();
        let mut landowners_out = Vec::new();
        for name in &all_names {
            let id = match name_match.remove(name).unwrap() {
                NameMatch::One(id) => {
                    landowners_out.push(json!({ "name": name, "action": "unchanged" }));
                    summary.landowners.unchanged += 1;
                    id
                }
                NameMatch::New => {
                    let id = create_landowner(tx, ctx, &json!({ "name": name })).await?;
                    audit::record(
                        tx, ctx,
                        AuditInput::new("landowner.core_table_created", id, format!("核心大表匯入新增地主「{name}」"))
                            .label(name.clone())
                            .payload(json!({ "changes": { "name": { "from": null, "to": name } } })),
                    ).await.ok();
                    landowners_out.push(json!({ "name": name, "action": "create" }));
                    summary.landowners.created += 1;
                    id
                }
                NameMatch::Ambiguous => unreachable!("已經在 Phase A 擋掉"),
            };
            name_to_id.insert(name.clone(), id);
        }

        // ── 分配：只有地主欄記在案場的地主關係上；單一地主 ratio=100，多位原本有就保留、沒有就空著。 ──
        let single = row.landowner_names.len() == 1;
        for name in &row.landowner_names {
            let Some(&lid) = name_to_id.get(name) else { continue };
            let existing: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM mn_energy_landowner_station WHERE organization_id = $1 AND station_id = $2 AND landowner_id = $3 AND deleted_at IS NULL LIMIT 1",
            )
            .bind(org)
            .bind(station_id)
            .bind(lid)
            .fetch_optional(&mut **tx)
            .await?;
            if existing.is_none() {
                let ratio = if single { Some(json!(100)) } else { None };
                if ratio.is_none() && !pending.iter().any(|p| p == "多位地主沒有比例") {
                    pending.push("多位地主沒有比例".into());
                }
                allocations::create_allocation(tx, ctx, station_id, &json!({ "landownerId": lid, "method": "ratio", "ratio": ratio })).await?;
            }
        }

        // ── 合約：資料識別碼＝contract_no；對象是出租方（lessor）。沒起日或沒出租方就不建合約。 ──
        let rent_calc_code = row.formula_type.as_deref().and_then(rent_calc_code_of);
        if rent_calc_code.is_none() && row.formula_type.is_some() {
            pending.push("代號空著".into());
        }
        let valid_from = row.rule_valid_from.or(row.signed_on);
        let valid_to = row.rule_valid_to.or(row.lease_valid_to);
        let lessor_ids: Vec<i64> = row.lessor_names.iter().filter_map(|n| name_to_id.get(n).copied()).collect();
        let mut contract_action = "unchanged";
        let mut contract_changes = json!({});
        if row.lessor_names.is_empty() {
            pending.push("沒有出租方".into());
        } else if valid_from.is_none() {
            pending.push("沒有起日".into());
        } else {
            let valid_from = valid_from.unwrap();
            let existing_id: Option<i64> =
                sqlx::query_scalar("SELECT id FROM tbl_energy_contract WHERE organization_id = $1 AND contract_no = $2 AND deleted_at IS NULL")
                    .bind(org)
                    .bind(&row.data_id)
                    .fetch_optional(&mut **tx)
                    .await?;
            match existing_id {
                None => {
                    let body = json!({
                        "contractNo": row.data_id, "landownerIds": lessor_ids, "stationIds": [station_id],
                        "validFrom": day(valid_from), "validTo": day_opt(valid_to), "signedOn": day_opt(row.signed_on),
                        "rentCalcCode": rent_calc_code,
                    });
                    let contract_id = contracts::create_contract(tx, ctx, &body).await?;
                    crate::bindings::bind_by_contract_codes(tx, ctx, contract_id).await.ok();
                    contract_action = "create";
                    contract_changes = json!({ "contractNo": row.data_id });
                    summary.contracts.created += 1;
                }
                Some(contract_id) => {
                    let existing = contracts::contract_in_org(tx, org, contract_id).await?;
                    hash_snapshots.push(json!({
                        "contract": contract_id, "validFrom": day(existing.valid_from), "validTo": day_opt(existing.valid_to),
                        "signedOn": day_opt(existing.signed_on), "rentCalcCode": existing.rent_calc_code,
                    }));
                    let mut changes = Map::new();
                    if existing.valid_from != valid_from {
                        changes.insert("validFrom".into(), json!({ "from": day(existing.valid_from), "to": day(valid_from) }));
                    }
                    if existing.valid_to != valid_to {
                        changes.insert("validTo".into(), json!({ "from": day_opt(existing.valid_to), "to": day_opt(valid_to) }));
                    }
                    if existing.signed_on != row.signed_on {
                        changes.insert("signedOn".into(), json!({ "from": day_opt(existing.signed_on), "to": day_opt(row.signed_on) }));
                    }
                    let final_code = rent_calc_code.map(String::from).or_else(|| existing.rent_calc_code.clone());
                    if existing.rent_calc_code != final_code {
                        changes.insert("rentCalcCode".into(), json!({ "from": existing.rent_calc_code, "to": final_code }));
                    }
                    if let Some(code) = &final_code {
                        if let Some(new_code) = rent_calc_code {
                            if existing.rent_calc_code.as_deref() != Some(new_code) {
                                let kind: Option<String> = sqlx::query_scalar("SELECT data->'rentCycleDraft'->>'kind' FROM tbl_energy_contract WHERE id = $1")
                                    .bind(contract_id)
                                    .fetch_optional(&mut **tx)
                                    .await?
                                    .flatten();
                                if let Some(kind) = kind {
                                    if !rent_kind_ok(code, &kind) {
                                        pending.push("租金試算設定要重設".into());
                                    }
                                }
                            }
                        }
                    }
                    if !changes.is_empty() || existing.landowner_id != lessor_ids.first().copied() {
                        let body = json!({
                            "contractNo": existing.contract_no, "landownerIds": lessor_ids, "stationIds": [station_id],
                            "validFrom": day(valid_from), "validTo": day_opt(valid_to), "signedOn": day_opt(row.signed_on),
                            "rentCalcCode": final_code,
                            "signMethod": existing.sign_method, "stage": existing.stage, "installmentTotal": existing.installment_total,
                            "note": existing.note, "contractType": existing.contract_type, "renewalRemindOn": day_opt(existing.renewal_remind_on),
                            "termsVersion": existing.terms_version, "paymentFrequency": existing.payment_frequency,
                            "paymentTerms": existing.payment_terms, "billCodes": existing.bill_codes, "rentPayCode": existing.rent_pay_code,
                        });
                        contracts::update_contract(tx, ctx, contract_id, &body).await?;
                        crate::bindings::bind_by_contract_codes(tx, ctx, contract_id).await.ok();
                    }
                    contract_action = if changes.is_empty() { "unchanged" } else { "update" };
                    contract_changes = Value::Object(changes);
                    match contract_action {
                        "update" => summary.contracts.updated += 1,
                        _ => summary.contracts.unchanged += 1,
                    }
                }
            }
        }
        if matches!(contract_action, "unchanged") && (row.lessor_names.is_empty() || valid_from.is_none()) {
            summary.contracts.pending += 1;
        }

        row_results.push(json!({
            "row": row.row_no, "dataId": row.data_id, "station": row.station_name, "skipped": false,
            "pending": pending,
            "stationAction": station_action, "stationChanges": station_changes,
            "landowners": landowners_out,
            "contractAction": contract_action, "contractChanges": contract_changes,
        }));
    }

    let file_marker = json!({ "snapshots": hash_snapshots });
    let plan_hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&file_marker).unwrap()));
    let mut summary_json = json!({
        "stations": summary.stations.json(), "landowners": summary.landowners.json(), "contracts": summary.contracts.json(),
    });
    // 沒選第二個檔（assetFile）時完全不加這個鍵，結果跟沒有 S6 之前一模一樣。
    if let Some(map) = asset_map {
        let core_codes: std::collections::HashSet<&str> = rows.iter().filter_map(|r| r.project_code.as_deref()).collect();
        let unique_codes: Vec<&String> = map.iter().filter_map(|(pc, e)| matches!(e, AssetEntry::Unique(_)).then_some(pc)).collect();
        let matched = unique_codes.iter().filter(|pc| core_codes.contains(pc.as_str())).count();
        summary_json["assetFile"] = json!({
            "rows": map.len(),
            "matchedProjectCodes": matched,
            "unmatchedProjectCodes": map.len() - matched,
        });
    }
    Ok((summary_json, row_results, plan_hash))
}

pub async fn import_handler(
    core: Core,
    user: CurrentUser,
    meta: RequestMeta,
    org: web::Path<i64>,
    req: HttpRequest,
    payload: web::Payload,
) -> ApiResult<HttpResponse> {
    let org = org.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::MASTER_MANAGE).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mp = actix_multipart::Multipart::new(req.headers(), payload);
    let mut form = eo_platform::upload::read_multipart(mp, 10 * 1024 * 1024).await?;
    let file = form.take_file("file").ok_or_else(|| err(400, "file_required"))?;
    let dry_run = form.field("dryRun").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false);
    let client_plan_hash = form.field("planHash").map(|v| v.to_string());

    let rows = read_core_rows(&file.bytes)?;
    // 資產管理清單（S6）：可選的第二個檔；沒帶就是 None，案場那段完全不受影響。
    let asset_map = match form.take_file("assetFile") {
        Some(asset_file) => Some(read_asset_rows(&asset_file.bytes)?),
        None => None,
    };
    let mut tx = core.pool.begin().await?;
    let (summary, rows_out, plan_hash) = run_import(&mut tx, &ctx, &rows, &asset_map).await?;

    let stale = client_plan_hash.as_deref().is_some_and(|h| h != plan_hash);
    if stale {
        tx.rollback().await?;
        return Err(err_d(409, "plan_stale", json!({ "message": "資料在預覽之後變了，請重新預覽" })));
    }
    if dry_run {
        tx.rollback().await?;
    } else {
        tx.commit().await?;
    }
    Ok(HttpResponse::Ok().json(json!({ "dryRun": dry_run, "planHash": plan_hash, "summary": summary, "rows": rows_out })))
}
