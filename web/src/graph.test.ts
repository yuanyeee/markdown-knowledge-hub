import { expect, test } from 'vitest';
import { layoutNodes } from './graph';
test('a graph lays out nodes with finite coordinates and keeps document identities', () => {
  const nodes=layoutNodes([{path:'a.md',title:'注文'},{path:'b.md',title:'顧客'}]);
  expect(nodes.map(n=>n.path)).toEqual(['a.md','b.md']);
  expect(nodes.every(n=>Number.isFinite(n.x)&&Number.isFinite(n.y))).toBe(true);
  expect(nodes[0].x).not.toBe(nodes[1].x);
  expect(layoutNodes([])).toEqual([]);
  expect(layoutNodes([{path:'a.md',title:'A'}])[0]).toMatchObject({x:0,y:0});
});
