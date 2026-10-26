#!/usr/bin/env node
// Real Chrome + installed Host only. Never sends synthetic Native frames,
// grants permissions, changes consent, or connects to personal Chrome profiles.
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { execFile } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';

const execute = promisify(execFile);
const extension = JSON.parse(readFileSync(new URL('../config/distribution.json', import.meta.url))).chromeExtensionId;
const marker = '.kwel-connect-e2e.json';
const flags = process.argv.slice(2);
function option(name) { const index = flags.indexOf(name); assert.ok(index >= 0 && flags[index + 1], `Missing ${name}`); return flags[index + 1]; }
function loopbackEndpoint(value) {
  const url = new URL(value);
  assert.ok(url.protocol === 'http:' && ['127.0.0.1', '[::1]'].includes(url.hostname) && !url.username && !url.password && url.pathname === '/', 'CDP must be a loopback test endpoint.');
  return url;
}

class CDP {
  constructor(socket) {
    this.socket = socket; this.sequence = 0; this.pending = new Map(); this.listeners = new Set();
    socket.addEventListener('message', event => {
      if (typeof event.data !== 'string' || event.data.length > 1_000_000) { socket.close(); return; }
      let value;
      try { value = JSON.parse(event.data); } catch { socket.close(); return; }
      if (value.id) {
        const request = this.pending.get(value.id);
        if (!request) return;
        this.pending.delete(value.id); clearTimeout(request.timer);
        if (value.error) request.reject(new Error('Chrome rejected an E2E command.'));
        else request.resolve(value.result);
      } else for (const listener of this.listeners) listener(value);
    });
    socket.addEventListener('close', () => {
      for (const request of this.pending.values()) { clearTimeout(request.timer); request.reject(new Error('Test Chrome disconnected.')); }
      this.pending.clear();
    });
  }
  static async connect(endpoint) {
    const url = loopbackEndpoint(endpoint);
    const response = await fetch(new URL('/json/version', url), { signal: AbortSignal.timeout(5000) });
    const version = await response.json();
    const address = new URL(version.webSocketDebuggerUrl);
    assert.ok(['127.0.0.1', '[::1]'].includes(address.hostname), 'Chrome returned a non-loopback endpoint.');
    const socket = new WebSocket(address);
    await new Promise((resolve, reject) => { socket.addEventListener('open', resolve, { once: true }); socket.addEventListener('error', () => reject(new Error('Could not connect to test Chrome.')), { once: true }); });
    return new CDP(socket);
  }
  call(method, params = {}, sessionId) {
    assert.ok(this.pending.size < 16, 'Too many E2E browser commands.');
    const id = ++this.sequence;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.pending.delete(id); reject(new Error(`Chrome E2E command timed out: ${method}`)); }, 50_000);
      this.pending.set(id, { resolve, reject, timer });
      this.socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    });
  }
  event(predicate) {
    let listener;
    const promise = new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.listeners.delete(listener); reject(new Error('Expected Chrome event did not arrive.')); }, 15_000);
      listener = value => { if (predicate(value)) { clearTimeout(timer); this.listeners.delete(listener); resolve(value); } };
      this.listeners.add(listener);
    });
    // The command following registration may fail before the event arrives.
    // Mark the promise handled while preserving rejection for its awaiter.
    promise.catch(() => {});
    return promise;
  }
  async evaluate(expression, session) {
    const value = await this.call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }, session);
    assert.ok(!value.exceptionDetails && value.result.type !== 'undefined', 'Kwel test-page evaluation failed.');
    return value.result.value;
  }
  close() { this.socket.close(); }
}

async function main() {
  if (flags[0] === 'prepare') {
    const root = mkdtempSync(path.join(tmpdir(), 'kwel-connect-e2e-'));
    const plan = { nonce: randomUUID(), profiles: {} };
    for (const role of ['A', 'B']) {
      const directory = path.join(root, role); mkdirSync(directory, { mode: 0o700 });
      writeFileSync(path.join(directory, marker), JSON.stringify({ nonce: plan.nonce, role }), { mode: 0o600 });
      plan.profiles[role] = directory;
    }
    writeFileSync(path.join(root, 'plan.json'), JSON.stringify(plan, null, 2), { mode: 0o600 });
    console.log(JSON.stringify({ plan: path.join(root, 'plan.json'), ...plan.profiles }, null, 2));
    return;
  }
  const plan = JSON.parse(readFileSync(option('--plan'), 'utf8'));
  assert.ok(/^[a-f0-9-]{36}$/iu.test(plan.nonce), 'Invalid test plan.');
  const cli = path.resolve(option('--cli'));
  const output = path.resolve(option('--out'));
  const endpointA = option('--a'), endpointB = option('--b');
  assert.ok(endpointA !== endpointB, 'Two separate Chrome test profiles are required.');
  loopbackEndpoint(endpointA); loopbackEndpoint(endpointB);
  const browsers = [], created = [];
  const query = randomUUID().replaceAll('-', '');
  const server = createServer((request, response) => {
    if (!request.url.startsWith(`/${plan.nonce}/`)) { response.writeHead(404); response.end(); return; }
    response.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store' });
    response.end(`<title>Kwel Connect ${query}</title><p>Disposable local history fixture.</p>`);
  });
  await new Promise((resolve, reject) => {
    server.once('error', () => reject(new Error('Could not start the local E2E fixture server.')));
    server.listen(0, '127.0.0.1', resolve);
  });
  const base = `http://127.0.0.1:${server.address().port}/${plan.nonce}`;
  const evidence = { schema: 1, os: process.platform, time: new Date().toISOString(), chrome: [], profiles: 2, checks: {}, alfredUI: 'unverified', permissionDialogs: 'manual prerequisite' };
  async function command(profile, args, failureCode) {
    let result;
    try { result = await execute(cli, [...args, '--profile-id', profile, '--format', 'json'], { timeout: 20_000, maxBuffer: 1_000_000, windowsHide: true }); }
    catch (error) { assert.ok(failureCode && error.stdout, 'Installed JSON CLI failed.'); result = error; }
    const value = JSON.parse(result.stdout);
    assert.ok(!result.stderr && (failureCode ? value.code === failureCode : value.type !== 'error'), 'Unexpected JSON CLI response.');
    return value;
  }
  try {
    for (const [index, endpoint] of [endpointA, endpointB].entries()) {
      const role = ['A', 'B'][index];
      const browser = await CDP.connect(endpoint); browsers.push(browser);
      const { arguments: arguments_ } = await browser.call('Browser.getBrowserCommandLine');
      const directory = arguments_.find(value => value.startsWith('--user-data-dir='))?.slice('--user-data-dir='.length);
      assert.ok(directory && path.resolve(directory) === path.resolve(plan.profiles[role]), 'Refusing a Chrome profile outside the E2E plan.');
      const owned = JSON.parse(readFileSync(path.join(directory, marker), 'utf8'));
      assert.ok(owned.nonce === plan.nonce && owned.role === role, 'Missing E2E profile ownership marker.');
      assert.ok(arguments_.some(value => /Google Chrome.app|[\\/]google[\\/]chrome[\\/]chrome|Google[\\/]Chrome[\\/]Application[\\/]chrome.exe/iu.test(value)), 'Use Google Chrome, not a substituted browser.');
      if (process.platform === 'darwin') assert.ok(arguments_.includes('--use-mock-keychain'), 'Test Chrome must use the mock keychain.');
      evidence.chrome.push((await browser.call('Browser.getVersion')).product);
      await browser.call('Target.setDiscoverTargets', { discover: true });
      const { targetInfos } = await browser.call('Target.getTargets');
      const page = targetInfos.find(value => value.type === 'page' && value.url.startsWith(`chrome-extension://${extension}/`));
      assert.ok(page, 'Open Kwel Settings in the disposable test Chrome.');
      const session = (await browser.call('Target.attachToTarget', { targetId: page.targetId, flatten: true })).sessionId;
      browser.session = session;
      const state = await browser.evaluate(`chrome.storage.local.get(['kwel.local.profileId','kwel.local.status','kwel.local.enabled','kwel.local.consentVersion'])`, session);
      assert.ok(state['kwel.local.enabled'] === true && state['kwel.local.consentVersion'] === 1 && state['kwel.local.status'] === 'connected', 'Enable Local Connection through Kwel Settings and wait for Connected.');
      browser.profile = state['kwel.local.profileId'];
      assert.ok(typeof browser.profile === 'string' && /^[a-f0-9-]{36}$/iu.test(browser.profile), 'Kwel has no valid test profile ID.');
      const probe = await command(browser.profile, ['profile-probe']);
      assert.ok(probe.profileId === browser.profile, 'Installed Host did not answer the Chrome profile probe.');
      browser.expected = [`${base}/${role}/one`, `${base}/${role}/two`, `${base}/shared`];
    }
    assert.ok(browsers[0].profile !== browsers[1].profile, 'Duplicate profile IDs are not an isolation test.');
    evidence.checks.chromeLaunchedHostAndHelloAck = true;
    for (const browser of browsers) {
      for (const url of browser.expected) {
        const target = await browser.call('Target.createTarget', { url: 'about:blank' });
        created.push([browser, target.targetId]);
        const session = (await browser.call('Target.attachToTarget', { targetId: target.targetId, flatten: true })).sessionId;
        await browser.call('Page.enable', {}, session);
        const loaded = browser.event(value => value.method === 'Page.loadEventFired' && value.sessionId === session);
        await browser.call('Page.navigate', { url }, session); await loaded;
      }
      const pages = await browser.evaluate(`new Promise((resolve,reject)=>{
        const expected=${JSON.stringify(browser.expected)},query=${JSON.stringify(query)};
        let probing=false,dirty=false,done=false;
        const timer=setTimeout(()=>{chrome.runtime.onMessage.removeListener(changed);reject(new Error('Fixture indexing timed out'));},45000);
        async function check(){if(done)return;dirty=true;if(probing)return;probing=true;try{do{
          dirty=false;
          const response=await chrome.runtime.sendMessage({apiVersion:1,requestId:crypto.randomUUID(),method:'SEARCH_PAGES',payload:{query}});
          if(response.ok && expected.every(url=>response.data.pages.some(page=>page.url===url))){done=true;clearTimeout(timer);chrome.runtime.onMessage.removeListener(changed);resolve(response.data.pages);}
        }while(dirty&&!done);}catch{done=true;clearTimeout(timer);chrome.runtime.onMessage.removeListener(changed);reject(new Error('Kwel fixture search failed'));}finally{probing=false;}}
        function changed(value){if(value.target==='kwel-ui' && value.kind==='state-changed')void check();}
        chrome.runtime.onMessage.addListener(changed);void check();
      })`, browser.session);
      assert.ok(pages.length === browser.expected.length && pages.every(value => browser.expected.includes(value.url)), 'Chrome profile histories are mixed or the fixture is incomplete.');
      const result = await command(browser.profile, ['search', '--query', query, '--limit', '20']);
      assert.ok(JSON.stringify(result.items.map(({ title, url, domain }) => ({ title, url, domain }))) === JSON.stringify(pages.map(({ title, url, domain }) => ({ title, url, domain }))), 'CLI changed Kwel ranking or result data.');
      browser.results = result.items;
      await command(browser.profile, ['search', '--query', '', '--limit', '20']);
    }
    evidence.checks.rankingUrlsEmptySearchAndProfileIsolation = true;
    const [a, b] = browsers, selected = a.results[0];
    await command(b.profile, ['open', '--ref', selected.openRef], 'RESULT_EXPIRED');
    const beforeA = new Set((await a.call('Target.getTargets')).targetInfos.map(value => value.targetId));
    const beforeB = new Set((await b.call('Target.getTargets')).targetInfos.map(value => value.targetId));
    const opened = a.event(value => ['Target.targetCreated', 'Target.targetInfoChanged'].includes(value.method) && value.params.targetInfo.url === selected.url && !beforeA.has(value.params.targetInfo.targetId));
    await command(a.profile, ['open', '--ref', selected.openRef]);
    const target = (await opened).params.targetInfo; created.push([a, target.targetId]);
    assert.ok(!(await b.call('Target.getTargets')).targetInfos.some(value => value.url === selected.url && !beforeB.has(value.targetId)), 'openRef opened in another Chrome profile.');
    await command(a.profile, ['open', '--ref', selected.openRef], 'RESULT_EXPIRED');
    evidence.checks.openInOriginalProfileCrossProfileRejectionAndSingleUse = true;
    writeFileSync(output, JSON.stringify(evidence, null, 2) + '\n', { mode: 0o600, flag: 'wx' });
    console.log('Real Google Chrome / Kwel / installed Host E2E passed. Alfred UI and OS approval prompts require separate manual evidence.');
  } finally {
    for (const [browser, targetId] of created) await browser.call('Target.closeTarget', { targetId }).catch(() => {});
    for (const browser of browsers) browser.close();
    server.closeAllConnections(); await new Promise(resolve => server.close(resolve));
  }
}
main().catch(error => { console.error(error instanceof assert.AssertionError ? error.message : 'Chrome E2E failed; no private Chrome response was logged.'); process.exitCode = 1; });
