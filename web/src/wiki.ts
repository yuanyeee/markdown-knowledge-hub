type Node = {
  type: string;
  value?: string;
  url?: string;
  children?: Node[];
  data?: Record<string, unknown>;
};

export type HeadingItem = {
  id: string;
  text: string;
  level: number;
};

export type HighlightPart = {
  text: string;
  match: boolean;
};

export function headingId(text: string): string {
  return [...text.trim().toLowerCase()]
    .filter((c) => /\s|[\p{L}\p{N}_-]/u.test(c))
    .map((c) => (/\s/.test(c) ? '-' : c))
    .join('');
}

type HtmlNode = {
  type: string;
  tagName?: string;
  value?: string;
  properties?: Record<string, unknown>;
  children?: HtmlNode[];
};

function stripExplicitAttrFromTail(node: HtmlNode): void {
  if (node.value) {
    node.value = node.value.replace(/\s+\{#[^}\s]+\}\s*$/, '');
    return;
  }
  const last = node.children?.at(-1);
  if (last) {
    stripExplicitAttrFromTail(last);
  }
}

export function rehypeHeadingIds() {
  return (tree: HtmlNode) => {
    const counts = new Map<string, number>();
    const text = (node: HtmlNode): string =>
      node.value ?? node.children?.map(text).join('') ?? '';

    function walk(node: HtmlNode) {
      if (node.type === 'element' && /^h[1-6]$/.test(node.tagName || '')) {
        const full = text(node);
        const explicit = full.match(/\s+\{#([^}\s]+)\}\s*$/);
        const slug = headingId(explicit ? full.slice(0, explicit.index) : full);
        const count = counts.get(slug) || 0;
        counts.set(slug, count + 1);
        node.properties = {
          ...node.properties,
          id: explicit?.[1] || (count ? `${slug}-${count}` : slug),
        };
        if (explicit) {
          stripExplicitAttrFromTail(node);
        }
      }
      node.children?.forEach(walk);
    }

    walk(tree);
  };
}

export function displayMarkdown(raw: string): string {
  return raw.replace(/^---\r?\n[\s\S]*?\r?\n---\r?\n/, '');
}

function cleanInlineHeadingText(raw: string): string {
  return raw
    .replace(/\[\[([^\]|\n]+)\|([^\]\n]+)\]\]/g, '$2')
    .replace(/\[\[([^\]\n]+)\]\]/g, '$1')
    .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
    .replace(/[`*_~]/g, '')
    .trim();
}

export function extractHeadings(raw: string): HeadingItem[] {
  const body = displayMarkdown(raw);
  const counts = new Map<string, number>();
  const headings: HeadingItem[] = [];
  let inFence: string | null = null;

  for (const line of body.split(/\r?\n/)) {
    const fence = line.match(/^\s*(`{3,}|~{3,})/);
    if (fence) {
      const marker = fence[1][0];
      if (!inFence) {
        inFence = marker;
      } else if (inFence === marker) {
        inFence = null;
      }
      continue;
    }
    if (inFence) continue;

    const match = line.match(/^\s{0,3}(#{1,6})\s+(.+?)\s*#*\s*$/);
    if (!match) continue;

    const level = match[1].length;
    const rawHeading = match[2];
    const explicit = rawHeading.match(/\s+\{#([^}\s]+)\}\s*$/);
    const visibleRaw = explicit ? rawHeading.slice(0, explicit.index) : rawHeading;
    const label = cleanInlineHeadingText(visibleRaw);
    if (!label) continue;

    const slug = headingId(label);
    const count = counts.get(slug) || 0;
    counts.set(slug, count + 1);
    const id = explicit?.[1] || (count ? `${slug}-${count}` : slug);
    headings.push({ id, text: label, level });
  }

  return headings;
}

export function highlightParts(text: string, query: string): HighlightPart[] {
  const words = Array.from(
    new Set(
      query
        .trim()
        .split(/\s+/)
        .filter((w) => w.length > 0),
    ),
  );
  if (!text || words.length === 0) {
    return text ? [{ text, match: false }] : [];
  }
  const escaped = words
    .sort((a, b) => b.length - a.length)
    .map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'));
  const pattern = new RegExp(`(${escaped.join('|')})`, 'gi');
  const parts: HighlightPart[] = [];
  let lastIndex = 0;

  for (const hit of text.matchAll(pattern)) {
    const start = hit.index ?? 0;
    if (start > lastIndex) {
      parts.push({ text: text.slice(lastIndex, start), match: false });
    }
    parts.push({ text: hit[0], match: true });
    lastIndex = start + hit[0].length;
  }
  if (lastIndex < text.length) {
    parts.push({ text: text.slice(lastIndex), match: false });
  }
  return parts;
}

export function sameTarget(url: string | undefined, target: string): boolean {
  if (!url) return false;
  try {
    return decodeURIComponent(url) === decodeURIComponent(target);
  } catch {
    return false;
  }
}

export function wikiNodes(value: string): Node[] {
  const result: Node[] = [];
  const regex = /\[\[([^\]\n]+)\]\]/g;
  let start = 0;
  for (const match of value.matchAll(regex)) {
    const offset = match.index!;
    if (offset > start) {
      result.push({ type: 'text', value: value.slice(start, offset) });
    }
    const [url, ...label] = match[1].split('|');
    result.push({
      type: 'link',
      url,
      children: [{ type: 'text', value: label.join('|') || url }],
    });
    start = offset + match[0].length;
  }
  if (start < value.length) {
    result.push({ type: 'text', value: value.slice(start) });
  }
  return result;
}

export function remarkWiki() {
  return (tree: Node) => {
    function walk(node: Node) {
      if (node.type === 'html') {
        const anchor = node.value?.match(
          /^\s*<a\s+(?:id|name)\s*=\s*["']([^"'<>\s]+)["']\s*>\s*(?:<\/a>\s*)?$/i,
        );
        if (anchor) {
          node.type = 'paragraph';
          node.value = undefined;
          node.data = { hName: 'span', hProperties: { id: anchor[1] } };
          node.children = [];
        }
        return;
      }
      if (
        ['code', 'inlineCode', 'link', 'image', 'html'].includes(node.type) ||
        !node.children
      ) {
        return;
      }
      node.children = node.children.flatMap((child) => {
        if (child.type === 'text' && child.value?.includes('[[')) {
          return wikiNodes(child.value);
        }
        walk(child);
        return [child];
      });
    }
    walk(tree);
  };
}
