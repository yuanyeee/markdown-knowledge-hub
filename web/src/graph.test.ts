import { expect, test } from 'vitest';
import { layoutNodes } from './graph';

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
    x: 0,
    y: 0,
  });
});

test('centered and large graphs place center at origin and spread across concentric rings', () => {
  const many = Array.from({ length: 40 }, (_, i) => ({
    path: `doc-${i}.md`,
    title: `Doc ${i}`,
  }));
  const centered = layoutNodes(many, 'doc-0.md');
  expect(centered[0]).toMatchObject({ path: 'doc-0.md', x: 0, y: 0 });
  const radii = new Set(
    centered
      .slice(1)
      .map((n) => Math.hypot(n.x, n.y).toFixed(2)),
  );
  expect(radii.size).toBeGreaterThan(1);

  const overview = layoutNodes(many);
  const overviewRadii = new Set(
    overview.map((n) => Math.hypot(n.x, n.y).toFixed(2)),
  );
  expect(overviewRadii.size).toBeGreaterThan(1);
});
