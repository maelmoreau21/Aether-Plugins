const assert = require('node:assert/strict');
const test = require('node:test');
const catalogue = require('../repository/manifest.json');
const { discover } = require('./discover-plugin-releases.cjs');

const base = 'https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/repository/packages/';

test('all catalogue entries resolve to a release asset and the agreed tag prefix', () => {
  const releases = discover(catalogue, base, 'repository/packages');
  assert.equal(releases.length, catalogue.extensions.length);
  assert.equal(releases.find(item => item.id === 'avatars').pkgFile, 'repository/packages/avatar-0.1.0.package.json');
  assert.equal(releases.find(item => item.id === 'avatars').tag, 'avatar-v0.1.0');
  assert.equal(releases.find(item => item.id === 'fanart-tv').pkgFile, 'repository/packages/fanart-0.1.0.package.json');
  assert.equal(releases.find(item => item.id === 'fanart-tv').tag, 'fanart-v0.1.0');
});

test('missing or foreign catalogue assets fail publication', () => {
  const entry = catalogue.extensions[0];
  const missing = { extensions: [{ ...entry, packageUrl: `${base}missing-${entry.manifest.version}.package.json` }] };
  assert.throws(() => discover(missing, base, 'repository/packages'), /Missing package/);
  const foreign = { extensions: [{ ...entry, packageUrl: `https://example.org/avatar-${entry.manifest.version}.package.json` }] };
  assert.throws(() => discover(foreign, base, 'repository/packages'), /Invalid package URL/);
});
