// Real production UI with a synthetic API and a deliberately delayed settings
// request. No real accounts, external requests, or disabled browser sandbox.
import { chromium } from 'playwright';
import { createServer } from 'node:http';
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { selectPersistedLocale } from './e2e-settings.mjs';
const root = fileURLToPath(new URL('..', import.meta.url));
const en = JSON.parse(await fs.readFile(path.join(root, 'dashboard/src/locales/en.json'), 'utf8'));
const state = { settings:{locale:'en',theme:'light'},providers:[],sites:[],accounts:[],jobs:[],approvals:[],engine:{available:false},version:'test' };
let writes = 0, rejectSave = false;
const server = createServer(async (req, res) => {
  if (req.url === '/api/events') { res.writeHead(200, {'Content-Type':'text/event-stream'}); res.end(': ready\n\n'); return; }
  if (req.url.startsWith('/api/')) {
    let body = ''; for await (const chunk of req) body += chunk;
    let result = {};
    if (req.url.startsWith('/api/session')) result = {csrf:'synthetic'};
    else if (req.url === '/api/state') result = state;
    else if (req.url === '/api/settings' && req.method === 'PATCH') {
      if (rejectSave) { res.writeHead(500, {'Content-Type':'application/json'}); res.end(JSON.stringify({error:{code:'internal'}})); return; }
      writes++; Object.assign(state.settings, JSON.parse(body)); result = state.settings;
    }
    res.writeHead(200, {'Content-Type':'application/json'}); res.end(JSON.stringify(result)); return;
  }
  const name = path.join(root,'web',req.url === '/' ? 'index.html' : req.url.split('?')[0]);
  try { const bytes = await fs.readFile(name); res.writeHead(200, {'Content-Type':name.endsWith('.js')?'text/javascript':name.endsWith('.css')?'text/css':'text/html'}); res.end(bytes); }
  catch { res.writeHead(404); res.end(); }
});
await new Promise(resolve => server.listen(0,'127.0.0.1',resolve));
const origin = `http://127.0.0.1:${server.address().port}`;
let browser, context;
try {
  browser = process.env.TEST_CDP ? await chromium.connectOverCDP(process.env.TEST_CDP) : await chromium.launch({headless:true, chromiumSandbox:true, executablePath:process.env.BROWSER_BIN});
  const open = async () => {
    const context = await browser.newContext();
    await context.route('**/*', route => new URL(route.request().url()).origin === origin ? route.continue() : route.abort());
    const page = await context.newPage(); page.setDefaultTimeout(10000);
    await page.goto(origin); await page.locator('.topbar select').waitFor();
    return {context,page,select:page.locator('.topbar select')};
  };
  let page, select;
  ({context,page,select} = await open());
  await selectPersistedLocale(page,select,'ar');
  assert.equal(state.settings.locale,'ar');
  let intercepted;
  const requested = new Promise(resolve => { intercepted = resolve; });
  await page.route('**/api/settings', async route => {
    intercepted();
    await new Promise(resolve => setTimeout(resolve,300));
    await route.continue().catch(() => {}); // Closing a context aborts its in-flight request.
  });
  const restoring = selectPersistedLocale(page,select,'en');
  await requested;
  assert.equal(state.settings.locale,'ar','The controlled request must still be pending');
  await restoring;
  await context.close();
  assert.equal(state.settings.locale,'en','Closing a suite must not abandon the locale restore');
  ({context,page,select} = await open());
  await page.getByRole('heading',{name:en['sites.title'],exact:true}).waitFor();
  const before = writes;
  await selectPersistedLocale(page,select,'en');
  assert.equal(writes,before,'An already-persisted locale needs no mutation');
  rejectSave = true;
  await assert.rejects(selectPersistedLocale(page,select,'ar'), /Locale update must succeed/);
  assert.equal(state.settings.locale,'en','Failed settings requests must not be treated as persisted');
  await context.close();
  console.log('PASS: delayed locale restore survives context teardown; fresh context is English; no-op and failed-save checks pass.');
} finally { await context?.close(); if (!process.env.TEST_CDP) await browser?.close(); server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
if (process.env.TEST_CDP) process.exit(0);
