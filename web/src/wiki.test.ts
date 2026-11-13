import { expect, test } from 'vitest';
import {
  displayMarkdown,
  extractHeadings,
  headingId,
  highlightParts,
  rehypeHeadingIds,
  remarkWiki,
  sameTarget,
  wikiNodes,
} from './wiki';

test('only safe empty anchors become DOM targets', () => {
  const tree = {
    type: 'root',
    children: [
      { type: 'html', value: '<a id="screen-login"></a>' },
      { type: 'html', value: '<a id="unsafe" onclick="code()"></a>' },
    ],
  };
  remarkWiki()(tree);
  expect(tree.children[0].type).toBe('paragraph');
  expect(tree.children[1].type).toBe('html');
});

test('heading IDs are stable, support explicit IDs, and metadata is hidden', () => {
  expect(headingId('詳細 Information!')).toBe('詳細-information');
  expect(displayMarkdown('---\naliases: [Test]\n---\n# Title')).toBe('# Title');
  const tree = {
    type: 'root',
    children: [
      {
        type: 'element',
        tagName: 'h2',
        properties: {},
        children: [{ type: 'text', value: 'Details' }],
      },
      {
        type: 'element',
        tagName: 'h2',
        properties: {},
        children: [{ type: 'text', value: 'Details' }],
      },
      {
        type: 'element',
        tagName: 'h3',
        properties: {},
        children: [
          {
            type: 'element',
            tagName: 'strong',
            children: [{ type: 'text', value: 'Login {#custom-login}' }],
          },
        ],
      },
    ],
  };
  rehypeHeadingIds()(tree);
  expect(tree.children[0].properties).toEqual({ id: 'details' });
  expect(tree.children[1].properties).toEqual({ id: 'details-1' });
  expect(tree.children[2].properties).toEqual({ id: 'custom-login' });
  const nestedStrong = tree.children[2].children?.[0];
  expect(
    nestedStrong && 'children' in nestedStrong
      ? nestedStrong.children[0].value
      : undefined,
  ).toBe('Login');
});

test('extractHeadings ignores fenced code and deduplicates heading slugs', () => {
  const md = [
    '---',
    'aliases: [Spec]',
    '---',
    '# 注文仕様',
    '```md',
    '# コード内の見出し',
    '```',
    '## 概要',
    '## 概要',
    '### **認証フロー** {#auth-flow}',
  ].join('\n');
  expect(extractHeadings(md)).toEqual([
    { id: '注文仕様', text: '注文仕様', level: 1 },
    { id: '概要', text: '概要', level: 2 },
    { id: '概要-1', text: '概要', level: 2 },
    { id: 'auth-flow', text: '認証フロー', level: 3 },
  ]);
});

test('highlightParts splits matched search terms case-insensitively', () => {
  expect(highlightParts('東京のCustomer注文管理', 'customer 注文')).toEqual([
    { text: '東京の', match: false },
    { text: 'Customer', match: true },
    { text: '注文', match: true },
    { text: '管理', match: false },
  ]);
  expect(highlightParts('プレーンテキスト', '')).toEqual([
    { text: 'プレーンテキスト', match: false },
  ]);
});

test('matches URL-encoded Japanese Markdown destinations', () => {
  expect(sameTarget('%E9%A1%A7%E5%AE%A2', '顧客')).toBe(true);
  expect(sameTarget('%E9%85%8D%E9%80%81.md', '配送.md')).toBe(true);
  expect(sameTarget('%invalid', '顧客')).toBe(false);
});

test('turns wikilinks into navigable AST nodes with aliases and fragments', () => {
  expect(wikiNodes('前 [[顧客#概要|説明]] 後')).toEqual([
    { type: 'text', value: '前 ' },
    {
      type: 'link',
      url: '顧客#概要',
      children: [{ type: 'text', value: '説明' }],
    },
    { type: 'text', value: ' 後' },
  ]);
});
