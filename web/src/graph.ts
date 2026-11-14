export type GraphNode = { path: string; title: string };

export type GraphEdgeInput = {
  source: string;
  target: string;
  call?: boolean;
};

export type NodeRole =
  | 'center'
  | 'incoming'
  | 'outgoing'
  | 'bidirectional'
  | 'other';

export type SplitLabel = {
  prefix?: string;
  main: string;
};

export const GRAPH_LABEL = {
  x: 18,
  lineHeight: 16,
  prefixBaseline: 18,
  mainBaseline: 22,
  mainBaselineWithPrefix: 35,
} as const;

export type PositionedGraphNode = GraphNode & {
  x: number;
  y: number;
  width: number;
  height: number;
  column: number;
  role: NodeRole;
  prefix?: string;
  mainLines: string[];
};

export type RoutedEdge = {
  source: string;
  target: string;
  call: boolean;
  role: 'incoming' | 'outgoing' | 'neutral';
  pathD: string;
};

export type GraphLayoutResult = {
  nodes: PositionedGraphNode[];
  edges: RoutedEdge[];
  bounds: {
    minX: number;
    minY: number;
    width: number;
    height: number;
  };
};

export function truncateText(value: string, maxChars: number): string {
  const chars = [...value];
  if (chars.length <= maxChars) return value;
  return chars.slice(0, Math.max(1, maxChars - 1)).join('') + '…';
}

export function splitNodeLabel(rawTitle: string, maxChars = 48): SplitLabel {
  const trimmed = rawTitle.trim();
  const separator = ' · ';
  const sepIdx = trimmed.indexOf(separator);
  if (sepIdx > 0) {
    const prefix = trimmed.slice(0, sepIdx).trim();
    const rest = trimmed.slice(sepIdx + separator.length).trim();
    if (rest) {
      return {
        prefix: truncateText(prefix, Math.min(32, maxChars)),
        main: truncateText(rest, maxChars),
      };
    }
  }
  return { main: truncateText(trimmed, maxChars) };
}

function estimateTextWidth(text: string, fontSize: number, weight = 400): number {
  let width = 0;
  for (const ch of text) {
    const code = ch.codePointAt(0) ?? 0;
    const isWide =
      (code >= 0x1100 && code <= 0x115f) ||
      (code >= 0x2e80 && code <= 0xa4cf) ||
      (code >= 0xac00 && code <= 0xd7a3) ||
      (code >= 0xf900 && code <= 0xfaff) ||
      (code >= 0xfe30 && code <= 0xfe6f) ||
      (code >= 0xff00 && code <= 0xff60) ||
      (code >= 0xffe0 && code <= 0xffe6);
    width += fontSize * (isWide ? 1.06 : 0.62);
  }
  const weightScale = weight >= 700 ? 1.08 : weight >= 600 ? 1.05 : 1;
  return width * weightScale;
}

function fitLabel(
  text: string,
  fontSize: number,
  maxWidth: number,
  weight: number,
): string {
  if (estimateTextWidth(text, fontSize, weight) <= maxWidth) return text;
  const chars = [...text];
  let lo = 1;
  let hi = chars.length;
  let best = '…';
  while (lo <= hi) {
    const mid = Math.floor((lo + hi) / 2);
    const candidate = `${chars.slice(0, Math.max(1, mid - 1)).join('')}…`;
    if (estimateTextWidth(candidate, fontSize, weight) <= maxWidth) {
      best = candidate;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return best;
}

function tokenizeLabel(text: string): string[] {
  const tokens: string[] = [];
  let buf = '';
  const chars = [...text];
  for (let i = 0; i < chars.length; i += 1) {
    const ch = chars[i];
    const prev = i > 0 ? chars[i - 1] : '';
    const next = i + 1 < chars.length ? chars[i + 1] : '';
    if (ch === ' ' || ch === '\u3000') {
      if (buf) tokens.push(buf);
      buf = '';
      continue;
    }
    const camelBreak =
      prev !== '' &&
      ((/[a-z0-9]/.test(prev) && /[A-Z]/.test(ch)) ||
        (/[A-Z]/.test(prev) && /[A-Z]/.test(ch) && /[a-z]/.test(next)));
    if (camelBreak && buf) {
      tokens.push(buf);
      buf = '';
    }
    buf += ch;
    if (ch === '_' || ch === '-' || ch === '/' || ch === '.') {
      tokens.push(buf);
      buf = '';
    }
  }
  if (buf) tokens.push(buf);
  return tokens.length > 0 ? tokens : [text];
}

function wrapLabel(
  text: string,
  fontSize: number,
  maxWidth: number,
  maxLines: number,
  weight: number,
): string[] {
  const tokens = tokenizeLabel(text);
  const lines: string[] = [];
  let current = '';
  let i = 0;
  while (i < tokens.length) {
    const token = tokens[i];
    if (
      current === '' &&
      estimateTextWidth(token, fontSize, weight) > maxWidth
    ) {
      const chars = [...token];
      let buf = '';
      for (let c = 0; c < chars.length; c += 1) {
        const next = buf + chars[c];
        if (buf && estimateTextWidth(next, fontSize, weight) > maxWidth) {
          if (lines.length === maxLines - 1) {
            const rest =
              buf + chars.slice(c).join('') + tokens.slice(i + 1).join('');
            lines.push(fitLabel(rest, fontSize, maxWidth, weight));
            return lines;
          }
          lines.push(buf);
          buf = chars[c];
        } else {
          buf = next;
        }
      }
      current = buf;
      i += 1;
      continue;
    }
    const candidate = current + token;
    if (estimateTextWidth(candidate, fontSize, weight) <= maxWidth) {
      current = candidate;
      i += 1;
      continue;
    }
    if (lines.length === maxLines - 1) {
      lines.push(
        fitLabel(
          candidate + tokens.slice(i + 1).join(''),
          fontSize,
          maxWidth,
          weight,
        ),
      );
      return lines;
    }
    lines.push(current);
    current = '';
  }
  if (current) lines.push(current);
  return lines.length > 0 ? lines.slice(0, maxLines) : [''];
}

function measureCard(
  rawTitle: string,
  isCenter: boolean,
): {
  width: number;
  height: number;
  prefix?: string;
  mainLines: string[];
} {
  const { prefix: rawPrefix, main } = splitNodeLabel(rawTitle, 96);
  const mainFont = isCenter ? 13.5 : 12.5;
  const prefixFont = 10.5;
  const weight = isCenter ? 700 : 600;
  const maxInner = isCenter ? 292 : 268;
  const mainLines = wrapLabel(main, mainFont, maxInner, 2, weight);
  const prefix = rawPrefix
    ? fitLabel(rawPrefix, prefixFont, maxInner, 600)
    : undefined;
  const contentW = Math.max(
    prefix ? estimateTextWidth(prefix, prefixFont, 600) : 0,
    ...mainLines.map((line) => estimateTextWidth(line, mainFont, weight)),
    0,
  );
  const width = Math.max(156, Math.min(340, Math.ceil(contentW + 40)));
  const firstBaseline = prefix
    ? GRAPH_LABEL.mainBaselineWithPrefix
    : GRAPH_LABEL.mainBaseline;
  const lastBaseline =
    firstBaseline + (mainLines.length - 1) * GRAPH_LABEL.lineHeight;
  const height = lastBaseline + 13;
  return { width, height, prefix, mainLines };
}

type ColumnAssign = {
  node: GraphNode;
  role: NodeRole;
  logicalCol: number;
};

function assignCenteredColumns(
  nodes: GraphNode[],
  center: string,
  edges: GraphEdgeInput[],
): ColumnAssign[] {
  const incomingSet = new Set<string>();
  const outgoingSet = new Set<string>();
  const callNeighbors = new Set<string>();

  for (const edge of edges) {
    if (edge.target === center && edge.source !== center) {
      incomingSet.add(edge.source);
      if (edge.call) callNeighbors.add(edge.source);
    }
    if (edge.source === center && edge.target !== center) {
      outgoingSet.add(edge.target);
      if (edge.call) callNeighbors.add(edge.target);
    }
  }

  const left: ColumnAssign[] = [];
  const right: ColumnAssign[] = [];
  const shared: ColumnAssign[] = [];
  let centerNode: GraphNode | undefined;

  const neighbors = nodes.filter((n) => {
    if (n.path === center) {
      centerNode = n;
      return false;
    }
    return true;
  });

  if (incomingSet.size === 0 && outgoingSet.size === 0) {
    neighbors.forEach((node, idx) => {
      if (idx % 2 === 0) {
        right.push({ node, role: 'other', logicalCol: 1 });
      } else {
        left.push({ node, role: 'other', logicalCol: -1 });
      }
    });
  } else {
    const sorted = [...neighbors].sort((a, b) => {
      const aCall = callNeighbors.has(a.path) ? 0 : 1;
      const bCall = callNeighbors.has(b.path) ? 0 : 1;
      if (aCall !== bCall) return aCall - bCall;
      return a.title.localeCompare(b.title, 'ja');
    });

    for (const node of sorted) {
      const isIn = incomingSet.has(node.path);
      const isOut = outgoingSet.has(node.path);
      if (isIn && !isOut) {
        left.push({ node, role: 'incoming', logicalCol: -1 });
      } else if (isOut && !isIn) {
        right.push({ node, role: 'outgoing', logicalCol: 1 });
      } else if (isIn && isOut) {
        shared.push({ node, role: 'bidirectional', logicalCol: 1 });
      } else {
        shared.push({ node, role: 'other', logicalCol: 1 });
      }
    }

    for (const item of shared) {
      if (left.length < right.length) {
        left.push({ ...item, logicalCol: -1 });
      } else {
        right.push({ ...item, logicalCol: 1 });
      }
    }
  }

  const maxPerCol = 14;
  const expandedLeft: ColumnAssign[] = left.map((item, idx) => ({
    ...item,
    logicalCol: -(1 + Math.floor(idx / maxPerCol)),
  }));
  const expandedRight: ColumnAssign[] = right.map((item, idx) => ({
    ...item,
    logicalCol: 1 + Math.floor(idx / maxPerCol),
  }));

  return [
    ...expandedLeft,
    ...(centerNode
      ? [{ node: centerNode, role: 'center' as const, logicalCol: 0 }]
      : []),
    ...expandedRight,
  ];
}

function assignGeneralColumns(
  nodes: GraphNode[],
  edges: GraphEdgeInput[],
): ColumnAssign[] {
  const validPaths = new Set(nodes.map((n) => n.path));
  const outAdj = new Map<string, string[]>();
  const inDeg = new Map<string, number>();
  const outDeg = new Map<string, number>();

  for (const n of nodes) {
    outAdj.set(n.path, []);
    inDeg.set(n.path, 0);
    outDeg.set(n.path, 0);
  }
  for (const e of edges) {
    if (
      e.source !== e.target &&
      validPaths.has(e.source) &&
      validPaths.has(e.target)
    ) {
      outAdj.get(e.source)!.push(e.target);
      inDeg.set(e.target, (inDeg.get(e.target) ?? 0) + 1);
      outDeg.set(e.source, (outDeg.get(e.source) ?? 0) + 1);
    }
  }

  // Greedy cycle-aware rank assignment (0 .. 4)
  const rank = new Map<string, number>();
  const visited = new Set<string>();
  const ordered = [...nodes].sort((a, b) => {
    const diffA = (outDeg.get(a.path) ?? 0) - (inDeg.get(a.path) ?? 0);
    const diffB = (outDeg.get(b.path) ?? 0) - (inDeg.get(b.path) ?? 0);
    if (diffB !== diffA) return diffB - diffA;
    return a.path.localeCompare(b.path);
  });

  const roots = ordered.filter((n) => (inDeg.get(n.path) ?? 0) === 0);
  const queue: string[] = (roots.length > 0 ? roots : ordered.slice(0, 1)).map(
    (n) => n.path,
  );
  for (const p of queue) {
    rank.set(p, 0);
    visited.add(p);
  }

  let head = 0;
  while (head < queue.length) {
    const curr = queue[head++];
    const currRank = rank.get(curr) ?? 0;
    for (const next of outAdj.get(curr) ?? []) {
      if (!visited.has(next)) {
        visited.add(next);
        rank.set(next, Math.min(4, currRank + 1));
        queue.push(next);
      } else {
        const prevRank = rank.get(next) ?? 0;
        if (prevRank < currRank + 1 && currRank < 4 && prevRank > currRank) {
          rank.set(next, Math.min(4, currRank + 1));
        }
      }
    }
    if (head === queue.length && visited.size < nodes.length) {
      const nextSeed = ordered.find((n) => !visited.has(n.path));
      if (nextSeed) {
        visited.add(nextSeed.path);
        rank.set(nextSeed.path, 0);
        queue.push(nextSeed.path);
      }
    }
  }

  // Group by initial rank and split oversized columns so rows stay readable
  const byRank = new Map<number, GraphNode[]>();
  for (const n of nodes) {
    const isIsolated =
      (inDeg.get(n.path) ?? 0) === 0 && (outDeg.get(n.path) ?? 0) === 0;
    const r = isIsolated && edges.length > 0 ? 5 : (rank.get(n.path) ?? 0);
    const arr = byRank.get(r) ?? [];
    arr.push(n);
    byRank.set(r, arr);
  }

  const maxPerCol = nodes.length > 48 ? 14 : 10;
  const result: ColumnAssign[] = [];
  let nextCol = 0;
  for (const r of [...byRank.keys()].sort((a, b) => a - b)) {
    const group = byRank.get(r)!;
    const perCol =
      edges.length === 0 || r === 5
        ? Math.max(1, Math.ceil(group.length / Math.max(2, Math.ceil(Math.sqrt(group.length)))))
        : maxPerCol;
    for (let i = 0; i < group.length; i += perCol) {
      const slice = group.slice(i, i + perCol);
      for (const node of slice) {
        result.push({ node, role: 'other', logicalCol: nextCol });
      }
      nextCol += 1;
    }
  }

  return result;
}

export function buildGraphLayout(
  nodes: GraphNode[],
  center?: string,
  edges: GraphEdgeInput[] = [],
): GraphLayoutResult {
  if (nodes.length === 0) {
    return {
      nodes: [],
      edges: [],
      bounds: { minX: 0, minY: 0, width: 100, height: 100 },
    };
  }

  const hasCenter = Boolean(center && nodes.some((n) => n.path === center));
  const assigned = hasCenter
    ? assignCenteredColumns(nodes, center!, edges)
    : assignGeneralColumns(nodes, edges);

  // Group into ordered columns
  const colKeys = [...new Set(assigned.map((a) => a.logicalCol))].sort(
    (a, b) => a - b,
  );
  const colBuckets = new Map<number, ColumnAssign[]>();
  for (const k of colKeys) colBuckets.set(k, []);
  for (const item of assigned) {
    colBuckets.get(item.logicalCol)!.push(item);
  }

  // Barycenter vertical ordering pass to minimize edge crossings
  const neighborsMap = new Map<string, string[]>();
  for (const n of nodes) neighborsMap.set(n.path, []);
  for (const e of edges) {
    if (e.source !== e.target) {
      neighborsMap.get(e.source)?.push(e.target);
      neighborsMap.get(e.target)?.push(e.source);
    }
  }

  const rowOrder = new Map<string, number>();
  for (const k of colKeys) {
    colBuckets.get(k)!.forEach((item, idx) => rowOrder.set(item.node.path, idx));
  }
  for (let pass = 0; pass < 3; pass += 1) {
    const keysPass = pass % 2 === 0 ? colKeys : [...colKeys].reverse();
    for (const k of keysPass) {
      if (hasCenter && k === 0) continue;
      const bucket = colBuckets.get(k)!;
      if (bucket.length <= 2) continue;
      bucket.sort((a, b) => {
        const aNeighbors = (neighborsMap.get(a.node.path) ?? []).filter(
          (p) => p !== center,
        );
        const bNeighbors = (neighborsMap.get(b.node.path) ?? []).filter(
          (p) => p !== center,
        );
        const baryA =
          aNeighbors.length > 0
            ? aNeighbors.reduce((s, p) => s + (rowOrder.get(p) ?? 0), 0) /
              aNeighbors.length
            : (rowOrder.get(a.node.path) ?? 0);
        const baryB =
          bNeighbors.length > 0
            ? bNeighbors.reduce((s, p) => s + (rowOrder.get(p) ?? 0), 0) /
              bNeighbors.length
            : (rowOrder.get(b.node.path) ?? 0);
        if (Math.abs(baryA - baryB) > 0.01) return baryA - baryB;
        return (
          (rowOrder.get(a.node.path) ?? 0) - (rowOrder.get(b.node.path) ?? 0)
        );
      });
      bucket.forEach((item, idx) => rowOrder.set(item.node.path, idx));
    }
  }

  // Measure cards and compute column X coordinates & row Y coordinates
  const cardMetrics = new Map<
    string,
    ReturnType<typeof measureCard>
  >();
  for (const n of nodes) {
    cardMetrics.set(n.path, measureCard(n.title, n.path === center));
  }

  const colGapX = 152;
  const rowGapY = 30;
  const colCenterX = new Map<number, number>();
  const colWidth = new Map<number, number>();

  for (const k of colKeys) {
    const maxW = Math.max(
      ...colBuckets.get(k)!.map((a) => cardMetrics.get(a.node.path)!.width),
    );
    colWidth.set(k, maxW);
  }

  if (hasCenter && colKeys.includes(0)) {
    colCenterX.set(0, 0);
    // Walk right from 0
    const rightKeys = colKeys.filter((k) => k > 0).sort((a, b) => a - b);
    let prevKey = 0;
    for (const k of rightKeys) {
      const prevX = colCenterX.get(prevKey)!;
      const step =
        colWidth.get(prevKey)! / 2 + colGapX + colWidth.get(k)! / 2;
      colCenterX.set(k, prevX + step);
      prevKey = k;
    }
    // Walk left from 0
    const leftKeys = colKeys.filter((k) => k < 0).sort((a, b) => b - a);
    prevKey = 0;
    for (const k of leftKeys) {
      const prevX = colCenterX.get(prevKey)!;
      const step =
        colWidth.get(prevKey)! / 2 + colGapX + colWidth.get(k)! / 2;
      colCenterX.set(k, prevX - step);
      prevKey = k;
    }
  } else {
    let cursorX = 0;
    for (const k of colKeys) {
      const w = colWidth.get(k)!;
      colCenterX.set(k, cursorX + w / 2);
      cursorX += w + colGapX;
    }
  }

  const positionedByPath = new Map<string, PositionedGraphNode>();

  for (const k of colKeys) {
    const bucket = colBuckets.get(k)!;
    const cx = colCenterX.get(k)!;
    // Stagger outer multi-columns by half a card pitch so long edges thread between cards
    const staggerY =
      hasCenter && Math.abs(k) > 1 ? (Math.abs(k) % 2 === 0 ? 32 : -16) : 0;
    const totalH =
      bucket.reduce(
        (sum, item) => sum + cardMetrics.get(item.node.path)!.height,
        0,
      ) +
      Math.max(0, bucket.length - 1) * rowGapY;
    let cursorY = -totalH / 2 + staggerY;

    for (const item of bucket) {
      const m = cardMetrics.get(item.node.path)!;
      const cy = cursorY + m.height / 2;
      positionedByPath.set(item.node.path, {
        ...item.node,
        x: cx,
        y: cy,
        width: m.width,
        height: m.height,
        column: k,
        role: item.role,
        prefix: m.prefix,
        mainLines: m.mainLines,
      });
      cursorY += m.height + rowGapY;
    }
  }

  const positionedNodes = nodes
    .map((n) => positionedByPath.get(n.path)!)
    .filter(Boolean);

  // Ports fan out when several edges share a card side. Curves stay in the
  // gutters between column boxes so they never cross a card or its label.
  const seenEdgePairs = new Set<string>();
  const validEdges = edges.filter((e) => {
    if (
      !positionedByPath.has(e.source) ||
      !positionedByPath.has(e.target) ||
      e.source === e.target
    ) {
      return false;
    }
    const key = `${e.source}->${e.target}`;
    if (seenEdgePairs.has(key)) return false;
    seenEdgePairs.add(key);
    return true;
  });

  const travelDir = (sourceCol: number, targetCol: number): number => {
    if (sourceCol === targetCol) return hasCenter && sourceCol > 0 ? -1 : 1;
    return sourceCol < targetCol ? 1 : -1;
  };

  const exitGroups = new Map<string, GraphEdgeInput[]>();
  const entryGroups = new Map<string, GraphEdgeInput[]>();
  for (const e of validEdges) {
    const s = positionedByPath.get(e.source)!;
    const t = positionedByPath.get(e.target)!;
    const dir = travelDir(s.column, t.column);
    const exitKey = `${e.source}|${dir}`;
    const entryKey = `${e.target}|${dir}`;
    const exitList = exitGroups.get(exitKey) ?? [];
    exitList.push(e);
    exitGroups.set(exitKey, exitList);
    const entryList = entryGroups.get(entryKey) ?? [];
    entryList.push(e);
    entryGroups.set(entryKey, entryList);
  }
  for (const [key, list] of exitGroups) {
    const side = key.endsWith('|-1') ? -1 : 1;
    list.sort((a, b) => {
      const ay = positionedByPath.get(a.target)!.y;
      const by = positionedByPath.get(b.target)!.y;
      return side === 1 ? ay - by : by - ay;
    });
  }
  for (const [key, list] of entryGroups) {
    const side = key.endsWith('|-1') ? -1 : 1;
    list.sort((a, b) => {
      const ay = positionedByPath.get(a.source)!.y;
      const by = positionedByPath.get(b.source)!.y;
      return side === 1 ? ay - by : by - ay;
    });
  }

  const portYOffset = (
    list: GraphEdgeInput[] | undefined,
    edge: GraphEdgeInput,
    cardH: number,
  ): number => {
    if (!list || list.length <= 1) return 0;
    const idx = list.indexOf(edge);
    if (idx < 0) return 0;
    const maxSpread = Math.max(0, Math.min(cardH - 20, (list.length - 1) * 11));
    if (maxSpread === 0) return 0;
    return -maxSpread / 2 + (idx / (list.length - 1)) * maxSpread;
  };

  const columnEdge = (col: number, dir: number): number => {
    const cx = colCenterX.get(col)!;
    const w = colWidth.get(col)!;
    return cx + (dir * w) / 2;
  };

  const fmt = (n: number): string => {
    const rounded = Math.round(n * 10) / 10;
    return Object.is(rounded, -0) ? '0' : String(rounded);
  };

  let minX = Infinity;
  let maxX = -Infinity;
  let minY = Infinity;
  let maxY = -Infinity;
  const note = (x: number, y: number) => {
    minX = Math.min(minX, x);
    maxX = Math.max(maxX, x);
    minY = Math.min(minY, y);
    maxY = Math.max(maxY, y);
  };
  for (const n of positionedNodes) {
    note(n.x - n.width / 2, n.y - n.height / 2);
    note(n.x + n.width / 2, n.y + n.height / 2);
  }

  const roundedPolyline = (
    raw: Array<[number, number]>,
    radius: number,
  ): string => {
    const pts = raw.filter(
      (p, index) =>
        index === 0 ||
        Math.hypot(p[0] - raw[index - 1][0], p[1] - raw[index - 1][1]) > 0.4,
    );
    if (pts.length === 0) return '';
    let d = `M ${fmt(pts[0][0])} ${fmt(pts[0][1])}`;
    note(pts[0][0], pts[0][1]);
    for (let i = 1; i < pts.length - 1; i += 1) {
      const [x0, y0] = pts[i - 1];
      const [x1, y1] = pts[i];
      const [x2, y2] = pts[i + 1];
      const v1x = x1 - x0;
      const v1y = y1 - y0;
      const v2x = x2 - x1;
      const v2y = y2 - y1;
      const len1 = Math.hypot(v1x, v1y) || 1;
      const len2 = Math.hypot(v2x, v2y) || 1;
      const r = Math.min(radius, len1 / 2.2, len2 / 2.2);
      const ax = x1 - (v1x / len1) * r;
      const ay = y1 - (v1y / len1) * r;
      const bx = x1 + (v2x / len2) * r;
      const by = y1 + (v2y / len2) * r;
      d += ` L ${fmt(ax)} ${fmt(ay)} Q ${fmt(x1)} ${fmt(y1)}, ${fmt(bx)} ${fmt(by)}`;
      note(ax, ay);
      note(x1, y1);
      note(bx, by);
    }
    const last = pts[pts.length - 1];
    d += ` L ${fmt(last[0])} ${fmt(last[1])}`;
    note(last[0], last[1]);
    return d;
  };

  type PendingDetour = {
    edge: GraphEdgeInput;
    role: RoutedEdge['role'];
    sy: number;
    ty: number;
    exitX: number;
    enterX: number;
    c1: number;
    c2: number;
    above: boolean;
    baseY: number;
    laneY: number;
  };

  const detours: PendingDetour[] = [];
  const directEdges: RoutedEdge[] = [];
  const gutter = Math.min(56, colGapX * 0.34);

  for (const e of validEdges) {
    const s = positionedByPath.get(e.source)!;
    const t = positionedByPath.get(e.target)!;
    const dir = travelDir(s.column, t.column);
    const edgeRole: RoutedEdge['role'] =
      hasCenter && e.source === center
        ? 'outgoing'
        : hasCenter && e.target === center
          ? 'incoming'
          : 'neutral';
    const sy =
      s.y + portYOffset(exitGroups.get(`${e.source}|${dir}`), e, s.height);
    const ty =
      t.y + portYOffset(entryGroups.get(`${e.target}|${dir}`), e, t.height);
    const exitX = s.x + (dir * s.width) / 2;
    const enterX = t.x - (dir * t.width) / 2;

    if (s.column === t.column) {
      const outer = columnEdge(s.column, dir);
      const bow = Math.min(64, Math.max(34, Math.abs(ty - sy) * 0.28));
      const x1 = outer + dir * 8;
      const xCurve = outer + dir * (8 + bow);
      const sameEnter = t.x + (dir * t.width) / 2;
      const pathD = `M ${fmt(exitX)} ${fmt(sy)} L ${fmt(x1)} ${fmt(sy)} C ${fmt(xCurve)} ${fmt(sy)}, ${fmt(xCurve)} ${fmt(ty)}, ${fmt(x1)} ${fmt(ty)} L ${fmt(sameEnter)} ${fmt(ty)}`;
      note(exitX, sy);
      note(x1, sy);
      note(xCurve, sy);
      note(xCurve, ty);
      note(x1, ty);
      note(sameEnter, ty);
      directEdges.push({
        source: e.source,
        target: e.target,
        call: Boolean(e.call),
        role: edgeRole,
        pathD,
      });
      continue;
    }

    const lo = Math.min(s.column, t.column);
    const hi = Math.max(s.column, t.column);
    const mids = colKeys.filter((col) => col > lo && col < hi);
    const x1 = columnEdge(s.column, dir) + dir * 8;
    const x2 = columnEdge(t.column, -dir) - dir * 8;

    if (mids.length === 0) {
      const span = Math.abs(x2 - x1);
      const dx = Math.max(32, span * 0.46);
      const yBow = seenEdgePairs.has(`${e.target}->${e.source}`) ? 14 : 0;
      const pathD = `M ${fmt(exitX)} ${fmt(sy)} L ${fmt(x1)} ${fmt(sy)} C ${fmt(x1 + dir * dx)} ${fmt(sy + yBow)}, ${fmt(x2 - dir * dx)} ${fmt(ty + yBow)}, ${fmt(x2)} ${fmt(ty)} L ${fmt(enterX)} ${fmt(ty)}`;
      note(exitX, sy);
      note(x1, sy);
      note(x1 + dir * dx, sy + yBow);
      note(x2 - dir * dx, ty + yBow);
      note(x2, ty);
      note(enterX, ty);
      directEdges.push({
        source: e.source,
        target: e.target,
        call: Boolean(e.call),
        role: edgeRole,
        pathD,
      });
      continue;
    }

    let top = Infinity;
    let bottom = -Infinity;
    for (const col of mids) {
      for (const item of colBuckets.get(col) ?? []) {
        const card = positionedByPath.get(item.node.path)!;
        top = Math.min(top, card.y - card.height / 2);
        bottom = Math.max(bottom, card.y + card.height / 2);
      }
    }
    const aboveY = top - 30;
    const belowY = bottom + 30;
    const aboveCost = Math.abs(sy - aboveY) + Math.abs(ty - aboveY);
    const belowCost = Math.abs(sy - belowY) + Math.abs(ty - belowY);
    const above = aboveCost <= belowCost;
    const c1 = columnEdge(s.column, dir) + dir * gutter;
    const c2 = columnEdge(t.column, -dir) - dir * gutter;
    detours.push({
      edge: e,
      role: edgeRole,
      sy,
      ty,
      exitX,
      enterX,
      c1,
      c2,
      above,
      baseY: above ? aboveY : belowY,
      laneY: above ? aboveY : belowY,
    });
  }

  const usedLanes: number[] = [];
  const claimLane = (desired: number, outward: number): number => {
    let y = desired;
    let guard = 0;
    while (usedLanes.some((lane) => Math.abs(lane - y) < 10) && guard < 48) {
      y += outward * 12;
      guard += 1;
    }
    usedLanes.push(y);
    return y;
  };
  const aboveDetours = detours
    .filter((d) => d.above)
    .sort((a, b) => b.baseY - a.baseY || a.sy - b.sy);
  const belowDetours = detours
    .filter((d) => !d.above)
    .sort((a, b) => a.baseY - b.baseY || a.sy - b.sy);
  for (const d of aboveDetours) d.laneY = claimLane(d.baseY, -1);
  for (const d of belowDetours) d.laneY = claimLane(d.baseY, 1);

  const routedEdges: RoutedEdge[] = [
    ...directEdges,
    ...detours.map((d) => ({
      source: d.edge.source,
      target: d.edge.target,
      call: Boolean(d.edge.call),
      role: d.role,
      pathD: roundedPolyline(
        [
          [d.exitX, d.sy],
          [d.c1, d.sy],
          [d.c1, d.laneY],
          [d.c2, d.laneY],
          [d.c2, d.ty],
          [d.enterX, d.ty],
        ],
        16,
      ),
    })),
  ];

  // Sort so colored / call edges paint on top of neutral background edges
  routedEdges.sort((a, b) => {
    const score = (edge: RoutedEdge) =>
      (edge.role !== 'neutral' ? 2 : 0) + (edge.call ? 1 : 0);
    return score(a) - score(b);
  });

  const pad = 48;

  return {
    nodes: positionedNodes,
    edges: routedEdges,
    bounds: {
      minX: minX - pad,
      minY: minY - pad,
      width: Math.max(200, maxX - minX + pad * 2),
      height: Math.max(140, maxY - minY + pad * 2),
    },
  };
}

export function layoutNodes(
  nodes: GraphNode[],
  center?: string,
  edges: GraphEdgeInput[] = [],
): PositionedGraphNode[] {
  return buildGraphLayout(nodes, center, edges).nodes;
}
