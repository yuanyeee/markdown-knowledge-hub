import { describe, expect, it } from 'vitest';
import { displayPath, managedPath, validateProjectId } from './folders';

describe('folder previews', () => {
  it('shows Windows drive roots and extended paths without changing the managed child', () => {
    expect(managedPath('D:\\', 'demo')).toBe('D:\\MarkdownKnowledgeHub\\demo');
    expect(managedPath('\\\\?\\D:\\Outputs\\', 'demo')).toBe(
      'D:\\Outputs\\MarkdownKnowledgeHub\\demo',
    );
    expect(displayPath('\\\\?\\UNC\\server\\share\\out')).toBe(
      '\\\\server\\share\\out',
    );
  });

  it('keeps POSIX roots and ordinary paths readable', () => {
    expect(managedPath('/', 'demo')).toBe('/MarkdownKnowledgeHub/demo');
    expect(managedPath('/tmp/output/', 'demo')).toBe(
      '/tmp/output/MarkdownKnowledgeHub/demo',
    );
    expect(displayPath('/tmp/input')).toBe('/tmp/input');
  });

  it('validates custom project IDs against invalid characters and Windows reserved names', () => {
    expect(validateProjectId('sales-spec_01')).toBeNull();
    expect(validateProjectId('InvalidUpper')).not.toBeNull();
    expect(validateProjectId('con')).not.toBeNull();
    expect(validateProjectId('com1')).not.toBeNull();
    expect(validateProjectId('../escape')).not.toBeNull();
  });
});
