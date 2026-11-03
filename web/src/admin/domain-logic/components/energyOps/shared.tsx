/**
 * energy-ops-console 前端共用件。
 *
 * 存在的理由：P1/P2/P4/P5 四頁都要「選了組織沒？沒權限？載入中？」的同一段樣板，
 * 以及同一套狀態色。抄四份就會四份長歪 —— 尤其是**燒橘 vs 紅**這種一眼看不出的偏差。
 *
 * 色票一律從 `jvTokens` 拿（`FRONTEND.md §0.2`），頁面內不 hardcode 十六進位。
 * 警示用 `jvTokens.warn`（燒橘 #B4522C），**不是紅色**。
 */
import * as React from 'react';
import { energyHttp, attachmentDownloadUrl } from '@/api/energyLegacy';
import {
  Box, Alert, Chip, CircularProgress, Typography, TableRow, TableCell,
  Button, Dialog, DialogTitle, DialogContent, DialogActions,
} from '@mui/material';
import { useTranslate, usePermissions } from 'react-admin';
import { jvTokens } from '../../../themes/jvTheme';
import { colorForField, fieldFill } from './fieldColors';

export { jvTokens };

/**
 * 「等待幾天算久」—— 是參數不是猜測（`DESIGN.md §0` 第一原則）。
 * 改這個常數就好，不需要動 schema；日後要做成 per-org 設定時，
 * 這裡改成讀 API 回的值即可，版面不用動。
 */
export const WAITING_DAYS_WARN = 3;

/** 狀態 → 顏色。不吃 raw key 當文案，只當查表的鍵。 */
const STATUS_COLOR: Record<string, string> = {
  // 電費單校對狀態
  pendingMatch: jvTokens.warn,
  pendingReview: jvTokens.gold,
  reviewed: jvTokens.ok,
  // 發電站
  construction: jvTokens.gold,
  operating: jvTokens.ok,
  abnormal: jvTokens.warn,
  // 期數／行事曆
  pending: jvTokens.gold,
  paid: jvTokens.ok,
  done: jvTokens.ok,
  missed: jvTokens.warn,
  cancelled: jvTokens.muted,
  // 主檔
  active: jvTokens.ok,
  archived: jvTokens.muted,
};

/**
 * 狀態 pill。
 *
 * **畫面上不出現 raw key**：`label` 一律是 i18n 翻譯後的字串；
 * 翻不到就顯示 `—`（不 fallback 成 key —— 那正是要避免的東西）。
 */
export const StatusChip: React.FC<{ i18nKey: string; value?: string | null }> = ({ i18nKey, value }) => {
  const translate = useTranslate();
  if (!value) return <Typography variant="caption" color="text.secondary">—</Typography>;
  const full = `${i18nKey}.${value}`;
  const label = translate(full, { _: '' });
  const color = STATUS_COLOR[value] ?? jvTokens.muted;
  return (
    <Chip
      size="small"
      variant="outlined"
      label={label || '—'}
      sx={{ color, borderColor: color, fontWeight: 600 }}
    />
  );
};

/** 金額：後端 DECIMAL 回的是**字串**，不要先 Number 再 toFixed（大數會失精度）。 */
export const Money: React.FC<{ value?: string | number | null; currency?: string }> = ({ value, currency }) => {
  if (value === null || value === undefined || value === '') {
    return <Typography component="span" variant="body2" color="text.secondary">—</Typography>;
  }
  const s = String(value);
  const neg = s.startsWith('-');
  const [intPart, frac] = (neg ? s.slice(1) : s).split('.');
  const grouped = intPart.replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  return (
    <Typography component="span" variant="body2" sx={{ fontVariantNumeric: 'tabular-nums' }}>
      {neg ? '-' : ''}{grouped}{frac ? `.${frac}` : ''}{currency ? ` ${currency}` : ''}
    </Typography>
  );
};

/** 期間：`2026-01-01 ~ 2026-02-28`；缺一邊就只顯示有的那邊。 */
export const Period: React.FC<{ from?: string | null; to?: string | null }> = ({ from, to }) => {
  const d = (v?: string | null) => (v ? String(v).slice(0, 10) : '');
  if (!from && !to) return <Typography component="span" variant="body2" color="text.secondary">—</Typography>;
  return <Typography component="span" variant="body2">{`${d(from)} ~ ${d(to)}`}</Typography>;
};

/**
 * 「等待 N 天」—— 超過 `WAITING_DAYS_WARN` 用**燒橘**（不是紅）。
 */
export const WaitingDays: React.FC<{ days?: number | null }> = ({ days }) => {
  const translate = useTranslate();
  if (days === null || days === undefined) {
    return <Typography component="span" variant="body2" color="text.secondary">—</Typography>;
  }
  const hot = days >= WAITING_DAYS_WARN;
  return (
    <Typography
      component="span"
      variant="body2"
      sx={{ color: hot ? jvTokens.warn : 'text.secondary', fontWeight: hot ? 700 : 400 }}
    >
      {translate('energy.common.days', { smart_count: days, days })}
    </Typography>
  );
};

/**
 * **「我看得到全組織的流程單嗎」** —— 前端這一份要與後端
 * `AcEnergyOpsController.hasViewAllExecutions()` 用**同一組** permission，
 * 否則會出現「選單看得到、點進去 403」。
 *
 * `ryxis:rbac:manage` 是**加入**不是取代（陷阱 #11：那是 console 的萬能鑰匙，
 * 取代掉會把現有管理者擋在自己的 console 外面）。
 *
 * `permissions` 還沒載入時（null/undefined）回 `false` —— **保守的那一邊**：
 * 先當自己只看得到參與過的，載入完再放寬。反過來會讓非 view-all 的人
 * 在載入的那一瞬間打出 `scope=all` 而吃到 403。
 */
export const useCanViewAllExecutions = (): boolean => {
  const { permissions } = usePermissions<string[]>();
  if (!permissions) return false;
  return permissions.includes('*')
    || permissions.includes('energy:flow:view-all')
    || permissions.includes('ryxis:rbac:manage');
};

/**
 * 四頁共用的外框：處理「沒選組織 / 沒權限 / 載入中」三種狀態。
 *
 * 前端 gating 只是 UX —— **後端 BFF 才是真相**。
 * 這裡把 403 畫成一句話，不是把資料藏起來（資料根本沒回來過）。
 */
export const EnergyPageFrame: React.FC<{
  orgId?: number | null;
  denied: boolean;
  loading: boolean;
  children: React.ReactNode;
}> = ({ orgId, denied, loading, children }) => {
  const translate = useTranslate();
  if (!orgId) {
    return <Box p={3}><Alert severity="info">{translate('energy.common.selectOrg')}</Alert></Box>;
  }
  if (denied) {
    return <Box p={3}><Alert severity="warning">{translate('energy.common.noAccess')}</Alert></Box>;
  }
  return (
    /**
     * `position: relative` 是給下面那個 overlay 用的錨點。
     *
     * **載入指示不可以佔版面空間。**
     * 原本是把 spinner 當成一個正常元素插在 `children` **上面**：
     * `loading` 一 true 就多出 56px（`p={2}` 的 16 ＋ 24 ＋ 16），
     * 底下所有內容往下掉 56px，載完移除又彈回來 —— 那就是「閃動」。
     *
     * 而每一個成功的動作都是「`notify()` ＋ `load()`」，
     * 所以使用者看到的是**每次跳 popup 畫面就抖一下** ——
     * 抖動跟 popup 沒有關係，它們只是同時發生。
     * 這一頁改一次，所有用 `EnergyPageFrame` 的頁面一起好。
     */
    <Box p={2} position="relative">
      {loading && (
        <Box
          data-testid="page-loading"
          sx={{
            // absolute ＝ 疊在內容上，**不參與版面計算**。
            position: 'absolute',
            top: 8,
            left: 0,
            right: 0,
            display: 'flex',
            justifyContent: 'center',
            zIndex: 2,
            // 蓋在內容上時不要把下面的按鈕變成點不到。
            pointerEvents: 'none',
          }}
        >
          <CircularProgress size={24} />
        </Box>
      )}
      {children}
    </Box>
  );
};

/** 表格沒資料時的那一列。不要留空白 —— 使用者分不出「沒資料」與「壞掉」。 */
/** 欄位在原檔上的位置（比例座標 0~1）。真 OCR 回 box 時換資料來源即可。 */
export interface SourceFieldBox {
  field: string;
  page: number;
  x: number;
  y: number;
  width: number;
  height: number;
}

/**
 * **原檔預覽**——校對時左欄那一格。
 *
 * 這一格在「校對電費單」（`BillDetailBody`）與「收件批次的就地校對」
 * （`IngestBatchDetailPage`）**是同一件事**，所以只能有一份實作。
 *
 * 它顯示的是**真的那份檔案**（`RECOGNITION.md §7`），從授權下載端點取 blob。
 * 在此之前這裡是一個**空的 A4 框** —— 而「每一站都能展開原文件反覆比對」
 * 是這個系統的核心工作方式：主管核准的動作本質上就是拿螢幕上的數字對紙上的數字。
 *
 * ### 為什麼 PDF 要自己 render（pdf.js），不用 `<iframe>`
 *
 * `<iframe src="blob:…">` 走的是**瀏覽器內建的 viewer**：開得起來、可捲可縮，
 * 但它裡面捲到第幾頁、縮放多少、頁面留白多寬，我們**一無所知**。
 * 於是疊在上面的高亮框**對不準** —— 而指錯位置比沒有位置更糟，
 * 因為覆核的人整件事就是在比對。
 *
 * 自己把該頁畫進 `<canvas>` 之後，canvas 的 CSS 尺寸是我們給的，
 * 服務回的 0–1 相對座標乘上去就是**真的那個位置**。
 *
 * ### 沒有座標是正常結果
 *
 * **沒有座標就不要畫框**。辨識服務只在能**唯一定位**到那個 token 時才給 box
 * （同一個數字在頁面上出現兩次就不給，`RECOGNITION.md §9-5`），
 * 而 OCR 勝出的欄位也可能沒有。畫面要能接受「這一格沒得高亮」。
 *
 * `data-testid="source-preview"` 與框上的 `data-field` / `data-active`
 * 是既有 crawler 的探針，**搬家時一個字都不能改**。
 */
interface SourceDocProps {
  blob: Blob | null;
  isPdf: boolean;
  imgUrl: string | null;
  page: number;
  onPageCount: (n: number) => void;
  onFailed: () => void;
  /** 這一頁**畫出來了沒**。用來決定要不要顯示載入指示。 */
  onPainted?: (painted: boolean) => void;
  boxes: SourceFieldBox[];
  focusField?: string | null;
  /** 框旁邊要標什麼字（呼叫端翻譯；沒給就不標）。 */
  boxLabel?: (field: string) => string;
  label: string;
}

/**
 * **把原檔畫出來 ＋ 把框疊上去。**
 *
 * 抽成獨立元件是因為它要出現**兩次**：左欄那一小格，以及放大之後的對話框。
 * 兩份各寫一次的話，「框對得準不準」這件事就會依你從哪裡看而不同 ——
 * 而那正好是整格存在的理由。
 */
const SourceDoc: React.FC<SourceDocProps> = ({
  blob, isPdf, imgUrl, page, onPageCount, onFailed, onPainted,
  boxes, focusField = null, boxLabel, label,
}) => {
  const canvasRef = React.useRef<HTMLCanvasElement | null>(null);
  const wrapRef = React.useRef<HTMLDivElement | null>(null);
  /** 容器寬度變了就重畫（放大對話框開起來的那一刻就是這種情況）。 */
  const [wrapWidth, setWrapWidth] = React.useState(0);

  React.useEffect(() => {
    const el = wrapRef.current;
    if (!el || typeof ResizeObserver === 'undefined') return undefined;
    const ro = new ResizeObserver(() => setWrapWidth(el.clientWidth));
    ro.observe(el);
    setWrapWidth(el.clientWidth);
    return () => ro.disconnect();
  }, []);

  React.useEffect(() => {
    if (!isPdf || !blob || !wrapWidth) return undefined;
    let cancelled = false;
    let task: any = null;
    // 換頁／換檔案時先收回「畫好了」，載入指示才會再出現。
    onPainted?.(false);
    (async () => {
      try {
        const pdfjs: any = await import('pdfjs-dist');
        /**
         * worker **打包進去**，不走 CDN。
         * `src/web` 的 pdf-signature 是從 unpkg 抓的 —— 那一頁是公開頁面，
         * 這裡不是：租戶後台不該因為外網不通就看不到自己的單。
         */
        if (!pdfjs.GlobalWorkerOptions.workerSrc) {
          // energy-ops-next：worker 檔放在 public/pdfjs/（與 pdfjs-dist 3.4.120 同版），取代 Vite 的 `?url` import。
          const workerUrl = '/pdfjs/pdf.worker.min.js';
          pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;
        }
        const data = await blob.arrayBuffer();
        if (cancelled) return;
        task = pdfjs.getDocument({ data });
        const doc = await task.promise;
        if (cancelled) return;
        onPageCount(doc.numPages);
        const pdfPage = await doc.getPage(Math.min(Math.max(page + 1, 1), doc.numPages));
        if (cancelled) return;
        const canvas = canvasRef.current;
        if (!canvas) return;
        // 以**容器寬度**決定縮放：框是相對座標，所以只要 canvas 的 CSS 尺寸
        // 與 viewport 等比，框就對得上。
        const base = pdfPage.getViewport({ scale: 1 });
        const scale = wrapWidth / base.width;
        // devicePixelRatio：不乘的話 Retina 上文字是糊的，而這一格的存在意義
        // 就是「讀得出紙上的數字」。
        const dpr = Math.min(window.devicePixelRatio || 1, 2);
        const viewport = pdfPage.getViewport({ scale: scale * dpr });
        canvas.width = Math.floor(viewport.width);
        canvas.height = Math.floor(viewport.height);
        canvas.style.width = '100%';
        canvas.style.height = 'auto';
        const ctx = canvas.getContext('2d');
        if (!ctx) return;
        await pdfPage.render({ canvasContext: ctx, viewport }).promise;
        if (!cancelled) onPainted?.(true);
      } catch {
        if (!cancelled) onFailed();
      }
    })();
    return () => {
      cancelled = true;
      try { task?.destroy?.(); } catch { /* 已經收掉了 */ }
    };
    // `onPageCount` / `onFailed` 是呼叫端的 callback，放進依賴會每次 render 重畫。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isPdf, blob, page, wrapWidth]);

  /** 這一頁上的框（框帶頁碼，不要把第 2 頁的框畫在第 1 頁上）。 */
  const pageBoxes = boxes.filter((b) => (isPdf ? b.page === page : true));

  return (
    <Box ref={wrapRef} sx={{ position: 'relative', width: '100%' }}>
      {isPdf && (
        <canvas
          ref={canvasRef}
          data-testid="source-preview-pdf"
          style={{ display: 'block', width: '100%' }}
        />
      )}
      {!isPdf && imgUrl && (
        <Box
          component="img"
          data-testid="source-preview-image"
          src={imgUrl}
          alt={label}
          sx={{ display: 'block', width: '100%' }}
        />
      )}

      {pageBoxes.map((b) => {
        const active = focusField === b.field;
        const text = boxLabel?.(b.field) ?? '';
        return (
          <Box
            key={`${b.field}-${b.page}`}
            data-testid="source-box"
            data-field={b.field}
            data-page={b.page}
            data-active={active ? '1' : '0'}
            sx={{
              position: 'absolute',
              left: `${b.x * 100}%`,
              top: `${b.y * 100}%`,
              width: `${b.width * 100}%`,
              height: `${b.height * 100}%`,
              borderRadius: 0.5,
              /**
               * **每個欄位一個顏色**（`fieldColors.ts`），左邊的框與右邊那一列同色。
               *
               * 在此之前所有框都是同一個淡灰：看到一個框只知道「這裡有東西」，
               * 不知道**它是哪一欄**，於是那個對照根本用不起來。
               *
               * 被點到的那一個再加強：粗一倍、底色更濃、加上欄位名 ——
               * 「更高亮」是**相對**的，全部一樣濃就等於沒有高亮。
               */
              border: `${active ? 3 : 1.5}px solid ${colorForField(b.field)}`,
              background: fieldFill(b.field, active ? '3A' : '14'),
              boxShadow: active ? `0 0 0 2px ${fieldFill(b.field, '55')}` : 'none',
              pointerEvents: 'none',
            }}
          >
            {/* **只有被點到的那一格標字。**
                全部都標會把單子蓋滿，而那就看不到紙上的字了 ——
                這一格存在的意義是讀紙，不是讀我們的標籤。
                標籤往上放，蓋住的是空白不是數字；貼在頁頂時改放下面。 */}
            {active && text && (
              <Typography
                data-testid="source-box-label"
                data-field={b.field}
                variant="caption"
                sx={{
                  position: 'absolute',
                  left: 0,
                  ...(b.y > 0.06 ? { bottom: '100%' } : { top: '100%' }),
                  mb: b.y > 0.06 ? 0.25 : 0,
                  mt: b.y > 0.06 ? 0 : 0.25,
                  px: 0.5,
                  borderRadius: 0.5,
                  whiteSpace: 'nowrap',
                  lineHeight: 1.4,
                  background: colorForField(b.field),
                  color: '#fff',
                  fontSize: 10,
                }}
              >
                {text}
              </Typography>
            )}
          </Box>
        );
      })}
    </Box>
  );
};

/**
 * **原檔預覽**——校對時左欄那一格。
 *
 * 這一格在「校對電費單」（`BillDetailBody`）與「收件批次的就地校對」
 * （`IngestBatchDetailPage`）**是同一件事**，所以只能有一份實作。
 *
 * 它顯示的是**真的那份檔案**（`RECOGNITION.md §7`），從授權下載端點取 blob。
 * 在此之前這裡是一個**空的 A4 框** —— 而「每一站都能展開原文件反覆比對」
 * 是這個系統的核心工作方式：主管核准的動作本質上就是拿螢幕上的數字對紙上的數字。
 *
 * ### 為什麼 PDF 要自己 render（pdf.js），不用 `<iframe>`
 *
 * `<iframe src="blob:…">` 走的是**瀏覽器內建的 viewer**：開得起來、可捲可縮，
 * 但它裡面捲到第幾頁、縮放多少、頁面留白多寬，我們**一無所知**。
 * 於是疊在上面的高亮框**對不準** —— 而指錯位置比沒有位置更糟，
 * 因為覆核的人整件事就是在比對。
 *
 * 自己把該頁畫進 `<canvas>` 之後，canvas 的 CSS 尺寸是我們給的，
 * 服務回的 0–1 相對座標乘上去就是**真的那個位置**。
 *
 * ### 一定要能放大
 *
 * 左欄只有 260px，那個寬度**讀不出紙上的數字** —— 而讀得出來是這一格唯一的用途。
 * 所以點一下就開全螢幕對話框，裡面是同一個元件（框一樣準）。
 *
 * ### 沒有座標是正常結果
 *
 * **沒有座標就不要畫框**。辨識服務只在能**唯一定位**到那個 token 時才給 box
 * （同一個數字在頁面上出現兩次就不給，`RECOGNITION.md §9-5`），
 * 而 OCR 勝出的欄位也可能沒有。畫面要能接受「這一格沒得高亮」。
 *
 * `data-testid="source-preview"` 與框上的 `data-field` / `data-active`
 * 是既有 crawler 的探針，**搬家時一個字都不能改**。
 */
export const SourcePreviewFrame: React.FC<{
  /** 左上角的小標（呼叫端自己翻譯，這裡不綁 i18n key）。 */
  label: string;
  /** 要顯示的那一份附件。沒有就只畫空框與提示。 */
  attachmentId?: number | null;
  boxes?: SourceFieldBox[];
  focusField?: string | null;
  /** 框旁邊標什麼字（呼叫端翻譯）。 */
  boxLabel?: (field: string) => string;
  /** 單欄（窄）時 A4 比例會把畫面吃掉。 */
  maxHeight?: number;
}> = ({
  label, attachmentId = null, boxes = [], focusField = null, boxLabel, maxHeight = 720,
}) => {
  const translate = useTranslate();
  const [blob, setBlob] = React.useState<Blob | null>(null);
  const [imgUrl, setImgUrl] = React.useState<string | null>(null);
  const [mime, setMime] = React.useState<string>('');
  const [failed, setFailed] = React.useState(false);
  /** 目前顯示第幾頁（0-based，與 `SourceFieldBox.page` 同一套）。 */
  const [page, setPage] = React.useState(0);
  const [pageCount, setPageCount] = React.useState(0);
  const [zoomed, setZoomed] = React.useState(false);
  /**
   * **標註可以關掉。**
   *
   * 框是疊在紙上的，而覆核的人有時要看的正是被框住的那幾個字 ——
   * 一個關不掉的標註會變成「擋住我要驗的東西」。
   * 預設開（那是這一格的價值），但一鍵就能收起來。
   */
  const [showBoxes, setShowBoxes] = React.useState(true);
  /**
   * **PDF 畫出來之前那一秒多，框是空的。**
   *
   * 檔案抓回來（`data-loaded=1`）到 pdf.js 把那一頁畫進 canvas 之間有一段空窗，
   * 而空窗期的畫面跟「這張單沒有原始檔」長得**一模一樣** ——
   * 使用者點一點、瞄一眼就走，看到的就是一個白框。
   * 這一格存在的意義是「看得到那張紙」，所以還沒畫好時要說出來。
   */
  const [painted, setPainted] = React.useState(false);

  React.useEffect(() => {
    if (!attachmentId) { setBlob(null); setImgUrl(null); setFailed(false); setPainted(false); return undefined; }
    let revoked = false;
    let url: string | null = null;
    setFailed(false);
    setPainted(false);
    setPage(0);
    setPageCount(0);
    (async () => {
      try {
        // 走**授權下載端點**（`ATTACHMENTS.md §4.2`）——
        // 前端不碰 storage key，也拿不到直連 URL。
        const res = await energyHttp.get(attachmentDownloadUrl(attachmentId), {
          responseType: 'blob',
        });
        if (revoked) return;
        const b = res.data as Blob;
        url = URL.createObjectURL(b);
        setMime(String(b.type || res.headers?.['content-type'] || ''));
        setBlob(b);
        setImgUrl(url);
      } catch {
        // 看不到原檔不該讓整頁壞掉 —— 但**要說出來**，
        // 空白的框會被當成「這張單沒有附件」。
        if (!revoked) setFailed(true);
      }
    })();
    return () => {
      revoked = true;
      if (url) URL.revokeObjectURL(url);
    };
  }, [attachmentId]);

  const isPdf = /pdf/i.test(mime);
  const loaded = !!blob;

  /**
   * **點到的欄位在別頁 → 自己翻過去。**
   * 少了這一段，兩頁式的高壓單點第 2 頁的欄位會什麼都沒發生 ——
   * 框其實畫好了，只是不在眼前。
   */
  React.useEffect(() => {
    if (!focusField) return;
    const hit = boxes.find((b) => b.field === focusField);
    if (hit && hit.page !== page) setPage(hit.page);
    // `boxes` 每次 render 都是新陣列 —— 依賴放 focusField 即可。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focusField]);

  const doc = (
    <SourceDoc
      blob={blob}
      isPdf={isPdf}
      imgUrl={imgUrl}
      page={page}
      onPageCount={setPageCount}
      onFailed={() => setFailed(true)}
      onPainted={setPainted}
      boxes={showBoxes ? boxes : []}
      focusField={focusField}
      boxLabel={boxLabel}
      label={label}
    />
  );

  const pager = pageCount > 1 ? (
    <Box sx={{ display: 'flex', gap: 0.5, justifyContent: 'center', py: 0.5 }}>
      {Array.from({ length: pageCount }, (_, i) => (
        <Chip
          key={i}
          size="small"
          label={i + 1}
          data-testid="source-page"
          data-page={i}
          data-active={i === page ? '1' : '0'}
          variant={i === page ? 'filled' : 'outlined'}
          onClick={(e) => { e.stopPropagation(); setPage(i); }}
          sx={i === page
            ? { background: jvTokens.primary, color: '#fff' }
            : { color: jvTokens.muted }}
        />
      ))}
    </Box>
  ) : null;

  return (
    <>
      <Box
        data-testid="source-preview"
        data-focused={focusField ?? ''}
        data-loaded={loaded ? '1' : '0'}
        data-page={page}
        data-page-count={pageCount}
        data-painted={painted ? '1' : '0'}
        onClick={() => { if (loaded) setZoomed(true); }}
        sx={{
          position: 'relative',
          width: '100%',
          ...(loaded ? {} : { aspectRatio: '1 / 1.414' }),
          maxHeight,
          border: `1px solid ${jvTokens.hairline}`,
          borderRadius: 1,
          background: jvTokens.hoverBg,
          mb: 1.5,
          overflow: 'auto',
          cursor: loaded ? 'zoom-in' : 'default',
        }}
      >
        {loaded && doc}
        {/* 抓到檔案、還沒畫好 —— 說出來。空白框跟「這張單沒有原始檔」長得一樣。 */}
        {loaded && isPdf && !painted && !failed && (
          <Box
            data-testid="source-preview-loading"
            sx={{
              position: 'absolute',
              inset: 0,
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              justifyContent: 'center',
              gap: 1,
              pointerEvents: 'none',
            }}
          >
            <CircularProgress size={22} />
            <Typography variant="caption" sx={{ color: jvTokens.muted }}>
              {translate('energy.review.sourceLoading')}
            </Typography>
          </Box>
        )}
        <Typography
          variant="caption"
          sx={{
            position: 'absolute',
            top: 6,
            left: 8,
            color: jvTokens.muted,
            // 蓋在文件上時要讀得到。
            background: loaded ? 'rgba(255,255,255,0.82)' : 'transparent',
            px: loaded ? 0.5 : 0,
            borderRadius: 0.5,
            pointerEvents: 'none',
          }}
        >
          {/* 沒有檔案時**不要**用那句承諾互動的標籤（「點右邊欄位會在紙上標出位置」）——
            沒有紙就不會有位置可標，那是 `FRONTEND.md §2.8` 的同一種謊。 */}
        {(() => {
          if (failed) return `${label}（讀不到原檔）`;
          if (!loaded) return translate('energy.review.sourceEmpty');
          return label;
        })()}
        </Typography>
        {/* 標註開關 —— 框會蓋在紙上，要驗被框住的那幾個字時得收得起來。
            放右上角、只在**真的有框**的時候出現（沒有框時它什麼也控制不了）。 */}
        {loaded && boxes.length > 0 && (
          <Chip
            size="small"
            data-testid="source-boxes-toggle"
            data-on={showBoxes ? '1' : '0'}
            label={translate(showBoxes ? 'energy.review.boxesOn' : 'energy.review.boxesOff')}
            onClick={(e) => { e.stopPropagation(); setShowBoxes((v) => !v); }}
            sx={{
              position: 'absolute',
              top: 6,
              right: 8,
              background: 'rgba(255,255,255,0.9)',
              border: `1px solid ${showBoxes ? jvTokens.primary : jvTokens.border}`,
              color: showBoxes ? jvTokens.primary : jvTokens.muted,
              cursor: 'pointer',
            }}
          />
        )}
        {/* 多頁：切頁。不要只給「下一頁」——兩頁的單看完第 2 頁要回得去。 */}
        {pager && (
          <Box sx={{
            position: 'sticky', bottom: 0, background: 'rgba(255,255,255,0.9)',
          }}
          >
            {pager}
          </Box>
        )}
      </Box>

      {/* 放大：左欄那個寬度讀不出紙上的數字，而讀得出來是這一格唯一的用途。 */}
      <Dialog
        open={zoomed}
        onClose={() => setZoomed(false)}
        fullWidth
        maxWidth="lg"
        data-testid="source-preview-zoom"
      >
        <DialogTitle sx={{ py: 1 }}>
          {label}
          {pageCount > 1 ? `　${page + 1} / ${pageCount}` : ''}
        </DialogTitle>
        <DialogContent dividers>
          {zoomed && loaded && doc}
        </DialogContent>
        <DialogActions>
          {pager}
          <Button size="small" onClick={() => setZoomed(false)} data-testid="source-preview-zoom-close">
            {translate('ra.action.close')}
          </Button>
        </DialogActions>
      </Dialog>
    </>
  );
};

export const EmptyRow: React.FC<{ colSpan: number; loading?: boolean }> = ({ colSpan, loading }) => {
  const translate = useTranslate();
  if (loading) return null;
  return (
    <TableRow>
      <TableCell colSpan={colSpan}>
        <Typography variant="body2" color="text.secondary">{translate('energy.common.empty')}</Typography>
      </TableCell>
    </TableRow>
  );
};
