#!/usr/bin/env node
// Execute current Kwel's actual parser against its immutable public Rust pin.
const assert = require('node:assert/strict');
const { createHash } = require('node:crypto');
const { execFileSync, spawnSync } = require('node:child_process');
const { readFileSync } = require('node:fs');
const { createRequire } = require('node:module');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const source = path.resolve(process.argv[2] || 'counterpart');
const fixtureBytes = readFileSync(path.join(root, 'crates/kwel-protocol/tests/fixtures/native-messaging-v1.json'));
const fixture = JSON.parse(fixtureBytes);
const provenance = JSON.parse(readFileSync(path.join(root, 'crates/kwel-protocol/tests/fixtures/native-messaging-v1.source.json')));
const hash = value => createHash('sha256').update(value).digest('hex');
assert.equal(hash(fixtureBytes), provenance.fixtureSha256);
{
  // In the private Kwel CI, the current PR is tested against its immutable
  // public Rust source pin. This changes the provenance direction, not checks.
  const pin = JSON.parse(readFileSync(path.join(source, 'tests/fixtures/local-connection/native-messaging-v1.source.json')));
  assert.equal(pin.repository, 'Masa-Ryu/kwel-connect');
  const head = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim();
  const review = process.argv.includes('--review-current');
  assert.match(pin.commit, /^[a-f0-9]{40}$/);
  if (review) {
    assert.notEqual(process.env.GITHUB_ACTIONS, 'true', 'CI must test its exact immutable pair pin.');
    execFileSync('git', ['merge-base', '--is-ancestor', pin.commit, head], { cwd: root });
    console.log(`Local review: current Rust ${head}; Kwel canonical pin ${pin.commit}. This does not update the Kwel CI pin.`);
  } else assert.equal(head, pin.commit);
  assert.equal(pin.fixtureSha256, hash(fixtureBytes));
  for (const file of pin.sources) {
    assert.ok(['crates/kwel-protocol/tests/fixtures/native-messaging-v1.json', 'docs/native-messaging-v1.md', 'docs/schemas/native-messaging-v1.schema.json'].includes(file.path));
    const bytes = review ? execFileSync('git', ['show', `${pin.commit}:${file.path}`], { cwd: root }) : readFileSync(path.join(root, file.path));
    assert.equal(hash(bytes), file.sha256);
  }
}
assert.equal(hash(readFileSync(path.join(source, 'tests/fixtures/local-connection/native-messaging-v1.json'))), provenance.fixtureSha256);
const requireKwel = createRequire(path.join(source, 'package.json'));
const ts = requireKwel('typescript');
const previous = require.extensions['.ts'];
require.extensions['.ts'] = (module, filename) => {
  const code = ts.transpileModule(readFileSync(filename, 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true },
    fileName: filename,
  });
  module._compile(code.outputText, filename);
};
const kwel = requireKwel(path.join(source, 'extension/src/contracts/local-connection/protocol.ts'));
if (previous) require.extensions['.ts'] = previous;
else delete require.extensions['.ts'];
const cases = [];
for (const input of fixture.hostMessages) {
  assert.equal(kwel.parseNativeLocalHostMessage(input.message).ok, input.valid, input.name);
  cases.push({ name: input.name, message: input.message, valid: input.valid, host: true });
}
for (const input of fixture.hello) {
  if (input.valid) kwel.buildNativeLocalHello(input.identity);
  else assert.throws(() => kwel.buildNativeLocalHello(input.identity));
  cases.push({ name: input.name, message: { v: 1, type: 'hello', ...input.identity }, valid: input.valid });
}
for (const input of fixture.searchResults) {
  const result = kwel.buildNativeLocalSearchResult('r', {
    pages: input.pages, cursor: input.cursor, partial: input.partial,
  }, input.limit, page => input.pages.find(value => value.id === page.id).openRef);
  assert.deepEqual(result, input.expected);
  cases.push({ name: input.name, message: result, valid: true });
}
for (const input of fixture.errors) {
  assert.deepEqual(kwel.nativeLocalError(input.requestId, input.code), input);
  cases.push({ name: `error ${input.code}`, message: input, valid: true });
}
for (const [name, message] of Object.entries(fixture.responses)) cases.push({ name, message, valid: true });
assert.equal(cases.length, 65);
for (const [query, valid] of [
  ['', true], ['あ'.repeat(200), true], ['あ'.repeat(201), false],
  ['😀'.repeat(100), true], ['😀'.repeat(101), false],
  ['あ'.repeat(198) + '😀', true], ['あ'.repeat(199) + '😀', false],
  ['\u{10000}'.repeat(100), true], ['\u{10ffff}'.repeat(101), false],
  ['\ud800', false], ['\udfff', false], ['a\ud800b', false],
]) {
  const message = { v: 1, type: 'search', requestId: 'boundary', query, limit: 20 };
  assert.equal(kwel.parseNativeLocalHostMessage(message).ok, valid);
  cases.push({ name: `${query.length} UTF-16 units`, message, valid, host: true });
}
const adapter = path.join(root, 'target/debug/examples/native-conformance' + (process.platform === 'win32' ? '.exe' : ''));
const processResult = spawnSync(adapter, [], {
  input: cases.map(input => JSON.stringify(input.message)).join('\n') + '\n', encoding: 'utf8',
  timeout: 30_000, maxBuffer: 4_000_000,
});
assert.equal(processResult.status, 0, 'Rust test adapter failed');
const results = processResult.stdout.trim().split('\n').map(line => JSON.parse(line));
assert.equal(results.length, cases.length);
cases.forEach((input, index) => {
  assert.equal(results[index].accepted, input.valid, input.name);
  if (input.valid && input.host) {
    assert.equal(kwel.parseNativeLocalHostMessage(results[index].message).ok, true, `Rust frame: ${input.name}`);
    if (input.message.type === 'search') assert.equal(results[index].message.query, input.message.query);
  }
});
console.log(`Kwel parser / Rust Native v1: ${cases.length} cases passed (65 shared + 12 Unicode boundaries).`);
console.log('This is a real parser conformance test, not Chrome/installer/Alfred E2E.');
