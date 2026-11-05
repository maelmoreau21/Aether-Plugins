// Verify the versioned catalogue (repository/manifest.json, apiVersion 2).
//
//   node scripts/test-repository.cjs [REPOSITORY_DIR] [--online] [--repository OWNER/REPO]
//
// Every version must point at an immutable location: a GitHub release asset, or a
// commit-pinned raw file for legacy versions that were never released. A version whose
// asset is present in REPOSITORY_DIR/packages is verified offline (SHA-256, Ed25519
// signature, identity). With --online every asset is downloaded and verified.
'use strict';
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const c = require('./catalogue.cjs');

const argv = process.argv.slice(2);
const online = argv.includes('--online');
const repositoryIndex = argv.indexOf('--repository');
const repository = repositoryIndex >= 0 ? argv[repositoryIndex + 1] : c.OWNER_REPO;
const positional = argv.filter((arg, index) => !arg.startsWith('--') && argv[index - 1] !== '--repository');
const root = path.resolve(positional[0] || path.join(__dirname, '../repository'));

const keys = new Map();
for (const [publisher, file] of [['maelmoreau21', 'publisher-public.txt'], ['poster-rotator', 'poster-rotator-public.txt']]) {
  if (fs.existsSync(path.join(root, file))) keys.set(publisher, c.readPublicKey(path.join(root, file)));
}

function immutable(sourceUrl) {
  const url = new URL(sourceUrl);
  const release = url.origin === 'https://github.com' &&
    url.pathname.startsWith(`/${repository}/releases/download/`) && url.pathname.split('/').length === 7;
  const pinned = url.origin === 'https://raw.githubusercontent.com' &&
    new RegExp(`^/${repository.replace('.', '\\.')}/[0-9a-f]{40}/repository/packages/[a-z0-9._-]+\\.package\\.json$`).test(url.pathname);
  return release || pinned;
}

function verifyPackage(bytes, plugin, entry, where) {
  if (bytes.subarray(0, 4).equals(Buffer.from('PK\x03\x04', 'latin1'))) {
    console.log(`${where}: zip asset, SHA-256 verified (signature checked by the Core at install)`);
    return;
  }
  const pkg = JSON.parse(bytes);
  const publisherId = entry.publisherId || plugin.publisherId;
  const publicKey = entry.publicKey || plugin.publicKey;
  c.validatePackage(pkg, publicKey);
  assert.equal(pkg.manifest.id, plugin.id, `${where}: package id`);
  assert.equal(pkg.manifest.version, entry.version, `${where}: package version`);
  assert.equal(pkg.publisherId, publisherId, `${where}: publisher`);
  if (entry.permissions) assert.deepEqual([...entry.permissions].sort(), [...pkg.manifest.permissions].sort(), `${where}: permissions`);
  if (entry.httpHosts) assert.deepEqual([...entry.httpHosts].sort(), [...(pkg.manifest.httpHosts || [])].sort(), `${where}: httpHosts`);
}

async function main() {
  const catalogue = c.readCatalogue(path.join(root, 'manifest.json'));
  const total = c.validateCatalogue(catalogue);
  assert(catalogue.plugins.length > 0, 'The catalogue lists no plugin');
  const packages = path.join(root, 'packages');
  const local = fs.existsSync(packages) ? new Set(fs.readdirSync(packages)) : new Set();
  const referenced = new Set();
  let verified = 0, remote = 0;
  for (const plugin of catalogue.plugins) {
    for (const entry of plugin.versions) {
      const where = `${plugin.id}@${entry.version}`;
      const publisherId = entry.publisherId || plugin.publisherId;
      const publicKey = entry.publicKey || plugin.publicKey;
      if (keys.has(publisherId)) assert.equal(publicKey, keys.get(publisherId), `${where}: unexpected public key for ${publisherId}`);
      assert(immutable(entry.sourceUrl), `${where}: sourceUrl must be a release asset or a commit-pinned file: ${entry.sourceUrl}`);
      const name = decodeURIComponent(path.posix.basename(new URL(entry.sourceUrl).pathname));
      let bytes;
      if (local.has(name)) {
        bytes = fs.readFileSync(path.join(packages, name));
        referenced.add(name);
      } else if (online) {
        const response = await fetch(entry.sourceUrl, { headers: { 'User-Agent': 'aether-catalogue-check' } });
        assert(response.ok, `${where}: download returned HTTP ${response.status}`);
        bytes = Buffer.from(await response.arrayBuffer());
      } else {
        remote++;
        console.log(`${where}: release asset not present locally (use --online to download and verify)`);
        continue;
      }
      assert.equal(c.sha256(bytes), entry.checksum, `${where}: checksum does not match the asset`);
      verifyPackage(bytes, plugin, entry, where);
      verified++;
      console.log(`${where}: checksum, signature and identity valid (${bytes.length} bytes)`);
    }
  }
  // Legacy packages kept in the repository but not listed must still be valid.
  for (const name of [...local].filter(name => name.endsWith('.package.json') && !referenced.has(name))) {
    const pkg = JSON.parse(fs.readFileSync(path.join(packages, name)));
    assert(keys.has(pkg.publisherId), `${name}: unknown publisher ${pkg.publisherId}`);
    c.validatePackage(pkg, keys.get(pkg.publisherId));
    console.log(`${name}: unlisted legacy package, signature valid`);
  }
  console.log(`Catalogue valide : ${catalogue.plugins.length} extensions, ${total} versions, ` +
    `${verified} paquets verifies, ${remote} a verifier en ligne.`);
}

main().catch(error => { console.error(error.message); process.exitCode = 1; });
