import * as React from 'react';
import {
  Box, Stack, Chip, Select, MenuItem, Typography, Alert,
} from '@mui/material';
import { energyHttp, energyBase } from '@/api/energyLegacy';
import { useNotify, useTranslate } from 'react-admin';
import { BILL_DOC_TYPES } from 'common/plugins/EnergyOpsCommonPlugin/documentIngest';
import { apiErrorCode } from '../../../dataProvider/apiError';
import { jvTokens } from './shared';
import { colorForField, fieldFill } from './fieldColors';

/**
 * **單別**：這張單是哪一種，以及它送出去會開哪些流程。
 *
 * ### 為什麼它要長成「一個可以改的欄位」
 *
 * 單別是**辨識判出來的**（版型 ＋ 幾個關鍵數字），跟金額、電號一樣會判錯。
 * 但它以前只以一句結論出現在畫面上 ——「依目前金額，這張單會走：電費單核可流程 v2」
 * —— 沒有任何入口可以說「不對，這是用電單」。
 *
 * 判錯的後果與其他欄位**不同量級**：金額錯了是數字錯，單別錯了是
 * **整張單進到別人的流程**。而流程在送審那一刻就開了，事後只能中斷重來。
 * 所以它必須：**在送審前**看得到、改得動，而且改完當場看得到後果。
 *
 * ### 為什麼改它是「立刻存」而不是跟著下面的欄位一起存
 *
 * 其他欄位是「把紙上的數字抄對」，可以一次改好幾格再存。
 * 單別不是數字是**分類**，而且它一改，「會開哪些流程」整段都要重算 ——
 * 那份重算來自後端（綁定是後端的事）。改完立刻存、立刻重讀，
 * 使用者才會看到自己那一下造成了什麼。
 */
const BillDocTypeField: React.FC<{
  orgId?: number | null;
  billId: number | null;
  docType: string | null;
  /** 辨識當初判成什麼（與 `docType` 不同時＝被人改過）。 */
  recognizedDocType: string | null;
  /** `billBindingStatus` 的回應：這張單會開哪些流程。 */
  binding: any;
  editable: boolean;
  /** 這張單已經開了流程 —— 那句話要用過去式（「開了」不是「會開」）。 */
  started?: boolean;
  onSaved: () => void;
}> = ({
  orgId, billId, docType, recognizedDocType, binding, editable, started, onSaved,
}) => {
  const translate = useTranslate();
  const notify = useNotify();
  const [busy, setBusy] = React.useState(false);
  const current = docType ?? recognizedDocType ?? '';
  const changed = !!docType && !!recognizedDocType && docType !== recognizedDocType;
  const flows: any[] = binding?.flows ?? [];

  const save = async (next: string) => {
    if (!orgId || !billId || next === current) return;
    setBusy(true);
    try {
      await energyHttp.patch(`${energyBase(orgId)}/bills/${billId}`, { docType: next });
      notify('energy.review.docTypeSaved', { type: 'success' });
      onSaved();
    } catch (e: any) {
      const code = apiErrorCode(e);
      // 已經在跑的單改不動 —— 那不是「存檔失敗」，要說得出**為什麼**。
      notify(
        code === 'doc_type_locked_by_execution' ? 'energy.review.docTypeLocked' : (code || 'save failed'),
        { type: code === 'doc_type_locked_by_execution' ? 'warning' : 'error' },
      );
    } finally { setBusy(false); }
  };

  // 單別在紙上沒有一個「位置」可以標（它是判斷不是欄位），所以沒有色點的實心/空心之分，
  // 但它仍然用**同一套顏色**：右邊表格的一列與左邊的框是同一個視覺語言。
  const color = colorForField('docType');

  return (
    <Box
      data-testid="bill-doc-type"
      data-doc-type={current}
      data-changed={changed ? '1' : '0'}
      sx={{
        mb: 1.5,
        p: 1,
        borderRadius: 1,
        border: `1px solid ${jvTokens.hairline}`,
        background: fieldFill('docType', '0F'),
      }}
    >
      <Stack direction="row" alignItems="center" gap={1} flexWrap="wrap">
        <Box sx={{ width: 10, height: 10, borderRadius: '50%', background: color }} />
        <Typography variant="caption" sx={{ fontWeight: 700, minWidth: 48 }}>
          {translate('energy.review.docTypeField')}
        </Typography>
        <Select
          size="small"
          value={current}
          disabled={!editable || busy}
          data-testid="doc-type-select"
          onChange={(e) => save(String(e.target.value))}
          sx={{ minWidth: 260, background: '#fff' }}
        >
          {/* 舊的粗分值不列進選項（人不該把單別「改回」比較粗的那一個），
              但如果這張單目前就是那個值，還是要列得出來，否則 Select 會顯示空白。 */}
          {[...BILL_DOC_TYPES, ...(BILL_DOC_TYPES.includes(current as any) ? [] : [current])]
            .filter(Boolean)
            // `data-doc-type`：驗收要能**指名**選哪一種 ——
            // 比對中文字串會被翻譯與換字咬到。
            .map((t) => (
              <MenuItem key={t} value={t} data-testid="doc-type-option" data-doc-type={t}>
                {translate(`energy.docType.${t}`)}
              </MenuItem>
            ))}
        </Select>
        {/* 辨識值要**看得到**：改過之後，人要知道它原本判成什麼。 */}
        {recognizedDocType && (
          <Typography variant="caption" color="text.secondary" data-testid="doc-type-recognized">
            {translate('energy.review.docTypeRecognized')}
            {'：'}
            {translate(`energy.docType.${recognizedDocType}`)}
          </Typography>
        )}
        {changed && (
          <Chip
            size="small"
            data-testid="doc-type-changed"
            label={translate('energy.bills.corrected')}
            sx={{ background: jvTokens.gold, color: '#000', fontWeight: 700, height: 20 }}
          />
        )}
      </Stack>

      <Typography variant="caption" color="text.secondary" display="block" mt={0.5}>
        {translate('energy.review.docTypeHint')}
      </Typography>

      {/**
        * **會開哪些流程** —— 一張單可以同時開多條（核可 ＋ 開發票）。
        * 這裡列的是綁定解出來的結果，不是寫死的映射：
        * 換句話說，它就是「按下送審會發生什麼」。
        */}
      <Box mt={0.75} data-testid="bill-flows" data-count={String(flows.length)}>
        {flows.length > 0 ? (
          <Stack direction="row" gap={0.5} flexWrap="wrap" alignItems="center">
            <Typography variant="caption" color="text.secondary">
              {translate(started ? 'energy.review.flowsStarted' : 'energy.review.flowsPreview')}
              {'：'}
            </Typography>
            {flows.map((f: any) => (
              <Chip
                key={f.workflowKey}
                size="small"
                data-testid="bill-flow"
                data-workflow={f.workflowKey}
                // 契約發動的流程要說得出**是哪一份契約** ——
                // 一張單的兩條流程可能來自不同契約。
                label={f.contractNo
                  ? `${f.workflowName ?? f.workflowKey} v${f.version} · ${f.contractNo}`
                  : `${f.workflowName ?? f.workflowKey} v${f.version}`}
                sx={{ borderColor: color, color: jvTokens.caption }}
                variant="outlined"
              />
            ))}
          </Stack>
        ) : (
          // 沒有流程**不是空白**：那代表送審會被退回，要現在就說。
          <Alert
            severity="warning"
            data-testid="bill-flows-none"
            sx={{
              py: 0,
              color: jvTokens.warn,
              border: `1px solid ${jvTokens.warn}`,
              backgroundColor: 'transparent',
            }}
          >
            {translate('energy.review.flowsNone')}
          </Alert>
        )}
      </Box>
    </Box>
  );
};

export default BillDocTypeField;
