import { expect, test } from 'vitest';
import { remarkFlowTables } from './flowTables';

type FlowAstNode = {
  type: string;
  value?: string;
  lang?: string;
  children?: FlowAstNode[];
};

test('synthesizes a Mermaid flowchart before an OML flow table when no diagram precedes it', () => {
  const cell = (value: string): FlowAstNode => ({
    type: 'tableCell',
    children: [{ type: 'text', value }],
  });
  const row = (...values: string[]): FlowAstNode => ({
    type: 'tableRow',
    children: values.map(cell),
  });
  const tree: FlowAstNode = {
    type: 'root',
    children: [
      { type: 'heading', children: [{ type: 'text', value: '処理フロー' }] },
      {
        type: 'table',
        children: [
          row('#', '種別', '処理', '次へ'),
          row('1', 'アクション実行', 'ValidateOrder', '→ 2'),
          row('2', '終了', 'End', ''),
        ],
      },
    ],
  };

  remarkFlowTables()(tree);
  expect(tree.children).toHaveLength(3);
  expect(tree.children?.[1]).toMatchObject({
    type: 'code',
    lang: 'mermaid',
  });
  expect(tree.children?.[1].value).toContain('n1 --> n2');
});
