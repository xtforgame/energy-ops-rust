import * as React from 'react';
import {
  ReactFlow, Background, Controls, type Node as RfNode, type Edge as RfEdge,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { Box, Typography, Stack, Chip } from '@mui/material';
import { useTranslate } from 'react-admin';
import { jvTokens } from './shared';

interface OrgChartMember { userId: number; name: string | null; role: 'manager' | 'member' }
interface OrgChartDepartment {
  id: number; key: string; name: string; parentId: number | null; members: OrgChartMember[];
}

const COL_WIDTH = 170;
const ROW_HEIGHT = 110;
const NODE_W = 150;
const NODE_H = 48;

/**
 * 由上往下的部門樹（卡 C／G01）：根節點是組織，往下是部門的上下層。
 *
 * 排版是 `FlowGraph.tsx` 的 `autoLayout`（分層＋同層並排）的樹狀版：
 * 深度 ＝ 從根算下來的層數；**上層置中在子部門上方**（leaf 從左到右依序排，
 * 內部節點的 x ＝ 自己所有直接下層 x 的平均，等於 d3 tree 的簡化版）。
 * 刻意不裝 d3-hierarchy／dagre（卡上待拍板第 2 題，使用者原話「照預設」＝不加依賴）。
 */
function layoutDepartments(departments: OrgChartDepartment[], rootLabel: string): {
  nodes: RfNode[]; edges: RfEdge[]; positions: Record<number, { x: number; y: number }>;
} {
  const byId = new Map(departments.map((d) => [d.id, d]));
  const childrenOf = new Map<number | 'root', OrgChartDepartment[]>();
  departments.forEach((d) => {
    const key: number | 'root' = d.parentId != null && byId.has(d.parentId) ? d.parentId : 'root';
    const list = childrenOf.get(key) ?? [];
    list.push(d);
    childrenOf.set(key, list);
  });
  // 穩定順序：同一層依 id 排，畫面每次重載才不會跳來跳去。
  childrenOf.forEach((list) => list.sort((a, b) => a.id - b.id));

  const depthById: Record<number, number> = {};
  const depthOf = (d: OrgChartDepartment): number => {
    if (depthById[d.id] !== undefined) return depthById[d.id];
    const parent = d.parentId != null ? byId.get(d.parentId) : undefined;
    const v = parent ? depthOf(parent) + 1 : 1;
    depthById[d.id] = v;
    return v;
  };
  departments.forEach(depthOf);

  let leafCounter = 0;
  const xById: Record<number, number> = {};
  const xOf = (d: OrgChartDepartment): number => {
    if (xById[d.id] !== undefined) return xById[d.id];
    const kids = childrenOf.get(d.id) ?? [];
    let x: number;
    if (!kids.length) {
      x = leafCounter * COL_WIDTH;
      leafCounter += 1;
    } else {
      x = kids.reduce((sum, k) => sum + xOf(k), 0) / kids.length;
    }
    xById[d.id] = x;
    return x;
  };
  const topLevel = childrenOf.get('root') ?? [];
  topLevel.forEach(xOf);

  const positions: Record<number, { x: number; y: number }> = {};
  departments.forEach((d) => {
    positions[d.id] = { x: xOf(d), y: depthOf(d) * ROW_HEIGHT };
  });
  const rootX = topLevel.length ? topLevel.reduce((s, d) => s + xOf(d), 0) / topLevel.length : 0;

  const nodeStyle: React.CSSProperties = {
    padding: '8px 12px',
    borderRadius: 8,
    fontSize: 12,
    minWidth: NODE_W,
    textAlign: 'center',
    border: `2px solid ${jvTokens.border}`,
    background: '#fff',
    cursor: 'pointer',
  };

  const nodes: RfNode[] = [
    {
      id: 'org-root',
      data: { label: <strong data-testid="org-chart-root">{rootLabel}</strong> },
      position: { x: rootX, y: 0 },
      style: { ...nodeStyle, background: jvTokens.primary, color: '#fff', borderColor: jvTokens.primary, cursor: 'default' },
      draggable: false,
      connectable: false,
      sourcePosition: 'bottom' as any,
      targetPosition: 'top' as any,
    },
    ...departments.map((d) => {
      const managers = d.members.filter((m) => m.role === 'manager');
      const label = (
        <Box data-testid="org-chart-node" data-dept-id={d.id} data-dept-key={d.key}>
          <div>{d.name}</div>
          <div style={{ fontSize: 10, color: jvTokens.muted }}>
            {managers.map((m) => m.name || `#${m.userId}`).join('、') || '—'} · {d.members.length}
          </div>
        </Box>
      );
      return {
        id: `dept-${d.id}`,
        data: { label },
        position: positions[d.id],
        style: nodeStyle,
        draggable: false,
        connectable: false,
        sourcePosition: 'bottom' as any,
        targetPosition: 'top' as any,
      };
    }),
  ];

  const edges: RfEdge[] = departments.map((d) => ({
    id: `edge-${d.id}`,
    source: d.parentId != null && byId.has(d.parentId) ? `dept-${d.parentId}` : 'org-root',
    target: `dept-${d.id}`,
    style: { stroke: jvTokens.muted, strokeWidth: 1.5 },
  }));

  return { nodes, edges, positions };
}

const OrgChart: React.FC<{
  orgName?: string | null;
  departments: OrgChartDepartment[];
  height?: number;
}> = ({ orgName, departments, height = 360 }) => {
  const translate = useTranslate();
  const [selectedId, setSelectedId] = React.useState<number | null>(null);

  const rootLabel = orgName || translate('eo.common.organization');
  const { nodes, edges } = React.useMemo(
    () => layoutDepartments(departments, rootLabel),
    [departments, rootLabel],
  );

  const selected = departments.find((d) => d.id === selectedId) ?? null;

  if (!departments.length) {
    return <Typography variant="body2" color="text.secondary">{translate('tc.departments.none')}</Typography>;
  }

  return (
    <Stack direction={{ xs: 'column', md: 'row' }} gap={2} data-testid="org-chart">
      <Box sx={{ flex: 2, height, border: `1px solid ${jvTokens.hairline}`, borderRadius: 1 }}>
        <ReactFlow
          nodes={nodes}
          edges={edges}
          fitView
          nodesDraggable={false}
          nodesConnectable={false}
          edgesReconnectable={false}
          elementsSelectable
          panOnScroll={false}
          onNodeClick={(_e, n) => {
            const id = Number(String(n.id).replace('dept-', ''));
            if (!Number.isNaN(id) && String(n.id).startsWith('dept-')) setSelectedId(id);
          }}
        >
          <Background />
          <Controls showInteractive={false} />
        </ReactFlow>
      </Box>
      <Box sx={{ flex: 1, minWidth: 220 }} data-testid="org-chart-detail">
        {selected ? (
          <>
            <Typography variant="subtitle2" gutterBottom>{selected.name}</Typography>
            <Typography variant="caption" color="text.secondary" display="block" mb={1}>
              {translate('tc.departments.managers')}
            </Typography>
            <Stack direction="row" gap={0.5} flexWrap="wrap" mb={1}>
              {selected.members.filter((m) => m.role === 'manager').map((m) => (
                <Chip key={m.userId} size="small" color="primary" label={m.name || `#${m.userId}`} />
              ))}
              {!selected.members.some((m) => m.role === 'manager') && (
                <Typography variant="caption" color="text.secondary">—</Typography>
              )}
            </Stack>
            <Typography variant="caption" color="text.secondary" display="block" mb={1}>
              {translate('tc.departments.members')}
            </Typography>
            <Stack direction="row" gap={0.5} flexWrap="wrap">
              {selected.members.filter((m) => m.role !== 'manager').map((m) => (
                <Chip key={m.userId} size="small" label={m.name || `#${m.userId}`} />
              ))}
              {!selected.members.some((m) => m.role !== 'manager') && (
                <Typography variant="caption" color="text.secondary">—</Typography>
              )}
            </Stack>
          </>
        ) : (
          <Typography variant="body2" color="text.secondary">{translate('tc.departments.orgChartPickHint')}</Typography>
        )}
      </Box>
    </Stack>
  );
};

export default OrgChart;
