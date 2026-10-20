// Production dashboard against an isolated synthetic API, never a live service.
// Build first, then: BROWSER_BIN=/path/to/chromium node dashboard/result-recovery-e2e.mjs
// No uploads, credentials, external requests, or disabled browser sandbox.
import { chromium } from 'playwright';
import { createServer } from 'node:http';
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
const root = fileURLToPath(new URL('..', import.meta.url));
const en = JSON.parse(await fs.readFile(path.join(root, 'dashboard/src/locales/en.json'), 'utf8'));
const now = () => new Date().toISOString();
const job = (id, status, extra = {}) => ({id:id.replaceAll(' ', '-'), kind:'action', status, site_id:'site', site_title:'Example', action_title:id, started_at:now(), updated_at:now(), finished_at:now(), found:[], notes:[], ...extra});
const state = { settings:{locale:'en', theme:'light', default_provider:'fixture'}, providers:[{id:'fixture',name:'Fixture AI',connected:true}], sites:[{id:'site',title:'Example',url:'https://example.test',host:'example.test',status:'ready',actions:[],updated_at:''}], accounts:[], jobs:[], approvals:[], engine:{available:true,compatible:true,version:'1'}, version:'test' };
let summaryCount = 0, hideNewest = false, refreshFails = false, holdSummary = false, releaseSummary;
const requests = [], streams = new Set();
const changed = () => { for (const stream of streams) stream.write('data: changed\n\n'); };
const server = createServer(async (req, res) => {
  if (req.url === '/api/events') { res.writeHead(200, {'Content-Type':'text/event-stream'}); res.write(': ready\n\n'); streams.add(res); req.on('close', () => streams.delete(res)); return; }
  if (req.url.startsWith('/api/')) {
    let body = ''; for await (const chunk of req) body += chunk;
    const input = body ? JSON.parse(body) : {};
    requests.push({path:req.url,method:req.method,input});
    let result;
    if (req.url.startsWith('/api/session')) result = {csrf:'synthetic'};
    else if (req.url === '/api/state') {
      if (refreshFails) { res.writeHead(503, {'Content-Type':'application/json'}); res.end(JSON.stringify({error:{code:'connection_lost'}})); return; }
      result = {...state, jobs:state.jobs.filter(j => !hideNewest || j.id !== 'summary-'+summaryCount).map(({result, ...summary}) => summary)};
    } else if (req.url.startsWith('/api/jobs/') && req.method === 'GET') result = state.jobs.find(j => j.id === req.url.split('/')[3]);
    else if (req.url.endsWith('/summarize') || req.url.endsWith('/retry')) {
      const previous = state.jobs.find(j => j.id === req.url.split('/')[3]);
      assert.ok(previous, 'Exact target exists');
      if (req.url.endsWith('/retry')) assert.equal(previous.status,'failed');
      const child = job('summary-'+(++summaryCount), 'running', {kind:'summary', parent_id:previous.kind === 'summary' ? previous.parent_id : previous.id, provider:previous.kind === 'summary' ? previous.provider : input.provider, phase:'summarize'});
      state.jobs.unshift(child);
      if (holdSummary) await new Promise(resolve => { releaseSummary = resolve; });
      else await new Promise(r => setTimeout(r,150));
      result = {job:child};
    } else { res.writeHead(404); res.end(); return; }
    res.writeHead(200, {'Content-Type':'application/json'}); res.end(JSON.stringify(result)); return;
  }
  const filename = path.join(root, 'web', req.url === '/' ? 'index.html' : decodeURIComponent(req.url.split('?')[0]));
  try { const bytes = await fs.readFile(filename); res.writeHead(200, {'Content-Type':filename.endsWith('.js')?'text/javascript':filename.endsWith('.css')?'text/css':filename.endsWith('.png')?'image/png':'text/html'}); res.end(bytes); } catch { res.writeHead(404); res.end(); }
});
await new Promise(r => server.listen(0, '127.0.0.1', r));
let browser;
try {
  browser = await chromium.launch({headless:true, chromiumSandbox:true, executablePath:process.env.BROWSER_BIN});
  const context = await browser.newContext();
  const origin = `http://127.0.0.1:${server.address().port}`;
  await context.route('**/*', route => new URL(route.request().url()).origin === origin ? route.continue() : route.abort());
  const page = await context.newPage(); page.setDefaultTimeout(10000);
  const errors = []; page.on('pageerror', e => errors.push(e.message));
  const button = (scope,key) => scope.getByRole('button',{name:en[key],exact:true});
  const dialog = () => page.getByRole('dialog');
  const visit = async () => { await page.goto(origin+'/#activity'); await page.reload({waitUntil:'domcontentloaded'}); };
  const open = async title => { await button(page.locator('.activity-item').filter({hasText:title}),'activity.viewResult').click(); };
  const close = async () => button(dialog(),'common.close').click();
  state.jobs = ['failed','cancelled','completed'].map(status => job('Empty '+status,status,status === 'failed' ? {error_code:'auth_required'} : {}));
  await visit();
  for (const status of ['failed','cancelled','completed']) {
    await open('Empty '+status);
    await dialog().locator('.result-heading').getByText(en['activity.'+status],{exact:true}).waitFor();
    if (status === 'completed') await dialog().getByText(en['actions.noResult'],{exact:true}).waitFor();
    else assert.equal(await dialog().getByText(en['actions.noResult'],{exact:true}).count(),0);
    if (status === 'failed') { await dialog().getByRole('alert').getByText(en['errors.auth_required'],{exact:true}).waitFor(); assert.equal(await button(dialog(),'accounts.autoFind').isVisible(),true); }
    await close();
  }
  console.log('PASS: empty-success text only on successful completion; failure remedy unchanged');
  const original = job('Read original','completed',{result:{answer:'Original read retained'}});
  state.jobs = [original,job('Other parent','completed',{result:{answer:'Independent result'}}),job('unrelated-summary','failed',{kind:'summary',parent_id:'different-parent',error_code:'ai_billing_error'})];
  await visit();
  const openRead = async () => { await open('Read original'); await dialog().getByText('Original read retained',{exact:true}).waitFor(); };
  await openRead();
  assert.equal(await dialog().getByText(en['errors.ai_billing_error'],{exact:true}).count(),0);
  // An accepted response must keep the button guarded even before state catches up.
  hideNewest = true; holdSummary = true;
  await button(dialog(),'actions.summary').dblclick();
  const submissionDeadline = Date.now()+10000;
  while (!releaseSummary && Date.now()<submissionDeadline) await new Promise(r => setTimeout(r,10));
  assert.ok(releaseSummary,'Synthetic server accepted the pending POST');
  await close(); await openRead();
  assert.equal(await button(dialog(),'actions.summary').isEnabled(),false,'Pending POST stays guarded after remount');
  assert.equal(summaryCount,1);
  await close(); await open('Other parent');
  await dialog().getByText('Independent result',{exact:true}).waitFor();
  assert.equal(await button(dialog(),'actions.summary').isEnabled(),true,'A different parent is not blocked');
  await close(); await openRead();
  holdSummary = false; releaseSummary();
  await dialog().locator('.summary-operation').getByRole('progressbar').waitFor();
  assert.equal(summaryCount,1);
  assert.equal(await button(dialog(),'actions.summary').isEnabled(),false);
  hideNewest = false; changed();
  await close(); await openRead();
  await dialog().locator('.summary-operation').getByRole('progressbar').waitFor();
  const first = state.jobs.find(j => j.id === 'summary-1'); Object.assign(first,{status:'failed',error_code:'ai_unavailable'}); changed();
  await dialog().locator('.summary-operation').getByText(en['errors.ai_unavailable'],{exact:true}).waitFor();
  assert.equal(summaryCount,1,'No automatic retry');
  assert.equal(await dialog().getByText('Original read retained',{exact:true}).isVisible(),true);
  await close(); await openRead();
  await dialog().locator('.summary-operation').getByText(en['errors.no_summary'],{exact:true}).waitFor();
  state.settings.default_provider = 'different-default'; changed();
  await button(dialog().locator('.summary-operation'),'common.retry').dblclick();
  await dialog().locator('.summary-operation').getByRole('progressbar').waitFor();
  assert.equal(summaryCount,2);
  assert.equal(requests.filter(r => r.method === 'POST' && r.path === '/api/jobs/summary-1/retry').length,1);
  assert.equal(state.jobs.find(j => j.id === 'summary-2').parent_id,original.id);
  assert.equal(state.jobs.find(j => j.id === 'summary-2').provider,'fixture');
  Object.assign(state.jobs.find(j => j.id === 'summary-2'),{status:'completed'}); original.summary = 'A helpful explanation.'; changed();
  await dialog().getByText(original.summary,{exact:true}).waitFor();
  assert.equal(await dialog().locator('.summary-operation').count(),0);
  assert.equal(await dialog().getByText('Original read retained',{exact:true}).isVisible(),true);
  await close(); await openRead(); await dialog().getByText(original.summary,{exact:true}).waitFor();
  assert.equal(summaryCount,2);
  console.log('PASS: exact failure/retry/reopen, original result preserved, unrelated child ignored');
  // Cancellation uses summarize, since the existing retry endpoint only accepts failed jobs.
  await button(dialog(),'actions.summary').click();
  await dialog().locator('.summary-operation').getByRole('progressbar').waitFor();
  state.jobs.find(j => j.id === 'summary-3').status = 'cancelled'; changed();
  await dialog().locator('.summary-operation').getByText(en['activity.cancelled'],{exact:true}).waitFor();
  refreshFails = true;
  const failedRefresh = page.waitForResponse(response => new URL(response.url()).pathname === '/api/state' && response.status() === 503);
  await button(dialog().locator('.summary-operation'),'common.retry').dblclick();
  await failedRefresh;
  await page.locator('.main-content > .notice').getByText(en['activity.connectionLost'],{exact:true}).waitFor();
  assert.equal(await dialog().getByRole('alert').count(),0,'Accepted summary must not report a command failure when only snapshot synchronization fails');
  assert.equal(summaryCount,4,'Refresh failure never repeats accepted operation');
  assert.equal(requests.filter(r => r.path === '/api/jobs/summary-3/retry').length,0);
  await dialog().locator('.summary-operation').getByRole('progressbar').waitFor();
  assert.equal(await button(dialog(),'actions.summary').isEnabled(),false);
  await close(); await openRead();
  await dialog().locator('.summary-operation').getByRole('progressbar').waitFor();
  assert.equal(await button(dialog(),'actions.summary').isEnabled(),false,'Accepted operation stays guarded after failed-refresh remount');
  assert.equal(summaryCount,4);
  state.jobs.find(j => j.id === 'summary-4').status = 'completed';
  await close(); await openRead();
  assert.equal(await button(dialog(),'actions.summary').isEnabled(),false,'Even completed server work cannot be repeated through stale snapshot');
  assert.equal(summaryCount,4);
  refreshFails = false; changed();
  await dialog().locator('.summary-operation').waitFor({state:'hidden'});
  assert.equal(await page.getByText(en['activity.connectionLost'],{exact:true}).count(),0,'Successful reconciliation clears the separate stale notice');
  assert.equal(await dialog().getByRole('alert').count(),0,'Reconciliation leaves no false command error');
  // A later Activity retry is authoritative once the exact accepted child was observed.
  await button(dialog(),'actions.summary').click();
  await dialog().locator('.summary-operation').getByRole('progressbar').waitFor();
  Object.assign(state.jobs.find(j => j.id === 'summary-5'),{status:'failed',error_code:'ai_unavailable'}); changed();
  await dialog().locator('.summary-operation').getByText(en['errors.ai_unavailable'],{exact:true}).waitFor();
  await close();
  const activityRetry = page.waitForResponse(response => new URL(response.url()).pathname === '/api/jobs/summary-5/retry');
  await button(page.locator('.activity-item').filter({has:page.getByRole('heading',{name:'summary-5',exact:true})}),'common.retry').click();
  assert.equal((await activityRetry).status(),200);
  await page.getByRole('heading',{name:'summary-6',exact:true}).waitFor();
  await openRead();
  await dialog().locator('.summary-operation').getByRole('progressbar').waitFor();
  assert.equal(await dialog().getByText(en['errors.ai_unavailable'],{exact:true}).count(),0,'Activity retry supersedes the previously accepted failure');
  assert.equal(state.jobs.find(j => j.id === 'summary-6').parent_id,original.id);
  state.jobs.find(j => j.id === 'summary-6').status = 'completed'; original.summary = 'Explanation from Activity retry.'; changed();
  await dialog().getByText(original.summary,{exact:true}).waitFor();
  assert.equal(await dialog().locator('.summary-operation').count(),0,'Newer success leaves no stale failure or Retry');
  await close(); await openRead();
  await dialog().getByText(original.summary,{exact:true}).waitFor();
  assert.equal(await dialog().locator('.summary-operation').count(),0);
  assert.equal(summaryCount,6);
  console.log('PASS: Activity retry supersedes observed accepted child and clears old failure');
  assert.deepEqual(errors,[]);
  console.log('PASS: cancelled summary explicit retry and accepted-write/failed-refresh guard');
  console.log('RESULT: synthetic production-dashboard regression passed; not live-service verification');
} finally { if (browser) await browser.close(); server.closeAllConnections(); await new Promise(r => server.close(r)); }
