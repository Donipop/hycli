import fs from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
import { stripTypeScriptTypes } from 'node:module';
import { test } from 'node:test';
const app = fs.readFileSync(new URL('./src/App.tsx', import.meta.url), 'utf8');
const api = fs.readFileSync(new URL('./src/api.ts', import.meta.url), 'utf8').replaceAll('export ', '');
const coordinator = fs.readFileSync(new URL('./src/snapshot.ts', import.meta.url), 'utf8').replaceAll('export ', '');
const start = app.indexOf('  const submit =', app.indexOf('function Websites('));
const submit = app.slice(start, app.indexOf('\n  return <>', start));
for (const mode of ['failure', 'hang', 'edited']) test(`production website submit accepts receipt with ${mode} refresh`, async () => {
  let error = '', busy = false, posts = 0, fresh = true, resolvePost;
  const draft = { url: 'https://fixture.example.test', intent: 'Read report' };
  const context = { URL, URLSearchParams, Response, location: { search: '' }, provider: 'fixture', draftRevision: { current: 0 }, ...draft,
    setError: v => error = v, setBusy: v => busy = v, setURL: v => draft.url = v, setIntent: v => draft.intent = v,
    setFresh: v => fresh = v,
    fetch: async (path, options) => {
      if (path === '/api/sites/prepare') { posts++; if (mode === 'edited') await new Promise(resolve => resolvePost = resolve); return new Response(JSON.stringify({ job: { id: 'accepted' } }), { status: 200 }); }
      assert.equal(path, '/api/state');
      if (mode === 'hang') return new Promise(() => {});
      throw new Error('synthetic outage');
    },
  };
  vm.createContext(context);
  vm.runInContext(stripTypeScriptTypes(`${api}\n${coordinator}\nconst sync=snapshotCoordinator(()=>request('/api/state'),()=>{},setFresh); const ops={act:(path, method, body)=>sync.act(()=>request(path,method,body))}; ${submit}\nglobalThis.submit=submit;`, { mode: 'transform' }), context);
  const pending = context.submit({ preventDefault() {} });
  if (mode === 'edited') {
    context.draftRevision.current++;
    draft.url = 'https://edited.example.test'; draft.intent = 'New draft';
    resolvePost();
  }
  await pending;
  assert.equal(posts, 1); assert.equal(error, ''); assert.equal(busy, false); assert.equal(fresh, false);
  assert.equal(draft.url, mode === 'edited' ? 'https://edited.example.test' : '');
  assert.equal(draft.intent, mode === 'edited' ? 'New draft' : '');
});
test('successful current publication clears only Workspace sync error and retires resolved approval receipts', () => {
  const start = app.indexOf('    next => {', app.indexOf('snapshotCoordinator('));
  const end = app.indexOf('\n    },', start);
  const publish = app.slice(start, end) + '\n    }';
  let workspaceError = 'connection_lost', formError = 'ai_unavailable';
  let resolved = new Set(['still-pending', 'resolved', 'removed']);
  const next = { approvals: [{ id: 'still-pending', status: 'pending' }, { id: 'resolved', status: 'approved' }] };
  const context = { snapshotRef: { current: {} }, setData() {}, setError: value => workspaceError = value, setResolvedApprovals: update => resolved = update(resolved) };
  vm.createContext(context); vm.runInContext(`globalThis.publish = ${publish}`, context);
  context.publish(next);
  assert.equal(workspaceError, ''); assert.equal(formError, 'ai_unavailable');
  assert.deepEqual([...resolved], ['still-pending']);
});
