const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const catalogue = require('../repository/manifest.json');
const { discover } = require('./discover-plugin-releases.cjs');

const bases = ['main', 'develop'].map(branch =>
  `https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/${branch}/repository/packages/`);

test('all catalogue entries resolve to a release asset and the agreed tag prefix', () => {
  // A development catalogue can retain unchanged main assets. Only these two
  // explicit bases are trusted; never derive an allowed origin from the input.
  const releases = catalogue.extensions.flatMap(entry => {
    const base = bases.find(candidate => entry.packageUrl.startsWith(candidate));
    assert.ok(base, `Unexpected catalogue package URL: ${entry.packageUrl}`);
    return discover({ extensions: [entry] }, base, 'repository/packages');
  });
  assert.equal(releases.length, catalogue.extensions.length);
  const fanartVersion = catalogue.extensions.find(entry => entry.manifest.id === 'fanart-tv').manifest.version;
  assert.equal(releases.find(item => item.id === 'fanart-tv').pkgFile, `repository/packages/fanart-${fanartVersion}.package.json`);
  assert.equal(releases.find(item => item.id === 'fanart-tv').tag, `fanart-v${fanartVersion}`);
});

function fixture(t, base) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'plugin-discovery-'));
  t.after(() => {
    assert.equal(path.dirname(path.resolve(root)), path.resolve(os.tmpdir()));
    assert.ok(path.basename(root).startsWith('plugin-discovery-'));
    fs.rmSync(root, { recursive: true, force: true });
  });
  // Previous signed versions can remain on disk; discovery selects catalogue
  // entries only. Integrity/signature checks belong to test-repository.cjs.
  for (const version of ['0.1.0', '0.1.1']) {
    fs.writeFileSync(path.join(root, `fanart-${version}.package.json`), '{}');
  }
  const entry = { manifest: { id: 'fanart-tv', name: 'Fanart', version: '0.1.1' },
    packageUrl: `${base}fanart-0.1.1.package.json` };
  return { root, entry };
}

for (const base of bases) {
  test(`discovery uses the exact configured base ${base}`, t => {
    const { root, entry } = fixture(t, base);
    const releases = discover({ extensions: [entry] }, base, root);
    assert.equal(releases.length, 1, 'archived files must not become releases');
    assert.equal(releases[0].tag, 'fanart-v0.1.1');
    const old = { ...entry, manifest: { ...entry.manifest, version: '0.1.0' },
      packageUrl: `${base}fanart-0.1.0.package.json` };
    assert.equal(discover({ extensions: [old] }, base, root)[0].tag, 'fanart-v0.1.0');
    const otherBranch = bases.find(candidate => candidate !== base);
    assert.throws(() => discover({ extensions: [{ ...entry, packageUrl: `${otherBranch}fanart-0.1.1.package.json` }] }, base, root), /Invalid package URL/);
  });
}

test('missing, foreign, modified and mismatched catalogue assets fail publication', t => {
  const base = bases[0];
  const { root, entry } = fixture(t, base);
  for (const packageUrl of [
    'https://example.org/fanart-0.1.1.package.json',
    base.replace('raw.githubusercontent.com', 'raw.githubusercontent.com.example.org') + 'fanart-0.1.1.package.json',
    base.replace('/Aether-Plugins/', '/other-repo/') + 'fanart-0.1.1.package.json',
    `${base}fanart-0.1.1.package.json?download=1`,
    `${base}fanart-0.1.1.package.json#fragment`,
    `${base}fanart-0.1.0.package.json`,
  ]) {
    assert.throws(() => discover({ extensions: [{ ...entry, packageUrl }] }, base, root), /Invalid package URL/);
  }
  assert.throws(() => discover({ extensions: [{ ...entry, packageUrl: `${base}missing-0.1.1.package.json` }] }, base, root), /Missing package/);
  assert.throws(() => discover({ extensions: [entry, entry] }, base, root), /Duplicate release tag/);
});
