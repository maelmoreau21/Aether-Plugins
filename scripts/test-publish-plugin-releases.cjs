const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const { HttpError, githubClient, publisher, publicationPlan, sha256 } = require('./publish-plugin-releases.cjs');

const bytes = Buffer.from('{"signed":"immutable fixture"}\n');
const target = 'a'.repeat(40);
const item = { tag: 'fixture-v0.1.0-beta', target, title: 'Fixture', prerelease: true,
  pkgFile: 'repository/packages/fixture-0.1.0.package.json' };
const asset = () => ({ id: 42, name: 'fixture-0.1.0.package.json', state: 'uploaded',
  size: bytes.length, digest: sha256(bytes) });
const release = () => ({ id: 7, tag_name: item.tag, prerelease: true, draft: false,
  upload_url: 'https://uploads.github.com/repos/owner/repo/releases/7/assets{?name,label}' });

function fixture(options = {}) {
  const state = { release: options.release || null, assets: options.assets || [], calls: [], sleeps: [] };
  const creates = [...(options.creates || [])];
  const uploads = [...(options.uploads || [])];
  const request = async (method, endpoint, payload, binary) => {
    state.calls.push({ method, endpoint, payload });
    if (method === 'GET' && endpoint.startsWith('/releases/tags/')) {
      if (options.lookupError) throw new HttpError(options.lookupError, method);
      if (!state.release) throw new HttpError(404, method);
      return state.release;
    }
    if (method === 'GET' && endpoint.startsWith('/git/ref/tags/')) {
      if (!options.tag) throw new HttpError(404, method);
      return { object: options.tag };
    }
    if (method === 'GET' && endpoint.startsWith('/git/tags/')) return { object: options.annotated };
    if (method === 'GET' && endpoint.includes('/assets?')) return state.assets;
    if (method === 'GET' && endpoint === '/releases/assets/42') {
      assert.equal(binary, true);
      return options.download || bytes;
    }
    if (method === 'POST' && endpoint === '/releases') {
      const outcome = creates.shift();
      if (!outcome || outcome.committed) state.release = release();
      if (outcome) throw new HttpError(outcome.status, method);
      return state.release;
    }
    if (method === 'POST' && endpoint.startsWith('https://uploads.github.com/')) {
      assert.deepEqual(payload, bytes);
      const outcome = uploads.shift();
      if (!outcome || outcome.committed) state.assets = [outcome?.asset || asset()];
      if (outcome) throw new HttpError(outcome.status, method);
      return state.assets[0];
    }
    throw new Error(`Unexpected test request ${method} ${endpoint}`);
  };
  return { state, publish: publisher(request, { attempts: 3,
    sleep: async milliseconds => state.sleeps.push(milliseconds), log: () => {} }) };
}
const posts = state => state.calls.filter(call => call.method === 'POST');

test('new release uses exact package commit and uploads unchanged bytes', async () => {
  const { state, publish } = fixture();
  await publish(item, bytes);
  assert.equal(posts(state).length, 2);
  assert.equal(posts(state)[0].payload.target_commitish, target);
  assert.equal(posts(state)[0].payload.prerelease, true);
  assert.equal(state.assets[0].digest, sha256(bytes));
});

test('existing signed asset is verified without recreating release or moving its tag', async () => {
  const { state, publish } = fixture({ release: release(), assets: [asset()],
    tag: { type: 'commit', sha: 'b'.repeat(40) } });
  await publish(item, bytes);
  assert.equal(posts(state).length, 0);
  assert.ok(state.calls.every(call => !call.endpoint.startsWith('/git/')));
});

test('ambiguous release POST is reconciled before any second write', async () => {
  for (const status of [0, 500, 422]) {
    const { state, publish } = fixture({ creates: [{ status, committed: true }] });
    await publish(item, bytes);
    assert.equal(posts(state).filter(call => call.endpoint === '/releases').length, 1);
    assert.ok(state.calls.findIndex(call => call.method === 'POST') <
      state.calls.findLastIndex(call => call.endpoint.startsWith('/releases/tags/')));
  }
});

test('absent release after transient POST permits bounded retry with same identity', async () => {
  const { state, publish } = fixture({ creates: [{ status: 500 }] });
  await publish(item, bytes);
  const requests = posts(state).filter(call => call.endpoint === '/releases');
  assert.equal(requests.length, 2);
  assert.deepEqual(requests[0].payload, requests[1].payload);
  assert.deepEqual(state.sleeps, [2000]);
});

test('unavailable lookup is never treated as release absence', async () => {
  const { state, publish } = fixture({ lookupError: 503 });
  await assert.rejects(publish(item, bytes), /HTTP 503/);
  assert.equal(posts(state).length, 0);
  assert.equal(state.calls.length, 3);
});

test('permanent authentication and validation failures do not retry writes', async () => {
  for (const status of [401, 403]) {
    const { state, publish } = fixture({ lookupError: status });
    await assert.rejects(publish(item, bytes), new RegExp(`HTTP ${status}`));
    assert.equal(state.calls.length, 1);
    assert.equal(posts(state).length, 0);
  }
  const { state, publish } = fixture({ creates: [{ status: 422 }] });
  await assert.rejects(publish(item, bytes), /HTTP 422/);
  assert.equal(posts(state).length, 1);
  assert.equal(state.sleeps.length, 0);
});

test('persistent mutation failures stop after the configured budget', async () => {
  const { state, publish } = fixture({ creates: Array(3).fill({ status: 500 }) });
  await assert.rejects(publish(item, bytes), /HTTP 500/);
  assert.equal(posts(state).length, 3);
  assert.deepEqual(state.sleeps, [2000, 4000]);
});

test('release without asset resumes upload, including an ambiguous upload response', async () => {
  const { state, publish } = fixture({ release: release(), uploads: [{ status: 502, committed: true }] });
  await publish(item, bytes);
  assert.equal(posts(state).length, 1);
  assert.ok(posts(state)[0].endpoint.startsWith('https://uploads.github.com/'));
});

test('conflicting signed bytes and incomplete upload placeholders are never overwritten', async () => {
  for (const existing of [
    { ...asset(), digest: sha256(Buffer.from('other signed package')) },
    { ...asset(), state: 'starter', size: 0 },
    { ...asset(), size: bytes.length + 1 },
  ]) {
    const { state, publish } = fixture({ release: release(), assets: [existing] });
    await assert.rejects(publish(item, bytes), /refusing replacement/);
    assert.equal(posts(state).length, 0);
  }
});

test('legacy assets without digest are downloaded and checked before reuse', async () => {
  const old = asset();
  delete old.digest;
  const { state, publish } = fixture({ release: release(), assets: [old] });
  await publish(item, bytes);
  assert.ok(state.calls.some(call => call.endpoint === '/releases/assets/42'));
  assert.equal(posts(state).length, 0);
  const conflicting = fixture({ release: release(), assets: [old], download: Buffer.from('different') });
  await assert.rejects(conflicting.publish(item, bytes), /SHA-256 conflict/);
  assert.equal(posts(conflicting.state).length, 0);
});

test('pre-existing lightweight and annotated tags keep their exact SHA', async () => {
  for (const options of [
    { tag: { type: 'commit', sha: target } },
    { tag: { type: 'tag', sha: 'c'.repeat(40) }, annotated: { type: 'commit', sha: target } },
  ]) {
    const { state, publish } = fixture(options);
    await publish(item, bytes);
    assert.ok(posts(state).every(call => !call.endpoint.startsWith('/git/')));
  }
  const { state, publish } = fixture({ tag: { type: 'commit', sha: 'b'.repeat(40) } });
  await assert.rejects(publish(item, bytes), /tag targets another commit/);
  assert.equal(posts(state).length, 0);
});

test('draft or wrong-channel releases are rejected before upload', async () => {
  for (const existing of [{ ...release(), draft: true }, { ...release(), prerelease: false }]) {
    const { state, publish } = fixture({ release: existing });
    await assert.rejects(publish(item, bytes), /publication state mismatch/);
    assert.equal(posts(state).length, 0);
  }
});

test('develop recovery uses existing numeric packages and only beta tags', () => {
  const entries = [{ id: 'fixture', version: '0.1.0', tag: 'fixture-v0.1.0',
    pkgFile: item.pkgFile, name: 'Fixture' }];
  const plan = publicationPlan(entries, 'develop', () => target);
  assert.deepEqual(plan.map(entry => [entry.tag, entry.version, entry.pkgFile, entry.target]),
    [[item.tag, '0.1.0', item.pkgFile, target]]);
  assert.equal(plan[0].prerelease, true);
  assert.equal(publicationPlan(entries, 'main', () => target)[0].tag, 'fixture-v0.1.0');
  const workflow = fs.readFileSync('.github/workflows/publish-develop-plugins.yml', 'utf8');
  const recovery = workflow.split('- name: Publier ou reprendre les releases bêta')[1].split('- name:')[0];
  assert.match(recovery, /publish-plugin-releases.cjs develop/);
  assert.doesNotMatch(recovery, /if:|cargo |pack |git commit/);
});

test('API adapter rejects foreign URLs before sending credentials and hides response bodies', async () => {
  let called = 0;
  const client = githubClient('owner/repo', 'never-print-this-token', async () => {
    called++;
    return { ok: false, status: 403, text: () => 'sensitive response body' };
  });
  await assert.rejects(client('POST', 'https://example.org/repos/owner/repo/releases'), /Unexpected GitHub API URL/);
  assert.equal(called, 0);
  await assert.rejects(client('GET', '/releases'), error =>
    /HTTP 403/.test(error.message) && !/sensitive|never-print/.test(error.message));
});
