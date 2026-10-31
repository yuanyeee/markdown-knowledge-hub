import { expect, test } from 'vitest';
import { wikiNodes, sameTarget } from './wiki';
test('matches URL-encoded Japanese Markdown destinations', () => {
  expect(sameTarget('%E9%A1%A7%E5%AE%A2', '顧客')).toBe(true);
  expect(sameTarget('%E9%85%8D%E9%80%81.md', '配送.md')).toBe(true);
  expect(sameTarget('%invalid', '顧客')).toBe(false);
});
test('turns wikilinks into navigable AST nodes with aliases and fragments', () => {
  expect(wikiNodes('前 [[顧客#概要|説明]] 後')).toEqual([
    { type: 'text', value: '前 ' },
    { type: 'link', url: '顧客#概要', children: [{ type: 'text', value: '説明' }] },
    { type: 'text', value: ' 後' }
  ]);
});
