import * as React from 'react';
import {
  Box, Card, CardContent, Typography, Button, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, Tooltip, IconButton, TextField, Alert, Tabs, Tab, Chip, Divider, MenuItem, Select,
  Dialog, DialogTitle, DialogContent, DialogActions,
} from '@mui/material';
import RefreshIcon from '@mui/icons-material/Refresh';
import ArrowBackIcon from '@mui/icons-material/ArrowBack';
import DownloadIcon from '@mui/icons-material/Download';
import SaveIcon from '@mui/icons-material/Save';
import InboxIcon from '@mui/icons-material/MoveToInbox';
import LinkIcon from '@mui/icons-material/Link';
import { energyHttp, energyBase, attachmentDownloadUrl, uploadViaStorage } from '@/api/energy';
import { useNotify, useTranslate } from 'react-admin';
import { useNavigate } from 'react-router-dom';
import { useSelection } from '@/platform/selection/store';
import { apiErrorCode, apiErrorPayload } from '../../../dataProvider/apiError';
import {
  EnergyPageFrame, EmptyRow, StatusChip, jvTokens, SourcePreviewFrame,
} from './shared';
import RecognitionNote from './RecognitionNote';
import { stationRoute, ingestBatchRoute, executionRoute, billRoute } from './routes';
import FlowGraph from './FlowGraph';
import ActionBar, { type TaskAction, type ActionBlocked } from './ActionBar';
import FieldDiffGrid, { billFieldLabelKey, type ReviewField } from './FieldDiffGrid';
import BillDocTypeField from './BillDocTypeField';
// 第四個「接縫」：欄位在原檔上的位置。mock 也是介面 —— 真 OCR 接上時這頁不用改。
import {
  sourceFieldBoxes,
  billReviewFieldsFor,
  ALL_BILL_REVIEW_FIELDS,
  BILL_DETAIL_FIELDS,
  BILL_DETAIL_FIELD_KEYS,
  recognitionSummary,
  type SourceFieldBox,
} from 'common/plugins/EnergyOpsCommonPlugin/taipowerExtractor';

interface StationOption { id: number; name: string; customerNumber: string; status: string }

/** 附件類別：封閉代碼，照後端 `eo_domain::attachment_limits::AttachmentCategory`。 */
const ATTACHMENT_CATEGORIES = ['taipowerNotice', 'bankbookCopy', 'transferList', 'approvalSheet', 'receipt', 'other'] as const;

// 欄位清單與那一格的呈現規則都在 `FieldDiffGrid` —— 這裡只是消費它。

interface BillDetail {
  id: number;
  stationId: number | null;
  customerNumberRaw: string | null;
  sourceAttachmentId: number | null;
  /** 上游送件批次（沒有＝遷移／seed 建的單，不是錯誤）。 */
  ingest: {
    batchId: number; itemId: number; originalFilename: string | null;
    attachmentId: number | null;
    submittedAt: string | null; submittedByName: string | null;
  } | null;
  ocrRaw: Record<string, any>;
  ocrExtracted: Record<string, any>;
  reviewStatus: string;
  changedFields: string[];
  station: { id: number; name: string; customerNumber: string } | null;
  executions: Array<{
    id: number; status: string; currentNodeKey: string; businessDate: string;
    /** 哪一條流程 —— 一張單可以同時開多條（核可 ＋ 開發票）。 */
    workflowKey?: string | null;
  }>;
  /**
   * **後端是模式的唯一真相**（`LIFECYCLE.md §6.4`）。
   *
   * 前端的唯讀只是**體驗**不是防線 —— `PATCH /bills/:id` 自己會再判一次、
   * `act`/`readonly` 一律 403。但只有後端擋是不夠的：
   * 一個看起來可以編輯、按下去才說「不能改」的頁面，
   * 使用者已經把校對的工做完了。**最貴的失敗時機。**
   */
  businessKey?: string | null;
  documentNo?: string | null;
  mode?: 'intake' | 'revise' | 'act' | 'readonly' | 'denied';
  modeReason?: {
    code: string; nodeName?: string | null; assigneeHint?: string | null; at?: string | null;
  } | null;
  canEditSubject?: boolean;
  canAddAttachment?: boolean;
  [k: string]: any;
}

/**
 * 單據詳情的**內容**（`LIFECYCLE.md §8.4`）——
 * 全案最重的一塊，而且**全站只有這一份實作**。
 *
 * 它有兩個容器，內容一模一樣：
 *
 * | 容器 | 用在哪 | 差別 |
 * |---|---|---|
 * | `ReviewConsolePage` | 路由 `#/energyBillConsole/:id` | 有返回鍵 |
 * | `BillDetailDrawer` | 「我的工作」的抽屜 | 有「第 N / M 筆」＋佇列語意的動作列 |
 *
 * **不要為抽屜複製一份。** 複製出來的那份會在第三次改版時開始說謊 ——
 * 而第三次改版一定會發生，因為這是全案最重的一頁。
 *
 * 佇列模式（`queue` 有值）只改**兩件事**：動作按鈕的文案、以及做完之後往哪去。
 * 載入、欄位、流程圖、附件、理由對話框**全部共用同一份程式碼**。
 *
 * ---
 *
 * P3 單據詳情／校對台（`FRONTEND.md §2-P3`、`REQUIREMENTS.md §五-4`）。
 *
 * 版面（雙欄，僅桌機）：
 * ```
 * ┌──────────────┬──────────────────────────┐
 * │ 左：原檔      │ 右：辨識值 vs 最終值       │
 * ├──────────────┴──────────────────────────┤
 * │ 流程圖（C2 的元件）＋ 動作按鈕列           │
 * ├─────────────────────────────────────────┤
 * │ 附件 ｜ 稽核歷程 ｜ 血緣  —— 分頁籤        │
 * └─────────────────────────────────────────┘
 * ```
 *
 * **這頁的核心是「辨識值與最終值並排」**（`DESIGN.md §1.4` 的三層值）：
 * **不要用最終值覆蓋辨識值的顯示** —— 「OCR 準不準」要看得出來，
 * 否則沒人知道該不該繼續信任辨識結果。被人工改過的列要標出來。
 *
 * 校對只動 ③ 最終值。`ocrRaw` / `ocrExtracted` 一個字都不能碰（後端也擋著）。
 *
 * **窄螢幕**：`REQUIREMENTS.md 附錄 A.4` 標了「OCR 頁僅桌機」，
 * v1 顯示一句「請用桌機開啟」的擋板（`FRONTEND.md §8-3` 可改）。
 */
export interface BillDetailQueue {
  /** 第幾筆（1-based）／共幾筆 —— 佇列的進度感，沒有它使用者不知道還要多久。 */
  index: number;
  total: number;
  /** 這一筆處理完 → 換下一筆（跳過已處理的）。到底了容器自己關掉抽屜。 */
  onNext: () => void;
  /** 處理完就關掉（不接下一筆）。 */
  onClose: () => void;
  /** 動作成功後把**更新後的那一列**交回容器，讓它就地替換（不重抓整頁）。 */
  onRowChanged?: (row: any) => void;
  /** 這一筆對應的 task —— 佇列模式下動作打的是 task 不是 bill。 */
  taskId?: number | null;
  /**
   * **這個待辦屬於哪一條流程**。
   *
   * 一張單現在可以同時開多條（核可 ＋ 開發票），而抽屜以前一律綁
   * `bill.executions[0]` —— 於是主管點開「承辦確認」那一筆時，
   * 抽屜綁到的是**另一條**流程，畫面說「這一關沒有待處理的任務」，
   * 按鈕一個都沒有。使用者看到的是「點開來什麼都不能做」。
   */
  executionId?: number | null;
  /** 還沒認領時要畫〔認領並開始處理〕。 */
  claimable?: boolean;
  claimedByMe?: boolean;
}

export interface BillDetailBodyProps {
  billId: number;
  /** 有才畫返回鍵。路由頁傳 navigate、抽屜傳關閉。 */
  onBack?: () => void;
  /** 有值 ＝ 佇列模式（在「我的工作」的抽屜裡）。 */
  queue?: BillDetailQueue | null;
}

const BillDetailBody: React.FC<BillDetailBodyProps> = ({ billId, onBack, queue }) => {
  const notify = useNotify();
  const translate = useTranslate();
  const navigate = useNavigate();
  const orgId = useSelection()?.organizationId;

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [bill, setBill] = React.useState<BillDetail | null>(null);
  const [draft, setDraft] = React.useState<Partial<Record<ReviewField, string>>>({});
  /** 卡 B S3：電費單明細 19 欄（＋通知單號），獨立一份草稿、獨立存檔，不混進上面 7 欄的。 */
  const [detailDraft, setDetailDraft] = React.useState<Partial<Record<string, string>>>({});
  const [tab, setTab] = React.useState(0);
  const [attachments, setAttachments] = React.useState<any[]>([]);
  const [audit, setAudit] = React.useState<any[]>([]);
  /** 卡 G3：finance 沒有 `energy:audit:view`，/audit-events 回 403——要分得出「沒權限」
   *  跟「真的沒資料」（HF11，B 文案），不能兩種都顯示「沒有資料。」。 */
  const [auditDenied, setAuditDenied] = React.useState(false);
  const [lineage, setLineage] = React.useState<any[]>([]);
  const [actions, setActions] = React.useState<TaskAction[]>([]);
  /** 不能做事時後端會說**為什麼**（C7 增補 A）。前端不自己算。 */
  const [blocked, setBlocked] = React.useState<ActionBlocked | null>(null);
  /** 這一關的「本關要求」：`.../executions/:id/actions` 頂層回的，每項帶 met（流程引擎：必傳附件／必填欄位）。 */
  const [requirements, setRequirements] = React.useState<any[]>([]);
  /** 按下動作被 422 step_requirements_unmet 擋下時，逐項列出缺項。 */
  const [requirementsBlocked, setRequirementsBlocked] = React.useState<{ missingAttachments: string[]; missingFields: string[] } | null>(null);
  const [openTaskId, setOpenTaskId] = React.useState<number | null>(null);
  /**
   * 這一關**認領了沒**（後端給，前端不自己從 state 推）。
   *
   * 人工關卡要 assignee 才做得了動作。這一頁原本沒有認領的入口 ——
   * 認領在【9】被搬進「我的工作」的抽屜，於是從流程單／單據詳情頁進來的人
   * 看得到動作按鈕、按下去 403，而畫面上是「按了沒反應」。
   */
  const [claimState, setClaimState] = React.useState<
    { claimable: boolean; claimedByMe: boolean } | null
  >(null);
  const [pendingAction, setPendingAction] = React.useState<TaskAction | null>(null);
  /** 理由對話框關掉之後要「接下一筆」還是「關抽屜」—— 記住按的是哪一顆。 */
  const [pendingThen, setPendingThen] = React.useState<'next' | 'close' | undefined>(undefined);
  const [reason, setReason] = React.useState('');
  /** 增補 B-1：點右欄欄位 → 左欄原檔高亮那一格。 */
  const [focusField, setFocusField] = React.useState<string | null>(null);
  /** 增補 B-2：改金額 → 分支即時重算的結果。 */
  const [branch, setBranch] = React.useState<any>(null);
  /** 應付紀錄（US-6：財務要看得到「它是怎麼算的」）。 */
  const [payouts, setPayouts] = React.useState<any[]>([]);

  const base = energyBase(orgId);
  /**
   * 這一頁在講**哪一條流程**。
   *
   * 一張單可能有多筆 execution —— 以前只有「中斷後重跑」一種情形，取最新的就對了；
   * 現在還會**同時**開多條（核可 ＋ 開發票，`RECOGNITION.md §9-9`）。
   *
   * 所以順序是：佇列指定的那一條（點開的待辦自己的流程）→ 還在跑的 → 最新的。
   * 少了第一條，從「我的工作」點開待辦會綁到另一條流程，
   * 畫面說「這一關沒有待處理的任務」而按鈕全部消失。
   */
  const execution = React.useMemo(() => {
    const list: any[] = bill?.executions ?? [];
    if (!list.length) return null;
    if (queue?.executionId) {
      const owned = list.find((e: any) => Number(e.id) === Number(queue.executionId));
      if (owned) return owned;
    }
    return list.find((e: any) => ['running', 'abortPending'].includes(String(e.status))) ?? list[0];
  }, [bill?.executions, queue?.executionId]);

  const load = React.useCallback(async () => {
    if (!orgId || !billId) return;
    setLoading(true); setDenied(false);
    try {
      const { data } = await energyHttp.get(`${base}/bills/${billId}`);
      setBill(data);
      // 草稿一律從**最終值**起始，不是從辨識值 —— 使用者要改的是最終值。
      const d: any = {};
      ALL_BILL_REVIEW_FIELDS.forEach((f) => { d[f] = data[f] == null ? '' : String(data[f]); });
      setDraft(d);
      // 卡 B S3：明細 19 欄的草稿也從最終值（`detailFields`）起始。
      const dd: any = {};
      const detailFields = data.detailFields ?? {};
      BILL_DETAIL_FIELD_KEYS.forEach((f) => { dd[f] = detailFields[f] == null ? '' : String(detailFields[f]); });
      setDetailDraft(dd);
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
      setLoading(false);
      return;
    }
    setLoading(false);
  }, [orgId, billId, base, notify]);

  React.useEffect(() => { load(); }, [load]);

  // 應付紀錄掛在**單**上（不是掛在 execution 上：重算過的單會有好幾筆）。
  React.useEffect(() => {
    if (!orgId || !billId) { setPayouts([]); return; }
    let alive = true;
    energyHttp.get(`${base}/bills/${billId}/payouts`)
      .then(({ data }) => { if (alive) setPayouts(Array.isArray(data) ? data : []); })
      .catch(() => { if (alive) setPayouts([]); });
    return () => { alive = false; };
  }, [orgId, billId, base]);

  /**
   * 附件與「本關要求」清單一起重抓（S3/S4）——補件辨識中的時候要每 3 秒重拉這兩個，
   * 直到沒有檔案在辨識；不重拉 audit／lineage／task，那幾個不會因為辨識而變。
   */
  const refreshAttachmentsAndActions = React.useCallback((eid: number) => {
    energyHttp.get(`${energyBase(orgId)}/executions/${eid}/attachments`)
      .then(({ data }) => setAttachments(Array.isArray(data) ? data : [])).catch(() => setAttachments([]));
    // 動作按鈕由後端決定（前端不判斷 kind）。
    // 回的是**可執行性過濾後**的結果：`{ availableActions, actionBlocked, requirements }`。
    // 不再是「這一關有哪些動作」——那等於對使用者說謊（`FRONTEND.md §2.8`）。
    energyHttp.get(`${base}/executions/${eid}/actions`)
      .then(({ data }) => {
        setActions(Array.isArray(data?.availableActions) ? data.availableActions : []);
        setBlocked(data?.actionBlocked ?? null);
        setRequirements(Array.isArray(data?.requirements) ? data.requirements : []);
        // 認領狀態：沒有它的話這一頁畫得出按鈕、按下去卻 403（見後端那一段）。
        setClaimState({
          claimable: !!data?.claimable,
          claimedByMe: !!data?.claimedByMe,
        });
      })
      .catch(() => { setActions([]); setBlocked(null); setClaimState(null); setRequirements([]); });
  }, [orgId, base]);

  // 分頁籤與動作列的資料 —— 都掛在 execution 上，沒有 execution 就沒有這些。
  React.useEffect(() => {
    if (!orgId || !execution) {
      setAttachments([]); setAudit([]); setAuditDenied(false); setLineage([]); setActions([]); setBlocked(null); setRequirements([]); return;
    }
    const eid = execution.id;
    refreshAttachmentsAndActions(eid);
    setAuditDenied(false);
    energyHttp.get(`${base}/audit-events`, { params: { targetType: 'energyWorkflowExecution', targetId: String(eid) } })
      .then(({ data }) => setAudit(Array.isArray(data) ? data : []))
      // 卡 G3：403 是「沒權限看」，跟真的查到 0 筆不是同一件事——分開記，
      //    其他錯誤（網路、500……）照舊當成空陣列，不誤判成「沒權限」。
      .catch((e) => {
        setAudit([]);
        setAuditDenied(e?.response?.status === 403);
      });
    energyHttp.get(`${base}/executions/${eid}/lineage`)
      .then(({ data }) => setLineage(Array.isArray(data) ? data : (data?.nodes ?? []))).catch(() => setLineage([]));
    // 目前這一關的 task —— 動作要打在 task 上。
    energyHttp.get(`${base}/executions/${eid}`)
      .then(({ data }) => {
        const open = (data?.tasks ?? []).find((t: any) => ['ready', 'reserved', 'inProgress'].includes(t.state));
        setOpenTaskId(open ? open.id : null);
      })
      .catch(() => setOpenTaskId(null));
  }, [orgId, execution, base, refreshAttachmentsAndActions]);

  /** 有附件在辨識中就每 3 秒重抓附件與要求，直到沒有（S3/S4：使用者怎麼看畫面自動變化）。 */
  const hasRecognizing = attachments.some((a) => a.classifyState === 'recognizing');
  React.useEffect(() => {
    if (!execution || !hasRecognizing) return undefined;
    const eid = execution.id;
    const timer = setInterval(() => refreshAttachmentsAndActions(eid), 3000);
    return () => clearInterval(timer);
  }, [execution, hasRecognizing, refreshAttachmentsAndActions]);

  /** 補齊之後，上一次「通過」留下的被擋提示不該繼續賴著——都 ✓ 了就自己收掉。 */
  React.useEffect(() => {
    if (requirements.length > 0 && requirements.every((r: any) => r.met)) setRequirementsBlocked(null);
  }, [requirements]);

  /**
   * 增補 B-2：**改金額 → 下方流程圖的分支即時重算**。
   *
   * 「改成 12 萬會多一關高層加簽」要在**按送審之前**看到 ——
   * 不是送出去才發現。校對的人有權知道他改的數字會改變什麼。
   * 條件由**後端**用引擎自己的選邊函式算（前端不比門檻：
   * 比錯了會給出一個看起來很肯定的錯誤答案）。
   */
  React.useEffect(() => {
    const raw = draft.totalAmount ?? '';
    const amount = Number(raw);
    if (!orgId || !billId || raw === '' || !Number.isFinite(amount)) { setBranch(null); return; }
    let alive = true;
    // 打字時會一直觸發 → **debounce**，不要每個按鍵都打一次 API。
    const timer = setTimeout(() => {
      energyHttp.get(`${base}/bills/${billId}/branch-preview`, { params: { totalAmount: amount } })
        .then(({ data }) => { if (alive) setBranch(data); })
        .catch(() => { if (alive) setBranch(null); });
    }, 400);
    return () => { alive = false; clearTimeout(timer); };
  }, [draft.totalAmount, orgId, billId, base]);

  /**
   * 這次修改**改變了審批路線**嗎（`FRONTEND.md §2.86`）。
   * 兩個方向都算 —— 只看「多一關」等於漏掉危險的那個方向。
   */
  const routeChanged = !!branch?.available
    && (((branch.nodes ?? []).some((n: any) => n.added)) || ((branch.removed ?? []).length > 0));

  /**
   * 欄位在原檔上的位置 —— **真的座標**（`RECOGNITION.md §9-5`）。
   *
   * 在此之前這裡是 `mockFieldBoxLocator`：一組寫死的假框。
   * 假框跟「畫得出來但按不動的按鈕」是同一種謊 ——
   * 它讓人以為系統知道欄位在哪。辨識服務只在能**唯一定位**到那個 token 時
   * 才給座標，所以沒有 box 就不畫，而那是正確行為不是缺陷。
   */
  /**
   * **並排哪幾個欄位由版型決定**（`RECOGNITION.md §7.1`）：
   * 高壓用電單看「繳費期限」、躉購單看「台電開單日」。
   * 草稿與存檔則掃**聯集** —— 見 `ALL_BILL_REVIEW_FIELDS` 的說明。
   */
  const reviewFields = React.useMemo(
    () => billReviewFieldsFor(bill?.templateId),
    [bill?.templateId],
  );

  /** 「這份辨識可不可信」—— 與收件台同一個元件、同一份摘要邏輯。 */
  const recognition = React.useMemo(
    () => recognitionSummary(bill?.ocrRaw),
    [bill?.ocrRaw],
  );

  const boxes: SourceFieldBox[] = React.useMemo(
    // 版型由辨識結果自己帶（`sourceFieldBoxes` 從 record 讀），這裡不再傳一次。
    () => sourceFieldBoxes(bill?.ocrRaw, reviewFields),
    [bill?.ocrRaw, reviewFields],
  );

  const dirty = React.useMemo(() => {
    if (!bill) return false;
    if (ALL_BILL_REVIEW_FIELDS.some((f) => String(bill[f] ?? '') !== String(draft[f] ?? ''))) return true;
    const detailFields = bill.detailFields ?? {};
    return BILL_DETAIL_FIELD_KEYS.some((f) => String(detailFields[f] ?? '') !== String(detailDraft[f] ?? ''));
  }, [bill, draft, detailDraft]);

  /**
   * **金額與度數必須送 `number`**（`correctBillSchema` 用 `z.number()`）。
   * 輸入框拿到的是字串 —— 直接送 `'98123.00'` 會 **400**，
   * 而且畫面上只會看到「值沒存進去」，不會有人聯想到型別。
   * 日期則相反：只取前 10 碼（`YYYY-MM-DD`），不要把時間也送過去。
   */
  const NUMERIC_FIELDS: ReviewField[] = ['totalAmount', 'generationKwh'];
  /**
   * 只有**日期**要截前 10 碼。
   *
   * 原本是「不是數字就截 10 碼」—— 那在只有日期與金額的年代是對的，
   * 但電號一進來就爆：`00-60-1600-79-5` 有 15 碼，會被無聲地存成 `00-60-1600`，
   * 然後配不到案場。截字串這種事**一定要指名道姓**，不能用「其餘」當規則。
   */
  const DATE_FIELDS: ReviewField[] = ['billingPeriodStart', 'billingPeriodEnd', 'issuedAt', 'dueDate'];
  const coerce = (f: ReviewField, v: string) => {
    if (v === '') return null;
    if (NUMERIC_FIELDS.includes(f)) {
      const n = Number(v);
      return Number.isFinite(n) ? n : null;
    }
    if (DATE_FIELDS.includes(f)) return String(v).slice(0, 10);
    return String(v).trim();
  };

  /** 明細 19 欄的型別表驅動 coerce —— 跟上面 7 欄同一個規則，不逐欄手寫。 */
  const coerceDetail = (f: string, v: string) => {
    if (v === '') return null;
    const kind = BILL_DETAIL_FIELDS.find((x) => x.key === f)?.kind;
    if (kind === 'num') { const n = Number(v); return Number.isFinite(n) ? n : null; }
    if (kind === 'date') return String(v).slice(0, 10);
    return String(v).trim();
  };

  const save = async () => {
    if (!bill) return;
    const body: any = {};
    ALL_BILL_REVIEW_FIELDS.forEach((f) => {
      if (String(bill[f] ?? '') !== String(draft[f] ?? '')) body[f] = coerce(f, draft[f] ?? '');
    });
    const detailFields = bill.detailFields ?? {};
    const detail: any = {};
    BILL_DETAIL_FIELD_KEYS.forEach((f) => {
      if (String(detailFields[f] ?? '') !== String(detailDraft[f] ?? '')) detail[f] = coerceDetail(f, detailDraft[f] ?? '');
    });
    if (Object.keys(detail).length) body.detail = detail;
    if (!Object.keys(body).length) return;
    try {
      await energyHttp.patch(`${base}/bills/${bill.id}`, body);
      notify('energy.review.saved', { type: 'success' });
      load();
    } catch (e: any) { notify(apiErrorCode(e) || 'save failed', { type: 'error' }); }
  };

  /** S1：合約後補之後，重新比對一次（不用改電號、改期間這種繞路的方式）。 */
  const rematch = async () => {
    if (!bill) return;
    try {
      await energyHttp.post(`${base}/bills/${bill.id}/rematch`);
      notify('energy.review.rematched', { type: 'success' });
      load();
    } catch (e: any) { notify(apiErrorCode(e) || 'rematch failed', { type: 'error' }); }
  };

  const markReviewed = async () => {
    if (!bill) return;
    try {
      await energyHttp.patch(`${base}/bills/${bill.id}`, { markReviewed: true });
      notify('energy.review.marked', { type: 'success' });
      load();
    } catch (e: any) { notify(apiErrorCode(e) || 'mark failed', { type: 'error' }); }
  };

  /**
   * 做一個動作。
   *
   * `then` 決定**做完之後往哪去**，而那是頁面模式與佇列模式**唯一**的差別：
   * 頁面模式重載自己；佇列模式把更新後的那一列交回容器，
   * 再依使用者按的是哪一顆決定「接下一筆」還是「關掉」。
   *
   * 後端回的 `row` 是**工作台那一列的同一份 DTO**（`LIFECYCLE.md §8.3`）——
   * 容器拿它就地替換，**不呼叫 load()**：重抓會讓那一列換區又重排，
   * 使用者剛按的那一列就跳到最下面去了。
   */
  const runAction = async (
    action: TaskAction, withReason?: string, then?: 'next' | 'close',
  ) => {
    if (!openTaskId) return;
    setRequirementsBlocked(null);
    try {
      const { data } = await energyHttp.post(`${base}/tasks/${openTaskId}/actions`, {
        actionKey: action.actionKey, ...(withReason ? { reason: withReason } : {}),
      });
      setPendingAction(null); setReason('');
      if (queue) {
        if (data?.row) queue.onRowChanged?.(data.row);
        /**
         * **「接下一筆」的那一顆不在這裡發 notify。**
         *
         * RA 的通知是**佇列式的**：一次只顯示一個，後來的排隊。
         * 這裡先發「已送出」、容器接著發「這一區處理完了」的話，
         * 第二句會被第一句擋住 —— 使用者要等前一句自己消失才看得到，
         * 而那時他已經在看列表了，那句話的意義也就沒了。
         *
         * 所以交給容器發**一次**：還有下一筆就說「已送出」，
         * 到底了就說「這一區處理完了」。
         */
        if (then === 'next') { queue.onNext(); return; }
        notify('energy.myTasks.actionDone', { type: 'success' });
        if (then === 'close') queue.onClose();
        else load();
        return;
      }
      notify('energy.myTasks.actionDone', { type: 'success' });
      load();
    } catch (e: any) {
      const payload = apiErrorPayload(e);
      // 送出前的最後一道（S2）：422 step_requirements_unmet，被擋提示逐項列出缺項，不送出。
      if (payload?.code === 'step_requirements_unmet') {
        setRequirementsBlocked({
          missingAttachments: Array.isArray(payload.details?.missingAttachments) ? payload.details.missingAttachments : [],
          missingFields: Array.isArray(payload.details?.missingFields) ? payload.details.missingFields : [],
        });
        return;
      }
      notify(apiErrorCode(e) || 'action failed', { type: 'error' });
    }
  };

  /**
   * 〔認領並開始處理〕—— **只在抽屜裡**（`LIFECYCLE.md §8.5`）。
   *
   * 順序是刻意反過來的：**先看再認領**。
   * 留在列上當一個獨立開關的話，就會回到「認領了卻不知道去哪看」。
   */
  const claimAndStart = async () => {
    if (!openTaskId) return;
    try {
      const { data } = await energyHttp.post(`${base}/tasks/${openTaskId}/claim`);
      notify('energy.myTasks.claimed_msg', { type: 'success' });
      if (data?.row) queue?.onRowChanged?.(data.row);
      load();
    } catch (e: any) { notify(apiErrorCode(e) || 'claim failed', { type: 'error' }); }
  };

  /**
   * **指認案場**（`LIFECYCLE.md §7.4`）—— 從獨立的「例外件」頁搬過來。
   *
   * 邏輯**原封不動重用**（`UnmatchedBillsPage` 的三步：搜尋 → 選 → POST match）；
   * 搬家不是重寫。重寫一份的話兩邊會慢慢長歪，
   * 而這件事在**送件詳情頁**還有第三份入口（那是【4】做的 dialog）。
   *
   * 搜尋走 `GET .../stations?q=`（輕量搜尋端點），
   * 不是 P5 用的 `station-list` —— 那支會帶經緯度與地主，指認對話框不需要。
   */
  const [matchOpen, setMatchOpen] = React.useState(false);
  const [stationQuery, setStationQuery] = React.useState('');
  const [stationOptions, setStationOptions] = React.useState<StationOption[]>([]);
  const [pickedStation, setPickedStation] = React.useState('');
  const [matchError, setMatchError] = React.useState<string | null>(null);

  const searchStations = React.useCallback(async (q: string) => {
    if (!orgId) return;
    try {
      const { data } = await energyHttp.get(`${energyBase(orgId)}/stations`, {
        params: q ? { q } : {},
      });
      setStationOptions(Array.isArray(data) ? data : []);
    } catch { setStationOptions([]); }
  }, [orgId]);

  const openMatch = async () => {
    setMatchError(null); setPickedStation(''); setMatchOpen(true);
    // 預帶原樣電號當搜尋字 —— 多數情況只是主檔的電號多了空白或前綴。
    const initial = bill?.customerNumberRaw ?? '';
    setStationQuery(initial);
    await searchStations(initial);
  };

  const submitMatch = async () => {
    if (!pickedStation) { setMatchError(translate('energy.unmatched.pickStation')); return; }
    setMatchError(null);
    try {
      await energyHttp.post(`${base}/bills/${billId}/match`, { stationId: Number(pickedStation) });
      notify('energy.unmatched.matched', { type: 'success' });
      setMatchOpen(false); setPickedStation(''); setStationOptions([]);
      load();
    } catch (e: any) {
      setMatchError(apiErrorCode(e) || 'match failed');
    }
  };

  /**
   * **附件的一生**（`LIFECYCLE.md §6.7`）。
   *
   * | role | 什麼時候產生 | 顯示在哪 |
   * |---|---|---|
   * | `source`（舊資料 `billScan`） | 送件時上傳的**原始掃描檔** | **左欄** |
   * | `submitted` | 送件時一併附上的佐證 | 附件頁籤 |
   * | `added` | **流程中**才補上的（補件） | 附件頁籤 |
   *
   * 左欄五個模式都要有，**沒有例外**：主管核准的動作本質上就是
   * 拿螢幕上的數字對紙上的數字 —— 看不到紙就沒辦法核。
   */
  const SOURCE_ROLES = ['source', 'billScan'];
  const isSource = (a: any) => (a?.links ?? []).some((l: any) => SOURCE_ROLES.includes(String(l.role)));

  /**
   * 左欄要顯示的那一份，**三個來源依序找**：
   *
   * 1. `bill.sourceAttachmentId`（正規欄位）
   * 2. execution 的附件裡 `role='source'` 那份 —— 舊資料沒補上第 1 點時的後路
   * 3. `bill.ingest.attachmentId` —— **`intake` 模式沒有 execution**，
   *    那時第 2 點根本查不到（端點是 execution-scoped）
   *
   * 少了第 3 點，還沒送審的單在校對台上永遠看不到原檔 ——
   * 而那正是最需要對照的時刻。
   */
  const sourceAttachmentId: number | null = bill?.sourceAttachmentId
    ?? (attachments.find(isSource)?.id ?? null)
    ?? (bill?.ingest?.attachmentId ?? null);

  /** 附件頁籤只列**不是左欄那一份**的（左欄已經顯示了，列兩次只是雜訊）。 */
  const extraAttachments = attachments.filter((a) => Number(a.id) !== Number(sourceAttachmentId));

  const [uploading, setUploading] = React.useState(false);
  const sourceInputRef = React.useRef<HTMLInputElement | null>(null);
  const addedInputRef = React.useRef<HTMLInputElement | null>(null);

  /**
   * 上傳一份附件到這筆 execution 上。
   *
   * `role` 決定它是「原檔」還是「補件」——
   * 補件**不推進流程、不解除任何鎖**，它只是多一份檔案（§6.7.3）。
   */
  const uploadAttachment = async (file: File, role: 'source' | 'added') => {
    if (!execution) { notify('energy.review.needsExecutionToUpload', { type: 'warning' }); return; }
    setUploading(true);
    try {
      // energy-ops-next：直傳 Storage（大檔不經 API 請求本體，見 auth/directUpload.ts）。
      await uploadViaStorage(`${base}/executions/${execution.id}/attachments`, file, {
        target: 'execution', executionId: execution.id, organizationId: orgId,
      }, { role });
      notify('energy.review.attachmentAdded', { type: 'success' });
      load();
    } catch (e: any) {
      notify(apiErrorCode(e) || 'upload failed', { type: 'error' });
    } finally {
      setUploading(false);
      if (sourceInputRef.current) sourceInputRef.current.value = '';
      if (addedInputRef.current) addedInputRef.current.value = '';
    }
  };

  /** 人選類別（判不出或要改類別都走這裡，S4）；選完即存、重抓附件與要求。 */
  const setAttachmentCategory = async (attachmentId: number, category: string) => {
    try {
      await energyHttp.patch(`${base}/attachments/${attachmentId}/category`, { category });
      if (execution) refreshAttachmentsAndActions(execution.id);
    } catch (e: any) { notify(apiErrorCode(e) || 'save failed', { type: 'error' }); }
  };

  /**
   * **`modeReason` 那一句** —— `readonly` 一定要印出來
   * （`LIFECYCLE.md §6.4`）：一個什麼都不能按、又不解釋為什麼的頁面，
   * 使用者只會以為它壞了。
   *
   * 寫成 helper 是因為它要出現在**兩個位置**：
   * 欄位那一區（「為什麼不能改」）與動作列（「為什麼沒有按鈕」）。
   * 抄兩份的話，日後只會有一邊被更新。
   */
  const modeSentence = () => {
    if (!bill?.modeReason) {
      return (
        <Typography variant="body2" color="text.secondary" data-testid="review-actions-reason">
          {translate('energy.review.noOpenTask')}
        </Typography>
      );
    }
    return (
      <Alert
        severity="info"
        icon={false}
        data-testid="review-actions-reason"
        data-mode={bill?.mode ?? ''}
        data-code={bill?.modeReason?.code ?? ''}
        sx={{
          color: jvTokens.caption,
          border: '1px solid',
          borderColor: jvTokens.border,
          backgroundColor: 'transparent',
        }}
      >
        {translate(`energy.modeReason.${bill.modeReason.code}`, {
          _: translate('energy.modeReason.generic'),
          nodeName: bill.modeReason.nodeName ?? '—',
          assigneeHint: bill.modeReason.assigneeHint ?? translate('energy.actionBlocked.someoneElse'),
          at: bill.modeReason.at ?? '—',
        })}
      </Alert>
    );
  };

  /**
   * **從詳情頁直接送審**（`intake` 模式的出口）。
   *
   * 三種重複各有**自己的一句話**（`LIFECYCLE.md §7.2`）——
   * 共用一句會讓「還可能被退回」與「已經結案」長得一樣，
   * 而那兩件事該做的處置完全不同。
   */
  const [submitting, setSubmitting] = React.useState(false);
  const [dup, setDup] = React.useState<{ code: string; details: any } | null>(null);
  const submitBill = async () => {
    setSubmitting(true);
    try {
      await energyHttp.post(`${base}/bills/${billId}/submit`);
      notify('energy.bills.submitted', { type: 'success' });
      load();
    } catch (e: any) {
      const payload = apiErrorPayload(e);
      if (payload && ['bill_already_submitted', 'already_in_review', 'duplicate_document'].includes(payload.code)) {
        setDup({ code: payload.code, details: payload.details ?? {} });
        return;
      }
      notify(apiErrorCode(e) || 'submit failed', { type: 'error' });
    } finally { setSubmitting(false); }
  };

  /** 捨棄手上這一份 —— 三種重複的共同出口之一。 */
  const discardThis = async () => {
    try {
      const item = bill?.ingest;
      if (!item?.itemId) { notify('energy.ingest.dup.noIngestItem', { type: 'warning' }); return; }
      await energyHttp.post(`${base}/ingest-items/${item.itemId}/discard`, { reason: 'duplicate' });
      notify('energy.ingest.discarded', { type: 'success' });
      setDup(null);
      load();
    } catch (e: any) { notify(apiErrorCode(e) || 'discard failed', { type: 'error' }); }
  };

  if (!bill && !loading) {
    return (
      <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
        <Alert severity="info">{translate('energy.review.notFound')}</Alert>
      </EnergyPageFrame>
    );
  }

  /**
   * 佇列模式的動作列（`LIFECYCLE.md §8.5`）。
   *
   * 跟頁面模式的差別**只有文案與去向**，動作本身走同一個 `runAction`。
   *
   * | 狀態 | 畫什麼 |
   * |---|---|
   * | 未認領 | 主要鈕〔認領並開始處理〕—— **先看再認領** |
   * | 已認領（我） | 〔核准並處理下一筆〕主要 ／〔核准並關閉〕／〔退回〕 |
   * | 別人認領中／不輪到我 | 不畫按鈕，印那一句原因（`ActionBar` 自己會處理） |
   *
   * 「並處理下一筆」是**主要按鈕**：目標是把佇列清空，不是回到列表。
   */
  const queueActionBar = () => {
    if (!queue) return null;
    // 還沒認領 —— 先讓他看，看完再認領。
    if (queue.claimable && !queue.claimedByMe) {
      return (
        <Stack direction="row" gap={1} flexWrap="wrap" data-testid="queue-actions">
          <Button
            variant="contained"
            size="small"
            data-testid="queue-claim"
            onClick={claimAndStart}
          >
            {translate('energy.myWork.claimAndStart')}
          </Button>
        </Stack>
      );
    }
    // 不是我的（別人認領中／不輪到我）—— 交給 ActionBar 印原因，不畫假按鈕。
    if (!actions.length) {
      return <ActionBar actions={actions} blocked={blocked} onAction={() => {}} />;
    }
    const sorted = [...actions].sort((a, b) => a.sortOrder - b.sortOrder);
    return (
      <Stack direction="row" gap={1} flexWrap="wrap" data-testid="queue-actions">
        {sorted.map((a) => {
          const label = translate(a.labelKey, { _: a.actionKey });
          const fire = (then?: 'next' | 'close') => {
            if (a.requiresReason) {
              setReason(''); setPendingThen(then); setPendingAction(a); return;
            }
            runAction(a, undefined, then);
          };
          // 推進類的動作才有「並處理下一筆」——
          // 退回／中斷之後接著看下一筆是說得通的，但它們不是「清掉一件」的主線，
          // 所以只給一顆，按完關掉回到列表。
          if (a.kind === 'advance') {
            return (
              <React.Fragment key={a.actionKey}>
                <Button
                  variant="contained"
                  size="small"
                  data-testid="queue-act-next"
                  data-action={a.actionKey}
                  onClick={() => fire('next')}
                >
                  {translate('energy.myWork.actAndNext', { label })}
                </Button>
                <Button
                  variant="outlined"
                  size="small"
                  data-testid="queue-act-close"
                  data-action={a.actionKey}
                  onClick={() => fire('close')}
                >
                  {translate('energy.myWork.actAndClose', { label })}
                </Button>
              </React.Fragment>
            );
          }
          /**
           * **副作用不關抽屜。**
           *
           * 「標記已付款」「登記已開立發票」是**登記一件外部發生的事**，
           * 它不推進流程 —— 登記完還要按「完成」，這條線才真的結束
           * （流程定義就是這樣寫的：`markPaid` 是 `sideEffect`，`submit` 才是 `advance`）。
           * 原本這裡一律 `close`，於是使用者按完「標記已付款」抽屜就關了，
           * 得回清單重新找那一列、重新點開，才能按「完成」——
           * 而畫面上沒有任何地方提示他還有一半沒做完。
           *
           * 退回／中斷不一樣：那兩種**確實**把這一筆從我手上拿走了，關掉是對的。
           */
          const keepOpen = a.kind === 'sideEffect';
          return (
            <Button
              key={a.actionKey}
              variant="outlined"
              size="small"
              data-testid="queue-act-other"
              data-action={a.actionKey}
              onClick={() => fire(keepOpen ? undefined : 'close')}
            >
              {label}
            </Button>
          );
        })}
      </Stack>
    );
  };

  /** 「辨識值 vs 最終值」的一列。這是整頁的核心。 */


  return (
    <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
      {/* 這一頁綁在哪一條流程上 —— 看不見，但驗收指名得到。
          多流程之後這裡是會出錯的地方（`TASKS.md #76`），而錯了的畫面
          看起來完全正常：只是動作變少了。 */}
      <span
        hidden
        data-testid="bill-detail-execution"
        data-execution-id={execution?.id ?? ''}
        data-workflow-key={execution?.workflowKey ?? ''}
      />
      {/* 窄螢幕擋板（`FRONTEND.md §8-3`）—— 這頁的雙欄在手機上沒有意義。 */}
      <Alert severity="info" sx={{ display: { xs: 'flex', md: 'none' }, mb: 2 }}>
        {translate('energy.review.desktopOnly')}
      </Alert>

      {/* `FRONTEND.md §2.85`：**未綁定流程的案場收不了單** ——
          這個錯誤原本要到**送審那一刻**才爆（`no_binding_for_business_date`），
          那時候承辦已經校對完整張單了。
          **在人已經做完工才告訴他做不了，是最貴的失敗時機。** */}
      {/* S2：找不到生效合約 —— 跟「尚未綁定流程」是兩種各自的提示，不要混在一起。 */}
      {bill?.binding?.bound === false && bill.binding.reason === 'no_contract' && (
        <Alert
          severity="warning"
          data-testid="no-contract-warning"
          sx={{
            mb: 2,
            color: jvTokens.warn,
            border: `1px solid ${jvTokens.warn}`,
            backgroundColor: 'transparent',
          }}
        >
          {translate('energy.review.noContractWarn', {
            station: bill.binding.stationName ?? bill.binding.customerNumber ?? '',
            from: bill.binding.billingPeriodStart ?? '—',
            to: bill.binding.billingPeriodEnd ?? '—',
          })}
          {bill?.canEditSubject && (
            <Button size="small" data-testid="bill-rematch" sx={{ ml: 1, color: jvTokens.warn }} onClick={rematch}>
              {translate('energy.review.rematch')}
            </Button>
          )}
        </Alert>
      )}
      {bill?.binding?.bound === false && bill.binding.reason === 'no_binding' && (
        <Alert
          severity="warning"
          data-testid="no-binding-warning"
          sx={{
            mb: 2,
            color: jvTokens.warn,
            border: `1px solid ${jvTokens.warn}`,
            backgroundColor: 'transparent',
          }}
        >
          {translate('energy.review.noBindingWarn', {
            station: bill.binding.stationName ?? bill.binding.customerNumber ?? '',
          })}
        </Alert>
      )}

      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2}>
        <Stack direction="row" alignItems="center" gap={1}>
          {/* 返回**由容器決定**：路由頁回清單、抽屜是關掉自己。
              沒給 onBack 就不畫 —— 一顆不知道會去哪的返回鍵比沒有更糟。 */}
          {onBack && (
            <IconButton data-testid="detail-back" onClick={onBack}><ArrowBackIcon /></IconButton>
          )}
          <Typography variant="h6">
            {bill?.station?.name || bill?.customerNumberRaw || `#${billId}`}
          </Typography>
          <StatusChip i18nKey="energy.reviewStatus" value={bill?.reviewStatus} />
          {/* 麵包屑：單 → 案場 → 地主（`IA.md §5` 矩陣的 P3 那一列）。
              沒有它，使用者要回選單才找得到這張單屬於誰 —— 而那正是「孤島」的定義。 */}
          {bill?.station && (
            <Button
              size="small"
              data-testid="review-open-station"
              onClick={() => navigate(stationRoute(bill.station!.id))}
            >
              {bill.station.name}
            </Button>
          )}
          {/* 地主**刻意不在這裡**：`getBill` 沒有回地主（單只認案場），
              而「單 → 案場 → 地主」這條路 P11 已經接好了 ——
              為了少一次點擊在這裡多查一次 DB 不划算。 */}
          {/* 「這張單怎麼來的」—— 回**那一批**送件（`LIFECYCLE.md §7.6`）。
              前一版連到的是送單紀錄的**清單頁**，等於把「回到來源」變成
              「回到一堆來源裡自己找」。方向是不對稱的：一批送件 → 多張電費單（1:N），
              但一張電費單**只會有一個來源送件** —— 所以這裡是一條麵包屑不是一個清單。
              舊單／遷移進來的單沒有上游批次，這時**不顯示**（不要畫一顆點不動的按鈕）。 */}
          {bill?.ingest?.batchId != null && (
            <Button
              size="small"
              startIcon={<InboxIcon fontSize="small" />}
              data-testid="bill-open-batch"
              onClick={() => navigate(ingestBatchRoute(bill.ingest!.batchId, bill.ingest!.itemId))}
            >
              {translate('energy.review.fromBatch', {
                file: bill.ingest.originalFilename ?? '',
                who: bill.ingest.submittedByName ?? '—',
                when: String(bill.ingest.submittedAt ?? '').slice(0, 10),
              })}
            </Button>
          )}
        </Stack>
        <Tooltip title={translate('ra.action.refresh')}>
          <IconButton onClick={load} disabled={loading}><RefreshIcon /></IconButton>
        </Tooltip>
      </Stack>

      {/**
        * **這張單沒有經過重複檢查**（`LIFECYCLE.md §6.3.5`）。
        *
        * `businessKey` 為 null ＝ 抽不到電號或計費期間 → 去重整段**放行**。
        * 那是刻意的（不然那張單永遠送不出去），但**核准的人也需要知道** ——
        * 他按下去的那一刻是最後一道關，而系統在這一張上沒有幫他擋過任何東西。
        *
        * 只在載入完成後判斷：`bill` 還是 null 時畫出來會閃一下假警告。
        */}
      {bill && !bill.businessKey && (
        <Alert
          severity="warning"
          icon={false}
          data-testid="review-no-business-key"
          sx={{
            mb: 2,
            color: jvTokens.warn,
            border: '1px solid',
            borderColor: jvTokens.warn,
            backgroundColor: 'transparent',
          }}
        >
          {translate('energy.review.noBusinessKey')}
        </Alert>
      )}

      {/* ---- 雙欄：左原檔 / 右辨識值 vs 最終值 ---- */}
      {/*
        **`containerType` 一定要在父層。**
        CSS 的規則是「容器只對**後代**生效」—— 元素不能用 `@container` 查詢自己。
        第一版把 `containerType` 跟 `@container` 寫在同一個 Box 上，
        於是查詢永遠不成立、永遠走單欄，而畫面上那張 A4 比例的示意圖
        變全寬之後把欄位推到一千多 px 以下。

        當時 crawler 是綠的（欄位「勉強」還在視窗內），
        而那個 `top: 1056 / 視窗 1100` 的擦邊數字就是徵兆 —— 數字與畫面矛盾時以畫面為準。
      */}
      <Box sx={{ containerType: 'inline-size' }}>
      <Box
        sx={{
          /**
           * **依容器寬度決定欄數，不是依視窗寬度。**
           *
           * 前一版是 `{ xs: 'block', md: 'grid' }` ＋ 固定 `360px 1fr` ——
           * 那是為**整頁全寬**設計的。同一塊內容塞進 56% 寬的抽屜之後，
           * 視窗還是 `md` 以上（所以照樣畫兩欄），但右欄只剩四百多 px，
           * 「最終值」那一格被壓到只看得見前兩個字（`60000.00` 變成 `60`）。
           *
           * 視窗寬度回答不了「**這塊內容有多少空間**」——
           * 同一個元件現在有兩個容器，只有容器查詢問得對。
           */
          // 真的很窄（手機）才堆疊 —— 這一頁本來就只給桌機用。
          display: 'block',
          '& > :not(:first-of-type)': { mt: 2 },
          /**
           * **抽屜（約 800px）也要維持雙欄。**
           *
           * 第一版把門檻設在 900px，於是抽屜退化成單欄堆疊 ——
           * 而左欄的原檔示意圖是 `aspectRatio: 1/1.414`（A4），
           * 一變成全寬就從 360px 高長到一千多 px，
           * **把右邊的「辨識值 vs 最終值」整個推到看不見的地方**。
           *
           * 那比原本「欄位被壓扁」更糟：壓扁還看得到，推走就等於沒有。
           * 而且 crawler 全綠 —— 它驗的是「抽屜開得起來」，
           * 不是「打開之後**看得到要改的欄位**」。那條斷言後來補上了。
           */
          '@container (min-width: 640px)': {
            display: 'grid',
            // 抽屜裡左欄縮到 220，右欄才有足夠空間放四欄表格。
            /**
             * **左欄要一開始就夠大**。
             *
             * 320px 的 A4 讀不出紙上的數字 —— 於是「放大」變成必經動作，
             * 而校對本來就是「左邊看紙、右邊改值」來回好幾次的事。
             * 給到 480px（螢幕窄時退回 320）之後，多數欄位不點開就對得起來；
             * 放大鏡留給真的要逐字核對的那一次。
             */
            /**
             * 寬度用**容器的百分比**，不是 viewport 斷點。
             *
             * 這一格會出現在兩種寬度裡：整頁（~1150px）與佇列抽屜（明顯更窄）。
             * 用 `md`/`lg` 斷點的話，抽屜裡拿到的是「整頁該有的寬度」——
             * 於是欄位被擠到左欄下面，變成上下堆疊，而那正是 §8 驗收在守的東西。
             * 34% 在整頁約 390px（讀得出紙上的數字），在抽屜裡也還有 300px 上下。
             */
            gridTemplateColumns: 'minmax(260px, 34%) 1fr',
            gap: 2,
            alignItems: 'start',
            '& > :not(:first-of-type)': { mt: 0 },
          },
        }}
      >
        {/*
          左欄本身**一定存在**，無論這張單有沒有掛原始檔案
          （`LIFECYCLE.md §6.7.2`：沒有原檔時這裡顯示提示與補件入口）。

          `data-testid` 掛在**卡片**上不是掛在示意圖上：
          示意圖只有「有原檔」時才畫，拿它當「左欄在不在」的探針，
          會在 seed 沒走過收件台的環境下量到 `null` ——
          而那會被誤讀成「版面壞了」。
        */}
        <Card data-testid="review-left-column">
          <CardContent>
            <Typography variant="subtitle2" gutterBottom>{translate('energy.review.source')}</Typography>
            {/* **真的那份檔案** ＋ 欄位高亮（`RECOGNITION.md §7/§9-5`）。
                在此之前這裡是一個空的 A4 框加一組寫死的假座標 ——
                而「每一站都能展開原文件反覆比對」是這個系統的核心工作方式：
                主管核准的動作，本質上就是拿螢幕上的數字對紙上的數字。
                這一格已抽進 `shared.tsx`，收件頁的就地校對用的是**同一個元件**。 */}
            <SourcePreviewFrame
              label={translate('energy.review.sourceLayout')}
              attachmentId={sourceAttachmentId}
              boxes={boxes}
              focusField={focusField}
              // 框旁邊標**欄位名**：不標的話使用者看到一個框，但不知道那是哪一格的。
              boxLabel={(f) => translate(billFieldLabelKey(f, bill?.templateId), { _: f })}
            />
            {/* 左欄的原檔 —— **五個模式都有，沒有例外**（`LIFECYCLE.md §6.7.2`）。
                這一格不依 `mode` 變：`readonly` 的人一樣要看得到那張紙。 */}
            {sourceAttachmentId ? (
              <Stack gap={1} data-testid="review-source" data-attachment={sourceAttachmentId}>
                {/* 預覽在上面那一格。這顆是「開成新分頁看大的／存下來」的出口 ——
                    A4 框裡讀得了數字，但真要逐字核對還是會想開全螢幕。 */}
                <Button
                  variant="outlined"
                  startIcon={<DownloadIcon />}
                  href={attachmentDownloadUrl(sourceAttachmentId, orgId)}
                  target="_blank"
                  rel="noreferrer"
                  data-testid="review-open-source"
                >
                  {translate('energy.review.openSource')}
                </Button>
              </Stack>
            ) : (
              <Stack gap={1} data-testid="review-no-source">
                <Typography variant="body2" color="text.secondary">
                  {translate('energy.review.noSource')}
                </Typography>
                {/* 沒有原檔時給出口 —— 只說「沒有」而不給補的方法，
                    使用者唯一能做的就是關掉頁面。
                    `canAddAttachment` 比 `canEditSubject` **寬**：
                    `act` 不能改欄位，但補得了件。 */}
                {bill?.canAddAttachment && execution && (
                  <>
                    <input
                      ref={sourceInputRef}
                      type="file"
                      hidden
                      data-testid="review-source-input"
                      onChange={(e) => {
                        const f = e.target.files?.[0];
                        if (f) uploadAttachment(f, 'source');
                      }}
                    />
                    <Button
                      size="small"
                      variant="outlined"
                      disabled={uploading}
                      data-testid="review-upload-source"
                      onClick={() => sourceInputRef.current?.click()}
                    >
                      {translate('energy.review.uploadSource')}
                    </Button>
                  </>
                )}
              </Stack>
            )}

            <Divider sx={{ my: 2 }} />
            <Stack gap={0.5}>
              <Typography variant="caption" color="text.secondary">
                {translate('energy.bills.customerNumber')}
              </Typography>
              <Typography variant="body2" sx={{ fontVariantNumeric: 'tabular-nums' }}>
                {bill?.station?.customerNumber || bill?.customerNumberRaw || '—'}
              </Typography>
              {/* S1：有案場有合約時，詳情要看得到比對到哪一份合約。 */}
              {bill?.contractId != null && (
                <>
                  <Typography variant="caption" color="text.secondary" sx={{ mt: 1 }}>
                    {translate('energy.bills.contractNo')}
                  </Typography>
                  <Typography variant="body2" data-testid="bill-contract-no" sx={{ fontVariantNumeric: 'tabular-nums' }}>
                    {bill?.contractNo || `#${bill.contractId}`}
                  </Typography>
                </>
              )}
              {!bill?.stationId && (
                // 沒指認案場 ＝ 例外件，不能標記為已校對（後端也擋）。
                // 指認的動作**就在這裡**（§7.4）——
                // 前一版那句話叫使用者「請先到『例外件』指認」，
                // 而那一頁已經不存在了；只說問題不給出口，
                // 使用者唯一能做的就是關掉頁面然後困惑。
                <Alert
                  severity="warning"
                  data-testid="review-needs-match"
                  sx={{
                    mt: 1, color: jvTokens.warn, borderColor: jvTokens.warn,
                    border: '1px solid', backgroundColor: 'transparent',
                  }}
                  action={bill?.canEditSubject ? (
                    <Button
                      size="small"
                      startIcon={<LinkIcon />}
                      data-testid="review-match-station"
                      sx={{ color: jvTokens.warn }}
                      onClick={openMatch}
                    >
                      {translate('energy.unmatched.match')}
                    </Button>
                  ) : undefined}
                >
                  {translate('energy.review.needsMatch')}
                </Alert>
              )}
            </Stack>
          </CardContent>
        </Card>

        <Card>
          <CardContent>
            <Stack direction="row" alignItems="center" justifyContent="space-between" mb={1}>
              <Typography variant="subtitle2">{translate('energy.review.compare')}</Typography>
              <Stack direction="row" gap={1}>
                <Button
                  size="small"
                  variant="contained"
                  startIcon={<SaveIcon />}
                  disabled={!dirty || !bill?.canEditSubject}
                  onClick={save}
                  data-testid="review-save"
                >
                  {translate('energy.review.save')}
                </Button>
                <Button
                  size="small"
                  variant="outlined"
                  disabled={!bill?.canEditSubject || !bill?.stationId || bill?.reviewStatus === 'reviewed'}
                  onClick={markReviewed}
                  data-testid="review-mark"
                >
                  {translate('energy.review.markReviewed')}
                </Button>
              </Stack>
            </Stack>
            {/* ②三處變之二：`readonly` **一定要印出 `modeReason`**
                （`LIFECYCLE.md §6.4`）—— 一個什麼都不能按、又不解釋為什麼的頁面，
                使用者只會以為它壞了。 */}
            {!bill?.canEditSubject && bill?.modeReason && (
              <Box
                mb={1}
                data-testid="review-mode-reason"
                data-mode={bill?.mode ?? ''}
                data-code={bill?.modeReason?.code ?? ''}
              >
                {modeSentence()}
              </Box>
            )}
            <Typography variant="caption" color="text.secondary" display="block" mb={1}>
              {translate('energy.review.compareHint')}
            </Typography>
            {/* 「辨識值 vs 最終值」是**全站唯一一份**（`FieldDiffGrid`）——
                送件詳情頁的就地校對區 render 的是同一個元件。
                各寫一份的話，「OCR 準不準」的答案會依你從哪一頁看而不同。 */}
            <RecognitionNote summary={recognition} />
            {/**
              * **單別是這張單最重要的一個辨識結果** —— 它決定要開哪些流程。
              *
              * 它以前只以一句話出現（「這張單會走：電費單核可流程 v2」），
              * 沒有任何入口可以說「不對，這是用電單」。判錯的後果不是
              * 「有個欄位錯了」，是**整張單進到別人的流程**，
              * 而送審那一刻流程就開了 —— 所以它必須在**送審前**改得動。
              */}
            <BillDocTypeField
              orgId={orgId}
              billId={bill?.id ?? null}
              docType={bill?.docType ?? null}
              recognizedDocType={bill?.recognizedDocType ?? null}
              binding={bill?.binding ?? null}
              // 單別的可改與否**比其他欄位嚴**：流程一開就鎖（後端算的）。
              editable={bill?.docTypeEditable !== false && !!bill?.canEditSubject}
              started={(bill?.executions ?? []).length > 0}
              onSaved={load}
            />
            <FieldDiffGrid
              fields={reviewFields}
              templateId={bill?.templateId}
              // 哪些欄位在紙上找得到位置 —— 色點實心／空心靠它。
              boxedFields={boxes.map((b) => b.field)}
              ocrExtracted={bill?.ocrExtracted}
              changedFields={bill?.changedFields}
              draft={draft}
              onChange={(f, v) => setDraft({ ...draft, [f]: v })}
              editable={!!bill?.canEditSubject}
              rowTestId="review-field"
              inputTestId={(f) => `final-${f}`}
              ocrTestId="ocr-value"
              notExtractedLabel={translate('energy.review.notExtracted')}
              // 增補 B-1：**校對的動作本質是「拿螢幕上的數字對紙上的數字」**。
              // 點這一列 → 左欄原檔把對應位置框起來。
              focusedField={focusField as any}
              onFocusField={(f) => setFocusField(f)}
              extraMark={(f) => (
                // `FRONTEND.md §2.86`：在**欄位那一列**也標出來 ——
                // 只在下方流程圖旁邊講的話，正在改數字的人不會看到。
                f === 'totalAmount' && routeChanged ? (
                  <Chip
                    size="small"
                    data-testid="field-route-changed"
                    data-direction={(branch?.removed ?? []).length > 0 ? 'removed' : 'added'}
                    label={translate('energy.review.fieldRouteChanged')}
                    sx={{
                      mb: 0.5,
                      display: 'flex',
                      background: (branch?.removed ?? []).length > 0 ? jvTokens.gold : jvTokens.warn,
                      color: (branch?.removed ?? []).length > 0 ? '#000' : '#fff',
                      fontWeight: 700,
                      height: 20,
                    }}
                  />
                ) : null
              )}
            />
            {/* 卡 B S3：電費單明細 19 欄（整併版「電費單明細」DB-088～DB-107 加契約編號／區處），
                可校對；跟上面 7 欄同一個元件、同一套「辨識值 vs 最終值」規則，只是 fields 換一張表。 */}
            <Divider sx={{ my: 2 }} />
            <Typography variant="subtitle2" gutterBottom>{translate('energy.bills.detailTitle')}</Typography>
            <FieldDiffGrid
              fields={BILL_DETAIL_FIELD_KEYS}
              ocrExtracted={bill?.ocrExtracted}
              changedFields={bill?.detailChangedFields}
              draft={detailDraft as any}
              onChange={(f, v) => setDetailDraft({ ...detailDraft, [f]: v })}
              editable={!!bill?.canEditSubject}
              rowTestId="review-detail-field"
              inputTestId={(f) => `final-detail-${f}`}
              ocrTestId="ocr-detail-value"
              notExtractedLabel={translate('energy.review.notExtracted')}
            />
          </CardContent>
        </Card>
      </Box>
      </Box>

      {/* ---- 流程圖（C2 的元件）＋ 動作按鈕列 ---- */}
      <Card sx={{ mt: 2 }}>
        <CardContent>
          <Typography variant="subtitle2" gutterBottom>{translate('energy.review.flow')}</Typography>
          {/* 增補 B-2：改金額 → **這條路線立刻重算**。
              「改成 12 萬會多一關高層加簽」要在**按送審之前**看到。 */}
          {branch?.available && (
            <Box
              data-testid="branch-preview"
              data-added={(branch.nodes ?? []).filter((n: any) => n.added).map((n: any) => n.nodeKey).join(',')}
              sx={{
                mb: 1.5, p: 1, borderRadius: 1,
                border: `1px solid ${jvTokens.hairline}`, background: jvTokens.hoverBg,
              }}
            >
              <Typography variant="caption" color="text.secondary" display="block" mb={0.5}>
                {translate('energy.review.branchPreview', {
                  name: branch.definitionName, version: branch.version,
                })}
              </Typography>
              <Stack direction="row" gap={0.5} flexWrap="wrap" alignItems="center">
                {(branch.nodes ?? []).map((n: any, i: number) => (
                  <React.Fragment key={n.nodeKey}>
                    {i > 0 && <Typography variant="caption" color="text.secondary">›</Typography>}
                    <Chip
                      size="small"
                      variant={n.added ? 'filled' : 'outlined'}
                      data-testid="branch-node"
                      data-node={n.nodeKey}
                      data-added={n.added ? '1' : '0'}
                      label={n.added ? translate('energy.review.branchAdded', { name: n.name }) : n.name}
                      sx={n.added
                        // 多出來的關卡用**燒橘**（不是紅：多一關不是錯誤）。
                        ? { background: jvTokens.warn, color: '#fff', fontWeight: 700 }
                        : { color: jvTokens.caption, borderColor: jvTokens.border }}
                    />
                  </React.Fragment>
                ))}
              </Stack>

              {/* **少一關比多一關更需要看見**（`FRONTEND.md §2.86`）。
                  把 138,195 改成 99,999，高層加簽那關就消失了 ——
                  在此之前畫面上**什麼都沒發生**，而那正是規避大額控管最省事的辦法。
                  不擋、不跳確認框：OCR 可能就是抓錯，人有權改。
                  要的是他**看得見自己做了什麼**，而且稽核留得住（`AUDIT.md §7.5`）。 */}
              {(branch.removed ?? []).length > 0 && (
                <Stack
                  direction="row"
                  gap={0.75}
                  alignItems="center"
                  flexWrap="wrap"
                  mt={1}
                  data-testid="branch-removed"
                  data-removed={(branch.removed ?? []).map((n: any) => n.nodeKey).join(',')}
                >
                  {(branch.removed ?? []).map((n: any) => (
                    <Chip
                      key={n.nodeKey}
                      size="small"
                      data-testid="branch-removed-node"
                      data-node={n.nodeKey}
                      label={translate('energy.review.branchRemoved', { name: n.name })}
                      // **金色** —— 與「多一關」的燒橘刻意不同色：
                      // 兩個方向長一樣的話，最危險的那個就被稀釋掉了。
                      sx={{
                        background: jvTokens.gold,
                        color: '#000',
                        fontWeight: 700,
                      }}
                    />
                  ))}
                  <Typography variant="caption" sx={{ color: jvTokens.gold, fontWeight: 600 }}>
                    {translate('energy.review.branchRemovedHint')}
                  </Typography>
                </Stack>
              )}
            </Box>
          )}
          {execution ? (
            <>
              <FlowGraph organizationId={orgId} executionId={execution.id} height={300} />
              {/* 「本關要求」：這一關必傳的附件類別、必填欄位，缺的標紅（流程引擎 S1）。
                  按鈕照常可按，缺東西按下去才擋（S2）——不在這裡就 disable 按鈕。 */}
              {requirements.length > 0 && (
                <Box mt={2} data-testid="step-requirements">
                  <Typography variant="caption" sx={{ fontWeight: 700, display: 'block', mb: 0.5 }}>
                    {translate('energy.stepRequirements.title')}
                  </Typography>
                  <Stack direction="row" gap={0.75} flexWrap="wrap">
                    {requirements.map((r: any, i: number) => {
                      const label = r.type === 'attachment'
                        ? translate(`energy.attachmentCategory.${r.category}`, { _: r.category })
                        : translate(r.labelKey, { _: r.path });
                      return (
                        <Chip
                          key={`req-${r.type}-${r.type === 'attachment' ? r.category : r.path}-${i}`}
                          size="small"
                          data-testid="req-item"
                          data-met={r.met ? '1' : '0'}
                          label={`${r.met ? '✓' : '✗'} ${label}`}
                          variant="outlined"
                          sx={r.met
                            ? { color: jvTokens.caption, borderColor: jvTokens.border }
                            : { color: jvTokens.warn, borderColor: jvTokens.warn, fontWeight: 700 }}
                        />
                      );
                    })}
                  </Stack>
                </Box>
              )}
              {/* 被擋提示（S2）：按動作被 422 step_requirements_unmet 擋下時，逐項列出缺項。 */}
              {requirementsBlocked && (
                <Alert
                  severity="warning"
                  data-testid="step-requirements-blocked"
                  sx={{ mt: 1.5, border: `1px solid ${jvTokens.warn}`, backgroundColor: 'transparent', color: jvTokens.warn }}
                >
                  {translate('energy.stepRequirements.blocked')}
                  {'：'}
                  {[
                    ...requirementsBlocked.missingAttachments.map((c) => translate(`energy.attachmentCategory.${c}`, { _: c })),
                    ...requirementsBlocked.missingFields.map((p) => {
                      const match = requirements.find((r: any) => r.type === 'field' && r.path === p);
                      return match ? translate(match.labelKey, { _: p }) : p;
                    }),
                  ].join('、')}
                </Alert>
              )}
              {/* 動作按鈕在**圖外** —— 流程圖不是操作介面（`FRONTEND.md §3`）。
                  而且**不畫按不動的按鈕**：不能做時 `ActionBar` 會改印那一句原因
                  （C7 增補 A —— 這是它必須出現的三個地方之一）。 */}
              <Box mt={2} data-testid="review-actions" data-count={openTaskId ? actions.length : 0}>
                {openTaskId ? (
                  queue ? queueActionBar() : (
                    /**
                     * **還沒認領就只給「認領並開始處理」**，不畫那些按了會 403 的動作。
                     * 與佇列抽屜同一套語意（`LIFECYCLE.md §8.5`：認領是「開始處理」的
                     * 第一步）—— 兩邊不一致的話，同一件事在兩個入口的行為會不同。
                     */
                    claimState?.claimable && !claimState.claimedByMe ? (
                      <Stack direction="row" gap={1} flexWrap="wrap">
                        <Button
                          variant="contained"
                          size="small"
                          data-testid="review-claim"
                          onClick={claimAndStart}
                        >
                          {translate('energy.myWork.claimAndStart')}
                        </Button>
                      </Stack>
                    ) : (
                      <ActionBar
                        actions={actions}
                        blocked={blocked}
                        onAction={(a) => {
                          if (a.requiresReason) { setReason(''); setPendingAction(a); return; }
                          runAction(a);
                        }}
                      />
                    )
                  )
                ) : (
                  // 沒有開著的關卡（走完了、或計算節點正在跑）——
                  // 這時「為什麼沒有按鈕」是**頁面層**的問題，答案在 `modeReason`。
                  modeSentence()
                )}
              </Box>
            </>
          ) : (
            /**
             * 還沒送審。`intake` 模式的人**要能直接送**（那是這一頁的出口）；
             * 其他人看到的是 `notSubmitted` 那一句。
             */
            <Stack gap={1} data-testid="review-no-execution">
              {/* `intake` 的 `modeReason` 是 null（沒有什麼要解釋的，送審鍵就在旁邊）。
                  這裡若無條件呼叫 `modeSentence()`，還沒送審的單會印出
                  「目前這一關沒有待處理的任務」—— 而它根本還沒有任何一關。 */}
              {bill?.modeReason && modeSentence()}
              {bill?.mode === 'intake' && (
                <Box>
                  <Button
                    variant="contained"
                    size="small"
                    disabled={!bill?.stationId || submitting}
                    data-testid="review-submit"
                    onClick={submitBill}
                  >
                    {translate('energy.bills.submit')}
                  </Button>
                  {!bill?.stationId && (
                    <Typography variant="caption" color="text.secondary" display="block" mt={0.5}>
                      {translate('energy.review.needsMatch')}
                    </Typography>
                  )}
                </Box>
              )}
            </Stack>
          )}
        </CardContent>
      </Card>

      {/* ---- 附件 ｜ 稽核歷程 ｜ 血緣 ---- */}
      <Card sx={{ mt: 2 }}>
        <CardContent>
          <Tabs value={tab} onChange={(_, v) => setTab(v)} variant="scrollable">
            {/* 計數要用**列出來的那一份**（`extraAttachments`）——
                頁籤寫 2 而點開只有 1 筆，使用者會以為有一份載不出來。 */}
            <Tab label={`${translate('energy.review.tabAttachments')}（${extraAttachments.length}）`} />
            {/* 卡 G3：沒權限時頁籤寫「稽核（—）」，不是「稽核（0）」——後者看起來像「查過了，沒有」。 */}
            <Tab label={`${translate('energy.review.tabAudit')}（${auditDenied ? '—' : audit.length}）`} />
            <Tab label={`${translate('energy.review.tabLineage')}（${lineage.length}）`} />
            {/* US-6：財務要看得到「它是怎麼算的」。 */}
            <Tab label={`${translate('energy.review.tabPayouts')}（${payouts.length}）`} />
          </Tabs>

          {tab === 0 && (
            <>
              {/* 補件（`LIFECYCLE.md §6.7.3`）——
                  `canAddAttachment` 為 true 才畫。它比 `canEditSubject` **寬**：
                  主管說「缺一張證明」時，`act` 模式的人要有地方上傳。
                  補件不推進流程、不解除任何鎖。 */}
              <Stack direction="row" justifyContent="flex-end" mt={1}>
                {bill?.canAddAttachment && execution && (
                  <>
                    <input
                      ref={addedInputRef}
                      type="file"
                      hidden
                      data-testid="review-added-input"
                      onChange={(e) => {
                        const f = e.target.files?.[0];
                        if (f) uploadAttachment(f, 'added');
                      }}
                    />
                    <Button
                      size="small"
                      variant="outlined"
                      disabled={uploading}
                      data-testid="review-add-attachment"
                      onClick={() => addedInputRef.current?.click()}
                    >
                      {translate('energy.review.addAttachment')}
                    </Button>
                  </>
                )}
              </Stack>
              <Table size="small" sx={{ mt: 1 }}>
                <TableHead>
                  <TableRow>
                    <TableCell>{translate('energy.review.filename')}</TableCell>
                    <TableCell>{translate('energy.review.attachmentRole')}</TableCell>
                    <TableCell>{translate('energy.attachmentClassify.columnTitle')}</TableCell>
                    <TableCell>{translate('energy.review.size')}</TableCell>
                    <TableCell align="right" />
                  </TableRow>
                </TableHead>
                <TableBody>
                  {extraAttachments.length ? extraAttachments.map((a) => {
                    // 一份檔案可能有多個 link（同一份掛在不同關卡）——
                    // 取第一個當它的用途；沒有 link 就是 `submitted`（DB 的預設）。
                    const role = String((a.links ?? [])[0]?.role ?? 'submitted');
                    return (
                      <TableRow key={a.id} data-testid="review-attachment" data-role={role}>
                        <TableCell>{a.originalFilename}</TableCell>
                        <TableCell>
                          <Chip
                            size="small"
                            variant="outlined"
                            label={translate(`energy.attachmentRole.${role}`, { _: role })}
                            sx={role === 'added'
                              ? { color: jvTokens.gold, borderColor: jvTokens.gold }
                              : { color: jvTokens.caption, borderColor: jvTokens.border }}
                          />
                        </TableCell>
                        <TableCell data-testid="att-classify" data-state={a.classifyState ?? ''}>
                          {a.classifyState === 'recognizing' && (
                            <Typography variant="caption" color="text.secondary">
                              {translate('energy.attachmentClassify.recognizing')}
                            </Typography>
                          )}
                          {a.classifyState === 'auto' && (
                            <Typography variant="caption">
                              {translate('energy.attachmentClassify.auto', {
                                category: translate(`energy.attachmentCategory.${a.category}`, { _: a.category }),
                              })}
                            </Typography>
                          )}
                          {(a.classifyState === 'needsCategory' || a.classifyState === 'manual') && (
                            <Select
                              size="small"
                              value={a.category ?? ''}
                              displayEmpty
                              data-testid="att-category-select"
                              onChange={(e) => setAttachmentCategory(a.id, String(e.target.value))}
                              sx={{ minWidth: 160, background: '#fff' }}
                            >
                              <MenuItem value="" disabled data-testid="att-category-placeholder">
                                {translate('energy.attachmentClassify.pickCategory')}
                              </MenuItem>
                              {ATTACHMENT_CATEGORIES.map((c) => (
                                <MenuItem key={c} value={c} data-testid="att-category-option" data-category={c}>
                                  {translate(`energy.attachmentCategory.${c}`)}
                                </MenuItem>
                              ))}
                            </Select>
                          )}
                          {role === 'source' && !a.classifyState && (
                            <Typography variant="caption">
                              {translate(`energy.attachmentCategory.${a.category ?? 'taipowerNotice'}`)}
                            </Typography>
                          )}
                        </TableCell>
                        <TableCell>{Math.ceil(Number(a.size) / 1024)} KB</TableCell>
                        <TableCell align="right">
                          <IconButton
                            size="small"
                            href={attachmentDownloadUrl(a.id, orgId)}
                            target="_blank"
                            rel="noreferrer"
                          >
                            <DownloadIcon fontSize="small" />
                          </IconButton>
                        </TableCell>
                      </TableRow>
                    );
                  }) : <EmptyRow colSpan={5} />}
                </TableBody>
              </Table>
            </>
          )}

          {tab === 1 && (
            <Table size="small" sx={{ mt: 1 }} data-testid="review-audit">
              <TableHead>
                <TableRow>
                  <TableCell>{translate('energy.review.when')}</TableCell>
                  <TableCell>{translate('energy.review.what')}</TableCell>
                  <TableCell>{translate('energy.review.who')}</TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {audit.length ? audit.map((ev) => (
                  <TableRow key={ev.id}>
                    <TableCell sx={{ whiteSpace: 'nowrap' }}>
                      <Typography variant="caption">
                        {String(ev.occurredAt ?? '').slice(0, 19).replace('T', ' ')}
                      </Typography>
                    </TableCell>
                    <TableCell>
                      {/* 一律翻成中文，不印 raw type。翻不到就顯示 `—`。 */}
                      <Typography variant="body2">
                        {ev.summaryKey ? translate(ev.summaryKey, { _: '—' }) : '—'}
                      </Typography>
                    </TableCell>
                    <TableCell>
                      <Typography variant="body2">{ev.actorLabel || `#${ev.actorUserId ?? '—'}`}</Typography>
                      {/* 代理身分要看得出來（`AUDIT.md §4.2`）。 */}
                      {ev.onBehalfOfUserId ? (
                        <Typography variant="caption" sx={{ color: jvTokens.primary }}>
                          {translate('energy.review.onBehalfOf', { userId: ev.onBehalfOfUserId })}
                        </Typography>
                      ) : null}
                    </TableCell>
                  </TableRow>
                )) : auditDenied ? (
                  // 卡 G3（HF11 B 文案）：沒權限不能顯示成「沒有資料。」——
                  //    那會讓使用者以為真的查過、就是空的。真的沒資料時照舊是 EmptyRow。
                  <TableRow>
                    <TableCell colSpan={3}>
                      <Typography variant="body2" color="text.secondary">
                        {translate('energy.review.auditForbidden')}
                      </Typography>
                    </TableCell>
                  </TableRow>
                ) : <EmptyRow colSpan={3} />}
              </TableBody>
            </Table>
          )}

          {tab === 2 && (
            <Box mt={1}>
              {/* 血緣是**另一張圖**，節點是「一整個 execution」而不是「關卡」
                  （`WORKFLOW.md §4.9.6`）——不要跟上面的流程圖混在一起。 */}
              <Typography variant="caption" color="text.secondary" display="block" mb={1}>
                {translate('energy.review.lineageHint')}
              </Typography>
              <Table size="small">
                <TableHead>
                  <TableRow>
                    <TableCell>{translate('energy.review.lineageRelation')}</TableCell>
                    <TableCell>{translate('energy.review.lineageExecution')}</TableCell>
                  </TableRow>
                </TableHead>
                <TableBody>
                  {lineage.length ? lineage.map((l: any, i: number) => (
                    <TableRow key={l.id ?? i}>
                      <TableCell>{l.relation ?? l.kind ?? '—'}</TableCell>
                      <TableCell>#{l.executionId ?? l.upstreamExecutionId ?? l.id}</TableCell>
                    </TableRow>
                  )) : <EmptyRow colSpan={2} />}
                </TableBody>
              </Table>
            </Box>
          )}

          {/* 應付紀錄（US-6、`FRONTEND.md §10 P3`）——
              **算式 ＋ 策略版本 ＋ 參數快照**三樣缺一不可：
              只給一個金額，等於要財務相信一個黑盒子。
              被取代的版本也列出來並標狀態 —— 「這個數字改過」正是稽核要看的。 */}
          {tab === 3 && (
            <Table size="small" sx={{ mt: 1 }} data-testid="payout-table">
              <TableHead>
                <TableRow>
                  <TableCell>{translate('energy.review.payoutStrategy')}</TableCell>
                  <TableCell>{translate('energy.review.payoutFormula')}</TableCell>
                  <TableCell align="right">{translate('energy.bills.amount')}</TableCell>
                  <TableCell>{translate('energy.review.payoutStatus')}</TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {payouts.length ? payouts.map((r: any) => (
                  <TableRow key={r.id} data-testid="payout-row" data-status={r.status}>
                    <TableCell>
                      {/* 不印 strategyId —— 印「名稱 v版本」。 */}
                      <Typography variant="body2">
                        {r.strategyName ?? r.strategyType}
                        {r.strategyVersion != null ? ` v${r.strategyVersion}` : ''}
                      </Typography>
                      <Typography variant="caption" color="text.secondary">
                        {String(r.businessDate ?? '').slice(0, 10)}
                      </Typography>
                    </TableCell>
                    <TableCell>
                      {/* 參數快照 ＋ 輸入快照 ＝ 算式。是**當時**的快照，
                          不是現在去讀策略（策略改版後回頭看才不會對不上）。 */}
                      <Typography
                        variant="caption"
                        data-testid="payout-formula"
                        sx={{ fontFamily: 'monospace', color: jvTokens.caption }}
                      >
                        {JSON.stringify(r.paramsSnapshot ?? {})}
                        {' ← '}
                        {JSON.stringify(r.inputSnapshot ?? {})}
                      </Typography>
                    </TableCell>
                    <TableCell align="right">
                      <Typography variant="body2" sx={{ fontVariantNumeric: 'tabular-nums' }}>
                        {r.amount} {r.currency ?? ''}
                      </Typography>
                    </TableCell>
                    <TableCell>
                      <Chip
                        size="small"
                        variant="outlined"
                        label={translate(`energy.payoutStatus.${r.status}`, { _: r.status })}
                        sx={r.status === 'active'
                          ? { color: jvTokens.primary, borderColor: jvTokens.primary }
                          : { color: jvTokens.muted, borderColor: jvTokens.border }}
                      />
                    </TableCell>
                  </TableRow>
                )) : <EmptyRow colSpan={4} />}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

      {/* 三種重複，三句話，同一組出口（`LIFECYCLE.md §7.2`）。
          只說「被擋了」而不給出口，使用者唯一能做的就是關掉視窗然後困惑。 */}
      <Dialog open={!!dup} onClose={() => setDup(null)} fullWidth maxWidth="sm">
        <DialogTitle data-testid="review-dup-title">
          {dup ? translate(`energy.ingest.dup.${dup.code}Title`, { _: '' }) : ''}
        </DialogTitle>
        <DialogContent>
          <Typography variant="body2" data-testid="review-dup-body" data-code={dup?.code}>
            {(() => {
              if (!dup) return '';
              const d = dup.details ?? {};
              if (dup.code === 'bill_already_submitted') {
                return translate('energy.ingest.dup.alreadySubmitted', {
                  documentNo: d.documentNo ?? (d.subjectId != null ? `#${d.subjectId}` : '—'),
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
            })()}
          </Typography>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setDup(null)}>{translate('ra.action.close')}</Button>
          <Button
            data-testid="review-dup-goto"
            disabled={!dup?.details?.executionId && !dup?.details?.subjectId}
            onClick={() => {
              const d = dup?.details ?? {};
              setDup(null);
              if (d.executionId) { navigate(executionRoute(d.executionId)); return; }
              if (d.subjectId) navigate(billRoute(d.subjectId));
            }}
          >
            {translate('energy.ingest.dup.seeThatOne')}
          </Button>
          <Button
            variant="contained"
            sx={{ backgroundColor: jvTokens.warn }}
            data-testid="review-dup-discard"
            onClick={discardThis}
          >
            {translate('energy.ingest.dup.discardThis')}
          </Button>
        </DialogActions>
      </Dialog>

      {/* 指認案場（§7.4：從獨立的「例外件」頁搬過來的） */}
      <Dialog open={matchOpen} onClose={() => setMatchOpen(false)} fullWidth maxWidth="sm">
        <DialogTitle>{translate('energy.unmatched.matchTitle')}</DialogTitle>
        <DialogContent>
          <Box display="flex" flexDirection="column" gap={2} mt={1}>
            <Typography variant="body2" color="text.secondary">
              {translate('energy.unmatched.rawIs', { value: bill?.customerNumberRaw || '—' })}
            </Typography>
            <TextField
              size="small"
              label={translate('energy.unmatched.searchStation')}
              value={stationQuery}
              onChange={(e) => { setStationQuery(e.target.value); searchStations(e.target.value); }}
              inputProps={{ 'data-testid': 'station-search' }}
            />
            <TextField
              select
              label={translate('energy.unmatched.station')}
              value={pickedStation}
              onChange={(e) => setPickedStation(e.target.value)}
              helperText={stationOptions.length ? undefined : translate('energy.unmatched.noStationFound')}
              inputProps={{ 'data-testid': 'station-picker' }}
            >
              {stationOptions.map((st) => (
                <MenuItem key={st.id} value={String(st.id)}>
                  {`${st.name}（${st.customerNumber}）`}
                </MenuItem>
              ))}
            </TextField>
            {matchError && <Alert severity="error">{matchError}</Alert>}
          </Box>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setMatchOpen(false)}>{translate('ra.action.cancel')}</Button>
          <Button
            variant="contained"
            disabled={!pickedStation}
            data-testid="station-match-confirm"
            onClick={submitMatch}
          >
            {translate('energy.unmatched.match')}
          </Button>
        </DialogActions>
      </Dialog>

      {/* 需要理由的動作 */}
      <Dialog open={!!pendingAction} onClose={() => setPendingAction(null)} fullWidth maxWidth="xs">
        <DialogTitle>
          {pendingAction ? translate(pendingAction.labelKey, { _: pendingAction.actionKey }) : ''}
        </DialogTitle>
        <DialogContent>
          <Box display="flex" flexDirection="column" gap={2} mt={1}>
            <Typography variant="caption" color="text.secondary">
              {translate('energy.myTasks.reasonRequired')}
            </Typography>
            <TextField
              autoFocus multiline minRows={3}
              label={translate('energy.myTasks.reason')}
              value={reason}
              onChange={(e) => setReason(e.target.value)}
            />
          </Box>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setPendingAction(null)}>{translate('ra.action.cancel')}</Button>
          <Button
            variant="contained"
            disabled={!reason.trim()}
            onClick={() => pendingAction && runAction(pendingAction, reason.trim(), pendingThen)}
          >
            {translate('ra.action.confirm')}
          </Button>
        </DialogActions>
      </Dialog>
    </EnergyPageFrame>
  );
};

export default BillDetailBody;
