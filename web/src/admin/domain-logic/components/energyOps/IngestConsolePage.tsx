import * as React from 'react';
import {
  Box, Card, CardContent, Typography, Button, Table, TableHead, TableBody, TableRow, TableCell,
  Stack, Chip, LinearProgress, IconButton, Tooltip, Alert,
} from '@mui/material';
import CloudUploadIcon from '@mui/icons-material/CloudUpload';
import RefreshIcon from '@mui/icons-material/Refresh';
import {
  energyHttp, energyBase, uploadViaStorage, fetchRecognitionState, type RecognitionState,
} from '@/api/energy';
import { useNotify, useTranslate, usePermissions } from 'react-admin';
import { useNavigate } from 'react-router-dom';
import { useSelection } from '@/platform/selection/store';
import { apiErrorCode } from '../../../dataProvider/apiError';
import { EnergyPageFrame, EmptyRow, jvTokens } from './shared';
import { ingestBatchRoute } from './routes';
import RecognizingIndicator from './RecognizingIndicator';

export interface BatchRow {
  id: number;
  submittedAt: string;
  submittedByName: string | null;
  fileCount: number;
  status: string;
  counts: Record<string, number>;
  /** 還沒送出去的筆數 —— 角標與置頂都看這個（後端算，見 `IngestService.openCount`）。 */
  pendingCount: number;
  billIds: number[];
  workflowCounts: Record<string, number>;
}

/**
 * **送件 Console：清單**（`LIFECYCLE.md §7.2`）。
 *
 * ### 這一頁是「收件台 ＋ 送單紀錄」合併的結果
 *
 * 之前是兩個 resource：`energyIntakeConsole`（收件台）與 `energyBatchConsole`（送單紀錄）。
 * 兩者列的是**同一張表**（`energyIngestBatch`），只是一個在「建立」、一個在「查歷史」。
 *
 * > **一個資源不可以有兩個 Console**（`LIFECYCLE.md §7.1`）。
 * > 「建立/編輯」與「查閱歷史」不是兩個資源，是同一個資源的 List / Create / Show。
 *
 * 這條看起來理所當然，卻很容易違反 —— 因為**兩個動作感覺很不一樣**
 * （「我在收件」vs「我在查紀錄」）。**動作不同不代表資源不同。**
 *
 * ### 合併順手解掉了一個缺陷
 *
 * 舊收件台是**一次性精靈**：上傳完的批次只活在 React state 裡，
 * 離開那一頁（或 refresh）就再也回不去，沒校對完的檔案等於消失（缺陷 A1）。
 *
 * 合併之後**不需要為此寫任何程式碼** —— 清單 → 詳情、詳情從 DB 讀，
 * 是 CRUD 的天生行為。設計初稿曾經打算在收件台加一段「進來先讀未完成的批次」，
 * 那其實是在**手工模擬一個清單頁**。
 *
 * ### 排序不是時間軸
 *
 * 未完成的置頂（後端排的）。這頁是承辦的**待辦清單**：
 * 上面留的是他今天還要做的事，已經送完的沉到下面。
 */
/** 辨識沒送出去的原因 → 提示文字（energy-ops-next：平台的預算與服務開關）。 */
const recognizeSkipMessage = (code?: string | null) => {
  if (code === 'serverless_runner_ref_budget_exceeded' || code === 'serverless_runner_budget_exceeded') return 'energy.intake.recognizeBudget';
  if (code === 'serverless_runner_ref_disabled' || code === 'serverless_runner_service_disabled') return 'energy.intake.recognizeDisabled';
  return code && !/^serverless_runner_|^tpr_|^http_/.test(code) ? code : 'energy.intake.recognizeLater';
};

const IngestConsolePage: React.FC = () => {
  const notify = useNotify();
  const translate = useTranslate();
  const navigate = useNavigate();
  const orgId = useSelection()?.organizationId;

  /**
   * **「能不能進這一頁」與「能不能上傳」是兩件事。**
   *
   * 這一頁對唯讀身分是有用的（它是血緣鏈的最上游，稽核從電費單的麵包屑進來），
   * 所以路由對 `flow:view` 也開。但拖放區要**有 `flow:act` 才畫** ——
   * 畫一顆按下去必定 403 的按鈕，就是專案記過的「按鈕會說謊」。
   *
   * `permissions` 還沒載入時（undefined）先畫：避免每次進頁都閃一下。
   * 後端 BFF 仍是真相 —— 這裡只是不要讓人白做工。
   */
  const { permissions } = usePermissions<string[]>();
  const canUpload = permissions == null
    || permissions.includes('*')
    || permissions.includes('energy:flow:act')
    || permissions.includes('platform:rbac:manage');

  const [loading, setLoading] = React.useState(false);
  const [denied, setDenied] = React.useState(false);
  const [rows, setRows] = React.useState<BatchRow[]>([]);
  const [busy, setBusy] = React.useState(false);
  const [progress, setProgress] = React.useState<{ done: number; total: number } | null>(null);
  const inputRef = React.useRef<HTMLInputElement | null>(null);
  /** 辨識服務的狀態由後端的 status 端點說（沒設定就是「服務未開」）。 */
  const [recognition, setRecognition] = React.useState<RecognitionState>('unknown');

  const base = energyBase(orgId);

  const load = React.useCallback(async () => {
    if (!orgId) { setRows([]); setDenied(false); return; }
    setLoading(true); setDenied(false);
    try {
      const { data } = await energyHttp.get(`${energyBase(orgId)}/ingest-batches`);
      setRows(Array.isArray(data) ? data : []);
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'load failed', { type: 'error' });
    } finally { setLoading(false); }
  }, [orgId, notify]);

  React.useEffect(() => { load(); }, [load]);

  React.useEffect(() => {
    let alive = true;
    setRecognition('unknown');
    fetchRecognitionState(orgId).then((st) => { if (alive) setRecognition(st); });
    return () => { alive = false; };
  }, [orgId]);

  /**
   * 收件的三步：**開批 → 逐檔上傳原始檔 → 送去辨識**。
   *
   * 順序是刻意的（`RECOGNITION.md §6.2`）：辨識要的是**檔案內容**。
   * 舊的順序是「開批的同時就分類」，那時候檔案還沒上傳 ——
   * 於是分類只能看檔名，而那正是陷阱 #52/#58 的根因。
   *
   * 辨識是**非同步**的（三來源全跑約 12 秒／頁），所以這裡只負責送出，
   * 進度看批次詳情頁 —— 完成後**直接進那一批**：使用者剛丟完檔，
   * 下一件事一定是看辨識結果，不該讓他自己回清單找。
   */
  const onFiles = async (files: FileList | null) => {
    if (!orgId || !files || !files.length) return;
    setBusy(true); setDenied(false); setProgress({ done: 0, total: files.length });
    try {
      const { data: created } = await energyHttp.post(`${base}/ingest-batches`, {
        fileCount: files.length,
      });
      const batchId = created.id;

      const list = Array.from(files);
      let uploaded = 0;
      for (let i = 0; i < list.length; i += 1) {
        try {
          // energy-ops-next：直傳 Storage（大檔不經 API 請求本體，見 auth/directUpload.ts）。
          // eslint-disable-next-line no-await-in-loop
          await uploadViaStorage(`${base}/ingest-batches/${batchId}/attachments`, list[i], {
            target: 'batch', organizationId: orgId, batchId,
          });
          uploaded += 1;
        } catch {
          // 單檔上傳失敗**不中斷整批** —— 其餘幾份照樣要能辨識。
          // 不要讓一份壞檔把 19 份也一起丟掉。
        }
        setProgress({ done: i + 1, total: list.length });
      }

      if (uploaded > 0 && recognition === 'off') {
        // 服務沒設定：不送辨識（送了也只會被拒），直接說清楚。
        notify('energy.intake.recognizeDisabled', { type: 'warning' });
      } else if (uploaded > 0) {
        try {
          const { data: outcome } = await energyHttp.post(`${base}/ingest-batches/${batchId}/recognize`);
          // 服務沒設定／連不上時**明說** —— 檔案已經在了，
          // 讓人知道是「還沒辨識」而不是「傳失敗」。
          // energy-ops-next：OCR 走平台（E3），預算用完／服務沒開時要說清楚，不是「稍後重試」。
          if (!outcome?.jobId) notify(recognizeSkipMessage(outcome?.skipped), { type: 'warning' });
        } catch (e: any) {
          notify(recognizeSkipMessage(apiErrorCode(e)), { type: 'warning' });
        }
      }

      notify('energy.intake.done', { type: 'success' });
      navigate(ingestBatchRoute(batchId));
    } catch (e: any) {
      if (e?.response?.status === 403) setDenied(true);
      else notify(apiErrorCode(e) || e?.message || 'upload failed', { type: 'error' });
    } finally {
      setBusy(false); setProgress(null);
      if (inputRef.current) inputRef.current.value = '';
    }
  };

  /** 一個計數 chip。需要人處理的用**燒橘**（不是紅）。 */
  const countChip = (key: string, n: number, hot = false) => (n > 0 ? (
    <Chip
      key={key}
      size="small"
      variant="outlined"
      data-testid="batch-count"
      data-count-key={key}
      label={`${translate(`energy.itemStatus.${key}`, { _: key })} ${n}`}
      sx={hot
        ? { color: jvTokens.warn, borderColor: jvTokens.warn }
        : { color: jvTokens.caption, borderColor: jvTokens.border }}
    />
  ) : null);

  return (
    <EnergyPageFrame orgId={orgId} denied={denied} loading={loading}>
      <Stack direction="row" alignItems="center" justifyContent="space-between" mb={2}>
        <Typography variant="h6">{translate('energy.ingest.title')}</Typography>
        <Tooltip title={translate('ra.action.refresh')}>
          <span>
            <IconButton onClick={load} disabled={loading}><RefreshIcon /></IconButton>
          </span>
        </Tooltip>
      </Stack>

      {/* ---- 頂部：新增送件（拖放區）----
          這裡**唯一的輸入元素是一顆選檔按鈕**。
          不要在這裡加「單別」「案場」「期間」—— 那些都是在問一個他
          **還不知道答案**的問題，而那些答案該由辨識結果回答。 */}
      {recognition === 'off' && (
        <Alert severity="warning" sx={{ mb: 2 }} data-testid="recognition-off">
          {translate('energy.intake.recognitionOff')}
        </Alert>
      )}
      {recognition === 'down' && (
        <Alert severity="info" sx={{ mb: 2 }} data-testid="recognition-down">
          {translate('energy.intake.recognitionDown')}
        </Alert>
      )}
      {canUpload && (
      <Card>
        <CardContent>
          <Box
            data-testid="intake-dropzone"
            onDragOver={(e) => e.preventDefault()}
            onDrop={(e) => { e.preventDefault(); onFiles(e.dataTransfer.files); }}
            sx={{
              border: `2px dashed ${jvTokens.border}`,
              borderRadius: 2,
              p: 4,
              textAlign: 'center',
              background: jvTokens.hoverBg,
            }}
          >
            <CloudUploadIcon sx={{ fontSize: 36, color: jvTokens.muted }} />
            <Typography variant="subtitle1" sx={{ mt: 1 }}>
              {translate('energy.intake.dropHere')}
            </Typography>
            <Typography variant="caption" color="text.secondary" display="block" mb={2}>
              {translate('energy.intake.hint')}
            </Typography>
            <input
              ref={inputRef}
              type="file"
              multiple
              hidden
              data-testid="intake-file-input"
              onChange={(e) => onFiles(e.target.files)}
            />
            <Button
              variant="contained"
              disabled={busy}
              data-testid="ingest-new"
              onClick={() => inputRef.current?.click()}
            >
              {translate('energy.ingest.new')}
            </Button>
          </Box>
          {progress && (
            <Box mt={2}>
              <LinearProgress variant="determinate" value={(progress.done / progress.total) * 100} />
              <Typography variant="caption" color="text.secondary">
                {translate('energy.intake.uploading', { done: progress.done, total: progress.total })}
              </Typography>
            </Box>
          )}
        </CardContent>
      </Card>
      )}

      {/* ---- 清單：未完成置頂（排序由後端決定）---- */}
      <Card sx={{ mt: 2 }}>
        <CardContent>
          <Typography variant="caption" color="text.secondary" display="block" mb={1}>
            {translate('energy.ingest.listHint')}
          </Typography>
          <Table size="small">
            <TableHead>
              <TableRow>
                <TableCell sx={{ width: 6, p: 0 }} />
                <TableCell>{translate('energy.batches.when')}</TableCell>
                <TableCell>{translate('energy.batches.who')}</TableCell>
                <TableCell align="right">{translate('energy.batches.files')}</TableCell>
                <TableCell>{translate('energy.batches.breakdown')}</TableCell>
                <TableCell>{translate('energy.intake.workflow')}</TableCell>
                <TableCell align="right" />
              </TableRow>
            </TableHead>
            <TableBody>
              {rows.length ? rows.map((b) => {
                const pending = Number(b.pendingCount ?? 0);
                return (
                  <TableRow
                    key={b.id}
                    hover
                    data-testid="batch-row"
                    data-files={b.fileCount}
                    data-pending={pending}
                    sx={{ cursor: 'pointer' }}
                    onClick={() => navigate(ingestBatchRoute(b.id))}
                  >
                    {/* 左側色條：未完成的那幾批一眼看得出來，不必讀字。 */}
                    <TableCell sx={{ width: 6, p: 0, background: pending > 0 ? jvTokens.gold : 'transparent' }} />
                    <TableCell sx={{ whiteSpace: 'nowrap' }}>
                      <Typography variant="caption">
                        {String(b.submittedAt ?? '').slice(0, 19).replace('T', ' ')}
                      </Typography>
                    </TableCell>
                    <TableCell>{b.submittedByName ?? '—'}</TableCell>
                    <TableCell align="right" data-testid="batch-filecount">{b.fileCount}</TableCell>
                    <TableCell>
                      <Stack direction="row" gap={0.5} flexWrap="wrap" alignItems="center">
                        {/* 角標：還有幾筆等他處理。這是這一列**最重要的數字** ——
                            「上傳了幾份」不告訴他今天還要不要回來。 */}
                        {pending > 0 && (
                          <Chip
                            size="small"
                            data-testid="batch-pending-badge"
                            label={translate('energy.ingest.pendingBadge', { count: pending })}
                            sx={{
                              color: jvTokens.warn,
                              borderColor: jvTokens.warn,
                              backgroundColor: 'transparent',
                              border: '1px solid',
                            }}
                          />
                        )}
                        {/* 清單上也要看得出「這一批還在跑」——
                            不然使用者會點進去、看一眼、退回來，再點一次。 */}
                        {['uploading', 'recognizing', 'classifying'].includes(String(b.status)) && (
                          <RecognizingIndicator size={12} />
                        )}
                        {countChip('submitted', b.counts?.submitted ?? 0)}
                        {countChip('extracted', b.counts?.extracted ?? 0)}
                        {countChip('unclassified', b.counts?.unclassified ?? 0, true)}
                        {countChip('unmatched', b.counts?.unmatched ?? 0, true)}
                        {countChip('failed', b.counts?.failed ?? 0, true)}
                      </Stack>
                    </TableCell>
                    <TableCell>
                      <Stack direction="row" gap={0.5} flexWrap="wrap">
                        {Object.entries(b.workflowCounts ?? {}).map(([k, n]) => (
                          <Typography key={k} variant="caption" color="text.secondary">
                            {translate(`energy.workflowName.${k}`, { _: k })}
                            {' × '}
                            {n}
                          </Typography>
                        ))}
                      </Stack>
                    </TableCell>
                    <TableCell align="right">
                      <Button
                        size="small"
                        data-testid="batch-open"
                        onClick={(e) => { e.stopPropagation(); navigate(ingestBatchRoute(b.id)); }}
                      >
                        {translate(pending > 0 ? 'energy.ingest.continue' : 'energy.ingest.open')}
                      </Button>
                    </TableCell>
                  </TableRow>
                );
              }) : <EmptyRow colSpan={7} loading={loading} />}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </EnergyPageFrame>
  );
};

export default IngestConsolePage;
