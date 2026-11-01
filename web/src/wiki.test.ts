import { expect, test } from 'vitest';
import { wikiNodes, sameTarget,headingId,displayMarkdown,rehypeHeadingIds,remarkWiki } from './wiki';
test('only safe empty anchors become DOM targets',()=>{
  const tree={type:'root',children:[{type:'html',value:'<a id="screen-login"></a>'},{type:'html',value:'<a id="unsafe" onclick="code()"></a>'}]};
  remarkWiki()(tree);
  expect(tree.children[0].type).toBe('paragraph');
  expect(tree.children[1].type).toBe('html');
});
test('heading IDs are stable and metadata is hidden',()=>{
  expect(headingId('詳細 Information!')).toBe('詳細-information');
  expect(displayMarkdown('---\naliases: [Test]\n---\n# Title')).toBe('# Title');
  const tree={type:'root',children:[{type:'element',tagName:'h2',properties:{},children:[{type:'text',value:'Details'}]},{type:'element',tagName:'h2',properties:{},children:[{type:'text',value:'Details'}]}]};
  rehypeHeadingIds()(tree);
  expect(tree.children[0].properties).toEqual({id:'details'});
  expect(tree.children[1].properties).toEqual({id:'details-1'});
});
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
