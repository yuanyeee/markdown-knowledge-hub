type Node = { type: string; value?: string; url?: string; children?: Node[];data?:Record<string,unknown> };
export function headingId(text:string):string {
  return [...text.trim().toLowerCase()].filter(c=>/\s|[\p{L}\p{N}_-]/u.test(c)).map(c=>/\s/.test(c)?'-':c).join('');
}
type HtmlNode={type:string;tagName?:string;value?:string;properties?:Record<string,unknown>;children?:HtmlNode[]};
export function rehypeHeadingIds(){
  return(tree:HtmlNode)=>{
    const counts=new Map<string,number>();
    const text=(node:HtmlNode):string=>node.value??node.children?.map(text).join('')??'';
    function walk(node:HtmlNode){
      if(node.type==='element'&&/^h[1-6]$/.test(node.tagName||'')){
        const full=text(node);const explicit=full.match(/\s+\{#([^}\s]+)\}\s*$/);
        const slug=headingId(explicit?full.slice(0,explicit.index):full);const count=counts.get(slug)||0;counts.set(slug,count+1);
        node.properties={...node.properties,id:explicit?.[1]||(count?`${slug}-${count}`:slug)};
        if(explicit){const last=node.children?.at(-1);if(last?.value)last.value=last.value.replace(/\s+\{#[^}\s]+\}\s*$/,'');}
      }
      node.children?.forEach(walk);
    }walk(tree);
  };
}
export function displayMarkdown(raw:string):string{return raw.replace(/^---\r?\n[\s\S]*?\r?\n---\r?\n/,'');}
export function sameTarget(url: string | undefined, target: string): boolean {
  if (!url) return false;
  try { return decodeURIComponent(url) === decodeURIComponent(target); }
  catch { return false; }
}
export function wikiNodes(value: string): Node[] {
  const result: Node[] = [];
  const regex = /\[\[([^\]\n]+)\]\]/g;
  let start = 0;
  for (const match of value.matchAll(regex)) {
    const offset = match.index!;
    if (offset > start) result.push({ type: 'text', value: value.slice(start, offset) });
    const [url, ...label] = match[1].split('|');
    result.push({ type: 'link', url, children: [{ type: 'text', value: label.join('|') || url }] });
    start = offset + match[0].length;
  }
  if (start < value.length) result.push({ type: 'text', value: value.slice(start) });
  return result;
}
export function remarkWiki() {
  return (tree: Node) => {
    function walk(node: Node) {
      if(node.type==='html'){
        const anchor=node.value?.match(/^\s*<a\s+(?:id|name)\s*=\s*["']([^"'<>\s]+)["']\s*>\s*(?:<\/a>\s*)?$/i);
        if(anchor){node.type='paragraph';node.value=undefined;node.data={hName:'span',hProperties:{id:anchor[1]}};node.children=[];}
        return;
      }
      if (['code', 'inlineCode', 'link', 'image', 'html'].includes(node.type) || !node.children) return;
      node.children = node.children.flatMap(child => {
        if (child.type === 'text' && child.value?.includes('[[')) return wikiNodes(child.value);
        walk(child); return [child];
      });
    }
    walk(tree);
  };
}
