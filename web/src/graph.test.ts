import { expect, test } from 'vitest';
import {
  buildGraphLayout,
  layoutNodes,
  splitNodeLabel,
  type GraphLayoutResult,
} from './graph';

function samplePath(d: string): Array<[number, number]> {
  const tokens = d.match(/[MLCQ]|-?\d+(?:\.\d+)?/g) ?? [];
  const points: Array<[number, number]> = [];
  let i = 0;
  let cx = 0;
  let cy = 0;
  const push = (x: number, y: number) => points.push([x, y]);
  while (i < tokens.length) {
    const cmd = tokens[i];
    if (cmd === 'M' || cmd === 'L') {
      const x = Number(tokens[i + 1]);
      const y = Number(tokens[i + 2]);
      if (cmd === 'L') {
        for (let step = 0; step <= 8; step += 1) {
          const t = step / 8;
          push(cx + (x - cx) * t, cy + (y - cy) * t);
        }
      } else {
        push(x, y);
      }
      cx = x;
      cy = y;
      i += 3;
    } else if (cmd === 'Q') {
      const x1 = Number(tokens[i + 1]);
      const y1 = Number(tokens[i + 2]);
      const x = Number(tokens[i + 3]);
      const y = Number(tokens[i + 4]);
      for (let step = 0; step <= 10; step += 1) {
        const t = step / 10;
        const u = 1 - t;
        push(
          u * u * cx + 2 * u * t * x1 + t * t * x,
          u * u * cy + 2 * u * t * y1 + t * t * y,
        );
      }
      cx = x;
      cy = y;
      i += 5;
    } else if (cmd === 'C') {
      const x1 = Number(tokens[i + 1]);
      const y1 = Number(tokens[i + 2]);
      const x2 = Number(tokens[i + 3]);
      const y2 = Number(tokens[i + 4]);
      const x = Number(tokens[i + 5]);
      const y = Number(tokens[i + 6]);
      for (let step = 0; step <= 12; step += 1) {
        const t = step / 12;
        const u = 1 - t;
        push(
          u * u * u * cx +
            3 * u * u * t * x1 +
            3 * u * t * t * x2 +
            t * t * t * x,
          u * u * u * cy +
            3 * u * u * t * y1 +
            3 * u * t * t * y2 +
            t * t * t * y,
        );
      }
      cx = x;
      cy = y;
      i += 7;
    } else {
      throw new Error(`未対応のパス命令: ${cmd}`);
    }
  }
  return points;
}

function assertEdgesStayOutsideCards(layout: GraphLayoutResult) {
  expect(layout.edges.length).toBeGreaterThan(0);
  for (const edge of layout.edges) {
    const samples = samplePath(edge.pathD);
    expect(samples.length).toBeGreaterThan(1);
    for (const [x, y] of samples) {
      expect(x).toBeGreaterThanOrEqual(layout.bounds.minX - 1);
      expect(x).toBeLessThanOrEqual(layout.bounds.minX + layout.bounds.width + 1);
      expect(y).toBeGreaterThanOrEqual(layout.bounds.minY - 1);
      expect(y).toBeLessThanOrEqual(
        layout.bounds.minY + layout.bounds.height + 1,
      );
      for (const node of layout.nodes) {
        const hitsLabel =
          x > node.x - node.width / 2 + 1 &&
          x < node.x + node.width / 2 - 1 &&
          y > node.y - node.height / 2 + 1 &&
          y < node.y + node.height / 2 - 1;
        expect(
          hitsLabel,
          `${edge.source} → ${edge.target} が ${node.path} のカードを横切っています (${x}, ${y})`,
        ).toBe(false);
      }
    }
  }
}

function assertCardsDoNotOverlap(layout: GraphLayoutResult) {
  for (let i = 0; i < layout.nodes.length; i += 1) {
    for (let j = i + 1; j < layout.nodes.length; j += 1) {
      const a = layout.nodes[i];
      const b = layout.nodes[j];
      const overlapX =
        Math.min(a.x + a.width / 2, b.x + b.width / 2) -
        Math.max(a.x - a.width / 2, b.x - b.width / 2);
      const overlapY =
        Math.min(a.y + a.height / 2, b.y + b.height / 2) -
        Math.max(a.y - a.height / 2, b.y - b.height / 2);
      expect(overlapX > 0 && overlapY > 0).toBe(false);
    }
  }
}

test('a graph lays out nodes with finite coordinates and keeps document identities', () => {
  const nodes = layoutNodes([
    { path: 'a.md', title: '注文' },
    { path: 'b.md', title: '顧客' },
  ]);
  expect(nodes.map((n) => n.path)).toEqual(['a.md', 'b.md']);
  expect(nodes.every((n) => Number.isFinite(n.x) && Number.isFinite(n.y))).toBe(
    true,
  );
  expect(nodes[0].x).not.toBe(nodes[1].x);
  expect(layoutNodes([])).toEqual([]);
  expect(layoutNodes([{ path: 'a.md', title: 'A' }])[0]).toMatchObject({
    path: 'a.md',
    y: 0,
  });
});

test('centered ego-graph separates incoming callers to left and outgoing targets to right with non-overlapping card boxes', () => {
  const nodes = [
    {
      path: 'center.md',
      title: 'サーバーアクション · BL_17_UpdateSellingPriceAndCosting_Flag',
    },
    {
      path: 'caller.md',
      title: '画面 · ETR_S003_01_EstimateHeadlineInput',
    },
    {
      path: 'callee1.md',
      title: 'サーバーアクション · EstimateDetailHeadline_CreateOrUpdate',
    },
    {
      path: 'callee2.md',
      title: 'PCR_BEF_BL_17_BusinessLogic',
    },
    {
      path: 'struct.md',
      title: 'Structure · DetailHeadlineUpdateValue',
    },
  ];
  const edges = [
    { source: 'caller.md', target: 'center.md', call: true },
    { source: 'center.md', target: 'callee1.md', call: true },
    { source: 'center.md', target: 'callee2.md', call: false },
    { source: 'center.md', target: 'struct.md', call: false },
  ];

  const layout = buildGraphLayout(nodes, 'center.md', edges);
  const byPath = new Map(layout.nodes.map((n) => [n.path, n]));
  const centerCard = byPath.get('center.md')!;
  const callerCard = byPath.get('caller.md')!;

  expect(centerCard).toMatchObject({ x: 0, y: 0, role: 'center' });
  expect(callerCard.x + callerCard.width / 2).toBeLessThan(
    centerCard.x - centerCard.width / 2 - 80,
  );

  for (const outId of ['callee1.md', 'callee2.md', 'struct.md']) {
    const outCard = byPath.get(outId)!;
    expect(outCard.x - outCard.width / 2).toBeGreaterThan(
      centerCard.x + centerCard.width / 2 + 80,
    );
  }

  assertCardsDoNotOverlap(layout);
  assertEdgesStayOutsideCards(layout);
  for (const node of layout.nodes) {
    expect(node.mainLines.length).toBeGreaterThan(0);
    expect(node.mainLines.length).toBeLessThanOrEqual(2);
    expect(node.width).toBeLessThanOrEqual(340);
  }
});

test('large multi-node graphs distribute across multiple columns without card collisions', () => {
  const many = Array.from({ length: 40 }, (_, i) => ({
    path: `doc-${i}.md`,
    title: `サーバーアクション · ActionName_${i}`,
  }));
  const centered = buildGraphLayout(many, 'doc-0.md');
  expect(centered.nodes[0]).toMatchObject({ path: 'doc-0.md', x: 0 });
  const columns = new Set(centered.nodes.map((n) => n.column));
  expect(columns.size).toBeGreaterThan(2);

  assertCardsDoNotOverlap(centered);
});

test('long OML names wrap inside the card and edges route around every card', () => {
  const nodes = [
    {
      path: 'center.md',
      title:
        'サーバーアクション · BL_17_UpdateSellingPriceAndCosting_Flag',
    },
  ];
  const edges: { source: string; target: string; call?: boolean }[] = [];
  for (let i = 0; i < 18; i += 1) {
    nodes.push({
      path: `in-${i}.md`,
      title: `画面 · ETR_S${String(i).padStart(3, '0')}_EstimateHeadlineInput`,
    });
    edges.push({ source: `in-${i}.md`, target: 'center.md', call: i % 2 === 0 });
  }
  for (let i = 0; i < 14; i += 1) {
    nodes.push({
      path: `out-${i}.md`,
      title: `サーバーアクション · EstimateDetailHeadline_CreateOrUpdate_${i}`,
    });
    edges.push({ source: 'center.md', target: `out-${i}.md`, call: true });
  }
  edges.push({ source: 'in-0.md', target: 'out-0.md', call: true });
  edges.push({ source: 'in-4.md', target: 'out-6.md' });
  edges.push({ source: 'out-1.md', target: 'center.md' });

  const layout = buildGraphLayout(nodes, 'center.md', edges);
  assertCardsDoNotOverlap(layout);
  assertEdgesStayOutsideCards(layout);
  const center = layout.nodes.find((n) => n.path === 'center.md')!;
  expect(center.prefix).toBe('サーバーアクション');
  expect(center.mainLines.length).toBeGreaterThan(0);
  expect(center.mainLines.length).toBeLessThanOrEqual(2);
  expect(center.mainLines.join('')).toContain('UpdateSelling');
});

test('general graphs keep shortcut edges outside intermediate cards', () => {
  const nodes = Array.from({ length: 24 }, (_, i) => ({
    path: `n-${i}.md`,
    title:
      i % 2 === 0
        ? `サーバーアクション · Action_${i}_UpdateSellingPrice`
        : `顧客マスタ_${i}`,
  }));
  const edges = nodes.flatMap((node, i) => {
    const next = [
      { source: node.path, target: `n-${(i + 1) % nodes.length}.md` },
    ];
    if (i % 3 === 0) {
      next.push({
        source: node.path,
        target: `n-${(i + 6) % nodes.length}.md`,
      });
    }
    return next;
  });
  const layout = buildGraphLayout(nodes, undefined, edges);
  assertCardsDoNotOverlap(layout);
  assertEdgesStayOutsideCards(layout);
});

test('splitNodeLabel separates OML element kind prefix from identifier', () => {
  expect(
    splitNodeLabel('サーバーアクション · EstimateDetailHeadline_CreateOrUpdate', 60),
  ).toEqual({
    prefix: 'サーバーアクション',
    main: 'EstimateDetailHeadline_CreateOrUpdate',
  });
  expect(splitNodeLabel('PCR_BEF_BL_17_BusinessLogic', 60)).toEqual({
    main: 'PCR_BEF_BL_17_BusinessLogic',
  });
});
