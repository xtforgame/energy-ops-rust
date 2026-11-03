//! 辨識驅動：把已上傳的原檔送去台電辨識服務，結果寫回 item，再刪掉服務那一側的副本
//! （照 App 5 `recognition/RecognitionService.ts` 與 `platform/recognitionDriver.ts`）。
//!
//! 送出之後不在請求裡輪詢：每次 tick 只問服務一次（批次詳情被讀取時對那一批問一次、
//! 背景任務定期對所有辨識中的批次各問一次）；同一批同時只有一個呼叫在處理（批次 data 上的原子租約）。
//!
//! 硬規則：服務掛掉不可以讓收件失敗（檔案已經在了，退回 `uploaded` 由對帳補送）；
//! 辨識失敗絕不 fallback 成假結果；輪詢一定有終點（時間預算，超過就 failBatch）。

use super::service::{self, recompute_batch_status};
use crate::attachments::service as attachments;
use crate::audit::{self, AuditInput};
use crate::common::{err_d, Ctx, Tx};
use crate::tpr_client::{TprClient, TprError, TprUploadFile};
use chrono::{DateTime, Duration, Utc};
use eo_domain::document_ingest::is_recognizable_mime;
use eo_domain::taipower_canonical::{is_tpr_terminal, CanonicalDocument, TPR_PASSWORD_REQUIRED};
use eo_domain::taipower_extractor::as_canonical_document;
use eo_platform::core::CoreRef;
use eo_platform::error::{ApiError, ApiResult};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};

/// 每份檔給的時間上限（兩頁式高壓單約 25 秒，留 4 倍餘裕）與最低預算。
pub const BUDGET_PER_FILE: Duration = Duration::seconds(120);
pub const MIN_BUDGET: Duration = Duration::seconds(120);
/// 租約長度：一次 tick 做不完這麼久就讓下一個人接手。
pub const LEASE: Duration = Duration::minutes(3);
/// 停在 `uploading` 多久算卡住、由對帳重送。
pub const RESEND_AFTER: Duration = Duration::minutes(10);

pub fn client(core: &CoreRef) -> Option<TprClient> {
    TprClient::from_config(&core.config)
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecognizeOutcome {
    /// 第一批的 jobId（之後幾批由 tick 依序送）。
    pub job_id: Option<String>,
    pub item_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunks: Option<usize>,
    /// 沒送出去的原因（服務沒設定／連不上／沒有待辨識的檔案）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
}

/// 重送也不會成功的錯誤（檔案本身的問題）：退回 `uploaded` 會變成無限重試。
fn is_permanent(e: &TprError) -> bool {
    matches!(e.status, 400 | 413 | 415) || e.code == "unsupported_file"
}

/// 「辨識沒能完成」是沒有人會主動發現的狀態 —— 這一筆是唯一會叫的東西，fail-closed、自己一個交易。
async fn record_failure(ctx: &Ctx, batch_id: i64, code: &str, message: &str, item_count: usize, job_id: Option<&str>) -> ApiResult<()> {
    let mut tx = ctx.core.pool.begin().await?;
    let mut payload = json!({ "code": code, "message": message, "itemCount": item_count });
    if let Some(j) = job_id {
        payload["jobId"] = json!(j);
    }
    audit::record(
        &mut tx,
        ctx,
        AuditInput::new("ingest.recognition_failed", batch_id, format!("辨識沒能完成（{code}）"))
            .label(format!("批次 #{batch_id}"))
            .outcome("failed")
            .payload(payload),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn recognizing_items(tx: &mut Tx<'_>, batch_id: i64) -> ApiResult<Vec<i64>> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM tbl_energy_ingest_item WHERE batch_id = $1 AND status = 'recognizing' AND deleted_at IS NULL ORDER BY id",
    )
    .bind(batch_id)
    .fetch_all(&mut **tx)
    .await?)
}

/// 整批沒能辨識完：item 退回 `uploaded`（檔案沒問題，是我們沒辨識完）；永久錯誤則標 `failed` 並說出原因。
pub async fn fail_batch(ctx: &Ctx, batch_id: i64, job_id: Option<&str>, code: &str, message: Option<&str>, permanent: bool) -> ApiResult<()> {
    let mut tx = ctx.core.pool.begin().await?;
    let ids = recognizing_items(&mut tx, batch_id).await?;
    if !ids.is_empty() {
        if permanent {
            sqlx::query(
                "UPDATE tbl_energy_ingest_item SET status = 'failed', error_message = $2, data = data || $3
                  WHERE id = ANY($1) AND deleted_at IS NULL",
            )
            .bind(&ids)
            .bind(message.unwrap_or(code))
            .bind(json!({ "recognitionErrorCode": code }))
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query("UPDATE tbl_energy_ingest_item SET status = 'uploaded' WHERE id = ANY($1) AND deleted_at IS NULL")
                .bind(&ids)
                .execute(&mut *tx)
                .await?;
        }
    }
    recompute_batch_status(&mut tx, batch_id).await?;
    tx.commit().await?;
    record_failure(ctx, batch_id, code, message.unwrap_or(code), ids.len(), job_id).await
}

struct Candidate {
    item_id: i64,
    attachment_id: i64,
    file_name: String,
    password: Option<String>,
}

/// 把這一批還沒辨識的檔案送出去（只送第一批，剩下的由 tick 依序送）。回得很快。
/// `passwords`：item id → 密碼（受保護的 PDF）。密碼只在這一次請求的記憶體裡，不寫 DB、不進稽核。
pub async fn recognize_batch(ctx: &Ctx, batch_id: i64, passwords: &HashMap<i64, String>) -> ApiResult<RecognizeOutcome> {
    let mut tx = ctx.core.pool.begin().await?;
    service::batch_in_org(&mut tx, ctx.org_id(), batch_id).await?;
    let items: Vec<(i64, Option<i64>, Option<String>)> = sqlx::query_as(
        "SELECT id, attachment_id, original_filename FROM tbl_energy_ingest_item
          WHERE batch_id = $1 AND status = 'uploaded' AND deleted_at IS NULL ORDER BY id",
    )
    .bind(batch_id)
    .fetch_all(&mut *tx)
    .await?;
    drop(tx);
    if items.is_empty() {
        return Ok(RecognizeOutcome { skipped: Some("nothing_to_recognize".into()), ..Default::default() });
    }
    let Some(tpr) = client(&ctx.core) else {
        // 沒設定不是壞了，是這個環境沒有接辨識服務。檔案留著，狀態不動。
        record_failure(ctx, batch_id, "tpr_not_configured", "未設定辨識服務位址", items.len(), None).await?;
        return Ok(RecognizeOutcome { item_count: items.len(), skipped: Some("tpr_not_configured".into()), ..Default::default() });
    };

    let candidates: Vec<Candidate> = items
        .into_iter()
        .filter_map(|(id, att, name)| {
            att.map(|a| Candidate {
                item_id: id,
                attachment_id: a,
                file_name: name.unwrap_or_else(|| format!("file-{id}")),
                password: passwords.get(&id).cloned(),
            })
        })
        .collect();
    let att_ids: Vec<i64> = candidates.iter().map(|c| c.attachment_id).collect();
    let mimes: HashMap<i64, String> = if att_ids.is_empty() {
        HashMap::new()
    } else {
        sqlx::query_as::<_, (i64, String)>("SELECT id, mime_type FROM tbl_energy_attachment WHERE id = ANY($1) AND deleted_at IS NULL")
            .bind(&att_ids)
            .fetch_all(&ctx.core.pool)
            .await?
            .into_iter()
            .collect()
    };
    // 辨識服務收不下的檔案（HEIC）在送出去之前就擋掉 —— 送過去會拿到永久的 415。
    let mut pending = Vec::new();
    for c in candidates {
        let mime = mimes.get(&c.attachment_id).cloned().unwrap_or_default();
        if !mime.is_empty() && !is_recognizable_mime(&mime) {
            let mut tx = ctx.core.pool.begin().await?;
            service::mark_item_failed(
                &mut tx,
                ctx,
                c.item_id,
                &format!("辨識服務不支援 {mime}（只收 PDF／PNG／JPG）。請轉成 PDF 或 JPG 後重傳，或直接在這一列指定單別，自己把欄位填上去。"),
                Some("unsupported_for_recognition"),
            )
            .await?;
            tx.commit().await?;
            continue;
        }
        pending.push(c);
    }
    if pending.is_empty() {
        return Ok(RecognizeOutcome { skipped: Some("no_readable_files".into()), ..Default::default() });
    }

    // 拆成多個 job 依序送：服務是單工佇列，整包送出去會把別的組織卡住十幾分鐘。
    let size = ctx.core.config.tpr_batch_size.clamp(1, 50);
    let total = pending.len();
    let chunks = total.div_ceil(size);
    {
        let mut tx = ctx.core.pool.begin().await?;
        let ids: Vec<i64> = pending.iter().map(|p| p.item_id).collect();
        service::mark_items_recognizing(&mut tx, &ids).await?;
        recompute_batch_status(&mut tx, batch_id).await?;
        tx.commit().await?;
    }
    let first: Vec<Candidate> = pending.into_iter().take(size).collect();
    match send_chunk(ctx, &tpr, batch_id, &first).await {
        Ok(job_id) => Ok(RecognizeOutcome { job_id: Some(job_id), item_count: total, chunks: Some(chunks), skipped: None }),
        Err(e) => {
            // 第一批就送不出去：退回 `uploaded` 讓對帳再送；但永久錯誤標 failed（否則畫面永遠停在「辨識中」）。
            let permanent = is_permanent(&e);
            fail_batch(ctx, batch_id, None, &e.code, Some(&e.message), permanent).await?;
            Ok(RecognizeOutcome { job_id: None, item_count: total, chunks: Some(chunks), skipped: Some(e.code) })
        }
    }
}

/// 送一批出去：讀原檔、建 job、把 jobId 記在批次上、留一筆稽核。不動 item 狀態（已一次標好）。
async fn send_chunk(ctx: &Ctx, tpr: &TprClient, batch_id: i64, chunk: &[Candidate]) -> Result<String, TprError> {
    let internal = |e: ApiError| TprError::new("tpr_error", 500, e.to_string());
    let mut files = Vec::new();
    for c in chunk {
        match attachments::read_bytes(ctx, c.attachment_id).await {
            Ok((row, bytes)) => files.push(TprUploadFile {
                // 來歷直接穿透：sourceFileId ＝ 我們的 item id。
                source_file_id: c.item_id.to_string(),
                file_name: c.file_name.clone(),
                bytes,
                content_type: row.mime_type,
                password: c.password.clone().filter(|p| !p.is_empty()),
            }),
            Err(_) => {
                // 讀不回原檔就標在那一份上，不讓整批倒。
                let mut tx = ctx.core.pool.begin().await.map_err(|e| internal(e.into()))?;
                service::mark_item_failed(&mut tx, ctx, c.item_id, "讀不到已上傳的原始檔", Some("attachment_unreadable"))
                    .await
                    .map_err(internal)?;
                tx.commit().await.map_err(|e| internal(e.into()))?;
            }
        }
    }
    if files.is_empty() {
        return Err(TprError::new("no_readable_files", 502, "這一批的原始檔都讀不回來"));
    }
    let with_password = files.iter().filter(|f| f.password.is_some()).count();
    let file_count = files.len();
    let summary = tpr.create_job(files).await?;
    let job_id = summary.job_id;
    let record = async {
        let mut tx = ctx.core.pool.begin().await?;
        let data: Value = sqlx::query_scalar("SELECT data FROM tbl_energy_ingest_batch WHERE id = $1 FOR UPDATE")
            .bind(batch_id)
            .fetch_one(&mut *tx)
            .await?;
        let mut ids: Vec<Value> = data.get("recognitionJobIds").and_then(Value::as_array).cloned().unwrap_or_default();
        ids.push(json!(job_id));
        let now = ctx.now();
        let started = data.get("recognitionStartedAt").filter(|v| v.is_string()).cloned().unwrap_or_else(|| json!(now));
        sqlx::query("UPDATE tbl_energy_ingest_batch SET data = data || $2 WHERE id = $1")
            .bind(batch_id)
            .bind(json!({
                // 目前這一批（tick 靠它問進度）；全部的 jobId 也留著：出事時查得到分成幾次送。
                "recognitionJobId": job_id,
                "recognitionJobIds": ids,
                "recognitionStartedAt": started,
                "recognitionChunkStartedAt": now,
            }))
            .execute(&mut *tx)
            .await?;
        audit::record(
            &mut tx,
            ctx,
            AuditInput::new("ingest.recognition_requested", batch_id, format!("把 {file_count} 份檔案送去辨識"))
                .label(format!("批次 #{batch_id}"))
                .payload(json!({ "jobId": job_id, "fileCount": file_count, "withPassword": with_password })),
        )
        .await?;
        tx.commit().await?;
        Ok::<(), ApiError>(())
    };
    record.await.map_err(internal)?;
    Ok(job_id)
}

/// 取結果 → 逐份寫回 → 刪掉服務那一側的副本。
async fn consume(ctx: &Ctx, tpr: &TprClient, batch_id: i64, job_id: &str, elapsed_ms: i64) -> ApiResult<()> {
    let result = match tpr.get_result_raw(job_id).await {
        Ok(r) => r,
        Err(e) => return fail_batch(ctx, batch_id, Some(job_id), &e.code, Some(&e.message), false).await,
    };
    let docs = result.get("documents").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut by_template: BTreeMap<String, u64> = BTreeMap::new();
    let mut failed = 0usize;
    let mut blocking = 0usize;
    for raw in &docs {
        let Some(item_id) = raw.get("sourceFileId").and_then(Value::as_str).and_then(|s| s.parse::<i64>().ok()) else { continue };
        let mut tx = ctx.core.pool.begin().await?;
        let outcome = match as_canonical_document(raw) {
            Some(CanonicalDocument::Failed(f)) => {
                failed += 1;
                // 需要密碼與檔案損壞是兩件事：前者使用者做得了什麼。
                let message = if f.error.code == TPR_PASSWORD_REQUIRED {
                    format!("{}（請提供密碼後重新辨識）", f.error.message)
                } else {
                    f.error.message.clone()
                };
                service::mark_item_failed(&mut tx, ctx, item_id, &message, Some(&f.error.code)).await.map(|_| ())
            }
            Some(CanonicalDocument::Succeeded(s)) => {
                let t = s.record.template_id.clone().unwrap_or_else(|| "unknown".into());
                *by_template.entry(t).or_insert(0) += 1;
                blocking += s.issues.iter().filter(|i| i.blocking).count();
                service::apply_recognition_to_item(&mut tx, ctx, item_id, raw).await.map(|_| ())
            }
            None => {
                failed += 1;
                service::mark_item_failed(&mut tx, ctx, item_id, "辨識服務回傳的結果無法解讀", Some("tpr_bad_document")).await.map(|_| ())
            }
        };
        match outcome {
            Ok(()) => tx.commit().await?,
            // 某一份寫不回去（例如 item 已不在本組織）只影響那一份。
            Err(e) => log::warn!("[energy][recognition] 批次 {batch_id} item {item_id} 寫回失敗：{e}"),
        }
    }
    let mut tx = ctx.core.pool.begin().await?;
    audit::record(
        &mut tx,
        ctx,
        AuditInput::new(
            "ingest.recognition_completed",
            batch_id,
            format!("辨識完成，{} 份成功、{failed} 份失敗", docs.len() - failed),
        )
        .label(format!("批次 #{batch_id}"))
        .outcome(if failed > 0 { "failed" } else { "success" })
        .payload(json!({
            "jobId": job_id,
            "documents": docs.len(),
            "failed": failed,
            "byTemplate": by_template,
            "blockingIssues": blocking,
            "elapsedMs": elapsed_ms.max(0),
        })),
    )
    .await?;
    tx.commit().await?;
    // 刪掉服務上的第二份原始檔；失敗不 throw，但一定要留痕。
    let ok = tpr.delete_job(job_id).await;
    let _ = audit::record_best_effort(
        &ctx.core.pool,
        ctx,
        AuditInput::new(
            "ingest.recognition_purged",
            batch_id,
            if ok { "清掉辨識服務上留的原始檔副本" } else { "辨識服務上的原始檔副本**沒有刪成功**" },
        )
        .label(format!("批次 #{batch_id}"))
        .outcome(if ok { "success" } else { "failed" })
        .payload(json!({ "jobId": job_id, "ok": ok })),
    )
    .await;
    Ok(())
}

/// 原子租約：同一批同時只有一個呼叫在處理。
async fn claim(core: &CoreRef, batch_id: i64) -> ApiResult<bool> {
    let now = core.clock.now();
    let until = now + LEASE;
    let hit: Option<i64> = sqlx::query_scalar(
        "UPDATE tbl_energy_ingest_batch
            SET data = jsonb_set(COALESCE(data, '{}'::jsonb), '{driverLeaseUntil}', to_jsonb($2::text))
          WHERE id = $1 AND deleted_at IS NULL
            AND (data->>'driverLeaseUntil' IS NULL OR (data->>'driverLeaseUntil')::timestamptz < $3)
          RETURNING id",
    )
    .bind(batch_id)
    .bind(until.to_rfc3339())
    .bind(now)
    .fetch_optional(&core.pool)
    .await?;
    Ok(hit.is_some())
}

async fn release(core: &CoreRef, batch_id: i64) {
    if let Err(e) = sqlx::query("UPDATE tbl_energy_ingest_batch SET data = data - 'driverLeaseUntil' WHERE id = $1")
        .bind(batch_id)
        .execute(&core.pool)
        .await
    {
        log::error!("[energy][recognition] 釋放租約失敗 batch={batch_id}: {e}");
    }
}

fn parse_time(v: Option<&Value>) -> Option<DateTime<Utc>> {
    v.and_then(Value::as_str).and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|t| t.with_timezone(&Utc))
}

/// 一批：問服務一次，到終態就寫回。回傳做了什麼（給 log 與測試）。
pub async fn tick_batch(core: &CoreRef, batch_id: i64) -> ApiResult<&'static str> {
    let Some(tpr) = client(core) else { return Ok("not_configured") };
    if !claim(core, batch_id).await? {
        return Ok("leased");
    }
    let r = tick_claimed(core, &tpr, batch_id).await;
    release(core, batch_id).await;
    r
}

async fn tick_claimed(core: &CoreRef, tpr: &TprClient, batch_id: i64) -> ApiResult<&'static str> {
    let row: Option<(String, Value, i64)> =
        sqlx::query_as("SELECT status, data, organization_id FROM tbl_energy_ingest_batch WHERE id = $1 AND deleted_at IS NULL")
            .bind(batch_id)
            .fetch_optional(&core.pool)
            .await?;
    let Some((status, data, org_id)) = row else { return Ok("not_recognizing") };
    if status != "recognizing" {
        return Ok("not_recognizing");
    }
    let Some(job_id) = data.get("recognitionJobId").and_then(Value::as_str).map(String::from) else { return Ok("no_job") };
    let ctx = Ctx::system(core, org_id);
    let now = core.clock.now();
    let started = parse_time(data.get("recognitionChunkStartedAt")).or_else(|| parse_time(data.get("recognitionStartedAt"))).unwrap_or(now);
    let elapsed = now - started;
    let recognizing = {
        let mut tx = core.pool.begin().await?;
        recognizing_items(&mut tx, batch_id).await?.len()
    };
    let budget = std::cmp::max(MIN_BUDGET, BUDGET_PER_FILE * recognizing as i32);

    let status = match tpr.get_job(&job_id).await {
        Ok(s) => s.status,
        Err(e) => {
            if elapsed > budget {
                fail_batch(&ctx, batch_id, Some(&job_id), &e.code, Some(&e.message), false).await?;
                return Ok("failed_unreachable");
            }
            return Ok("retry_later");
        }
    };
    if !is_tpr_terminal(status.as_str()) {
        if elapsed > budget {
            let msg = format!("辨識超過 {} 秒仍未完成", budget.num_seconds());
            fail_batch(&ctx, batch_id, Some(&job_id), "tpr_timeout", Some(&msg), false).await?;
            return Ok("failed_timeout");
        }
        return Ok("running");
    }

    consume(&ctx, tpr, batch_id, &job_id, elapsed.num_milliseconds()).await?;

    // 超過一批的量：剩下的還標著 recognizing → 退回 uploaded 再送下一批。
    let left = {
        let mut tx = core.pool.begin().await?;
        let left = recognizing_items(&mut tx, batch_id).await?;
        if !left.is_empty() {
            sqlx::query("UPDATE tbl_energy_ingest_item SET status = 'uploaded' WHERE id = ANY($1) AND deleted_at IS NULL")
                .bind(&left)
                .execute(&mut *tx)
                .await?;
            recompute_batch_status(&mut tx, batch_id).await?;
            sqlx::query("UPDATE tbl_energy_ingest_batch SET data = data || '{\"recognitionJobId\": null}'::jsonb WHERE id = $1")
                .bind(batch_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        left
    };
    if !left.is_empty() {
        recognize_batch(&ctx, batch_id, &HashMap::new()).await?;
        return Ok("consumed_next_chunk");
    }
    Ok("consumed")
}

/// 背景對帳一輪：所有辨識中的批次各問一次；停在 `uploading` 超過寬限期的照原規則重送。
pub async fn tick_all(core: &CoreRef) -> ApiResult<BTreeMap<String, u64>> {
    let mut out: BTreeMap<String, u64> = BTreeMap::new();
    let mut bump = |k: &str| *out.entry(k.to_string()).or_insert(0) += 1;
    if client(core).is_none() {
        bump("not_configured");
        return Ok(out);
    }
    let recognizing: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM tbl_energy_ingest_batch WHERE status = 'recognizing' AND deleted_at IS NULL ORDER BY id LIMIT 50",
    )
    .fetch_all(&core.pool)
    .await?;
    for id in recognizing {
        match tick_batch(core, id).await {
            Ok(k) => bump(k),
            Err(e) => {
                log::error!("[energy][recognition] tick batch={id}: {e}");
                bump("error");
            }
        }
    }
    let older_than = core.clock.now() - RESEND_AFTER;
    let uploading: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT b.id, b.organization_id FROM tbl_energy_ingest_batch b
          WHERE b.status = 'uploading' AND b.submitted_at < $1 AND b.deleted_at IS NULL
            AND EXISTS (SELECT 1 FROM tbl_energy_ingest_item i WHERE i.batch_id = b.id AND i.status = 'uploaded' AND i.deleted_at IS NULL)
          ORDER BY b.id LIMIT 50",
    )
    .bind(older_than)
    .fetch_all(&core.pool)
    .await?;
    for (id, org) in uploading {
        if !claim(core, id).await? {
            continue;
        }
        let ctx = Ctx::system(core, org);
        match recognize_batch(&ctx, id, &HashMap::new()).await {
            Ok(_) => bump("resent"),
            Err(e) => {
                log::error!("[energy][recognition] resend batch={id}: {e}");
                bump("error");
            }
        }
        release(core, id).await;
    }
    Ok(out)
}

/// 辨識服務有沒有設定、現在能不能用。
pub async fn status(core: &CoreRef) -> Value {
    let mut m = Map::new();
    match client(core) {
        None => {
            m.insert("configured".into(), json!(false));
            m.insert("available".into(), json!(false));
            m.insert("reason".into(), json!("tpr_not_configured"));
        }
        Some(c) => {
            let ok = c.ready().await;
            m.insert("configured".into(), json!(true));
            m.insert("available".into(), json!(ok));
            if !ok {
                m.insert("reason".into(), json!("tpr_not_ready"));
            }
        }
    }
    m.insert("batchSize".into(), json!(core.config.tpr_batch_size));
    m.insert("tickSeconds".into(), json!(core.config.recognition_tick_secs));
    Value::Object(m)
}

/// 背景任務：每 `secs` 秒對帳一輪。
pub fn spawn_background(core: CoreRef, secs: u64) -> actix_web::rt::task::JoinHandle<()> {
    actix_web::rt::spawn(async move {
        let mut iv = actix_web::rt::time::interval(std::time::Duration::from_secs(secs));
        loop {
            iv.tick().await;
            match tick_all(&core).await {
                Ok(r) if !r.is_empty() => log::info!("[energy][recognition] reconcile {r:?}"),
                Ok(_) => {}
                Err(e) => log::error!("[energy][recognition] reconcile failed: {e}"),
            }
        }
    })
}

/// 整理密碼表（body 的 `passwords: { "<itemId>": "..." }`）。
pub fn parse_passwords(body: &Value) -> ApiResult<HashMap<i64, String>> {
    let mut out = HashMap::new();
    match body.get("passwords") {
        None | Some(Value::Null) => {}
        Some(Value::Object(m)) => {
            for (k, v) in m {
                if let (Ok(id), Some(p)) = (k.parse::<i64>(), v.as_str()) {
                    if !p.is_empty() {
                        out.insert(id, p.to_string());
                    }
                }
            }
        }
        Some(_) => return Err(err_d(400, "invalid_body", json!({ "field": "passwords" }))),
    }
    Ok(out)
}
