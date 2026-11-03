//! 部門與部門成員。部門是主責者解析（R1／R2）的依據，所以每個異動都留稽核。

use crate::audit::{self, AuditInput};
use crate::common::{err, parse_body, user_name, user_names, Ctx, RequestMeta, Tx};
use crate::gates;
use actix_web::{web, HttpResponse};
use eo_domain::audit_events::diff_changes;
use eo_domain::schemas::{CreateDepartmentInput, DepartmentMemberRole, UpdateDepartmentInput};
use eo_platform::auth::CurrentUser;
use eo_platform::core::CoreRef;
use eo_platform::error::ApiResult;
use eo_platform::rbac::OrgAccess;
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct DepartmentRow {
    pub id: i64,
    pub key: String,
    pub name: String,
    pub sort_order: i32,
    pub organization_id: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepartmentMemberView {
    pub user_id: i64,
    pub name: Option<String>,
    pub role: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepartmentView {
    pub id: i64,
    pub key: String,
    pub name: String,
    pub sort_order: i32,
    pub members: Vec<DepartmentMemberView>,
}

const COLS: &str = "id, key, name, sort_order, organization_id";

fn role_str(r: DepartmentMemberRole) -> &'static str {
    match r {
        DepartmentMemberRole::Manager => "manager",
        DepartmentMemberRole::Member => "member",
    }
}

/// 部門所屬的組織由 server 從部門列推導：別的組織的部門 → 403，不存在 → 404。
pub async fn load_in_org(tx: &mut Tx<'_>, access: &OrgAccess, department_id: i64) -> ApiResult<DepartmentRow> {
    let row: Option<DepartmentRow> =
        sqlx::query_as(&format!("SELECT {COLS} FROM tbl_energy_department WHERE id = $1 AND deleted_at IS NULL"))
            .bind(department_id)
            .fetch_optional(&mut **tx)
            .await?;
    let row = row.ok_or_else(|| err(404, "department_not_found"))?;
    if row.organization_id != access.org_id() {
        return Err(err(403, "department_not_in_org"));
    }
    Ok(row)
}

/// 依 key 找本組織的部門（key 只在組織內唯一）。
pub async fn by_key(tx: &mut Tx<'_>, access: &OrgAccess, key: &str) -> ApiResult<Option<DepartmentRow>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {COLS} FROM tbl_energy_department WHERE organization_id = $1 AND key = $2 AND deleted_at IS NULL ORDER BY id LIMIT 1"
    ))
    .bind(access.org_id())
    .bind(key)
    .fetch_optional(&mut **tx)
    .await?)
}

/// 本組織的部門清單（含成員），依 sortOrder → id。
pub async fn list_departments(tx: &mut Tx<'_>, access: &OrgAccess) -> ApiResult<Vec<DepartmentView>> {
    let rows: Vec<DepartmentRow> = sqlx::query_as(&format!(
        "SELECT {COLS} FROM tbl_energy_department WHERE organization_id = $1 AND deleted_at IS NULL ORDER BY sort_order ASC, id ASC"
    ))
    .bind(access.org_id())
    .fetch_all(&mut **tx)
    .await?;
    if rows.is_empty() {
        return Ok(vec![]);
    }
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let members: Vec<(i64, i64, String)> = sqlx::query_as(
        "SELECT department_id, user_id, role FROM mn_energy_department_member
          WHERE department_id = ANY($1) AND deleted_at IS NULL ORDER BY id ASC",
    )
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    let user_ids: Vec<i64> = crate::common::uniq(members.iter().map(|m| m.1));
    let names = user_names(tx, &user_ids).await?;
    Ok(rows
        .into_iter()
        .map(|r| DepartmentView {
            members: members
                .iter()
                .filter(|m| m.0 == r.id)
                .map(|m| DepartmentMemberView { user_id: m.1, name: names.get(&m.1).cloned(), role: m.2.clone() })
                .collect(),
            id: r.id,
            key: r.key,
            name: r.name,
            sort_order: r.sort_order,
        })
        .collect())
}

/// 建部門。同組織 key 重複 → 409。
pub async fn create_department(tx: &mut Tx<'_>, ctx: &Ctx, input: &CreateDepartmentInput) -> ApiResult<DepartmentView> {
    if by_key(tx, &ctx.access, &input.key).await?.is_some() {
        return Err(err(409, "department_key_taken"));
    }
    let row: DepartmentRow = sqlx::query_as(&format!(
        "INSERT INTO tbl_energy_department (organization_id, key, name, sort_order) VALUES ($1, $2, $3, $4) RETURNING {COLS}"
    ))
    .bind(ctx.org_id())
    .bind(&input.key)
    .bind(&input.name)
    .bind(input.sort_order.unwrap_or(0) as i32)
    .fetch_one(&mut **tx)
    .await?;
    audit::record(
        tx,
        ctx,
        AuditInput::new("department.created", row.id, format!("建立部門「{}」（{}）", input.name, input.key))
            .label(input.name.clone())
            .payload(json!({ "departmentKey": input.key, "departmentName": input.name })),
    )
    .await?;
    Ok(DepartmentView { id: row.id, key: row.key, name: row.name, sort_order: row.sort_order, members: vec![] })
}

async fn update_department(tx: &mut Tx<'_>, ctx: &Ctx, id: i64, input: &UpdateDepartmentInput) -> ApiResult<DepartmentView> {
    let before = load_in_org(tx, &ctx.access, id).await?;
    let after: DepartmentRow = sqlx::query_as(&format!(
        "UPDATE tbl_energy_department SET name = COALESCE($2, name), sort_order = COALESCE($3, sort_order)
          WHERE id = $1 AND organization_id = $4 AND deleted_at IS NULL RETURNING {COLS}"
    ))
    .bind(id)
    .bind(input.name.as_deref())
    .bind(input.sort_order.map(|n| n as i32))
    .bind(ctx.org_id())
    .fetch_one(&mut **tx)
    .await?;
    // 差異對：只存真的變了的欄位。
    let b = json!({ "name": before.name, "sortOrder": before.sort_order });
    let a = json!({ "name": after.name, "sortOrder": after.sort_order });
    let diff = diff_changes(b.as_object(), a.as_object(), &[]);
    audit::record(
        tx,
        ctx,
        AuditInput::new("department.updated", id, format!("更新部門「{}」", after.name))
            .label(after.name.clone())
            .payload(json!({ "departmentKey": after.key, "departmentName": after.name, "changes": diff.changes })),
    )
    .await?;
    Ok(DepartmentView { id: after.id, key: after.key, name: after.name, sort_order: after.sort_order, members: vec![] })
}

async fn delete_department(tx: &mut Tx<'_>, ctx: &Ctx, id: i64) -> ApiResult<u64> {
    let dept = load_in_org(tx, &ctx.access, id).await?;
    let now = ctx.now();
    let removed_members = sqlx::query("UPDATE mn_energy_department_member SET deleted_at = $2 WHERE department_id = $1 AND deleted_at IS NULL")
        .bind(id)
        .bind(now)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    let removed = sqlx::query(
        "UPDATE tbl_energy_department SET deleted_at = $2 WHERE id = $1 AND organization_id = $3 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(now)
    .bind(ctx.org_id())
    .execute(&mut **tx)
    .await?
    .rows_affected();
    audit::record(
        tx,
        ctx,
        AuditInput::new("department.deleted", id, format!("刪除部門「{}」（連帶移除 {removed_members} 位成員）", dept.name))
            .label(dept.name.clone())
            .payload(json!({ "departmentKey": dept.key, "departmentName": dept.name, "removedMembers": removed_members })),
    )
    .await?;
    Ok(removed)
}

/// 指派／改派部門成員（upsert）。一個人可以在多個部門、一個部門可以有多位主管。
pub async fn set_department_member(
    tx: &mut Tx<'_>,
    ctx: &Ctx,
    department_id: i64,
    user_id: i64,
    role: DepartmentMemberRole,
) -> ApiResult<DepartmentMemberView> {
    let dept = load_in_org(tx, &ctx.access, department_id).await?;
    let label = user_name(tx, user_id).await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM tbl_user WHERE id = $1 AND deleted_at IS NULL)")
        .bind(user_id)
        .fetch_one(&mut **tx)
        .await?;
    if !exists {
        return Err(err(404, "user_not_found"));
    }
    let role = role_str(role);
    let existing: Option<(i64, String)> = sqlx::query_as(
        "SELECT id, role FROM mn_energy_department_member WHERE department_id = $1 AND user_id = $2 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(department_id)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?;
    match &existing {
        Some((id, _)) => {
            sqlx::query("UPDATE mn_energy_department_member SET role = $2 WHERE id = $1").bind(id).bind(role).execute(&mut **tx).await?;
        }
        None => {
            sqlx::query("INSERT INTO mn_energy_department_member (department_id, user_id, role) VALUES ($1, $2, $3)")
                .bind(department_id)
                .bind(user_id)
                .bind(role)
                .execute(&mut **tx)
                .await?;
        }
    }
    let who = label.clone().unwrap_or_else(|| format!("#{user_id}"));
    let mut base = json!({
        "departmentKey": dept.key, "departmentName": dept.name, "memberUserId": user_id, "role": role,
    });
    if let Some(l) = &label {
        base["memberLabel"] = json!(l);
    }
    // 主管異動改變的是候選池，跟「多一個同仁」分成兩個 type，稽核才問得出「誰動過候選池」。
    match existing {
        Some((_, prev)) if prev != role => {
            base["previousRole"] = json!(prev);
            audit::record(
                tx,
                ctx,
                AuditInput::new(
                    "department.manager_changed",
                    department_id,
                    format!("將「{who}」在部門「{}」的角色由 {prev} 改為 {role}", dept.name),
                )
                .label(dept.name.clone())
                .payload(base),
            )
            .await?;
        }
        None => {
            audit::record(
                tx,
                ctx,
                AuditInput::new("department.member_added", department_id, format!("將「{who}」加入部門「{}」（{role}）", dept.name))
                    .label(dept.name.clone())
                    .payload(base),
            )
            .await?;
        }
        // 沒有任何改變就不記（稽核不該有 noise）。
        Some(_) => {}
    }
    Ok(DepartmentMemberView { user_id, name: label, role: role.into() })
}

async fn remove_department_member(tx: &mut Tx<'_>, ctx: &Ctx, department_id: i64, user_id: i64) -> ApiResult<u64> {
    let dept = load_in_org(tx, &ctx.access, department_id).await?;
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT role FROM mn_energy_department_member WHERE department_id = $1 AND user_id = $2 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(department_id)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?;
    let removed = sqlx::query(
        "UPDATE mn_energy_department_member SET deleted_at = $3 WHERE department_id = $1 AND user_id = $2 AND deleted_at IS NULL",
    )
    .bind(department_id)
    .bind(user_id)
    .bind(ctx.now())
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if removed > 0 {
        let label = user_name(tx, user_id).await?;
        let who = label.clone().unwrap_or_else(|| format!("#{user_id}"));
        let mut payload = json!({ "departmentKey": dept.key, "departmentName": dept.name, "memberUserId": user_id });
        if let Some(l) = label {
            payload["memberLabel"] = json!(l);
        }
        if let Some(r) = existing {
            payload["role"] = json!(r);
        }
        audit::record(
            tx,
            ctx,
            AuditInput::new("department.member_removed", department_id, format!("將「{who}」移出部門「{}」", dept.name))
                .label(dept.name.clone())
                .payload(payload),
        )
        .await?;
    }
    Ok(removed)
}

// ── handlers ──

pub async fn list(core: web::Data<CoreRef>, user: CurrentUser, org: web::Path<i64>) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::GOVERN_ORG).await?;
    let mut tx = core.pool.begin().await?;
    let items = list_departments(&mut tx, &access).await?;
    Ok(HttpResponse::Ok().json(json!({ "items": items })))
}

pub async fn create(
    core: web::Data<CoreRef>,
    user: CurrentUser,
    meta: RequestMeta,
    org: web::Path<i64>,
    body: web::Json<Value>,
) -> ApiResult<HttpResponse> {
    let access = core.rbac.authorize_org(&user, org.into_inner(), gates::GOVERN_ORG).await?;
    let input: CreateDepartmentInput = parse_body(&body)?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    let view = create_department(&mut tx, &ctx, &input).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(view))
}

pub async fn update(
    core: web::Data<CoreRef>,
    user: CurrentUser,
    meta: RequestMeta,
    path: web::Path<(i64, i64)>,
    body: web::Json<Value>,
) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::GOVERN_ORG).await?;
    let input: UpdateDepartmentInput = parse_body(&body)?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    let view = update_department(&mut tx, &ctx, id, &input).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(view))
}

pub async fn delete(core: web::Data<CoreRef>, user: CurrentUser, meta: RequestMeta, path: web::Path<(i64, i64)>) -> ApiResult<HttpResponse> {
    let (org, id) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::GOVERN_ORG).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    let removed = delete_department(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "removed": removed })))
}

#[derive(Debug, serde::Deserialize)]
struct SetMemberBody {
    #[serde(default)]
    role: DepartmentMemberRole,
}

pub async fn set_member(
    core: web::Data<CoreRef>,
    user: CurrentUser,
    meta: RequestMeta,
    path: web::Path<(i64, i64, i64)>,
    body: Option<web::Json<Value>>,
) -> ApiResult<HttpResponse> {
    let (org, dept, member) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::GOVERN_ORG).await?;
    let body = body.map(|b| b.into_inner()).unwrap_or(json!({}));
    let b: SetMemberBody =
        serde_json::from_value(body).map_err(|e| crate::common::err_d(400, "invalid_body", json!({ "message": e.to_string() })))?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    let view = set_department_member(&mut tx, &ctx, dept, member, b.role).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(view))
}

pub async fn remove_member(
    core: web::Data<CoreRef>,
    user: CurrentUser,
    meta: RequestMeta,
    path: web::Path<(i64, i64, i64)>,
) -> ApiResult<HttpResponse> {
    let (org, dept, member) = path.into_inner();
    let access = core.rbac.authorize_org(&user, org, gates::GOVERN_ORG).await?;
    let ctx = Ctx::new(&core, access, &meta);
    let mut tx = core.pool.begin().await?;
    let removed = remove_department_member(&mut tx, &ctx, dept, member).await?;
    tx.commit().await?;
    Ok(HttpResponse::Ok().json(json!({ "removed": removed })))
}
