export function displayPath(value:string):string {
  if(value.startsWith('\\\\?\\UNC\\'))return '\\\\'+value.slice(8);
  return value.startsWith('\\\\?\\')?value.slice(4):value;
}
export function managedPath(parent:string,id:string):string {
  const path=displayPath(parent.trim());
  const separator=path.includes('\\')||/^[a-z]:/i.test(path)?'\\':'/';
  return `${path.replace(/[\\/]+$/,'')}${separator}MarkdownKnowledgeHub${separator}${id}`;
}
