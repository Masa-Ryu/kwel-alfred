#!/usr/bin/env node
// Real Chrome + production Kwel code + source-built Rust Host. The temporary
// unpacked manifest pregrants nativeMessaging; permission prompts/OS installers
// are separate manual evidence. Production manifests and personal profiles are
// never changed. This script does not generate or answer Native messages.
import assert from 'node:assert/strict';
import { spawn, execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import { watch } from 'node:fs';
import { appendFile, cp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { CDP } from './test-chrome-e2e.mjs';

const execute = promisify(execFile);
const flags = process.argv.slice(2);
const option = name => { const index = flags.indexOf(name); assert.ok(index >= 0 && flags[index + 1], `Missing ${name}`); return path.resolve(flags[index + 1]); };
const verifier = fileURLToPath(new URL('./test-chrome-e2e.mjs', import.meta.url));
const distribution = JSON.parse(await readFile(new URL('../config/distribution.json', import.meta.url)));
const identity = JSON.parse(await readFile(new URL('../config/chrome-e2e-extension.json', import.meta.url)));
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const extensionId = Array.from(createHash('sha256').update(Buffer.from(identity.publicKey, 'base64')).digest().subarray(0, 16)).flatMap(byte => [byte >> 4, byte & 15]).map(value => String.fromCharCode(97 + value)).join('');
assert.equal(extensionId, distribution.chromeExtensionId, 'E2E public key does not match production Host origin.');
assert.equal(extensionId, identity.extensionId);
let phase = 'preflight';
function stage(value) { phase = value; console.log(`Native Chrome phase: ${value}`); }

async function treeHash(directory) {
  const digest = createHash('sha256');
  async function visit(current, prefix) {
    for (const entry of (await readdir(current, { withFileTypes: true })).sort((a, b) => a.name.localeCompare(b.name))) {
      const relative = prefix + entry.name;
      assert.ok(!entry.isSymbolicLink(), 'E2E bundle cannot contain symlinks.');
      if (entry.isDirectory()) await visit(path.join(current, entry.name), `${relative}/`);
      else { digest.update(relative); digest.update('\0'); digest.update(await readFile(path.join(current, entry.name))); }
    }
  }
  await visit(directory, ''); return digest.digest('hex');
}

function activePort(directory, process_) {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => finish(new Error('Test Chrome did not expose its loopback debugger.')), 20_000);
    const watcher = watch(directory, () => { void check(); });
    const failed = () => finish(new Error('Test Chrome exited before debugger startup.'));
    process_.once('exit', failed);
    process_.once('error', failed);
    let done = false;
    function finish(error, port) {
      if (done) return; done = true; clearTimeout(timeout); watcher.close(); process_.off('exit', failed); process_.off('error', failed);
      if (error) reject(error); else resolve(`http://127.0.0.1:${port}`);
    }
    async function check() {
      try {
        const text = await readFile(path.join(directory, 'DevToolsActivePort'), 'utf8');
        const port = Number(text.split('\n')[0]);
        if (port > 0 && port <= 65535) finish(null, port);
      } catch (error) { if (error.code !== 'ENOENT') finish(new Error('Cannot read test Chrome debugger state.')); }
    }
    void check();
  });
}

async function uiButton(browser, session, selector) {
  const rectangle = await browser.evaluate(`new Promise((resolve,reject)=>{
    const observer=new MutationObserver(check),timer=setTimeout(()=>{observer.disconnect();reject(new Error('Kwel control not ready'));},30000);
    function check(){const element=document.querySelector(${JSON.stringify(selector)});if(element&&!element.disabled){element.scrollIntoView({block:'center',behavior:'instant'});const r=element.getBoundingClientRect(),hit=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2);if(r.width&&r.height&&(hit===element||element.contains(hit))){clearTimeout(timer);observer.disconnect();resolve({x:r.x+r.width/2,y:r.y+r.height/2});}}}
    observer.observe(document.documentElement,{childList:true,subtree:true,attributes:true});check();
  })`, session);
  await browser.call('Input.dispatchMouseEvent', { type: 'mousePressed', button: 'left', clickCount: 1, ...rectangle }, session);
  await browser.call('Input.dispatchMouseEvent', { type: 'mouseReleased', button: 'left', clickCount: 1, ...rectangle }, session);
}

async function main() {
  assert.ok(process.platform !== 'win32', 'Use the installed-Host manual verifier for Windows; this source-build launcher uses Unix manifest registration.');
  const chrome = option('--chrome'), kwel = option('--kwel-dist'), host = option('--host'), cli = option('--cli'), output = option('--out');
  const prepared = JSON.parse((await execute(process.execPath, [verifier, 'prepare'], { timeout: 5000 })).stdout);
  const root = path.dirname(prepared.plan), bundle = path.join(root, 'extension'), browsers = [], processes = [];
  const internalOutput = path.join(root, 'transport-checks.json');
  let evidence;
  const environment = { ...process.env };
  if (process.platform === 'linux') { environment.XDG_RUNTIME_DIR = path.join(root, 'runtime'); await mkdir(environment.XDG_RUNTIME_DIR, { mode: 0o700 }); }
  const buildHash = await treeHash(kwel);
  try {
    stage('prepare production bundle');
    await cp(kwel, bundle, { recursive: true, dereference: false });
    const manifest = JSON.parse(await readFile(path.join(bundle, 'manifest.json'), 'utf8'));
    assert.ok(manifest.name === 'Kwel' && !manifest.permissions.includes('nativeMessaging') && manifest.optional_permissions.includes('nativeMessaging'), 'Use an unchanged production Kwel build with optional Native Messaging.');
    manifest.key = identity.publicKey;
    manifest.permissions.push('nativeMessaging');
    manifest.optional_permissions = manifest.optional_permissions.filter(value => value !== 'nativeMessaging');
    await writeFile(path.join(bundle, 'manifest.json'), JSON.stringify(manifest));
    for (const role of ['A', 'B']) {
      stage(`launch Chrome ${role}`);
      const directory = prepared[role];
      const registered = path.join(directory, 'NativeMessagingHosts'); await mkdir(registered, { mode: 0o700 });
      await writeFile(path.join(registered, 'com.kwel.bridge.json'), JSON.stringify({ name: 'com.kwel.bridge', description: 'Kwel Local Connection', path: host, type: 'stdio', allowed_origins: [`chrome-extension://${extensionId}/`] }), { mode: 0o600 });
      const arguments_ = ['--headless=new', `--user-data-dir=${directory}`, '--remote-debugging-address=127.0.0.1', '--remote-debugging-port=0', '--enable-automation', '--enable-unsafe-extension-debugging', '--no-first-run', '--no-default-browser-check'];
      if (process.platform === 'darwin') arguments_.push('--use-mock-keychain');
      arguments_.push('about:blank');
      const child = spawn(chrome, arguments_, { env: environment, stdio: 'ignore' }); processes.push(child);
      const endpoint = await activePort(directory, child);
      const browser = await CDP.connect(endpoint); browsers.push(browser); browser.endpoint = endpoint;
      stage(`load Kwel ${role}`);
      const loaded = await browser.call('Extensions.loadUnpacked', { path: bundle });
      assert.equal(loaded.id, extensionId, 'Loaded extension identity differs from the published origin.');
      const target = await browser.call('Target.createTarget', { url: 'about:blank' });
      const session = (await browser.call('Target.attachToTarget', { targetId: target.targetId, flatten: true })).sessionId;
      await browser.call('Page.enable', {}, session);
      const pageLoaded = browser.event(value => value.method === 'Page.loadEventFired' && value.sessionId === session);
      await browser.call('Page.navigate', { url: `chrome-extension://${extensionId}/dashboard.html` }, session);
      await pageLoaded;
      const settings = await browser.evaluate(`new Promise((resolve,reject)=>{
        const observer=new MutationObserver(check),timer=setTimeout(()=>{observer.disconnect();reject(new Error('Kwel navigation not ready'));},30000);
        function check(){const button=[...document.querySelectorAll('button')].find(value=>value.textContent.trim()==='Settings'||value.getAttribute('aria-label')==='Settings');if(button){button.setAttribute('data-native-e2e-settings','true');clearTimeout(timer);observer.disconnect();resolve(true);}}
        observer.observe(document.documentElement,{childList:true,subtree:true});check();
      })`, session);
      assert.ok(settings, 'Kwel Settings navigation was not rendered.');
      stage(`enable Local Connection ${role}`);
      await uiButton(browser, session, '[data-native-e2e-settings="true"]');
      await uiButton(browser, session, 'button.local-connection-switch[aria-checked="false"]');
      stage(`await hello/hello-ack ${role}`);
      assert.ok(await browser.evaluate(`new Promise((resolve,reject)=>{
        const timer=setTimeout(()=>{chrome.storage.onChanged.removeListener(check);reject(new Error('Native handshake did not finish'));},30000);
        async function check(){const s=await chrome.storage.local.get(['kwel.local.status','kwel.local.enabled','kwel.local.consentVersion']);if(s['kwel.local.status']==='connected'&&s['kwel.local.enabled']===true&&s['kwel.local.consentVersion']===1){clearTimeout(timer);chrome.storage.onChanged.removeListener(check);resolve(true);}}
        chrome.storage.onChanged.addListener(check);void check();
      })`, session), 'Kwel did not establish hello/hello-ack.');
    }
    stage('search / ranking / openRef / profile isolation');
    try {
      await execute(process.execPath, [verifier, '--plan', prepared.plan, '--a', browsers[0].endpoint, '--b', browsers[1].endpoint, '--cli', cli, '--out', internalOutput], { env: environment, timeout: 180_000, maxBuffer: 1_000_000 });
    } catch (error) {
      // The verifier emits only our own static stage/assertion messages, never
      // a raw Chrome/Native response, query, URL or profile ID.
      for (const line of error.stderr?.split('\n') || []) {
        if (line.length < 600 && (line.startsWith('Chrome E2E failed at ') || line.startsWith('Fixture comparison: '))) console.error(line);
      }
      throw new Error('Real transport verifier failed.');
    }
    evidence = JSON.parse(await readFile(internalOutput, 'utf8'));
    const rustRoot = fileURLToPath(new URL('..', import.meta.url)), kwelRoot = path.dirname(kwel);
    const gitValue = async (directory, arguments_) => (await execute('git', arguments_, { cwd: directory, timeout: 5000 })).stdout.trim();
    const sourceDirty = Boolean(await gitValue(rustRoot, ['status', '--porcelain', '--untracked-files=no']) || await gitValue(kwelRoot, ['status', '--porcelain', '--untracked-files=no']));
    if (process.env.GITHUB_ACTIONS === 'true') assert.ok(!sourceDirty, 'Real pair CI requires clean tracked source.');
    Object.assign(evidence, { kind: 'real Chrome / production extension code / source-built Rust Host', architecture: process.arch, rustCommit: await gitValue(rustRoot, ['rev-parse', 'HEAD']), kwelCommit: await gitValue(kwelRoot, ['rev-parse', 'HEAD']), sourceDirty, kwelBuildSha256: buildHash, nativeHostSha256: hash(await readFile(host)), jsonCliSha256: hash(await readFile(cli)), permissionFixture: 'nativeMessaging pregranted only in the temporary unpacked manifest; Local Connection enabled through its real UI', osInstaller: 'not exercised by this source-built-host test' });
  } finally {
    for (const browser of browsers) { await browser.call('Browser.close').catch(() => {}); browser.close(); }
    for (const child of processes) {
      if (child.exitCode !== null || child.signalCode !== null) continue;
      await new Promise(resolve => { const timer = setTimeout(() => { child.kill('SIGKILL'); }, 5000); child.once('exit', () => { clearTimeout(timer); resolve(); }); child.kill('SIGTERM'); });
    }
    await rm(root, { recursive: true, force: true });
  }
  await writeFile(output, JSON.stringify(evidence, null, 2) + '\n', { mode: 0o600, flag: 'wx' });
  console.log(JSON.stringify(evidence));
  if (process.env.GITHUB_STEP_SUMMARY) await appendFile(process.env.GITHUB_STEP_SUMMARY, `## Real Rust Host / Kwel / Chrome\n\n\`\`\`json\n${JSON.stringify(evidence, null, 2)}\n\`\`\`\n`);
}
main().catch(error => { console.error(`Native Chrome failed at ${phase}: ${error instanceof assert.AssertionError ? error.message : 'private browser responses were not logged.'}`); process.exitCode = 1; });
