// node --test scripts/test-repository-history.cjs
// The versioned catalogue keeps previous versions installable and verifiable.
'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs'), os = require('node:os'), path = require('node:path'), crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');
const c = require('./catalogue.cjs');

const verifier = path.join(__dirname, 'test-repository.cjs');

function fixture(change) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'plugin-history-'));
  try {
    fs.mkdirSync(path.join(root, 'packages'));
    const { privateKey, publicKey } = crypto.generateKeyPairSync('ed25519');
    const publicRaw = publicKey.export({ format: 'der', type: 'spki' }).subarray(-32).toString('base64');
    fs.writeFileSync(path.join(root, 'publisher-public.txt'), publicRaw);
    const catalogue = { apiVersion: 2, plugins: [] };
    const files = {};
    for (const version of ['0.1.0', '0.1.1']) {
      const wasm = Buffer.from('0061736d01000000', 'hex');
      const sha256 = c.sha256(wasm);
      const manifest = { apiVersion: 1, id: 'fixture', name: 'Fixture', version, description: 'Synthetic', author: 'Tests',
        license: 'MIT', repository: 'https://example.org', permissions: ['metadata.read'], checksum: sha256 };
      const signature = crypto.sign(null, c.signingBytes(manifest, 'maelmoreau21', sha256), privateKey).toString('base64');
      const pkg = { manifest, publisherId: 'maelmoreau21', wasm: wasm.toString('base64'), sha256, signature };
      const name = `fixture-${version}.package.json`;
      const bytes = Buffer.from(JSON.stringify(pkg));
      fs.writeFileSync(path.join(root, 'packages', name), bytes);
      files[version] = { name, pkg };
      c.addVersion(catalogue, { pkg, publicKey: publicRaw, checksum: c.sha256(bytes),
        sourceUrl: c.releaseUrl(`fixture-v${version}`, name), timestamp: '2026-11-05T00:00:00Z' });
    }
    change?.(root, catalogue, files);
    fs.writeFileSync(path.join(root, 'manifest.json'), JSON.stringify(catalogue));
    return spawnSync(process.execPath, [verifier, root], { encoding: 'utf8' });
  } finally {
    assert.equal(path.dirname(path.resolve(root)), path.resolve(os.tmpdir()));
    assert(path.basename(root).startsWith('plugin-history-'));
    fs.rmSync(root, { recursive: true, force: true });
  }
}

test('both versions stay listed and verified', () => {
  const result = fixture();
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /1 extensions, 2 versions, 2 paquets verifies/);
});
test('a corrupted previous version is rejected', () => {
  const result = fixture((root, catalogue, files) => {
    const old = files['0.1.0'];
    old.pkg.wasm = Buffer.from('corrupted').toString('base64');
    const bytes = Buffer.from(JSON.stringify(old.pkg));
    fs.writeFileSync(path.join(root, 'packages', old.name), bytes);
    catalogue.plugins[0].versions[1].checksum = c.sha256(bytes);
  });
  assert.notEqual(result.status, 0);
});
test('a checksum that does not match the asset is rejected', () => {
  const result = fixture((root, catalogue) => { catalogue.plugins[0].versions[0].checksum = 'f'.repeat(64); });
  assert.notEqual(result.status, 0);
});
test('a mutable branch URL is rejected', () => {
  const result = fixture((root, catalogue) => {
    catalogue.plugins[0].versions[0].sourceUrl = 'https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/repository/packages/fixture-0.1.1.package.json';
  });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /release asset or a commit-pinned file/);
});
test('versions must be listed newest first', () => {
  const result = fixture((root, catalogue) => catalogue.plugins[0].versions.reverse());
  assert.notEqual(result.status, 0);
});
