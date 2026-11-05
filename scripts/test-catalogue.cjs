// node --test scripts/test-catalogue.cjs
// Signing compatibility with the Core and versioned catalogue updates, offline.
// Set AETHER_EXTENSION_PACK to a built `media-extension-pack` to also verify a
// package signed here with the Core's own validation.
'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const { execFileSync } = require('node:child_process');
const c = require('./catalogue.cjs');
const { sign } = require('./sign-package.cjs');

const packages = path.join(c.root, 'repository', 'packages');

// `media-extension-pack keygen` (ring) writes RFC 5958 v2 PKCS#8 with the public key
// as [1] IMPLICIT; build one byte for byte the same way.
function ringKey() {
  const { privateKey } = crypto.generateKeyPairSync('ed25519');
  const seed = privateKey.export({ format: 'der', type: 'pkcs8' }).subarray(-32);
  const publicRaw = crypto.createPublicKey(privateKey).export({ format: 'der', type: 'spki' }).subarray(-32);
  const der = Buffer.concat([Buffer.from('3051020101300506032b657004220420', 'hex'), seed,
    Buffer.from('812100', 'hex'), publicRaw]);
  return { der, publicKey: publicRaw.toString('base64') };
}

const wasm = Buffer.from('0061736d01000000', 'hex');
const manifest = version => ({
  apiVersion: 1, id: 'fixture', name: 'Fixture', version, description: 'Synthétique', author: 'Tests',
  license: 'MIT', repository: 'https://example.org', permissions: ['metadata.read'], httpHosts: [],
  configSchema: [{ key: 'api_key', label: 'Clé', fieldType: 'password', required: true, secret: true }],
});

test('canonical serialization reproduces every signed package of the repository', () => {
  for (const name of fs.readdirSync(packages)) {
    const pkg = JSON.parse(fs.readFileSync(path.join(packages, name)));
    assert.equal(JSON.stringify(c.canonicalManifest(pkg.manifest)), JSON.stringify(pkg.manifest), name);
  }
});

test('a package signed here verifies with the repository rules', () => {
  const { der, publicKey } = ringKey();
  const privateKey = c.privateKeyObject(der);
  assert.equal(c.rawPublicKey(privateKey), publicKey);
  const pkg = sign({ manifest: manifest('1.0.0'), wasm, privateKey, publisherId: 'tests' });
  c.validatePackage(pkg, publicKey, { strictText: true });
  // Serde defaults are made explicit and empty optional arrays are omitted.
  assert.equal(pkg.manifest.httpHosts, undefined);
  assert.deepEqual(Object.keys(pkg.manifest.configSchema[0]), ['key', 'label', 'description', 'fieldType', 'required', 'secret']);
  assert.throws(() => sign({ manifest: { ...manifest('1.0.0'), unknown: 1 }, wasm, privateKey, publisherId: 'tests' }), /Unknown manifest field/);
  const tampered = { ...pkg, manifest: { ...pkg.manifest, permissions: ['metadata.write'] } };
  assert.throws(() => c.validatePackage(tampered, publicKey), /signature/);
  const pack = process.env.AETHER_EXTENSION_PACK;
  if (pack) {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'aether-sign-'));
    try {
      fs.writeFileSync(path.join(dir, 'p.json'), JSON.stringify(pkg));
      fs.writeFileSync(path.join(dir, 'key.txt'), publicKey);
      assert.match(execFileSync(pack, ['verify', path.join(dir, 'p.json'), path.join(dir, 'key.txt')], { encoding: 'utf8' }), /is valid/);
    } finally { fs.rmSync(dir, { recursive: true, force: true }); }
  }
});

test('appending versions keeps history, is idempotent and refuses replacement', () => {
  const first = ringKey(), second = ringKey();
  const catalogue = { apiVersion: 2, plugins: [] };
  const add = (version, key, publisherId = 'tests', checksum = 'a'.repeat(64)) => c.addVersion(catalogue, {
    pkg: { manifest: manifest(version), publisherId }, publicKey: key.publicKey, checksum,
    sourceUrl: c.releaseUrl(`fixture-v${version}`, `fixture-${version}.package.json`),
    timestamp: '2026-11-05T00:00:00Z', changelog: `- ${version}`,
  });
  assert.equal(add('1.0.0', first), true);
  assert.equal(add('1.2.0', first), true);
  assert.equal(add('1.1.0', first), true);
  assert.equal(add('1.1.0', first), false);
  assert.throws(() => add('1.1.0', first, 'tests', 'b'.repeat(64)), /already published/);
  const plugin = catalogue.plugins[0];
  assert.deepEqual(plugin.versions.map(v => v.version), ['1.2.0', '1.1.0', '1.0.0']);
  // A new signing key keeps older versions verifiable with their own key.
  assert.equal(add('2.0.0', second, 'tests-2'), true);
  assert.equal(catalogue.plugins[0].publicKey, second.publicKey);
  assert.equal(catalogue.plugins[0].versions[1].publicKey, first.publicKey);
  assert.equal(catalogue.plugins[0].versions[0].publicKey, undefined);
  assert.equal(c.validateCatalogue(catalogue), 4);
});

test('the published catalogue is valid and lists every historical version', () => {
  const catalogue = c.readCatalogue(path.join(c.root, 'repository', 'manifest.json'));
  assert(c.validateCatalogue(catalogue) >= catalogue.plugins.length);
  assert(catalogue.plugins.some(plugin => plugin.versions.length > 1));
});
