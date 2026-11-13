const RESERVED_WIN_NAMES = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/;

export function displayPath(value: string): string {
  if (value.startsWith('\\\\?\\UNC\\')) return '\\\\' + value.slice(8);
  return value.startsWith('\\\\?\\') ? value.slice(4) : value;
}

export function managedPath(parent: string, id: string): string {
  const path = displayPath(parent.trim());
  const separator = path.includes('\\') || /^[a-z]:/i.test(path) ? '\\' : '/';
  return `${path.replace(/[\\/]+$/, '')}${separator}MarkdownKnowledgeHub${separator}${id}`;
}

export function validateProjectId(id: string): string | null {
  if (!id || id.length > 80 || !/^[a-z0-9_-]+$/.test(id)) {
    return '識別IDは80文字以内の半角英小文字・数字・ハイフン(-)・アンダースコア(_)で指定してください。';
  }
  if (RESERVED_WIN_NAMES.test(id)) {
    return 'Windowsの予約名は識別IDに使用できません。';
  }
  return null;
}
