//! 發電案場清單 Excel 匯入＋錯誤報告（卡 20261118-0810-a2 S3）。
//! `POST E/imports/stations`：multipart `file`＋`dryRun=1|0`。用表頭名稱對欄（順序不拘），
//! 缺必要欄整批退（`missing_column`，row 1）。**有任何錯整批不寫**：dryRun 與正式匯入都在
//! 同一個交易裡跑，dryRun 或有錯就 rollback（不進 repo、不留痕）。
//! 真實「發電案場清單」只准讀表頭核對 15 個欄名；要試匯只准匯進實作者／審查者自己的庫。

use crate::common::{err, err_d, Ctx, RequestMeta, Tx};
use crate::gates;
use actix_web::{web, HttpRequest, HttpResponse};
use calamine::{open_workbook_from_rs, Data, Reader, Xlsx};
use eo_platform::auth::CurrentUser;
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::Cursor;
use std::str::FromStr;

type Core = web::Data<CoreRef>;

/// 這張清單固定的 15 個欄名（照「發電案場清單」表頭；順序不拘）。
pub const REQUIRED_COLUMNS: &[&str] = &[
    "電廠編號", "電廠名", "電號", "案場區域", "電費單上的服務部門", "容量kW", "持有子公司", "子公司簡稱",
    "發票開立單的收款對象", "統一編號", "財會對應窗口", "發票寄送方式", "email 1", "email 2", "發票地址",
];

#[derive(Default)]
struct Counts {
    stations: i64,
    meters: i64,
    payees: i64,
    stations_updated: i64,
    skipped: i64,
}

fn msg(code: &str, column: &str) -> String {
    match code {
        "missing_column" => format!("缺少欄位「{column}」"),
        "required" => format!("「{column}」是必填欄位，這一列是空的"),
        "bad_number" => format!("「{column}」不是數字"),
        "duplicate_meter" => "這份檔案裡同一個電號出現兩次".into(),
        "meter_taken" => "這個電號已經掛在本組織別的案場上".into(),
        _ => code.into(),
    }
}

fn row_error(row: usize, column: &str, code: &str) -> Value {
    json!({ "row": row, "column": column, "code": code, "message": msg(code, column) })
}

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

fn read_rows(bytes: &[u8]) -> ApiResult<Vec<Vec<String>>> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut wb = open_workbook_from_rs::<Xlsx<_>, _>(cursor).map_err(|e| err_d(400, "bad_excel", json!({ "message": e.to_string() })))?;
    // The supplied multi-sheet workbook contains presentation forms before its master-data sheet.
    let names = wb.sheet_names();
    let sheet_name = names.iter().find(|n| n.as_str() == "自動化所需欄位")
        .or_else(|| names.first()).cloned().ok_or_else(|| err(400, "bad_excel"))?;
    let range = wb
        .worksheet_range(&sheet_name)
        .map_err(|e| err_d(400, "bad_excel", json!({ "message": e.to_string() })))?;
    let mut rows: Vec<Vec<String>> = range.rows().map(|row| row.iter().map(cell_to_string).collect()).collect();
    if sheet_name == "自動化所需欄位" {
        // These two fields do not exist in that supplied format. Keep unknown values empty,
        // rather than inventing a Taipower district from the region name.
        for column in ["電費單上的服務部門", "發票開立單的收款對象"] {
            if let Some(header) = rows.first() {
                if !header.iter().any(|v| v.trim() == column) {
                    let index = header.len();
                    for (i, row) in rows.iter_mut().enumerate() {
                        row.resize(index, String::new());
                        row.push(if i == 0 { column.into() } else { String::new() });
                    }
                }
            }
        }
    }
    Ok(rows)
}

async fn find_or_create_subsidiary(tx: &mut Tx<'_>, org: i64, name: &str, code: Option<&str>, counts: &mut Counts) -> ApiResult<i64> {
    if let Some(id) = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM tbl_energy_landowner WHERE organization_id = $1 AND name = $2 AND party_type = 'subsidiary' AND deleted_at IS NULL LIMIT 1",
    )
    .bind(org)
    .bind(name)
    .fetch_optional(&mut **tx)
    .await?
    {
        return Ok(id);
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO tbl_energy_landowner (organization_id, name, party_type, code, status) VALUES ($1, $2, 'subsidiary', $3, 'active') RETURNING id",
    )
    .bind(org)
    .bind(name)
    .bind(code)
    .fetch_one(&mut **tx)
    .await?;
    counts.payees += 1;
    Ok(id)
}

async fn find_or_create_taipower(tx: &mut Tx<'_>, org: i64, name: &str, tax_id: Option<&str>, counts: &mut Counts) -> ApiResult<i64> {
    if let Some(tid) = tax_id {
        if let Some(id) = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM tbl_energy_landowner WHERE organization_id = $1 AND tax_id = $2 AND party_type = 'taipower_district' AND deleted_at IS NULL LIMIT 1",
        )
        .bind(org)
        .bind(tid)
        .fetch_optional(&mut **tx)
        .await?
        {
            return Ok(id);
        }
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO tbl_energy_landowner (organization_id, name, party_type, tax_id, status) VALUES ($1, $2, 'taipower_district', $3, 'active') RETURNING id",
    )
    .bind(org)
    .bind(name)
    .bind(tax_id)
    .fetch_one(&mut **tx)
    .await?;
    counts.payees += 1;
    Ok(id)
}

async fn find_station(tx: &mut Tx<'_>, org: i64, plant_no: Option<&str>, name: &str) -> ApiResult<Option<i64>> {
    if let Some(pn) = plant_no {
        if let Some(id) = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM tbl_energy_station WHERE organization_id = $1 AND data->>'plantNo' = $2 AND deleted_at IS NULL LIMIT 1",
        )
        .bind(org)
        .bind(pn)
        .fetch_optional(&mut **tx)
        .await?
        {
            return Ok(Some(id));
        }
    }
    Ok(sqlx::query_scalar(
        "SELECT id FROM tbl_energy_station WHERE organization_id = $1 AND name = $2 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(org)
    .bind(name)
    .fetch_optional(&mut **tx)
    .await?)
}

async fn import_rows(tx: &mut Tx<'_>, ctx: &Ctx, rows: Vec<Vec<String>>) -> ApiResult<(Counts, Vec<Value>)> {
    let mut counts = Counts::default();
    let mut errors = Vec::new();
    let Some(header_row) = rows.first() else {
        return Err(err(400, "bad_excel"));
    };
    let header: Vec<String> = header_row.iter().map(|s| s.trim().to_string()).collect();
    let missing: Vec<&str> = REQUIRED_COLUMNS.iter().filter(|c| !header.iter().any(|h| h == *c)).copied().collect();
    if !missing.is_empty() {
        for col in missing {
            errors.push(row_error(1, col, "missing_column"));
        }
        return Ok((counts, errors));
    }
    let idx = |col: &str| header.iter().position(|h| h == col).unwrap();
    let org = ctx.org_id();
    let mut seen_meters: HashSet<String> = HashSet::new();
    // 這次匯入裡新建的案場 id：同一份檔案第二列才接到同一個案場（例如第二個電號），
    // 不算「更新既有案場」——它是這次匯入才生出來的，不是「已存在的案場被更新」。
    let mut created_this_run: HashSet<i64> = HashSet::new();
    // 「更新」算的是不重複的案場數：一個既有案場同一次匯入被多列碰到（例如兩個電號各一列），
    // 只算一次，不是碰幾次就加幾次。
    let mut updated_this_run: HashSet<i64> = HashSet::new();

    for (i, row) in rows.iter().enumerate().skip(1) {
        let row_no = i + 1; // 1-indexed，含表頭（表頭＝第 1 列）。
        let get = |col: &str| -> Option<String> { row.get(idx(col)).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) };
        let plant_no = get("電廠編號");
        let name = get("電廠名");
        let meter_no = get("電號");
        let region = get("案場區域");
        let service_district = get("電費單上的服務部門");
        let capacity_raw = get("容量kW");
        let subsidiary_name = get("持有子公司");
        let subsidiary_code = get("子公司簡稱");
        let taipower_name = get("發票開立單的收款對象");
        let taipower_tax_id = get("統一編號").map(|v| if v.chars().all(|c| c.is_ascii_digit()) && v.len() < 8 { format!("{v:0>8}") } else { v });
        let invoice_contact = get("財會對應窗口");
        let invoice_delivery = get("發票寄送方式");
        let email1 = get("email 1");
        let email2 = get("email 2");
        let invoice_address = get("發票地址");

        // 整列全空：容許尾端空列，不當錯誤也不計入略過。
        if plant_no.is_none() && name.is_none() && meter_no.is_none() {
            continue;
        }

        let mut row_ok = true;
        if name.is_none() {
            errors.push(row_error(row_no, "電廠名", "required"));
            row_ok = false;
        }
        if meter_no.is_none() {
            errors.push(row_error(row_no, "電號", "required"));
            row_ok = false;
        }
        if subsidiary_name.is_none() {
            errors.push(row_error(row_no, "持有子公司", "required"));
            row_ok = false;
        }
        let capacity: Option<Decimal> = match &capacity_raw {
            None => None,
            Some(s) => match Decimal::from_str(s) {
                Ok(d) => Some(d),
                Err(_) => {
                    errors.push(row_error(row_no, "容量kW", "bad_number"));
                    row_ok = false;
                    None
                }
            },
        };
        if let Some(m) = &meter_no {
            if !seen_meters.insert(m.clone()) {
                errors.push(row_error(row_no, "電號", "duplicate_meter"));
                row_ok = false;
            }
        }
        if !row_ok {
            counts.skipped += 1;
            continue;
        }
        let name = name.unwrap();
        let meter_no = meter_no.unwrap();
        let subsidiary_name = subsidiary_name.unwrap();

        let spv_id = find_or_create_subsidiary(tx, org, &subsidiary_name, subsidiary_code.as_deref(), &mut counts).await?;
        if let Some(tp_name) = &taipower_name {
            find_or_create_taipower(tx, org, tp_name, taipower_tax_id.as_deref(), &mut counts).await?;
        }

        let existing_station = find_station(tx, org, plant_no.as_deref(), &name).await?;
        let meter_station: Option<i64> = sqlx::query_scalar(
            "SELECT station_id FROM tbl_energy_station_meter WHERE organization_id = $1 AND customer_number = $2 AND deleted_at IS NULL",
        )
        .bind(org)
        .bind(&meter_no)
        .fetch_optional(&mut **tx)
        .await?;
        let invoice = json!({
            "contact": invoice_contact, "delivery": invoice_delivery, "email1": email1, "email2": email2, "address": invoice_address,
            "counterpartyName": taipower_name, "counterpartyTaxId": taipower_tax_id,
        });

        if let Some(st) = existing_station {
            if let Some(ms) = meter_station {
                if ms != st {
                    errors.push(row_error(row_no, "電號", "meter_taken"));
                    counts.skipped += 1;
                    continue;
                }
            }
            sqlx::query(
                "UPDATE tbl_energy_station SET region = COALESCE($2, region), service_district = COALESCE($3, service_district),
                   capacity_kw = COALESCE($4, capacity_kw), spv_payee_id = COALESCE($5, spv_payee_id),
                   data = jsonb_set(COALESCE(data, '{}'::jsonb), '{invoice}', $6::jsonb)
                 WHERE id = $1 AND organization_id = $7",
            )
            .bind(st)
            .bind(&region)
            .bind(&service_district)
            .bind(capacity)
            .bind(spv_id)
            .bind(&invoice)
            .bind(org)
            .execute(&mut **tx)
            .await?;
            if !created_this_run.contains(&st) && updated_this_run.insert(st) {
                counts.stations_updated += 1;
            }
            if meter_station.is_none() {
                sqlx::query(
                    "INSERT INTO tbl_energy_station_meter (organization_id, station_id, customer_number, usage) VALUES ($1, $2, $3, 'wholesale')",
                )
                .bind(org)
                .bind(st)
                .bind(&meter_no)
                .execute(&mut **tx)
                .await?;
                counts.meters += 1;
            } else {
                counts.skipped += 1;
            }
        } else {
            if meter_station.is_some() {
                errors.push(row_error(row_no, "電號", "meter_taken"));
                counts.skipped += 1;
                continue;
            }
            let id: i64 = sqlx::query_scalar(
                "INSERT INTO tbl_energy_station
                   (organization_id, landowner_id, customer_number, name, status, region, service_district, capacity_kw,
                    spv_payee_id, enabled, data)
                 VALUES ($1, $2, $3, $4, 'operating', $5, $6, $7, $8, true, jsonb_build_object('invoice', $9::jsonb, 'plantNo', $10))
                 RETURNING id",
            )
            .bind(org)
            .bind(spv_id)
            .bind(&meter_no)
            .bind(&name)
            .bind(&region)
            .bind(&service_district)
            .bind(capacity)
            .bind(spv_id)
            .bind(&invoice)
            .bind(&plant_no)
            .fetch_one(&mut **tx)
            .await?;
            sqlx::query(
                "INSERT INTO tbl_energy_station_meter (organization_id, station_id, customer_number, usage) VALUES ($1, $2, $3, 'wholesale')",
            )
            .bind(org)
            .bind(id)
            .bind(&meter_no)
            .execute(&mut **tx)
            .await?;
            sqlx::query(
                "INSERT INTO mn_energy_landowner_station (organization_id, landowner_id, station_id, method, ratio) VALUES ($1, $2, $3, 'ratio', 100)",
            )
            .bind(org)
            .bind(spv_id)
            .bind(id)
            .execute(&mut **tx)
            .await?;
            created_this_run.insert(id);
            counts.stations += 1;
            counts.meters += 1;
        }
    }
    Ok((counts, errors))
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

    let rows = read_rows(&file.bytes)?;
    let mut tx = core.pool.begin().await?;
    let (counts, errors) = import_rows(&mut tx, &ctx, rows).await?;
    let has_errors = !errors.is_empty();
    if dry_run || has_errors {
        tx.rollback().await?;
    } else {
        tx.commit().await?;
    }
    Ok(HttpResponse::Ok().json(json!({
        "dryRun": dry_run,
        "created": { "stations": counts.stations, "meters": counts.meters, "payees": counts.payees },
        "updated": { "stations": counts.stations_updated },
        "skipped": counts.skipped,
        "errors": errors,
    })))
}
