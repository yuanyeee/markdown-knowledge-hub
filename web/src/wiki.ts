type Node = { type: string; value?: string; url?: string; children?: Node[] };
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
      if (['code', 'inlineCode', 'link', 'image', 'html'].includes(node.type) || !node.children) return;
      node.children = node.children.flatMap(child => {
        if (child.type === 'text' && child.value?.includes('[[')) return wikiNodes(child.value);
        walk(child); return [child];
      });
    }
    walk(tree);
  };
}
