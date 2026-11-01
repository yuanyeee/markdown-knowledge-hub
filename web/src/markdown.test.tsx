import {test,expect} from 'vitest';
import {renderToStaticMarkup} from 'react-dom/server';
import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import {remarkWiki} from './wiki';
test('renders tables and safe anchors without executing HTML',()=>{
  const result=renderToStaticMarkup(<Markdown skipHtml remarkPlugins={[remarkGfm,remarkWiki]}>{'| A | B |\n|---|---|\n| 1 | 2 |\n\n<a id="screen-login"></a>\n\n<script>unsafe()</script>'}</Markdown>);
  expect(result).toContain('<table>');expect(result).toContain('id="screen-login"');expect(result).not.toContain('&lt;/a&gt;');expect(result).not.toContain('unsafe()');
});
