import * as React from 'react';
import {
  ReactFlow, Background, Controls, MarkerType, BaseEdge,
  type Node as RfNode, type Edge as RfEdge,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { Box, Typography, Stack, CircularProgress, Alert } from '@mui/material';
import { energyHttp, energyBase } from '@/api/energyLegacy';
import { useTranslate } from 'react-admin';
import { jvTokens } from './shared';

export interface GraphNode {
  nodeKey: string;
  name: string;
  nodeType: string;
  layout: { x: number; y: number } | null;
  /** completed | active | pending | cancelled | definition（P9 的定義圖沒有執行狀態） */
  state: string;
  serviceKey: string | null;
  /** P9：這一關現在積了幾張。執行圖沒有這個欄位（它只畫一張單）。 */
  waiting?: number;
  /** P9：其中幾張卡超過門檻 → **燒橘**（不是紅）。 */
  stuck?: number;
}
export interface GraphEdge {
  from: string; to: string; kind: string;
  label: string | null;
  /** 推導出來的退回邊帶的是 **i18n key**（後端不翻字串）。 */
  labelKey: string | null;
  hasCondition: boolean;
  /** true ＝ 這條邊不是 transition 表裡的列，是由 `revert` 動作推導的。 */
  implied: boolean;
}
export interface ExecutionGraph {
  /** 定義圖（P9）沒有這三個 —— 不要假裝有。 */
  executionId?: number; status?: string; current?: string[];
  nodes: GraphNode[]; edges: GraphEdge[];
}

/**
 * 唯讀流程圖（`FRONTEND.md §3`）。
 *
 * **這是獨立元件，不是頁面** —— C3 的校對台會把它嵌進版面裡
 * （`TASKS.md`：「不要先蓋好 P3 再回頭塞圖進去」）。
 *
 * **唯讀是硬性的**：`nodesDraggable` / `nodesConnectable` / `edgesReconnectable`
 * 全部 false。**流程圖不是操作介面** —— 動作按鈕在圖外的按鈕列。
 * 能拖的圖會讓人以為拖了就會改流程，但流程定義是 seed 出來的。
 *
 * 佈局：**優先用定義側的 `layout`**（`WORKFLOW.md §2.2`）；
 * 後端沒給才自動排版（見 `autoLayout`）。
 *
 * **血緣是另一張圖**（`WORKFLOW.md §4.9.6`）——那張圖的節點是「一整個 execution」，
 * 跟這張的「關卡」不是同一種東西。不要混進來。
 */

/**
 * **繞過節點的邊**（退回邊、以及會撞到中間節點的分支邊）。
 *
 * ### 為什麼要自己畫一條
 *
 * React Flow 的預設邊在「同一排」的節點之間是一條**直線**，
 * 而退回邊（主管核准 → 承辦確認）本來就是**往回走**的 ——
 * 它從右邊出發、往左邊接，於是那條線會**橫切過中間每一個節點**
 * （量出來的：`manager-approve→clerk-review` 穿過 `start`、
 * `exec-approve→manager-approve` 穿過 `clerk-review`、
 * `finance-release→calc` 穿過 `payment`）。線壓在字上，圖就讀不動了。
 *
 * `smoothstep` 也救不了：它的轉折發生在兩點之間，同一排時仍然貼著那一排走。
 *
 * 所以自己畫：**從節點下方（或上方）繞過去**。
 * 弧的深度隨水平距離增加 —— 繞得越遠、離那一排越遠，
 * 這樣兩條退回邊也不會疊在一起。
 */
const BowEdge: React.FC<any> = ({
  id, sourceX, sourceY, targetX, targetY, markerEnd, style, data,
}) => {
  // `dir` ＝ 1 往下繞（退回邊）、-1 往上繞（會撞到節點的分支邊）。
  const dir = Number(data?.dir ?? 1);
  const dx = Math.abs(targetX - sourceX);
  // 40 起跳、隨距離加深，但有上限 —— 不封頂的話跨半張圖的退回邊會掉出畫布。
  const depth = Math.min(46 + dx * 0.16, 130) * dir;
  const path = `M ${sourceX},${sourceY}`
    + ` C ${sourceX + 28},${sourceY + depth}`
    + ` ${targetX - 28},${targetY + depth}`
    + ` ${targetX},${targetY}`;
  return <BaseEdge id={id} path={path} markerEnd={markerEnd} style={style} />;
};

const EDGE_TYPES = { bow: BowEdge };

/** 節點框的估算大小（`nodeStyle` 的 `minWidth` ＋ 一行文字）—— 只用來判斷會不會撞到。 */
const NODE_W = 132;
const NODE_H = 40;

/** 線段有沒有穿過某個節點的框（`layout` 座標系，與畫面同構）。 */
function segmentHitsBox(
  a: { x: number; y: number },
  b: { x: number; y: number },
  box: { x: number; y: number },
): boolean {
  const left = box.x;
  const right = box.x + NODE_W;
  const top = box.y;
  const bottom = box.y + NODE_H;
  // 取樣就夠了：這是「要不要繞路」的判斷，不是碰撞物理。
  for (let i = 0; i <= 24; i += 1) {
    const t = i / 24;
    const x = a.x + (b.x - a.x) * t;
    const y = a.y + (b.y - a.y) * t;
    if (x > left + 2 && x < right - 2 && y > top + 2 && y < bottom - 2) return true;
  }
  return false;
}

/** 節點狀態 → 樣式。色票一律從 `jvTokens` 拿。 */
function nodeStyle(state: string): React.CSSProperties {
  const base: React.CSSProperties = {
    padding: '8px 14px',
    borderRadius: 8,
    fontSize: 13,
    fontWeight: 600,
    minWidth: 132,
    textAlign: 'center',
    borderWidth: 2,
    borderStyle: 'solid',
  };
  switch (state) {
    case 'completed':
      // 已完成＝實心紫
      return { ...base, background: jvTokens.primary, color: '#fff', borderColor: jvTokens.primary };
    case 'active':
      // 進行中＝紫外框 ＋ 強調（這是使用者最需要一眼找到的東西）
      return {
        ...base,
        background: '#fff',
        color: jvTokens.primaryDark,
        borderColor: jvTokens.primary,
        boxShadow: `0 0 0 4px ${jvTokens.track}`,
      };
    case 'cancelled':
      // 被取消＝**虛線**灰（不可畫成已完成——那會讓人以為那條分支走完了）
      return {
        ...base,
        background: jvTokens.hoverBg,
        color: jvTokens.muted,
        borderColor: jvTokens.border,
        borderStyle: 'dashed',
      };
    default:
      // 未到＝灰
      return { ...base, background: '#fff', color: jvTokens.muted, borderColor: jvTokens.border };
  }
}

/**
 * 沒有 `layout` 時的自動排版 —— 依邊做**分層**（longest-path），同層並排。
 *
 * 刻意做得簡單：這只是「定義沒填座標時的救急」。
 * 真的要好看就去填定義側的 `layout`，不要把排版演算法愈養愈大。
 */
function autoLayout(nodes: GraphNode[], edges: GraphEdge[]): Record<string, { x: number; y: number }> {
  const depth: Record<string, number> = {};
  nodes.forEach((n) => { depth[n.nodeKey] = 0; });
  // 只走 forward 邊 —— sendBack 是回頭邊，拿它算層數會無窮遞增。
  const forward = edges.filter((e) => e.kind !== 'sendBack');
  // 迴圈次數上限＝節點數，避免定義裡有環時卡死。
  for (let i = 0; i < nodes.length; i += 1) {
    let changed = false;
    forward.forEach((e) => {
      if (depth[e.to] !== undefined && depth[e.from] !== undefined
        && depth[e.to] < depth[e.from] + 1) {
        depth[e.to] = depth[e.from] + 1;
        changed = true;
      }
    });
    if (!changed) break;
  }
  const perDepth: Record<number, number> = {};
  const out: Record<string, { x: number; y: number }> = {};
  nodes.forEach((n) => {
    const d = depth[n.nodeKey] ?? 0;
    const idx = perDepth[d] ?? 0;
    perDepth[d] = idx + 1;
    out[n.nodeKey] = { x: d * 220, y: idx * 110 };
  });
  return out;
}

export const toReactFlow = (graph: ExecutionGraph): { nodes: RfNode[]; edges: RfEdge[] } => {
  const auto = autoLayout(graph.nodes, graph.edges);
  const nodes: RfNode[] = graph.nodes.map((n) => {
    const hasLoad = typeof n.waiting === 'number';
    const stuck = (n.stuck ?? 0) > 0;
    return {
      id: n.nodeKey,
      // 顯示的是定義裡的 `name`（客戶的語彙），不是 nodeKey，也不硬翻。
      data: {
        label: hasLoad ? (
          // 「現在積了幾張」——這一條把一張說明圖變成**營運儀表**（`FRONTEND.md §2 P9`）。
          <span data-testid="flow-node" data-node={n.nodeKey} data-waiting={n.waiting} data-stuck={n.stuck ?? 0}>
            {n.name}
            <span
              style={{
                marginLeft: 6,
                padding: '1px 6px',
                borderRadius: 10,
                fontSize: 11,
                fontWeight: 700,
                // 有卡住的用**燒橘**（不是紅：紅代表壞掉，塞車不是壞掉）。
                background: stuck ? jvTokens.warn : jvTokens.track,
                color: stuck ? '#fff' : jvTokens.caption,
              }}
            >
              {n.waiting}
            </span>
          </span>
        ) : n.name,
      },
      position: n.layout ?? auto[n.nodeKey] ?? { x: 0, y: 0 },
      style: {
        ...nodeStyle(n.state),
        // 塞車的關卡整個框燒橘 —— 圖上要**一眼**找得到瓶頸。
        ...(stuck ? { borderColor: jvTokens.warn, boxShadow: `0 0 0 4px ${jvTokens.warn}22` } : {}),
        ...(hasLoad ? { cursor: 'pointer' } : {}),
      },
      sourcePosition: 'right' as any,
      targetPosition: 'left' as any,
      draggable: false,
      connectable: false,
    };
  });

  /** 每個節點的座標（判斷邊會不會撞到節點用）。 */
  const posOf: Record<string, { x: number; y: number }> = {};
  graph.nodes.forEach((n) => {
    posOf[n.nodeKey] = n.layout ?? auto[n.nodeKey] ?? { x: 0, y: 0 };
  });

  const edges: RfEdge[] = graph.edges.map((e, i) => {
    const isSendBack = e.kind === 'sendBack';
    /**
     * **這條邊會不會從別的節點身上壓過去**。
     *
     * 前進邊在同一排時是直線；只要中間隔著一個節點（例如金額不到門檻、
     * 跳過「高層加簽」那一條），那條直線就會**穿過**它。
     * 退回邊則是本來就往回走，一定會壓到中間那幾關。
     *
     * 撞到就改走 `bow`：退回邊往**下**繞、前進邊往**上**繞
     * —— 兩個方向分開，兩種邊才不會在同一側疊在一起。
     */
    const a = posOf[e.from];
    const b = posOf[e.to];
    const hits = !!a && !!b && graph.nodes.some((n) => (
      n.nodeKey !== e.from && n.nodeKey !== e.to
      && segmentHitsBox(
        { x: a.x + NODE_W, y: a.y + NODE_H / 2 },
        { x: b.x, y: b.y + NODE_H / 2 },
        posOf[n.nodeKey],
      )
    ));
    const bowed = isSendBack || hits;
    // 退回邊＝**虛線 ＋ 燒橘**（不是紅：紅是「壞掉」，退回是正常流程）。
    const color = isSendBack ? jvTokens.warn : jvTokens.muted;
    return {
      id: `${e.from}->${e.to}-${e.kind}-${i}`,
      source: e.from,
      target: e.to,
      // 分支條件已經是資料 → 直接把 label 畫在邊上，前端不去解 condition。
      //
      // **退回邊刻意不上 label**：退回邊與分支邊常常擠在同一段路徑上，
      // 兩個 label 疊在一起會變成看不懂的亂碼（截圖驗到的：「金額 退回0000」）。
      // 圖例已經說明「橘色虛線＝退回路徑」，再標一次只是製造碰撞。
      label: isSendBack ? undefined : (e.label ?? undefined),
      labelStyle: { fill: jvTokens.caption, fontSize: 11 },
      labelBgStyle: { fill: '#fff' },
      labelBgPadding: [4, 2] as [number, number],
      /**
       * 撞得到節點的邊一律走 `bow`（自己畫的繞路邊）。
       *
       * 原本退回邊用 `smoothstep`，但它的轉折發生在兩點之間 ——
       * 同一排的節點照樣被橫切（實測：三條退回邊各壓過一到兩個節點）。
       */
      type: bowed ? 'bow' : 'default',
      // 退回邊往下繞、被擋住的前進邊往上繞。
      data: { dir: isSendBack ? 1 : -1 },
      style: { stroke: color, strokeWidth: 2, ...(isSendBack ? { strokeDasharray: '6 4' } : {}) },
      markerEnd: { type: MarkerType.ArrowClosed, color },
      // 退回邊墊在下層：它常常橫越分支邊的 label，疊在上面會把字劃掉。
      zIndex: isSendBack ? 0 : 1,
      animated: false,
    };
  });

  return { nodes, edges };
};

/** 圖例 —— 沒有圖例的話，「灰色虛線」跟「還沒到」分不出來。 */
const Legend: React.FC = () => {
  const translate = useTranslate();
  const item = (labelKey: string, style: React.CSSProperties) => (
    <Stack direction="row" gap={0.75} alignItems="center">
      <Box sx={{ width: 14, height: 14, borderRadius: '3px', ...style }} />
      <Typography variant="caption" color="text.secondary">{translate(labelKey)}</Typography>
    </Stack>
  );
  return (
    <Stack direction="row" gap={2} flexWrap="wrap" mt={1} data-testid="flow-legend">
      {item('energy.flowGraph.legendCompleted', { background: jvTokens.primary })}
      {item('energy.flowGraph.legendActive', { background: '#fff', border: `2px solid ${jvTokens.primary}` })}
      {item('energy.flowGraph.legendPending', { background: '#fff', border: `2px solid ${jvTokens.border}` })}
      {item('energy.flowGraph.legendCancelled', { background: jvTokens.hoverBg, border: `2px dashed ${jvTokens.border}` })}
      {item('energy.flowGraph.legendSendBack', { background: 'transparent', borderTop: `2px dashed ${jvTokens.warn}`, borderRadius: 0, height: 0 })}
    </Stack>
  );
};

/**
 * 自己抓資料的版本 —— 給 C3 校對台直接用。
 * 傳 `graph` 就不打 API（讓元件在測試與 storybook 裡可以純資料驅動）。
 */
const FlowGraph: React.FC<{
  organizationId?: number | null;
  executionId?: number | null;
  graph?: ExecutionGraph | null;
  height?: number;
  /** P9：點節點 → 列出卡在這一關的那幾張單。沒有它，節點數字就是不能點的數字。 */
  onNodeClick?: (nodeKey: string) => void;
}> = ({ organizationId, executionId, graph: given, height = 320, onNodeClick }) => {
  const translate = useTranslate();
  const [graph, setGraph] = React.useState<ExecutionGraph | null>(given ?? null);
  const [loading, setLoading] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (given) { setGraph(given); return; }
    if (!organizationId || !executionId) { setGraph(null); return; }
    let alive = true;
    setLoading(true); setError(null);
    energyHttp.get(`${energyBase(organizationId)}/executions/${executionId}/graph`)
      .then(({ data }) => { if (alive) setGraph(data); })
      .catch((e) => { if (alive) setError(e?.response?.data?.code || e?.message || 'load failed'); })
      .finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, [organizationId, executionId, given]);

  const rf = React.useMemo(() => (graph ? toReactFlow(graph) : { nodes: [], edges: [] }), [graph]);

  if (loading) return <Box display="flex" justifyContent="center" p={3}><CircularProgress size={22} /></Box>;
  if (error) return <Alert severity="warning">{error}</Alert>;
  if (!graph) return null;

  return (
    <Box data-testid="flow-graph">
      <Box sx={{ height, border: `1px solid ${jvTokens.hairline}`, borderRadius: 1 }}>
        <ReactFlow
          nodes={rf.nodes}
          edges={rf.edges}
          // 繞路邊（退回、以及會壓到節點的分支邊）——見 `BowEdge`。
          edgeTypes={EDGE_TYPES}
          fitView
          // 唯讀 —— 這五個 false 是規格的一部分，不是預設值調整。
          nodesDraggable={false}
          nodesConnectable={false}
          edgesReconnectable={false}
          elementsSelectable
          panOnScroll={false}
          proOptions={{ hideAttribution: false }}
          onNodeClick={onNodeClick ? (_e, n) => onNodeClick(String(n.id)) : undefined}
        >
          <Background />
          <Controls showInteractive={false} />
        </ReactFlow>
      </Box>
      <Legend />
      {/* 給 crawler / C3 用的可斷言真值：哪幾關進行中。不要拿它當狀態來源。 */}
      <Box
        data-testid="flow-current"
        data-current={(graph.current ?? []).join(',')}
        data-node-count={String(graph.nodes.length)}
        data-sendback-count={String(graph.edges.filter((e) => e.kind === 'sendBack').length)}
        sx={{ display: 'none' }}
      />
      {(graph.current?.length ?? 0) > 1 && (
        <Typography variant="caption" sx={{ color: jvTokens.primaryDark, mt: 0.5, display: 'block' }}>
          {translate('energy.flowGraph.parallel', { count: graph.current?.length ?? 0 })}
        </Typography>
      )}
    </Box>
  );
};

export default FlowGraph;
