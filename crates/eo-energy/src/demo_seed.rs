//! 示範資料：一個示範組織、七個角色帳號，與一組看得懂的主檔／流程設定，讓每一頁都有東西可看。
//!
//! 只放主檔（地主／案場／契約／期數／行事曆／部門／流程定義／計算策略／綁定），
//! 外加一筆跑完的保證金請款；真實帳單一張都不放（要看單據就從收件台上傳）。
//!
//! `EO_DEMO_SAMPLE_BILLS=1` 時另外灌四份範本電費單（`run_sample_bills`），給 e2e crawler 一組固定的起點。
//!
//! 冪等：每一筆都以穩定識別碼判斷是否已存在（組織 uniqueName、email、部門 key、電號、契約編號…），
//! 已存在就略過，所以重跑不會累積、也不會改動別人後來手動調過的資料。

use crate::attachments::service::{self as attachments, AttachmentStore, Owner, UploadInput};
use crate::common::{Ctx, RequestMeta, Tx};
use crate::workflow::engine::tasks::{claim, perform_action, ActionInput};
use crate::workflow::{install_builtin_definitions, start_execution, StartExecution};
use chrono::NaiveDate;
use eo_domain::business_key::normalize_customer_number;
use eo_domain::rbac::RbacScope;
use eo_domain::schemas::DepartmentMemberRole;
use eo_domain::workflow_defs::{self as defs, DEPT_ACCOUNTING, DEPT_BILL_REVIEW, DEPT_FINANCE, DEPT_SALES};
use eo_platform::auth::CurrentUser;
use eo_platform::core::CoreRef;
use eo_platform::error::{ApiError, ApiResult};
use eo_platform::rbac::grant::{self, GrantInput};
use eo_platform::tenancy;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// 示範組織的 uniqueName 與顯示名。
pub const DEMO_ORG_UNIQUE_NAME: &str = "demo-energy";
pub const DEMO_ORG_NAME: &str = "雲豹能源";
/// 所有示範帳號共用的密碼（只給展示環境用）。
pub const DEMO_PASSWORD: &str = "demo12345";

/// 換版那一天：流程與計算策略的效期軸都在這天切成兩段，畫面才看得到「換版」。
const SWITCH_DAY: &str = "2026-01-01";
const OPEN_FROM: &str = defs::DEFAULT_BINDING_VALID_FROM;

/// 角色帳號：(email 前綴, 顯示名, 能源角色)。單一帳號走不完整條鏈（防自核會把送出的人排除在下一關），所以要多帳號。
const ROLE_USERS: [(&str, &str, &str); 7] = [
    ("admin", "系統管理員", "energy:admin"),
    ("operator", "王承辦", "energy:operator"),
    ("manager", "李主管", "energy:operator"),
    ("finance", "張財務", "energy:finance"),
    ("accounting", "陳會計", "energy:finance"),
    // 金額 ≥ 加簽門檻的單會走到高層加簽，需要這個人才走得完。
    ("cxo", "吳執行長", "energy:cxo"),
    // 唯讀角色：驗「看不到動作按鈕」那條路徑。
    ("viewer", "趙唯讀", "energy:viewer"),
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DemoAccount {
    pub email: String,
    pub name: String,
    pub role: String,
    pub user_id: i64,
}

/// 這一次真的新建了幾筆（已存在而略過的不算）。
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DemoSeedCounts {
    pub organizations: usize,
    pub users: usize,
    pub grants: usize,
    pub landowners: usize,
    pub stations: usize,
    pub contracts: usize,
    pub contract_stations: usize,
    pub installments: usize,
    pub calendar_events: usize,
    pub departments: usize,
    pub department_members: usize,
    pub workflow_definitions: usize,
    pub rating_strategies: usize,
    pub workflow_bindings: usize,
    pub rating_bindings: usize,
    pub executions: usize,
    // ── 卡 A 擴欄（20261118-0427）──
    pub meters: usize,
    pub allocations: usize,
}

impl DemoSeedCounts {
    pub fn total(&self) -> usize {
        self.organizations
            + self.users
            + self.grants
            + self.landowners
            + self.stations
            + self.contracts
            + self.contract_stations
            + self.installments
            + self.calendar_events
            + self.departments
            + self.department_members
            + self.workflow_definitions
            + self.rating_strategies
            + self.workflow_bindings
            + self.rating_bindings
            + self.executions
            + self.meters
            + self.allocations
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DemoSeedReport {
    pub org_id: i64,
    pub org_unique_name: String,
    pub accounts: Vec<DemoAccount>,
    pub created: DemoSeedCounts,
    /// 保證金請款那一筆執行的 id（新建或既有）。
    pub deposit_execution_id: Option<i64>,
}

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").expect("示範資料的日期寫死，必定合法")
}

fn email_of(prefix: &str) -> String {
    format!("{prefix}@demo.energy")
}

/// 以某位使用者在示範組織的身分操作（成員資格必須已提交）。
async fn user_ctx(core: &CoreRef, user_id: i64, org_id: i64) -> ApiResult<Ctx> {
    let mut tx = core.pool.begin().await?;
    let u = tenancy::user_by_id(&mut tx, user_id).await?;
    tx.commit().await?;
    let cu = CurrentUser { id: u.id, name: u.name, email: u.email, privilege: u.privilege, session_id: 0 };
    let access = core.rbac.authorize_member(&cu, org_id).await?;
    Ok(Ctx::new(core, access, &RequestMeta { ip: None, trace_id: Some("demo-seed".into()) }))
}

/// 跑一次示範資料。重跑安全：第二次不會新建任何東西。
pub async fn run(core: &CoreRef) -> ApiResult<DemoSeedReport> {
    let mut created = DemoSeedCounts::default();

    // ── 帳號、組織、成員、角色 ──
    let mut tx = core.pool.begin().await?;
    let mut ids: HashMap<&str, i64> = HashMap::new();
    for (prefix, name, _) in ROLE_USERS {
        let email = email_of(prefix);
        let id = match tenancy::user_by_email(&mut tx, &email).await? {
            Some(u) => u.id,
            None => {
                created.users += 1;
                tenancy::create_user(&mut tx, name, &email, DEMO_PASSWORD, None).await?.id
            }
        };
        ids.insert(prefix, id);
    }
    let admin_id = ids["admin"];
    let existing_org: Option<i64> =
        sqlx::query_scalar("SELECT id FROM tbl_organization WHERE unique_name = $1 AND deleted_at IS NULL")
            .bind(DEMO_ORG_UNIQUE_NAME)
            .fetch_optional(&mut *tx)
            .await?;
    let org_id = match existing_org {
        Some(id) => id,
        None => {
            created.organizations += 1;
            tenancy::create_org(&mut tx, DEMO_ORG_UNIQUE_NAME, DEMO_ORG_NAME, admin_id).await?.id
        }
    };
    let mut accounts = Vec::new();
    for (prefix, name, role) in ROLE_USERS {
        let uid = ids[prefix];
        // 成員身分與角色是兩件事：少了成員身分，組織根本不會出現在清單裡。
        tenancy::add_org_member(&mut tx, uid, org_id, "member").await?;
        let r = grant::role_by_unique_name(&mut tx, RbacScope::Organization, role).await?;
        let has: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM tbl_user_rbac_role
               WHERE user_id = $1 AND role_id = $2 AND organization_id = $3 AND revoked_at IS NULL AND deleted_at IS NULL)",
        )
        .bind(uid)
        .bind(r.id)
        .bind(org_id)
        .fetch_one(&mut *tx)
        .await?;
        if !has {
            let input = GrantInput { user_id: uid, role_id: r.id, organization_id: Some(org_id), project_id: None, resource_identifier: None };
            grant::grant(&mut tx, &input, None).await?;
            created.grants += 1;
        }
        accounts.push(DemoAccount { email: email_of(prefix), name: name.into(), role: role.into(), user_id: uid });
    }
    let all_ids: Vec<i64> = accounts.iter().map(|a| a.user_id).collect();
    core.rbac.commit(tx, &all_ids).await?;

    // ── 主檔、部門、流程定義、策略、綁定（以 admin 身分發佈）──
    let admin = user_ctx(core, admin_id, org_id).await?;
    let mut tx = core.pool.begin().await?;
    let station_ids = seed_master_data(&mut tx, &admin, &mut created).await?;
    seed_flows(&mut tx, &admin, &ids, &station_ids, &mut created).await?;
    tx.commit().await?;

    // ── 一筆跑完的保證金請款 ──
    let deposit_execution_id = seed_deposit_execution(core, org_id, &ids, &station_ids, &mut created).await?;

    for a in &accounts {
        log::info!("[demo-seed] 帳號 {} 角色 {} 密碼 {}", a.email, a.role, DEMO_PASSWORD);
    }
    log::info!("[demo-seed] 組織 {DEMO_ORG_UNIQUE_NAME} (#{org_id}) 就緒；本次新建 {} 筆：{:?}", created.total(), created);

    Ok(DemoSeedReport {
        org_id,
        org_unique_name: DEMO_ORG_UNIQUE_NAME.into(),
        accounts,
        created,
        deposit_execution_id,
    })
}

/// 主檔建出來的 id（後面的段落要引用）。
struct SeededIds {
    stations: HashMap<&'static str, i64>,
    contracts: HashMap<&'static str, i64>,
    /// 建設期契約第 1 期（保證金請款的主體）。
    first_installment: i64,
}

/// `party_type`／`code`（卡 A 擴欄）：舊的個人地主一律 `individual`；子公司類才有 code。
async fn ensure_landowner(tx: &mut Tx<'_>, org: i64, row: [&str; 9], party_type: &str, code: Option<&str>, created: &mut DemoSeedCounts) -> ApiResult<i64> {
    let [name, tax_id, contact_name, contact_phone, contact_email, bank_name, bank_branch, bank_account, bank_account_name] = row;
    if let Some(id) = sqlx::query_scalar("SELECT id FROM tbl_energy_landowner WHERE organization_id = $1 AND name = $2 AND deleted_at IS NULL ORDER BY id LIMIT 1")
        .bind(org)
        .bind(name)
        .fetch_optional(&mut **tx)
        .await?
    {
        return Ok(id);
    }
    created.landowners += 1;
    let blank = |s: &str| if s.is_empty() { None } else { Some(s.to_string()) };
    Ok(sqlx::query_scalar(
        "INSERT INTO tbl_energy_landowner
           (organization_id, name, tax_id, contact_name, contact_phone, contact_email, bank_name, bank_branch, bank_account, bank_account_name,
            status, party_type, code)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'active', $11, $12) RETURNING id",
    )
    .bind(org)
    .bind(name)
    .bind(blank(tax_id))
    .bind(blank(contact_name))
    .bind(blank(contact_phone))
    .bind(blank(contact_email))
    .bind(blank(bank_name))
    .bind(blank(bank_branch))
    .bind(blank(bank_account))
    .bind(blank(bank_account_name))
    .bind(party_type)
    .bind(code)
    .fetch_one(&mut **tx)
    .await?)
}

/// 案場的其中一個電號（躉購或用電）；冪等以 `(station_id, usage)` 判斷是否已有。
async fn ensure_meter(tx: &mut Tx<'_>, org: i64, station_id: i64, customer_number: &str, usage: &str, created: &mut DemoSeedCounts) -> ApiResult<i64> {
    if let Some(id) = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM tbl_energy_station_meter WHERE organization_id = $1 AND station_id = $2 AND usage = $3 AND deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .bind(org)
    .bind(station_id)
    .bind(usage)
    .fetch_optional(&mut **tx)
    .await?
    {
        return Ok(id);
    }
    created.meters += 1;
    Ok(sqlx::query_scalar(
        "INSERT INTO tbl_energy_station_meter (organization_id, station_id, customer_number, usage) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(org)
    .bind(station_id)
    .bind(customer_number)
    .bind(usage)
    .fetch_one(&mut **tx)
    .await?)
}

/// 一位地主在一個案場的分配（比例，百分比字串如 `"60"`）；冪等以 `(landowner_id, station_id)` 判斷。
async fn ensure_allocation(tx: &mut Tx<'_>, org: i64, landowner_id: i64, station_id: i64, ratio: &str, created: &mut DemoSeedCounts) -> ApiResult<i64> {
    if let Some(id) = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM mn_energy_landowner_station WHERE organization_id = $1 AND landowner_id = $2 AND station_id = $3 AND deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .bind(org)
    .bind(landowner_id)
    .bind(station_id)
    .fetch_optional(&mut **tx)
    .await?
    {
        return Ok(id);
    }
    created.allocations += 1;
    Ok(sqlx::query_scalar(
        "INSERT INTO mn_energy_landowner_station (organization_id, landowner_id, station_id, method, ratio) VALUES ($1, $2, $3, 'ratio', $4::numeric) RETURNING id",
    )
    .bind(org)
    .bind(landowner_id)
    .bind(station_id)
    .bind(ratio)
    .fetch_one(&mut **tx)
    .await?)
}

/// 案場；座標給了就標成手動精確定位（地圖不該等 geocoder，手動的也不會被自動定位覆蓋）。
async fn ensure_station(
    tx: &mut Tx<'_>,
    ctx: &Ctx,
    landowner: i64,
    customer_number: &str,
    name: &str,
    status: &str,
    coords: Option<(&str, &str)>,
    address: &str,
    created: &mut DemoSeedCounts,
) -> ApiResult<i64> {
    let key = normalize_customer_number(Some(customer_number));
    if let Some(id) = sqlx::query_scalar(
        "SELECT id FROM tbl_energy_station WHERE organization_id = $1 AND (customer_number = $2 OR customer_number_key = $3) AND deleted_at IS NULL LIMIT 1",
    )
    .bind(ctx.org_id())
    .bind(customer_number)
    .bind(key.as_deref())
    .fetch_optional(&mut **tx)
    .await?
    {
        return Ok(id);
    }
    created.stations += 1;
    let (lat, lng, precision, source, at) = match coords {
        Some((lat, lng)) => (Some(lat), Some(lng), Some("exact"), Some("manual"), Some(ctx.now())),
        None => (None, None, None, None, None),
    };
    Ok(sqlx::query_scalar(
        "INSERT INTO tbl_energy_station
           (organization_id, landowner_id, customer_number, customer_number_key, name, station_type, status,
            latitude, longitude, address, geocode_precision, geocode_source, geocoded_at)
         VALUES ($1, $2, $3, $4, $5, 'solar', $6, $7::numeric, $8::numeric, $9, $10, $11, $12) RETURNING id",
    )
    .bind(ctx.org_id())
    .bind(landowner)
    .bind(customer_number)
    .bind(key)
    .bind(name)
    .bind(status)
    .bind(lat)
    .bind(lng)
    .bind(address)
    .bind(precision)
    .bind(source)
    .bind(at)
    .fetch_one(&mut **tx)
    .await?)
}

async fn ensure_contract(
    tx: &mut Tx<'_>,
    org: i64,
    landowner: i64,
    no: &str,
    sign_method: &str,
    valid_from: &str,
    stage: &str,
    installment_total: Option<i32>,
    note: Option<&str>,
    created: &mut DemoSeedCounts,
) -> ApiResult<i64> {
    if let Some(id) = sqlx::query_scalar("SELECT id FROM tbl_energy_contract WHERE organization_id = $1 AND contract_no = $2 AND deleted_at IS NULL")
        .bind(org)
        .bind(no)
        .fetch_optional(&mut **tx)
        .await?
    {
        return Ok(id);
    }
    created.contracts += 1;
    Ok(sqlx::query_scalar(
        "INSERT INTO tbl_energy_contract (organization_id, landowner_id, contract_no, sign_method, valid_from, valid_to, stage, installment_total, note)
         VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, $8) RETURNING id",
    )
    .bind(org)
    .bind(landowner)
    .bind(no)
    .bind(sign_method)
    .bind(date(valid_from))
    .bind(stage)
    .bind(installment_total)
    .bind(note)
    .fetch_one(&mut **tx)
    .await?)
}

/// 契約 × 案場的配對；分潤比例這類數字是「這一組配對」的屬性。
async fn ensure_coverage(tx: &mut Tx<'_>, contract: i64, station: i64, params: Value, created: &mut DemoSeedCounts) -> ApiResult<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM mn_energy_contract_station WHERE contract_id = $1 AND station_id = $2 AND deleted_at IS NULL)",
    )
    .bind(contract)
    .bind(station)
    .fetch_one(&mut **tx)
    .await?;
    if !exists {
        sqlx::query("INSERT INTO mn_energy_contract_station (contract_id, station_id, params) VALUES ($1, $2, $3)")
            .bind(contract)
            .bind(station)
            .bind(params)
            .execute(&mut **tx)
            .await?;
        created.contract_stations += 1;
    }
    Ok(())
}

async fn seed_master_data(tx: &mut Tx<'_>, ctx: &Ctx, created: &mut DemoSeedCounts) -> ApiResult<SeededIds> {
    let org = ctx.org_id();
    let chen = ensure_landowner(
        tx,
        org,
        ["陳明德", "A123456789", "陳小姐", "0912-345-678", "chen@example.com", "台灣銀行", "屏東分行", "012-345-678901", "陳明德"],
        "individual",
        None,
        created,
    )
    .await?;
    let lin = ensure_landowner(
        tx,
        org,
        ["林秀英", "B234567890", "林先生", "0922-111-222", "", "第一銀行", "嘉義分行", "098-765-432100", "林秀英"],
        "individual",
        None,
        created,
    )
    .await?;

    // 卡 A（20261118-0427）：受款對象擴類型 —— 2 子公司（假代號 ZA／ZB）、1 台電區處、1 公司、1 機關。
    // 全部假名、假統編（`00000001` 之類）。
    let sub_za = ensure_landowner(
        tx, org,
        ["示範子公司甲", "", "", "", "", "", "", "", ""],
        "subsidiary", Some("ZA"), created,
    ).await?;
    let sub_zb = ensure_landowner(
        tx, org,
        ["示範子公司乙", "", "", "", "", "", "", "", ""],
        "subsidiary", Some("ZB"), created,
    ).await?;
    let _taipower_district = ensure_landowner(
        tx, org,
        ["台電屏東區處", "", "", "", "", "", "", "", ""],
        "taipower_district", None, created,
    ).await?;
    let _demo_company = ensure_landowner(
        tx, org,
        ["示範能源股份有限公司", "00000001", "", "", "", "", "", "", ""],
        "company", None, created,
    ).await?;
    let _demo_agency = ensure_landowner(
        tx, org,
        ["示範鄉公所", "00000002", "", "", "", "", "", "", ""],
        "agency", None, created,
    ).await?;

    // 電號用四種範本電費單上的真值：上傳那四份 PDF 時才配得到案場。
    // 一座異常、一座建設中，狀態色都看得到；異常不等於停用，照樣收得了單。
    let demo = "（展示資料）";
    let s1 = ensure_station(tx, ctx, chen, "00-60-1600-79-5", "屏東枋寮一號", "operating", Some(("22.3654000", "120.5931000")), demo, created).await?;
    let s2 = ensure_station(tx, ctx, chen, "11-72-8007-95-8", "屏東枋寮二號", "abnormal", Some(("22.3701000", "120.5988000")), demo, created).await?;
    let s3 = ensure_station(tx, ctx, lin, "11-72-0328-95-9", "嘉義六腳一號", "operating", Some(("23.4921000", "120.3078000")), demo, created).await?;
    let s4 = ensure_station(tx, ctx, lin, "07-42-8560-90-1", "嘉義六腳二號", "construction", Some(("23.4977000", "120.3122000")), demo, created).await?;
    // 只有地址沒座標：「由地址推估」才有東西可按，也驗得到沒座標的不會被標在 (0,0)。
    let s5 = ensure_station(tx, ctx, chen, "01-2233-4477", "屏東枋寮三號（待定位）", "construction", None, "屏東縣枋寮鄉中山路 100 號", created).await?;
    // 地址是地段地號：推估只到行政區級，那種精度不可以填進座標。這一座刻意不綁流程（「未綁定」也是真實狀態）。
    let s6 = ensure_station(tx, ctx, lin, "02-7788-9922", "台南七股一號（地號）", "construction", None, "台南市七股區大潭段123地號", created).await?;

    // 卡 A（20261118-0427）：這 6 座既有示範案場各補一列主電號（躉購）＋ 100% 分配 ——
    // migration 0103 的 backfill 只在既有資料庫升級時補，這裡是全新資料庫（migration 先跑、demo seed 後跑），
    // 所以示範資料要自己建，否則電號區／分配區看起來永遠是空的。
    for (station_id, owner, cn) in [
        (s1, chen, "00-60-1600-79-5"), (s2, chen, "11-72-8007-95-8"), (s3, lin, "11-72-0328-95-9"),
        (s4, lin, "07-42-8560-90-1"), (s5, chen, "01-2233-4477"), (s6, lin, "02-7788-9922"),
    ] {
        ensure_meter(tx, org, station_id, cn, "wholesale", created).await?;
        ensure_allocation(tx, org, owner, station_id, "100", created).await?;
    }

    // 卡 A 的示範案場「示範案場（雙電號）」：兩個電號（躉購＋用電）、兩位地主 60％／40％，
    // 專案編號四樣齊全（併網日、型別、SPV 代號 ZB、序號）。電號用假號（`99-` 開頭）。
    let s7 = ensure_station(
        tx, ctx, chen, "99-0000-0001-00", "示範案場（雙電號）", "operating",
        Some(("22.6000000", "120.5500000")), "（展示資料）", created,
    ).await?;
    ensure_meter(tx, org, s7, "99-0000-0001-00", "wholesale", created).await?;
    ensure_meter(tx, org, s7, "99-0000-0002-00", "consumption", created).await?;
    ensure_allocation(tx, org, chen, s7, "60", created).await?;
    ensure_allocation(tx, org, lin, s7, "40", created).await?;
    {
        let grid_date = date("2024-06-01");
        let project_code = crate::master_data::project_code(Some(grid_date), Some("rooftop"), Some("ZB"), Some(3));
        sqlx::query(
            "UPDATE tbl_energy_station SET grid_date = $2, site_type = 'rooftop', spv_payee_id = $3, serial_no = 3,
               service_district = '台電屏東區處', capacity_kw = 850.000, project_code = $4
             WHERE id = $1",
        )
        .bind(s7)
        .bind(grid_date)
        .bind(sub_zb)
        .bind(&project_code)
        .execute(&mut **tx)
        .await?;
    }
    // sub_za 留給使用者在畫面上手動新增案場時選用（Story S1：114BZA0007）。
    let _ = sub_za;

    let c1 = ensure_contract(tx, org, chen, "CT-2026-001", "紙本用印", "2026-01-01", "operating", None, None, created).await?;
    let c2 = ensure_contract(tx, org, lin, "CT-2026-002", "電子簽章", "2026-02-01", "construction", Some(3), None, created).await?;
    ensure_coverage(tx, c1, s1, json!({ "sharePercent": 12.5, "note": "示範：售電分潤 12.5%" }), created).await?;
    ensure_coverage(tx, c2, s3, json!({ "sharePercent": 15, "note": "示範：同型契約、不同案主的比例" }), created).await?;

    // 四份測試契約：一種單別一份，綁到對應電號的案場；各自的比例刻意都不一樣。
    let mut contracts: HashMap<&'static str, i64> = HashMap::from([("CT-2026-001", c1), ("CT-2026-002", c2)]);
    for (no, owner, station, doc_type, share, note) in demo_contracts(chen, lin, s1, s2, s3, s4) {
        let id = ensure_contract(tx, org, owner, no, "紙本用印", OPEN_FROM, "operating", None, Some(&format!("測試契約：{doc_type}")), created).await?;
        ensure_coverage(tx, id, station, json!({ "sharePercent": share, "note": note }), created).await?;
        contracts.insert(no, id);
    }

    // 建設期契約三期保證金；第 1 期已付款（下面會替它把請款流程跑完，兩者對得起來）。
    let mut first_installment = 0;
    for seq in 1..=3i32 {
        let existing: Option<i64> =
            sqlx::query_scalar("SELECT id FROM tbl_energy_installment WHERE contract_id = $1 AND seq = $2 AND deleted_at IS NULL")
                .bind(c2)
                .bind(seq)
                .fetch_optional(&mut **tx)
                .await?;
        let id = match existing {
            Some(id) => id,
            None => {
                created.installments += 1;
                sqlx::query_scalar(
                    "INSERT INTO tbl_energy_installment (organization_id, contract_id, seq, due_date, amount, status)
                     VALUES ($1, $2, $3, $4, 150000.00, $5) RETURNING id",
                )
                .bind(org)
                .bind(c2)
                .bind(seq)
                .bind(date(&format!("2026-0{}-15", seq + 1)))
                .bind(if seq == 1 { "paid" } else { "pending" })
                .fetch_one(&mut **tx)
                .await?
            }
        };
        if seq == 1 {
            first_installment = id;
        }
    }

    let events = [
        (lin, c2, s3, "2026-03-15", "保證金第 2 期到期", "pending"),
        (lin, c2, s3, "2026-04-15", "保證金第 3 期到期", "pending"),
        (chen, c1, s1, "2026-01-15", "首期款到期", "done"),
    ];
    for (owner, contract, station, day, title, status) in events {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM tbl_energy_calendar_event
               WHERE organization_id = $1 AND contract_id = $2 AND event_date = $3 AND title = $4 AND deleted_at IS NULL)",
        )
        .bind(org)
        .bind(contract)
        .bind(date(day))
        .bind(title)
        .fetch_one(&mut **tx)
        .await?;
        if exists {
            continue;
        }
        sqlx::query(
            "INSERT INTO tbl_energy_calendar_event (organization_id, landowner_id, contract_id, station_id, event_date, kind, title, status)
             VALUES ($1, $2, $3, $4, $5, 'installmentDue', $6, $7)",
        )
        .bind(org)
        .bind(owner)
        .bind(contract)
        .bind(station)
        .bind(date(day))
        .bind(title)
        .bind(status)
        .execute(&mut **tx)
        .await?;
        created.calendar_events += 1;
    }

    Ok(SeededIds {
        stations: HashMap::from([
            ("00-60-1600-79-5", s1),
            ("11-72-8007-95-8", s2),
            ("11-72-0328-95-9", s3),
            ("07-42-8560-90-1", s4),
            ("01-2233-4477", s5),
            ("02-7788-9922", s6),
        ]),
        contracts,
        first_installment,
    })
}

/// (契約編號, 地主, 案場, 單別, 分潤比例, 說明)
fn demo_contracts(chen: i64, lin: i64, s1: i64, s2: i64, s3: i64, s4: i64) -> [(&'static str, i64, i64, &'static str, f64, &'static str); 4] {
    [
        ("CT-DEMO-RS", chen, s1, "renewableSurplus", 12.5, "躉購·有餘電：核可 ＋ 開發票"),
        ("CT-DEMO-RN", chen, s2, "renewableNoSurplus", 10.0, "躉購·無餘電：我們付台電"),
        ("CT-DEMO-CD", lin, s3, "consumptionDevice", 8.0, "逆變器·僅設備發電使用電費"),
        ("CT-DEMO-CR", lin, s4, "consumptionDeviceRental", 15.0, "逆變器·含電器租費"),
    ]
}

/// 某個 key 的某一版定義 id。
async fn definition_id(tx: &mut Tx<'_>, org: i64, key: &str, version: i32) -> ApiResult<Option<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM tbl_energy_workflow_definition WHERE organization_id = $1 AND key = $2 AND version = $3 AND deleted_at IS NULL",
    )
    .bind(org)
    .bind(key)
    .bind(version)
    .fetch_optional(&mut **tx)
    .await?)
}

async fn require_definition(tx: &mut Tx<'_>, org: i64, key: &str, version: i32) -> ApiResult<i64> {
    definition_id(tx, org, key, version)
        .await?
        .ok_or_else(|| ApiError::internal(format!("demo seed: definition {key} v{version} missing")))
}

/// 一段流程綁定：案場層給電號（station_id 只是補充欄），契約層給 contract。同一段已存在就略過。
struct Binding<'a> {
    station_key: Option<&'a str>,
    station_id: Option<i64>,
    contract_id: Option<i64>,
    doc_type: &'a str,
    workflow_key: &'a str,
    definition_id: i64,
    valid_from: &'a str,
    valid_to: Option<&'a str>,
    note: String,
}

async fn ensure_wf_binding(tx: &mut Tx<'_>, org: i64, b: Binding<'_>, created: &mut DemoSeedCounts) -> ApiResult<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM tbl_energy_workflow_binding
           WHERE organization_id = $1 AND station_key IS NOT DISTINCT FROM $2 AND contract_id IS NOT DISTINCT FROM $3
             AND doc_type = $4 AND workflow_key = $5 AND definition_id = $6 AND valid_from = $7 AND deleted_at IS NULL)",
    )
    .bind(org)
    .bind(b.station_key)
    .bind(b.contract_id)
    .bind(b.doc_type)
    .bind(b.workflow_key)
    .bind(b.definition_id)
    .bind(date(b.valid_from))
    .fetch_one(&mut **tx)
    .await?;
    if exists {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO tbl_energy_workflow_binding
           (organization_id, station_key, station_id, contract_id, doc_type, workflow_key, definition_id, valid_from, valid_to, note)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(org)
    .bind(b.station_key)
    .bind(b.station_id)
    .bind(b.contract_id)
    .bind(b.doc_type)
    .bind(b.workflow_key)
    .bind(b.definition_id)
    .bind(date(b.valid_from))
    .bind(b.valid_to.map(date))
    .bind(b.note)
    .execute(&mut **tx)
    .await?;
    created.workflow_bindings += 1;
    Ok(())
}

async fn ensure_member(tx: &mut Tx<'_>, ctx: &Ctx, dept_key: &str, user_id: i64, role: DepartmentMemberRole, created: &mut DemoSeedCounts) -> ApiResult<()> {
    let dept = crate::org::departments::by_key(tx, &ctx.access, dept_key)
        .await?
        .ok_or_else(|| ApiError::internal(format!("demo seed: department {dept_key} missing")))?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM mn_energy_department_member WHERE department_id = $1 AND user_id = $2 AND deleted_at IS NULL)",
    )
    .bind(dept.id)
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await?;
    if !exists {
        crate::org::departments::set_department_member(tx, ctx, dept.id, user_id, role).await?;
        created.department_members += 1;
    }
    Ok(())
}

async fn seed_flows(
    tx: &mut Tx<'_>,
    ctx: &Ctx,
    ids: &HashMap<&str, i64>,
    seeded: &SeededIds,
    created: &mut DemoSeedCounts,
) -> ApiResult<()> {
    let org = ctx.org_id();
    // 內建四份定義 v1、預設策略 v1（售電分潤 10%）、三個部門。
    let installed = install_builtin_definitions(tx, ctx).await?;
    created.workflow_definitions += installed.definitions.len();
    created.rating_strategies += installed.strategies.len();
    created.departments += installed.departments.len();

    // 部門編制：發起人的部門主管（R1）靠這張表解析，manager@ 一定要是主管，否則主管核准那關沒有候選人。
    // admin 同時是三個部門的成員，登入後就看得到待辦。
    use DepartmentMemberRole::{Manager, Member};
    let members = [
        (DEPT_SALES, "manager", Manager),
        (DEPT_SALES, "operator", Member),
        (DEPT_SALES, "admin", Member),
        (DEPT_ACCOUNTING, "accounting", Manager),
        (DEPT_ACCOUNTING, "admin", Member),
        (DEPT_FINANCE, "finance", Manager),
        (DEPT_FINANCE, "admin", Member),
        (DEPT_BILL_REVIEW, "manager", Member),
        (DEPT_BILL_REVIEW, "finance", Member),
        (DEPT_BILL_REVIEW, "accounting", Member),
    ];
    for (dept, who, role) in members {
        ensure_member(tx, ctx, dept, ids[who], role, created).await?;
    }

    let bill = defs::platform_review_bill();
    let utility_key = defs::utility_bill().key;
    let invoice_key = defs::invoice_issue().key;
    let deposit_key = defs::deposit_request().key;
    let v1 = require_definition(tx, org, &bill.key, 1).await?;
    // 第二版：同結構、指回 v1，只為了讓效期軸畫得出「換版」。進行中的單鎖在自己那一版，不受影響。
    let v2 = match definition_id(tx, org, &bill.key, 2).await? {
        Some(id) => id,
        None => {
            let mut d = bill.clone();
            d.name = format!("{}（2026 版）", bill.name);
            d.description = Some("展示用：與 v1 同結構，只為了讓效期軸畫得出「換版」。".into());
            created.workflow_definitions += 1;
            crate::workflow::definition::install_definition(tx, &ctx.access, &d, Some(v1), ctx.now()).await?.id
        }
    };
    let utility = require_definition(tx, org, &utility_key, 1).await?;
    let invoice = require_definition(tx, org, &invoice_key, 1).await?;
    let deposit = require_definition(tx, org, &deposit_key, 1).await?;

    let (pingtung, chiayi1, chiayi2) = ("00-60-1600-79-5", "11-72-0328-95-9", "07-42-8560-90-1");
    let st = |k: &str| seeded.stations.get(k).copied();
    let station_bindings = [
        // 屏東枋寮一號：核可流程 v1 → v2 兩段（在換版日切開）＋ 有餘電要平行開發票。
        (pingtung, "electricityBill", bill.key.as_str(), v1, OPEN_FROM, Some(SWITCH_DAY), "DESIGN.md §3 預設流程"),
        (pingtung, "electricityBill", bill.key.as_str(), v2, SWITCH_DAY, None, "FRONTEND.md §2.85.2：換版展示"),
        (pingtung, "renewableSurplus", invoice_key.as_str(), invoice, OPEN_FROM, None, "躉購有餘電：開發票給台電（與核可流程平行）"),
        // 嘉義六腳一號：有期數那張契約的案場 —— 保證金請款要綁在這裡，綁錯案場會取不到版。
        (chiayi1, "depositRequest", deposit_key.as_str(), deposit, OPEN_FROM, None, "DESIGN.md §3.2 保證金請款流程"),
        (chiayi1, "electricityBill", bill.key.as_str(), v2, OPEN_FROM, None, "FRONTEND.md §2.85.2：嘉義補齊"),
        // 用電電費單：一份定義綁兩座高壓案場（不要各發一版內容相同的定義）。
        (chiayi1, "consumptionBill", utility_key.as_str(), utility, OPEN_FROM, None, "RECOGNITION.md §9-1 用電電費單付款流程"),
        (chiayi2, "consumptionBill", utility_key.as_str(), utility, OPEN_FROM, None, "RECOGNITION.md §9-1 用電電費單付款流程"),
    ];
    for (key, doc_type, workflow_key, def_id, from, to, note) in station_bindings {
        let b = Binding {
            station_key: Some(key),
            station_id: st(key),
            contract_id: None,
            doc_type,
            workflow_key,
            definition_id: def_id,
            valid_from: from,
            valid_to: to,
            note: note.into(),
        };
        ensure_wf_binding(tx, org, b, created).await?;
    }

    // 契約層：四份契約 × 兩條流程（A 主流程、B 一律開立發票，測「一張單同時開兩條」）。契約層贏過案場層。
    for (no, _, _, doc_type, _, _) in demo_contracts(0, 0, 0, 0, 0, 0) {
        let contract = seeded.contracts[no];
        let (a_key, a_def) = if doc_type == "renewableSurplus" { (bill.key.as_str(), v2) } else { (utility_key.as_str(), utility) };
        for (workflow_key, def_id, note) in [(a_key, a_def, format!("{no}：A（主流程）")), (invoice_key.as_str(), invoice, format!("{no}：B（第二條，測試多流程）"))] {
            let b = Binding {
                station_key: None,
                station_id: None,
                contract_id: Some(contract),
                doc_type,
                workflow_key,
                definition_id: def_id,
                valid_from: OPEN_FROM,
                valid_to: None,
                note,
            };
            ensure_wf_binding(tx, org, b, created).await?;
        }
    }

    // 計算策略：第二版換另一種計法（度數 × 元/度），軸上兩段才看得出換版的意義。
    let strategy_key = crate::rating::DEFAULT_STRATEGY_KEY;
    let strategy = |version: i32| {
        sqlx::query_scalar::<_, i64>(
            "SELECT id FROM tbl_energy_rating_strategy WHERE organization_id = $1 AND key = $2 AND version = $3 AND deleted_at IS NULL",
        )
        .bind(org)
        .bind(strategy_key)
        .bind(version)
    };
    let share = strategy(1)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ApiError::internal("demo seed: default strategy missing"))?;
    let unit = match strategy(2).fetch_optional(&mut **tx).await? {
        Some(id) => id,
        None => {
            created.rating_strategies += 1;
            sqlx::query_scalar(
                "INSERT INTO tbl_energy_rating_strategy
                   (organization_id, key, version, name, type, params, status, published_at, published_by_user_id, currency)
                 VALUES ($1, $2, 2, '售電單價 1.9 元/度', 'generationUnitPrice', $3, 'published', $4, $5, 'TWD') RETURNING id",
            )
            .bind(org)
            .bind(strategy_key)
            .bind(json!({ "unitPrice": 1.9 }))
            .bind(ctx.now())
            .bind(ctx.user_id())
            .fetch_one(&mut **tx)
            .await?
        }
    };
    let s1 = seeded.stations[pingtung];
    let s3 = seeded.stations[chiayi1];
    for (station, strategy_id, from, to) in [(s1, share, OPEN_FROM, Some(SWITCH_DAY)), (s1, unit, SWITCH_DAY, None), (s3, share, OPEN_FROM, None)] {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM tbl_energy_rating_strategy_binding
               WHERE organization_id = $1 AND station_id = $2 AND strategy_id = $3 AND valid_from = $4 AND deleted_at IS NULL)",
        )
        .bind(org)
        .bind(station)
        .bind(strategy_id)
        .bind(date(from))
        .fetch_one(&mut **tx)
        .await?;
        if exists {
            continue;
        }
        sqlx::query(
            "INSERT INTO tbl_energy_rating_strategy_binding (organization_id, station_id, strategy_key, strategy_id, valid_from, valid_to)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(org)
        .bind(station)
        .bind(strategy_key)
        .bind(strategy_id)
        .bind(date(from))
        .bind(to.map(date))
        .execute(&mut **tx)
        .await?;
        created.rating_bindings += 1;
    }
    Ok(())
}

/// 保證金請款跑一筆到完成：它的主體是契約期數、沒有收件台路徑，不種就永遠看不到這一類。
/// 一定要跑完：留在放行或付款關，會和電費單擠在同一個關卡名下，示範時容易推錯單。
async fn seed_deposit_execution(
    core: &CoreRef,
    org_id: i64,
    ids: &HashMap<&str, i64>,
    seeded: &SeededIds,
    created: &mut DemoSeedCounts,
) -> ApiResult<Option<i64>> {
    let deposit_key = defs::deposit_request().key;
    let subject_id = seeded.first_installment.to_string();
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM tbl_energy_workflow_execution
          WHERE organization_id = $1 AND subject_type = 'energyInstallment' AND subject_id = $2 AND workflow_key = $3 AND deleted_at IS NULL
          ORDER BY id LIMIT 1",
    )
    .bind(org_id)
    .bind(&subject_id)
    .bind(&deposit_key)
    .fetch_optional(&core.pool)
    .await?;
    if let Some(id) = existing {
        return Ok(Some(id));
    }
    // 發起人不能是自己那一關的候選人（防自核）；用 admin 發起，放行與付款由財務、會計各自處理。
    let initiator = user_ctx(core, ids["admin"], org_id).await?;
    let finance = user_ctx(core, ids["finance"], org_id).await?;
    let accounting = user_ctx(core, ids["accounting"], org_id).await?;
    let station_key = "11-72-0328-95-9";

    let mut tx = core.pool.begin().await?;
    let exec = start_execution(
        &mut tx,
        &initiator,
        StartExecution {
            station_key: station_key.into(),
            workflow_key: deposit_key,
            subject_type: "energyInstallment".into(),
            subject_id,
            business_date: date("2026-02-15"),
            // 保證金沒有計算節點：金額直接來自契約期數。
            attributes: json!({ "installment": { "amount": 150000 } }),
            definition: None,
            station_id: seeded.stations.get(station_key).copied(),
        },
    )
    .await?;
    // 付款關先鎖副作用（已付款）再推進；順序反過來會出現「已放行但沒有付款紀錄」的單。
    step(&mut tx, &finance, exec.id, "finance-release", &["release"]).await?;
    step(&mut tx, &accounting, exec.id, "pay", &["markPaid", "submit"]).await?;
    tx.commit().await?;
    created.executions += 1;
    Ok(Some(exec.id))
}

/// 在某一關認領並依序按下動作。
async fn step(tx: &mut Tx<'_>, ctx: &Ctx, execution_id: i64, node_key: &str, actions: &[&str]) -> ApiResult<()> {
    let task: i64 = sqlx::query_scalar(
        "SELECT id FROM tbl_energy_workflow_task
          WHERE execution_id = $1 AND node_key = $2 AND state IN ('ready', 'reserved', 'inProgress') AND deleted_at IS NULL
          ORDER BY seq DESC LIMIT 1",
    )
    .bind(execution_id)
    .bind(node_key)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| ApiError::internal(format!("demo seed: no open task at {node_key}")))?;
    claim(tx, ctx, task).await?;
    for a in actions {
        perform_action(tx, ctx, task, ActionInput { action_key: (*a).into(), ..Default::default() }).await?;
    }
    Ok(())
}

/// 範本電費單那一段的備註；同一個組織已有這一批就不再灌（冪等）。
pub const SAMPLE_BATCH_NOTE: &str = "展示資料（範本 PDF）";

/// 四份範本各自扮演的狀態（每一張的電號、期間、金額都是紙上的值）：
/// 有餘電那張是主線（已校對、送審停在承辦確認）；僅設備發電那張校對完卻沒送出；
/// 含電表租費那張金額被人改過、流程已跑完（候選池情境）；無餘電那張只產單、不動它
/// （四個電號都種成了案場，所以它配得到屏東枋寮二號；例外件要由 crawler 自己上傳）。
const SAMPLES: [(&str, SampleRole); 4] = [
    ("電費單(有餘電需開發票).pdf", SampleRole::Main),
    ("逆變器電費單(僅設備發電使用電費).pdf", SampleRole::Reviewed),
    ("逆變器電費單(含電表租費&設備發電使用電費).pdf", SampleRole::Corrected),
    ("電費單(無餘電需付款).pdf", SampleRole::Plain),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SampleRole {
    Main,
    Reviewed,
    Corrected,
    Plain,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleBillsReport {
    pub batch_id: Option<i64>,
    pub bill_ids: Vec<i64>,
    /// 主線那張的執行（停在承辦確認）。
    pub main_execution_id: Option<i64>,
    /// 候選池情境那張的執行（manager 認領並推完、operator 只在候選池）。
    pub candidate_execution_id: Option<i64>,
    /// 這一次真的灌了（已存在而略過時為 false）。
    pub created: bool,
}

/// 灌四份範本電費單：走與使用者相同的路（開批 → 上傳原檔 → 套辨識結果 → 產單），再擺出各自的狀態。
/// 辨識結果取自 `canonical-samples.json`（假辨識服務回的也是它）。重跑安全：已有這一批就整段略過。
pub async fn run_sample_bills(core: &CoreRef, report: &DemoSeedReport, dir: &std::path::Path) -> ApiResult<SampleBillsReport> {
    let org_id = report.org_id;
    let existing: Option<i64> =
        sqlx::query_scalar("SELECT id FROM tbl_energy_ingest_batch WHERE organization_id = $1 AND note = $2 AND deleted_at IS NULL ORDER BY id LIMIT 1")
            .bind(org_id)
            .bind(SAMPLE_BATCH_NOTE)
            .fetch_optional(&core.pool)
            .await?;
    if let Some(batch_id) = existing {
        return Ok(SampleBillsReport { batch_id: Some(batch_id), ..Default::default() });
    }
    let id_of = |prefix: &str| {
        report.accounts.iter().find(|a| a.email == email_of(prefix)).map(|a| a.user_id).ok_or_else(|| ApiError::internal(format!("demo seed: account {prefix} missing")))
    };
    // 發起人用 admin（業務部一般成員）：manager／operator 都不能是發起人，否則候選池只剩一個人。
    let admin = user_ctx(core, id_of("admin")?, org_id).await?;
    let manager = user_ctx(core, id_of("manager")?, org_id).await?;
    let finance = user_ctx(core, id_of("finance")?, org_id).await?;
    let accounting = user_ctx(core, id_of("accounting")?, org_id).await?;

    let mut tx = core.pool.begin().await?;
    let batch = crate::ingest::service::create_batch(&mut tx, &admin, SAMPLES.len() as i32, Some(SAMPLE_BATCH_NOTE)).await?;
    tx.commit().await?;

    let mut out = SampleBillsReport { batch_id: Some(batch.id), created: true, ..Default::default() };
    let mut main_bill = None;
    let mut corrected_bill = None;
    for (file, role) in SAMPLES {
        let bytes = std::fs::read(dir.join(file)).map_err(|e| ApiError::internal(format!("demo seed: 讀不到範本 {}: {e}", dir.join(file).display())))?;
        let ocr = crate::fake_tpr::canonical_sample(file).ok_or_else(|| ApiError::internal(format!("demo seed: canonical sample {file} missing")))?;
        let store: &dyn AttachmentStore = &core.storage;
        let att = attachments::upload(
            &admin,
            store,
            UploadInput {
                owner: Owner::Batch(batch.id),
                bytes: bytes.into(),
                original_filename: file.into(),
                mime_type: "application/pdf".into(),
                task_id: None,
                role: Some("source".into()),
                note: None,
                supersedes_attachment_id: None,
            },
        )
        .await?;
        let mut tx = core.pool.begin().await?;
        let item = crate::ingest::service::register_uploaded_file(&mut tx, &admin, batch.id, att.id, file).await?;
        let item = crate::ingest::service::apply_recognition_to_item(&mut tx, &admin, item.id, &ocr).await?;
        if let Some(bill_id) = item.bill_id {
            out.bill_ids.push(bill_id);
            match role {
                SampleRole::Main | SampleRole::Reviewed => {
                    sqlx::query("UPDATE tbl_energy_bill SET review_status = 'reviewed' WHERE id = $1").bind(bill_id).execute(&mut *tx).await?;
                }
                // 只改最終值；ocr_extracted 那一層永遠是機器讀到的值（校對台要拿它對照）。
                SampleRole::Corrected => {
                    sqlx::query("UPDATE tbl_energy_bill SET total_amount = 4800.00 WHERE id = $1").bind(bill_id).execute(&mut *tx).await?;
                }
                SampleRole::Plain => {}
            }
            if role == SampleRole::Main {
                main_bill = Some(bill_id);
            }
            if role == SampleRole::Corrected {
                corrected_bill = Some(bill_id);
            }
        }
        crate::ingest::service::recompute_batch_status(&mut tx, batch.id).await?;
        tx.commit().await?;
    }

    // 候選池情境：業務部全員都是承辦確認的候選人；manager 認領並推完，operator 從頭到尾沒碰。一定要跑完，
    // 留在承辦確認會和主線那張擠在同一關。
    if let Some(bill_id) = corrected_bill {
        let mut tx = core.pool.begin().await?;
        let exec = start_execution(
            &mut tx,
            &admin,
            StartExecution {
                station_key: "07-42-8560-90-1".into(),
                workflow_key: defs::utility_bill().key,
                subject_type: "energyBill".into(),
                subject_id: bill_id.to_string(),
                business_date: date("2026-07-31"),
                attributes: json!({ "bill": { "totalAmount": 4800 } }),
                definition: None,
                station_id: None,
            },
        )
        .await?;
        step(&mut tx, &manager, exec.id, "clerk-review", &["submit"]).await?;
        step(&mut tx, &manager, exec.id, "manager-approve", &["approve"]).await?;
        step(&mut tx, &finance, exec.id, "finance-release", &["release"]).await?;
        step(&mut tx, &accounting, exec.id, "payment", &["markPaid", "submit"]).await?;
        tx.commit().await?;
        out.candidate_execution_id = Some(exec.id);
    }

    // 主線：用紙上的金額（127,848，≥ 加簽門檻）開核可流程，停在平台資料審核等人認領。
    if let Some(bill_id) = main_bill {
        let mut tx = core.pool.begin().await?;
        let exec = start_execution(
            &mut tx,
            &admin,
            StartExecution {
                station_key: "00-60-1600-79-5".into(),
                workflow_key: defs::platform_review_bill().key,
                subject_type: "energyBill".into(),
                subject_id: bill_id.to_string(),
                business_date: date("2026-08-10"),
                attributes: json!({ "bill": { "totalAmount": 127848 } }),
                definition: None,
                station_id: None,
            },
        )
        .await?;
        tx.commit().await?;
        out.main_execution_id = Some(exec.id);
    }
    Ok(out)
}
