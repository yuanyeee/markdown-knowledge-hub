import { afterEach,expect,test,vi } from 'vitest';
vi.mock('@tauri-apps/api/core',()=>({isTauri:vi.fn(()=>false),invoke:vi.fn()}));
import { isTauri,invoke } from '@tauri-apps/api/core';
import { read,write } from './transport';
afterEach(()=>{vi.restoreAllMocks();vi.mocked(isTauri).mockReturnValue(false);});
test('Web reads encode the query and management posts keep their protection header',async()=>{
  const fetcher=vi.spyOn(globalThis,'fetch').mockResolvedValue({ok:true,json:async()=>[]} as Response);
  await read('search','demo',{q:'注文'});
  expect(String(fetcher.mock.calls[0][0])).toContain('%E6%B3%A8%E6%96%87');
  await write('remove',{project:'demo'});
  expect(fetcher.mock.calls[1][1]).toMatchObject({method:'POST',headers:{'X-Hub-Admin':'1'}});
});
test('Desktop uses Core IPC without starting or contacting an HTTP server',async()=>{
  vi.mocked(isTauri).mockReturnValue(true);
  vi.mocked(invoke).mockResolvedValue([{id:'demo'}]);
  const fetcher=vi.spyOn(globalThis,'fetch');
  expect(await read('projects')).toEqual([{id:'demo'}]);
  expect(invoke).toHaveBeenCalledWith('hub_request',{operation:'projects',args:{project:''}});
  await write('rebuild',{project:'demo'});
  expect(invoke).toHaveBeenLastCalledWith('hub_request',{operation:'admin/rebuild',args:{project:'demo'}});
  expect(fetcher).not.toHaveBeenCalled();
});
test('local output settings reads require the explicit management header',async()=>{
  const fetcher=vi.spyOn(globalThis,'fetch').mockResolvedValue({ok:true,json:async()=>({last_output:null})} as Response);
  await read('output-settings');
  expect(fetcher).toHaveBeenLastCalledWith('/api/output-settings?project=',{headers:{'X-Hub-Admin':'1'}});
  await read('retired-outputs');
  expect(fetcher).toHaveBeenLastCalledWith('/api/retired-outputs?project=',{headers:{'X-Hub-Admin':'1'}});
});
