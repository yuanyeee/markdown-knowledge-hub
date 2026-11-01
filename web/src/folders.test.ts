import { describe,it,expect } from 'vitest';
import { displayPath,managedPath } from './folders';
describe('folder previews',()=>{
  it('shows Windows drive roots and extended paths without changing the managed child',()=>{
    expect(managedPath('D:\\','demo')).toBe('D:\\MarkdownKnowledgeHub\\demo');
    expect(managedPath('\\\\?\\D:\\Outputs\\','demo')).toBe('D:\\Outputs\\MarkdownKnowledgeHub\\demo');
    expect(displayPath('\\\\?\\UNC\\server\\share\\out')).toBe('\\\\server\\share\\out');
  });
  it('keeps POSIX roots and ordinary paths readable',()=>{
    expect(managedPath('/','demo')).toBe('/MarkdownKnowledgeHub/demo');
    expect(managedPath('/tmp/output/','demo')).toBe('/tmp/output/MarkdownKnowledgeHub/demo');
    expect(displayPath('/tmp/input')).toBe('/tmp/input');
  });
});
