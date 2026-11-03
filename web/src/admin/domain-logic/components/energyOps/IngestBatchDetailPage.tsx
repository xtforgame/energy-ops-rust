import * as React from 'react';
import {
  Box, Card, CardContent, Typography, Button, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, Chip, Alert, MenuItem, TextField, Tooltip, IconButton, Collapse, Divider,
  Dialog, DialogTitle, DialogContent, DialogActions,
} from '@mui/material';
import ArrowBackIcon from '@mui/icons-material/ArrowBack';
import RefreshIcon from '@mui/icons-material/Refresh';
import DownloadIcon from '@mui/icons-material/Download';
import LinkIcon from '@mui/icons-material/Link';
import CheckCircleIcon from '@mui/icons-material/CheckCircle';
import RemoveCircleOutlineIcon from '@mui/icons-material/RemoveCircleOutline';
import { energyHttp, energyBase, attachmentDownloadUrl } from '@/api/energy';
import { useNotify, useTranslate } from 'react-admin';
import FieldDiffGrid, { billFieldLabelKey, type ReviewField } from './FieldDiffGrid';
import BillDocTypeField from './BillDocTypeField';
import RecognizingIndicator from './RecognizingIndicator';
import { BILL_DOC_TYPES } from 'common/plugins/EnergyOpsCommonPlugin/documentIngest';
import {
  billReviewFieldsFor,
  ALL_BILL_REVIEW_FIELDS,
  sourceFieldBoxes,
  type RecognitionSummary,
} from 'common/plugins/EnergyOpsCommonPlugin/taipowerExtractor';
import { useParams, useNavigate, useSearchParams } from 'react-router-dom';
import { useSelection } from '@/platform/selection/store';
import { apiErrorCode, apiErrorPayload } from '../../../dataProvider/apiError';
import { EnergyPageFrame, Money, jvTokens, SourcePreviewFrame } from './shared';
import RecognitionNote from './RecognitionNote';
import { billRoute, executionRoute, ingestListRoute } from './routes';

/** 校對要並排的五個欄位 —— 與 P3 校對台**同一組**（`BillIngestService.changedFields`）。 */
// 欄位清單與那一格的呈現規則都在 `FieldDiffGrid` —— 這裡只是消費它。
const NUMERIC_FIELDS: string[] = ['totalAmount', 'generationKwh'];
/** 只有日期要截前 10 碼 —— 電號有 15 碼，用「其餘」當規則會把它切斷。 */
const DATE_FIELDS: string[] = ['billingPeriodStart', 'billingPeriodEnd', 'issuedAt', 'dueDate'];

interface IngestItem {
  id: number;
  batchId: number;
  originalFilename: string;
  docType: string | null;
  workflowKey: string | null;
  classifyReason: string | null;
  attachmentId: number | null;
  billId: number | null;
  installmentId: number | null;
  executionId: number | null;
  status: string;
  errorMessage: string | null;
  amountMismatch: boolean;
  alreadyPaid: boolean;
  extracted: any;
  /** 由 `decorateItems()` 補的（見 `IngestService`）。 */
  subjectType: string | null;
  subjectId: number | null;
  documentNo: string | null;
  businessKey: string | null;
  approvalState: string | null;
  stationId: number | null;
  reviewStatus: string | null;
  duplicateOfItemId: number | null;
  duplicateOfSeq: number | null;
  /** 辨識結果的摘要（`itemView`）——整包 `ocrRaw` 不在列表裡。 */
  recognition: RecognitionSummary | null;
  recognitionErrorCode: string | null;
}
interface Batch {
  id: number;
  submittedAt: string;
  submittedByName: string | null;
  fileCount: number;
  status: string;
  pendingCount: number;
  items: IngestItem[];
}
interface StationOption { id: number; name: string; customerNumber: string; status: string }

/**
 * 手動指定型別時可選的（`unclassified` 不在選項裡 —— 那是「還沒指定」不是一種型別）。
 *
 * **四種電費單全部列出來**：它們走的流程不一樣（有餘電要開發票、
 * 其餘三種是我們付台電），只給一個籠統的「電費單」等於要人事後再改一次。
 */
const ASSIGNABLE_DOC_TYPES = [...BILL_DOC_TYPES, 'depositRequest'];

/**
 * 狀態 → 左側色條。
 *
 * **缺漏與錯誤是兩種顏色**：金＝資料**不完整**（補上就好）；
 * 燒橘＝**要人判斷**（對不上、辨識失敗）。
 * 混成同一種紅，使用者就得每一筆都點開才知道要做什麼。
 */
/**
 * 「還在等辨識」的狀態 —— 這幾個狀態下**沒有任何事情要人做**，
 * 所以畫面要說的是「它還活著」，不是「請處理」。
 * `uploaded` 也算：檔案進來了但服務還沒輪到它。
 */
const RECOGNIZING_STATES = ['uploading', 'recognizing', 'classifying', 'uploaded'];

const STATUS_BAR: Record<string, string> = {
  extracted: jvTokens.ok,
  classified: jvTokens.ok,
  unclassified: jvTokens.gold,
  unmatched: jvTokens.warn,
  failed: jvTokens.warn,
  submitted: jvTokens.primary,
  discarded: jvTokens.muted,
};

/** 送審被擋的三種重複 —— 三句話**不可共用**（見元件本體的說明）。 */
const DUP_CODES = ['duplicate_document', 'already_in_review', 'duplicate_in_batch'];

/**
 * **送件詳情 ＝ 校對工作區**（`LIFECYCLE.md §6.5`／`§7.2`）。
 *
 * ### 為什麼校對是「就地展開」而不是換頁
 *
 * **草稿屬於批次** —— 離開批次，草稿就沒有家。
 * 點「校對」在那一列下方展開左右分欄，URL 寫 `?item=<id>`（可分享、refresh 回得來），
 * 但**不換路由**。
 *
 * ### 送審之後那一筆離開校對區
 *
 * 校對區只管「**還沒送出去的**」。送審成功的變成下方的一行
 * `檔名 → 單號 [看流程單]`。留在校對區只會讓人每天重新確認一次「這筆已經好了」。
 *
 * ### 三種重複，三句話
 *
 * | 碼 | 情況 | 為什麼不能共用同一句 |
 * |---|---|---|
 * | `duplicate_in_batch` | 同一批裡兩張同一張紙 | 它還在**這一頁上**，指得出「本批第幾筆」 |
 * | `already_in_review` | 別的地方有一筆**正在跑** | 它**還可能被退回** —— 說成「已經送審過了」會讓人以為結案了 |
 * | `duplicate_document` | 已經有一張**通過**了 | 這才是真的結案，該做的是捨棄手上這一份 |
 *
 * 三種都要 `[看那一筆]` ＋ `[捨棄這一份]`：只說「被擋了」而不給出口，
 * 使用者唯一能做的就是關掉視窗然後困惑。
 *
 * ### 例外件在這裡就地指認
 *
 * 電號對不上 → 那一列出現 `[指認案場]` → 開 dialog（**不跳去例外件頁**）。
 * 指認完 dialog 關掉，那一列**就地**變成可校對。
 */
const IngestBatchDetailPage: React.FC = () => {
  const notify = useNotify();
  const translate = useTranslate();
  const navigate = useNavigate();
  const { id } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const orgId = useSelection()?.organizationId;
  const batchId = Number(id);

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [batch, setBatch] = React.useState<Batch | null>(null);

  /** 目前展開校對的那一列（`?item=` 是它的鏡像，不是另一個路由）。 */
  const expandedId = searchParams.get('item') ? Number(searchParams.get('item')) : null;
  const [bill, setBill] = React.useState<any>(null);
  const [draft, setDraft] = React.useState<Partial<Record<ReviewField, string>>>({});
  const [saving, setSaving] = React.useState(false);
  /**
   * 點欄位 → 左邊那張紙把它框起來。
   *
   * 這一頁原本刻意不做高亮（「高亮綁在校對台的欄位列上」），
   * 但**第一次校對就發生在這裡** —— 兩個入口做同一件事卻只有一邊看得到位置，
   * 使用者會以為這一頁的辨識比較差。
   */
  const [focusField, setFocusField] = React.useState<string | null>(null);

  // 指認案場 dialog
  const [matchTarget, setMatchTarget] = React.useState<IngestItem | null>(null);
  const [query, setQuery] = React.useState('');
  const [options, setOptions] = React.useState<StationOption[]>([]);
  const [picked, setPicked] = React.useState('');
  const [matchError, setMatchError] = React.useState<string | null>(null);

  // 重複 dialog
  const [dup, setDup] = React.useState<{ item: IngestItem; code: string; details: any } | null>(null);

  const base = energyBase(orgId);

  /**
   * @param silent 背景輪詢用 —— **不要**掀起整頁的載入遮罩。
   *
   * 辨識中每 3 秒重讀一次，而每一次都 `setLoading(true)` 的話，
   * 整頁會每三秒閃一次全螢幕遮罩（截圖抓到的）——
   * 使用者要的是「安心等待」，而一直閃的遮罩正好相反：
   * 它讓畫面看起來一直在重載，而且那幾百毫秒裡什麼都點不了。
   */
  const load = React.useCallback(async (silent = false) => {
    if (!orgId || !Number.isInteger(batchId)) return;
    if (!silent) setLoading(true);
    setDenied(false);
    try {
      const { data } = await energyHttp.get(`${energyBase(orgId)}/ingest-batches/${batchId}`);
      setBatch(data);
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
    } finally { if (!silent) setLoading(false); }
  }, [orgId, batchId, notify]);

  React.useEffect(() => { load(); }, [load]);

  /**
   * **辨識中就自己重新整理**（`RECOGNITION.md §6.2`）。
   *
   * 辨識是非同步的：上傳完 → item 進 `recognizing` → 服務一份一份跑
   * （L2 三來源，一頁十幾秒）。沒有這一段的話畫面會停在「辨識中」不動，
   * 而使用者唯一能做的是自己按重新整理 —— 那正是「看起來壞掉」的樣子。
   *
   * 只在 `recognizing` 時輪詢，而且**用完就清掉**：
   * 一個永遠不停的 3 秒輪詢會把一個開著沒關的分頁變成打後端的機器。
   */
  React.useEffect(() => {
    if (batch?.status !== 'recognizing') return undefined;
    // 背景重讀 —— 靜默（等待指示已經在說「它還活著」了）。
    const timer = setInterval(() => { load(true); }, 3000);
    return () => clearInterval(timer);
  }, [batch?.status, load]);

  const items = batch?.items ?? [];
  /** 校對區＝還沒有結論的。已送審／已捨棄的下沉成一行摘要。 */
  const open = items.filter((i) => !['submitted', 'discarded'].includes(i.status));
  const closed = items.filter((i) => ['submitted', 'discarded'].includes(i.status));
  const ready = open.filter((i) => ['extracted', 'classified'].includes(i.status) && !i.duplicateOfItemId);
  const blockedCount = open.length - ready.length;

  /**
   * 展開一列時載入它的電費單（辨識值 vs 最終值要有 `ocrExtracted`）。
   * 保證金那類沒有 bill —— 那時 `bill` 留 null，右欄改印期數摘要。
   */
  React.useEffect(() => {
    const item = expandedId != null ? items.find((i) => i.id === expandedId) : null;
    if (!item || item.billId == null || !orgId) { setBill(null); setDraft({}); return; }
    let alive = true;
    energyHttp.get(`${energyBase(orgId)}/bills/${item.billId}`)
      .then(({ data }) => {
        if (!alive) return;
        setBill(data);
        const next: any = {};
        ALL_BILL_REVIEW_FIELDS.forEach((f) => { next[f] = data?.[f] != null ? String(data[f]) : ''; });
        setDraft(next);
      })
      .catch(() => { if (alive) { setBill(null); setDraft({}); } });
    // eslint-disable-next-line consistent-return
    return () => { alive = false; };
    // `items` 每次 load 都是新陣列 —— 用 expandedId 與筆數當依賴即可，
    // 掛上 `items` 會讓每次 reload 都重打一次 bill。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [expandedId, orgId, batch?.id, items.length]);

  const toggleExpand = (itemId: number) => {
    const next = new URLSearchParams(searchParams);
    if (expandedId === itemId) next.delete('item');
    else next.set('item', String(itemId));
    // `replace` —— 展開/收合不該在瀏覽器的上一頁堆一層。
    setSearchParams(next, { replace: true });
  };

  /** 這張單的欄位在紙上的位置（沒有座標的欄位不會出現在裡面）。 */
  const ingestBoxes = React.useMemo(
    () => sourceFieldBoxes(bill?.ocrRaw, billReviewFieldsFor(bill?.templateId)),
    [bill?.ocrRaw, bill?.templateId],
  );

  const patchItem = async (item: IngestItem, patch: any) => {
    try {
      await energyHttp.patch(`${base}/ingest-items/${item.id}`, patch);
      await load();
    } catch (e: any) { notify(apiErrorCode(e) || 'update failed', { type: 'error' }); }
  };

  const saveDraft = async (item: IngestItem, markReviewed = false) => {
    if (item.billId == null) return;
    setSaving(true);
    try {
      const body: any = { markReviewed };
      ALL_BILL_REVIEW_FIELDS.forEach((f) => {
        const raw = draft[f];
        if (raw === undefined) return;
        if (raw === '') { body[f] = null; return; }
        // 日期截前 10 碼、金額轉數字、其餘（電號）原樣送。
        if (NUMERIC_FIELDS.includes(f)) body[f] = Number(raw);
        else if (DATE_FIELDS.includes(f)) body[f] = String(raw).slice(0, 10);
        else body[f] = String(raw).trim();
      });
      await energyHttp.patch(`${base}/bills/${item.billId}`, body);
      /**
       * ⛔ **不可以把 PATCH 的回應當成這張單**（原本是 `setBill(data)`）。
       *
       * `GET /bills/:id` 回的是**加工過**的形狀：`mode` / `canEditSubject` /
       * `docTypeEditable` / `binding.flows` / `executions` 都是它加上去的；
       * PATCH 回的是裸的那一列。把裸的那一列塞進 `bill` 之後：
       *
       * - `canEditSubject` 變 undefined → 欄位與單別**全部變唯讀**（存一次就改不動了）
       * - `binding` 變 undefined → 畫面冒出「這張單目前沒有對應的流程 —— 送審會被退回」，
       *   而那是**假的**（它明明解得到兩條）
       *
       * 而且不會自己好：重讀 `bill` 的 effect 依賴 `items.length`，
       * `load()` 之後筆數沒變 → effect 不會重跑 → 使用者只能重新整理或收合再展開。
       * （2026-09-15 排練解說影片第 2 章時撞到：存完草稿就再也改不動單別。）
       */
      const { data } = await energyHttp.get(`${energyBase(orgId)}/bills/${item.billId}`);
      setBill(data);
      const next: any = {};
      ALL_BILL_REVIEW_FIELDS.forEach((f) => { next[f] = data?.[f] != null ? String(data[f]) : ''; });
      setDraft(next);
      notify('energy.review.saved', { type: 'success' });
      await load();
    } catch (e: any) {
      notify(apiErrorCode(e) || 'save failed', { type: 'error' });
    } finally { setSaving(false); }
  };

  const submitItem = async (item: IngestItem) => {
    try {
      await energyHttp.post(`${base}/ingest-items/${item.id}/submit`, {});
      notify('energy.intake.submitted', { type: 'success' });
      // 送審成功 → 那一列離開校對區，展開狀態一起收掉。
      if (expandedId === item.id) {
        const next = new URLSearchParams(searchParams); next.delete('item');
        setSearchParams(next, { replace: true });
      }
      await load();
    } catch (e: any) {
      const payload = apiErrorPayload(e);
      if (payload && DUP_CODES.includes(payload.code)) {
        setDup({ item, code: payload.code, details: payload.details ?? {} });
        return;
      }
      notify(apiErrorCode(e) || 'submit failed', { type: 'error' });
    }
  };

  const discardItem = async (item: IngestItem) => {
    try {
      await energyHttp.post(`${base}/ingest-items/${item.id}/discard`, { reason: 'duplicate' });
      notify('energy.ingest.discarded', { type: 'success' });
      setDup(null);
      await load();
    } catch (e: any) { notify(apiErrorCode(e) || 'discard failed', { type: 'error' }); }
  };

  const submitAll = async () => {
    for (const i of ready) {
      // eslint-disable-next-line no-await-in-loop
      await submitItem(i);
    }
  };

  // ---- 指認案場（就地，不跳頁）----
  const searchStations = React.useCallback(async (q: string) => {
    if (!orgId) return;
    try {
      const { data } = await energyHttp.get(`${energyBase(orgId)}/stations`, {
        params: q ? { q } : {},
      });
      setOptions(Array.isArray(data) ? data : []);
    } catch { setOptions([]); }
  }, [orgId]);

  const openMatch = async (item: IngestItem) => {
    setMatchError(null); setPicked(''); setMatchTarget(item);
    // 預帶原樣電號當搜尋字 —— 多數情況只是主檔的電號多了空白或前綴。
    const initial = String(item.extracted?.customerNumberRaw ?? item.extracted?.contractNoRaw ?? '');
    setQuery(initial);
    await searchStations(initial);
  };

  const submitMatch = async () => {
    if (!matchTarget || !picked) { setMatchError(translate('energy.unmatched.pickStation')); return; }
    if (matchTarget.billId == null) { setMatchError(translate('energy.ingest.matchNeedsBill')); return; }
    setMatchError(null);
    try {
      await energyHttp.post(`${base}/bills/${matchTarget.billId}/match`, { stationId: Number(picked) });
      notify('energy.unmatched.matched', { type: 'success' });
      const target = matchTarget;
      setMatchTarget(null); setPicked(''); setOptions([]);
      await load();
      // 指認完**那一列就地變成可校對** —— 直接把它展開，
      // 不要讓使用者關掉 dialog 之後還得自己找回那一列。
      const next = new URLSearchParams(searchParams);
      next.set('item', String(target.id));
      setSearchParams(next, { replace: true });
    } catch (e: any) {
      setMatchError(apiErrorCode(e) || 'match failed');
    }
  };

  // ---- 重複 dialog 的「看那一筆」----
  const gotoClash = () => {
    if (!dup) return;
    const d = dup.details ?? {};
    if (dup.code === 'duplicate_in_batch') {
      // 同一批的另一份就在這一頁上 —— 展開它，不換頁。
      const next = new URLSearchParams(searchParams);
      next.set('item', String(d.itemId));
      setSearchParams(next, { replace: true });
      setDup(null);
      return;
    }
    setDup(null);
    if (d.executionId) { navigate(executionRoute(d.executionId)); return; }
    if (d.subjectId) navigate(billRoute(d.subjectId));
  };

  const dupBody = () => {
    if (!dup) return null;
    const d = dup.details ?? {};
    if (dup.code === 'duplicate_in_batch') {
      return translate('energy.ingest.dup.inBatch', {
        seq: d.seq ?? '?', file: d.originalFilename ?? '',
      });
    }
    if (dup.code === 'already_in_review') {
      return translate('energy.ingest.dup.inReview', {
        who: d.submittedByName ?? '—',
        when: String(d.submittedAt ?? '').slice(0, 10),
        node: d.currentNodeName ?? '—',
      });
    }
    return translate('energy.ingest.dup.approved', {
      documentNo: d.documentNo ?? (d.subjectId != null ? `#${d.subjectId}` : '—'),
      who: d.submittedByName ?? '—',
      when: String(d.submittedAt ?? '').slice(0, 10),
    });
  };



  /** 就地展開的校對工作區（左原檔 / 右辨識值‖最終值）。 */
  const renderWorkspace = (item: IngestItem) => (
    <Box
      py={2}
      data-testid="ingest-workspace"
      data-item={item.id}
      sx={{
        display: { xs: 'block', md: 'grid' },
        // 左欄要一開始就夠大 —— 260px 的 A4 讀不出紙上的數字（同校對台）。
        // 同校對台：用容器百分比，斷點會在抽屜／窄容器裡把欄位擠成上下堆疊。
        gridTemplateColumns: 'minmax(260px, 34%) 1fr',
        gap: 2,
      }}
    >
      {/* 左：**真的那份檔案**（`RECOGNITION.md §7`）。
          預覽框與校對台（`BillDetailBody`）用同一個 `SourcePreviewFrame`，
          而且**連欄位高亮也一樣** —— 第一次校對就發生在這一頁，
          兩個入口做同一件事卻只有一邊看得到位置，使用者會以為這一頁的辨識比較差。
          （這裡曾經刻意不做高亮，2026-09-09 收斂成兩邊一致。） */}
      <Box>
        <SourcePreviewFrame
          label={translate('energy.review.sourceLayout')}
          attachmentId={item.attachmentId ?? null}
          boxes={ingestBoxes}
          focusField={focusField}
          boxLabel={(f) => translate(billFieldLabelKey(f, bill?.templateId), { _: f })}
        />
        {item.attachmentId ? (
          <Stack gap={1}>
            <Button
              variant="outlined"
              size="small"
              startIcon={<DownloadIcon />}
              href={attachmentDownloadUrl(item.attachmentId, orgId)}
              target="_blank"
              rel="noreferrer"
              data-testid="ingest-open-source"
            >
              {translate('energy.review.openSource')}
            </Button>
          </Stack>
        ) : (
          <Typography variant="body2" color="text.secondary">
            {translate('energy.review.noSource')}
          </Typography>
        )}
        <Divider sx={{ my: 2 }} />
        <Typography variant="caption" color="text.secondary" display="block">
          {translate('energy.bills.customerNumber')}
        </Typography>
        <Typography variant="body2" sx={{ fontVariantNumeric: 'tabular-nums' }}>
          {bill?.station?.customerNumber || bill?.customerNumberRaw
            || item.extracted?.customerNumberRaw || '—'}
        </Typography>
        {bill && !bill.stationId && (
          <Alert
            severity="warning"
            icon={false}
            sx={{
              mt: 1, color: jvTokens.warn, borderColor: jvTokens.warn,
              border: '1px solid', backgroundColor: 'transparent',
            }}
          >
            {translate('energy.ingest.needsStation')}
          </Alert>
        )}
      </Box>

      {/* 右：辨識值 ‖ 最終值 */}
      <Box>
        {item.billId != null ? (
          <>
            {/* 「辨識值 vs 最終值」是**全站唯一一份**（`FieldDiffGrid`）——
                單據詳情頁 render 的是同一個元件。
                這一頁與那一頁**不合併**（送件的 item 沒有 execution／流程圖／
                應付與血緣頁籤，那些本來就不同），但中間這一格必須共用：
                它編碼的是規則，不是版面。 */}
            {/* 先講「這份辨識可不可信」，再講「值對不對」——
                順序反過來的話，人會先開始改數字才發現整份都要重看。 */}
            <RecognitionNote summary={item.recognition} />
            {/* 單別要能**在送審前**改 —— 這一頁正是送審前的那一頁。 */}
            <BillDocTypeField
              orgId={orgId}
              billId={bill?.id ?? null}
              docType={bill?.docType ?? null}
              recognizedDocType={bill?.recognizedDocType ?? null}
              binding={bill?.binding ?? null}
              editable={bill?.docTypeEditable !== false && !!bill?.canEditSubject}
              started={(bill?.executions ?? []).length > 0}
              onSaved={() => {
                // 改完單別要重讀這張單（`binding.flows` 是後端算的）。
                energyHttp.get(`${base}/bills/${item.billId}`)
                  .then(({ data }) => setBill(data))
                  .catch(() => undefined);
              }}
            />
            <FieldDiffGrid
              // 並排哪幾個欄位由版型決定（高壓看繳費期限、躉購看開單日）。
              fields={billReviewFieldsFor(bill?.templateId)}
              templateId={bill?.templateId}
              boxedFields={ingestBoxes.map((b) => b.field)}
              focusedField={focusField as any}
              onFocusField={(f) => setFocusField(f)}
              ocrExtracted={bill?.ocrExtracted}
              changedFields={bill?.changedFields}
              draft={draft}
              onChange={(f, v) => setDraft((prev) => ({ ...prev, [f]: v }))}
              editable={!!bill?.canEditSubject}
              rowTestId="ingest-field-row"
              inputTestId={(f) => `ingest-field-${f}`}
              labelWidth={150}
              markWidth={90}
            />
            <Stack direction="row" gap={1} mt={2} justifyContent="flex-end">
              <Button
                size="small"
                disabled={saving || !bill?.canEditSubject}
                data-testid="ingest-save-draft"
                onClick={() => saveDraft(item)}
              >
                {translate('energy.review.save')}
              </Button>
              <Button
                size="small"
                variant="contained"
                disabled={saving || !['extracted', 'classified'].includes(item.status) || !!item.duplicateOfItemId}
                data-testid="ingest-submit-from-workspace"
                onClick={() => submitItem(item)}
              >
                {translate('energy.intake.submit')}
              </Button>
            </Stack>
          </>
        ) : (
          // 保證金那類**不產新單**，它對上的是既有的期數 —— 這裡只印摘要。
          <Stack gap={1}>
            <Typography variant="body2" color="text.secondary">
              {translate('energy.ingest.depositSummary')}
            </Typography>
            <Typography variant="body2">
              {item.extracted?.contractNoRaw ?? '—'}
              {item.extracted?.seq != null ? translate('energy.ingest.seqLabel', { seq: item.extracted.seq }) : ''}
              {item.extracted?.amount ? <>　<Money value={item.extracted.amount} /></> : null}
            </Typography>
            <Stack direction="row" gap={1} justifyContent="flex-end">
              <Button
                size="small"
                variant="contained"
                disabled={!['extracted', 'classified'].includes(item.status) || !!item.duplicateOfItemId}
                data-testid="ingest-submit-from-workspace"
                onClick={() => submitItem(item)}
              >
                {translate('energy.intake.submit')}
              </Button>
            </Stack>
          </Stack>
        )}
      </Box>
    </Box>
  );

  return (
    <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2}>
        <Stack direction="row" alignItems="center" gap={1} flexWrap="wrap">
          <Button
            size="small"
            startIcon={<ArrowBackIcon />}
            data-testid="ingest-back"
            onClick={() => navigate(ingestListRoute())}
          >
            {translate('energy.ingest.backToList')}
          </Button>
          <Typography variant="h6" data-testid="ingest-batch-title">
            {translate('energy.ingest.batchTitle', { id: batch?.id ?? batchId })}
          </Typography>
          <Typography variant="caption" color="text.secondary">
            {String(batch?.submittedAt ?? '').slice(0, 16).replace('T', ' ')}
            {batch?.submittedByName ? `　${batch.submittedByName}` : ''}
          </Typography>
          <Chip
            size="small"
            variant="outlined"
            // 驗收要能問「這批辨識完了沒」——狀態字串會被翻譯，所以另外掛 data-status。
            data-testid="intake-batch-status"
            data-status={batch?.status ?? ''}
            label={translate(`energy.batchStatus.${batch?.status}`, { _: batch?.status ?? '' })}
          />
        </Stack>
        <Tooltip title={translate('ra.action.refresh')}>
          <span><IconButton onClick={() => load()} disabled={loading}><RefreshIcon /></IconButton></span>
        </Tooltip>
      </Stack>

      {/**
        * **整批還在跑的時候，先講一句完整的話**。
        *
        * 靜止的「辨識中」三個字跟**卡住**長得一模一樣，而那三個字也沒有回答
        * 使用者真正的問題：「我現在要做什麼？」——答案是什麼都不用做。
        * 所以帶說明的那一塊放在標題**下方整行**（塞進標題列會被擠成一個小轉圈，
        * 而那句話就沒有地方出現）。
        */}
      {RECOGNIZING_STATES.includes(String(batch?.status)) && (
        <RecognizingIndicator
          variant="block"
          done={items.filter((i) => !RECOGNIZING_STATES.includes(String(i.status))).length}
          total={items.length}
        />
      )}

      {/* ---- 校對區：只有還沒送出去的 ---- */}
      <Card>
        <CardContent>
          <Stack direction="row" alignItems="center" justifyContent="space-between" mb={1}>
            <Typography variant="subtitle1">
              {translate('energy.ingest.pendingTitle', { count: open.length })}
            </Typography>
            <Button
              variant="contained"
              size="small"
              data-testid="intake-submit-all"
              // 有待處理的就不能一鍵送出（不讓人把有問題的單一次送掉）。
              disabled={blockedCount > 0 || !ready.length}
              onClick={submitAll}
            >
              {translate('energy.intake.submitAll', { count: ready.length })}
            </Button>
          </Stack>

          {blockedCount > 0 && (
            <Alert
              severity="warning"
              sx={{
                mb: 1, color: jvTokens.warn, borderColor: jvTokens.warn,
                border: '1px solid', backgroundColor: 'transparent',
              }}
            >
              {translate('energy.intake.blocked', { count: blockedCount })}
            </Alert>
          )}

          {/* 整張表也給一個 testid：要「一次講四列」時，標註得指得到整份清單。 */}
          <Table size="small" data-testid="intake-table">
            <TableHead>
              <TableRow>
                <TableCell sx={{ width: 6, p: 0 }} />
                <TableCell>{translate('energy.intake.file')}</TableCell>
                <TableCell>{translate('energy.intake.workflow')}</TableCell>
                <TableCell>{translate('energy.intake.state')}</TableCell>
                <TableCell align="right" />
              </TableRow>
            </TableHead>
            <TableBody>
              {open.length ? open.map((i) => (
                <React.Fragment key={i.id}>
                  <TableRow
                    hover
                    data-testid="intake-item"
                    data-item-id={i.id}
                    data-status={i.status}
                    data-doc-type={i.docType ?? ''}
                    data-duplicate={i.duplicateOfItemId ? '1' : '0'}
                  >
                    <TableCell sx={{ width: 6, p: 0, background: STATUS_BAR[i.status] ?? jvTokens.border }} />
                    <TableCell>
                      <Typography variant="body2" fontWeight={600} data-testid="intake-item-file">
                        {i.originalFilename}
                      </Typography>
                      {i.extracted?.contractNoRaw && (
                        <Typography variant="caption" color="text.secondary">
                          {i.extracted.contractNoRaw}
                          {i.extracted.seq != null ? translate('energy.ingest.seqLabel', { seq: i.extracted.seq }) : ''}
                          {i.extracted.amount ? <>　<Money value={i.extracted.amount} /></> : null}
                        </Typography>
                      )}
                      {i.classifyReason && (
                        <Typography variant="caption" color="text.secondary" display="block">
                          {i.classifyReason}
                        </Typography>
                      )}
                    </TableCell>
                    <TableCell>
                      {['unclassified', 'failed'].includes(i.status) ? (
                        /**
                         * `unclassified` 的**出口**：手動指定型別。
                         * 沒有出口的話這批檔就永遠卡在那。
                         *
                         * **`failed` 也要有這個出口**（2026-09-09，使用者回報）：
                         * 辨識服務不收的檔（HEIC、或根本不是電費單）會停在 `failed`，
                         * 而在此之前那一列只有一句錯誤訊息、沒有任何按得下去的東西 ——
                         * 那就是「卡死」。指定單別之後會建一張空白單，人就能自己填。
                         */
                        <TextField
                          select size="small" sx={{ minWidth: 160 }}
                          value=""
                          label={translate('energy.intake.assignType')}
                          inputProps={{ 'data-testid': 'intake-assign-type' }}
                          onChange={(e) => patchItem(i, { docType: e.target.value })}
                        >
                          {ASSIGNABLE_DOC_TYPES.map((t) => (
                            <MenuItem key={t} value={t}>{translate(`energy.docType.${t}`)}</MenuItem>
                          ))}
                        </TextField>
                      ) : (
                        <Typography variant="body2">
                          {i.workflowKey
                            ? translate(`energy.workflowName.${i.workflowKey}`, { _: i.workflowKey })
                            : '—'}
                        </Typography>
                      )}
                    </TableCell>
                    <TableCell>
                      <Stack direction="row" gap={0.5} alignItems="center" flexWrap="wrap">
                        <Chip
                          size="small"
                          variant="outlined"
                          data-testid="intake-item-status"
                          label={translate(`energy.itemStatus.${i.status}`, { _: i.status })}
                          sx={{
                            color: STATUS_BAR[i.status] ?? jvTokens.muted,
                            borderColor: STATUS_BAR[i.status] ?? jvTokens.border,
                          }}
                        />
                        {/* 這一列還在等辨識 —— 動畫在**列上**，
                            因為使用者問的是「我這一份好了沒」，不是「整批好了沒」。 */}
                        {RECOGNIZING_STATES.includes(String(i.status)) && (
                          <RecognizingIndicator size={12} />
                        )}
                        {/* 辨識自己說「核心欄位有問題」時，在列上就要看得到 ——
                            不然承辦會先把整份校對完，才在展開後發現它本來就不可信。 */}
                        <RecognitionNote summary={i.recognition} dense />
                        {/* 同批重複**辨識完就標**，不等到按了送審才說 ——
                            送審才報等於讓人先做完校對的工，再告訴他這份是多餘的。 */}
                        {i.duplicateOfItemId != null && (
                          <Chip
                            size="small"
                            variant="outlined"
                            data-testid="intake-item-duplicate"
                            label={translate('energy.ingest.duplicateOf', { seq: i.duplicateOfSeq ?? '?' })}
                            sx={{ color: jvTokens.warn, borderColor: jvTokens.warn }}
                          />
                        )}
                        {/* 兩個警示都**只標示不擋**。 */}
                        {i.amountMismatch && (
                          <Chip
                            size="small" variant="outlined"
                            label={translate('energy.intake.amountMismatch')}
                            sx={{ color: jvTokens.warn, borderColor: jvTokens.warn }}
                          />
                        )}
                        {i.alreadyPaid && (
                          <Chip
                            size="small" variant="outlined"
                            label={translate('energy.intake.alreadyPaid')}
                            sx={{ color: jvTokens.warn, borderColor: jvTokens.warn }}
                          />
                        )}
                      </Stack>
                      {i.errorMessage && (
                        <Typography variant="caption" sx={{ color: jvTokens.warn }} display="block">
                          {i.errorMessage}
                        </Typography>
                      )}
                    </TableCell>
                    <TableCell align="right">
                      <Stack direction="row" gap={1} justifyContent="flex-end">
                        {/* 電號對不上 → 就地指認（開 dialog，不跳去例外件頁）。 */}
                        {(i.status === 'unmatched' || (i.billId != null && i.stationId == null)) && (
                          <Button
                            size="small"
                            startIcon={<LinkIcon />}
                            sx={{ color: jvTokens.warn }}
                            data-testid="intake-match-station"
                            onClick={() => openMatch(i)}
                          >
                            {translate('energy.unmatched.match')}
                          </Button>
                        )}
                        {/**
                          * **有單就給得了入口**（2026-09-09，使用者回報）。
                          *
                          * 原本只有 `extracted` / `classified` 才顯示「就地校對」。
                          * 於是**有單但配不到案場**（`unmatched`）的那一列
                          * —— 包含手動指定單別後建出來的空白單 ——
                          * 一個可以打開欄位的按鈕都沒有：
                          * 使用者看得到那一列、知道它缺電號，卻**沒有地方把電號填進去**。
                          *
                          * 判斷改成「這一列有沒有單」：有單就有欄位要對，
                          * 那就是這個按鈕存在的全部理由。
                          */}
                        {(i.billId != null || ['extracted', 'classified'].includes(i.status)) && (
                          <Button
                            size="small"
                            data-testid="intake-review"
                            onClick={() => toggleExpand(i.id)}
                          >
                            {translate(expandedId === i.id ? 'energy.ingest.collapse' : 'energy.intake.review')}
                          </Button>
                        )}
                        {['extracted', 'classified'].includes(i.status) && (
                          <Button
                            size="small" variant="contained"
                            data-testid="intake-submit-one"
                            onClick={() => submitItem(i)}
                          >
                            {translate('energy.intake.submit')}
                          </Button>
                        )}
                        <Button
                          size="small"
                          sx={{ color: jvTokens.muted }}
                          data-testid="intake-discard"
                          onClick={() => discardItem(i)}
                        >
                          {translate('energy.ingest.discard')}
                        </Button>
                      </Stack>
                    </TableCell>
                  </TableRow>
                  <TableRow>
                    <TableCell colSpan={5} sx={{ py: 0, border: 0 }}>
                      <Collapse in={expandedId === i.id} unmountOnExit>
                        {renderWorkspace(i)}
                      </Collapse>
                    </TableCell>
                  </TableRow>
                </React.Fragment>
              )) : (
                <TableRow>
                  <TableCell colSpan={5}>
                    <Typography variant="body2" color="text.secondary" data-testid="ingest-all-done">
                      {translate('energy.ingest.allDone')}
                    </Typography>
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      {/* ---- 已有結論的：一行一筆 ---- */}
      {closed.length > 0 && (
        <Card sx={{ mt: 2 }}>
          <CardContent>
            <Typography variant="subtitle1" mb={1}>
              {translate('energy.ingest.doneTitle', { count: closed.length })}
            </Typography>
            <Stack gap={0.5}>
              {closed.map((i) => (
                <Stack
                  key={i.id}
                  direction="row"
                  alignItems="center"
                  gap={1}
                  flexWrap="wrap"
                  data-testid="ingest-done-row"
                  data-status={i.status}
                >
                  {i.status === 'submitted'
                    ? <CheckCircleIcon fontSize="small" sx={{ color: jvTokens.ok }} />
                    : <RemoveCircleOutlineIcon fontSize="small" sx={{ color: jvTokens.muted }} />}
                  <Typography variant="body2">{i.originalFilename}</Typography>
                  {i.status === 'submitted' && (
                    <>
                      <Typography variant="body2" color="text.secondary">→</Typography>
                      {/* 單號連到**電費單**（一批 → 多張單，這是 1:N 的那一側）。 */}
                      <Button
                        size="small"
                        disabled={i.subjectType !== 'energyBill' || i.subjectId == null}
                        data-testid="ingest-open-bill"
                        onClick={() => i.subjectId != null && navigate(billRoute(i.subjectId))}
                      >
                        {i.documentNo ?? (i.subjectId != null ? `#${i.subjectId}` : '—')}
                      </Button>
                      {i.executionId != null && (
                        <Button
                          size="small"
                          data-testid="ingest-open-execution"
                          onClick={() => navigate(executionRoute(i.executionId!))}
                        >
                          {translate('energy.ingest.openExecution')}
                        </Button>
                      )}
                    </>
                  )}
                  {i.status === 'discarded' && (
                    <Typography variant="caption" color="text.secondary">
                      {translate('energy.itemStatus.discarded')}
                      {i.errorMessage ? `（${i.errorMessage}）` : ''}
                    </Typography>
                  )}
                  {i.attachmentId != null && (
                    <Tooltip title={translate('energy.batches.source')}>
                      <IconButton
                        size="small"
                        data-testid="batch-download"
                        href={attachmentDownloadUrl(i.attachmentId, orgId)}
                        target="_blank"
                        rel="noreferrer"
                      >
                        <DownloadIcon fontSize="small" />
                      </IconButton>
                    </Tooltip>
                  )}
                </Stack>
              ))}
            </Stack>
          </CardContent>
        </Card>
      )}

      {/* ---- 指認案場（就地 dialog）---- */}
      <Dialog open={!!matchTarget} onClose={() => setMatchTarget(null)} fullWidth maxWidth="sm">
        <DialogTitle>{translate('energy.unmatched.matchTitle')}</DialogTitle>
        <DialogContent>
          <Box display="flex" flexDirection="column" gap={2} mt={1}>
            <Typography variant="body2" color="text.secondary">
              {translate('energy.unmatched.rawIs', {
                value: matchTarget?.extracted?.customerNumberRaw || '—',
              })}
            </Typography>
            <TextField
              size="small"
              label={translate('energy.unmatched.searchStation')}
              value={query}
              onChange={(e) => { setQuery(e.target.value); searchStations(e.target.value); }}
              inputProps={{ 'data-testid': 'station-search' }}
            />
            <TextField
              select
              label={translate('energy.unmatched.station')}
              value={picked}
              onChange={(e) => setPicked(e.target.value)}
              helperText={options.length ? undefined : translate('energy.unmatched.noStationFound')}
              inputProps={{ 'data-testid': 'station-picker' }}
            >
              {options.map((s) => (
                <MenuItem key={s.id} value={String(s.id)}>
                  {`${s.name}（${s.customerNumber}）`}
                </MenuItem>
              ))}
            </TextField>
            {matchError && <Alert severity="error">{matchError}</Alert>}
          </Box>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setMatchTarget(null)}>{translate('ra.action.cancel')}</Button>
          <Button variant="contained" disabled={!picked} data-testid="station-match-confirm" onClick={submitMatch}>
            {translate('energy.unmatched.match')}
          </Button>
        </DialogActions>
      </Dialog>

      {/* ---- 三種重複，三句話，同一組出口 ---- */}
      <Dialog open={!!dup} onClose={() => setDup(null)} fullWidth maxWidth="sm">
        <DialogTitle data-testid="ingest-dup-title">
          {dup ? translate(`energy.ingest.dup.${dup.code}Title`) : ''}
        </DialogTitle>
        <DialogContent>
          <Typography variant="body2" data-testid="ingest-dup-body" data-code={dup?.code}>
            {dupBody()}
          </Typography>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setDup(null)}>{translate('ra.action.close')}</Button>
          <Button data-testid="ingest-dup-goto" onClick={gotoClash}>
            {translate('energy.ingest.dup.seeThatOne')}
          </Button>
          <Button
            variant="contained"
            sx={{ backgroundColor: jvTokens.warn }}
            data-testid="ingest-dup-discard"
            onClick={() => dup && discardItem(dup.item)}
          >
            {translate('energy.ingest.dup.discardThis')}
          </Button>
        </DialogActions>
      </Dialog>
    </EnergyPageFrame>
  );
};

export default IngestBatchDetailPage;
