'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { plan, plugins } = require('./release-plan.cjs');
const version = plugins().find(plugin => plugin.id === 'tmdb').version;

test('beta tag keeps its exact name and points to the prerelease asset', () => {
  const result = plan({ TAG: `tmdb-v${version}-beta`, GITHUB_REPOSITORY: 'owner/plugins' });
  assert.equal(result.tag, `tmdb-v${version}-beta`);
  assert.equal(result.version, version);
  assert.equal(result.prerelease, 'true');
  assert.equal(result.source_url, `https://github.com/owner/plugins/releases/download/tmdb-v${version}-beta/tmdb-${version}.package.json`);
});

test('manual prerelease and stable tag produce distinct immutable URLs', () => {
  const beta = plan({ PLUGIN: 'tmdb', VERSION: version, PRERELEASE: 'true' });
  const stable = plan({ TAG: `tmdb-v${version}` });
  assert.equal(beta.tag, `tmdb-v${version}-beta`);
  assert.equal(stable.tag, `tmdb-v${version}`);
  assert.equal(stable.prerelease, 'false');
  assert.notEqual(beta.source_url, stable.source_url);
  assert.throws(() => plan({ TAG: 'tmdb-v9999.9999.9999-beta' }), /Cargo.toml has version/);
});

test('automatic releases may publish above Cargo.toml but never below it', () => {
  const [major, minor, patch] = version.split('.').map(Number);
  const next = `${major}.${minor}.${patch + 1}`;
  assert.equal(plan({ PLUGIN: 'tmdb', VERSION: next, AUTO: 'true', PRERELEASE: 'true' }).tag, `tmdb-v${next}-beta`);
  assert.throws(() => plan({ PLUGIN: 'tmdb', VERSION: next }), /Cargo.toml has version/);
  assert.throws(() => plan({ PLUGIN: 'tmdb', VERSION: '0.0.1', AUTO: 'true' }), /Cargo.toml has version/);
});

test('automatic plan helpers read release tags and ignore documentation', () => {
  const { compare, tagOf, docOnly } = require('./auto-release-plan.cjs');
  assert.ok(compare('0.1.10', '0.1.9') > 0);
  assert.equal(tagOf({ sourceUrl: 'https://github.com/o/r/releases/download/fanart-v0.1.3-beta/fanart-0.1.3.package.json' }), 'fanart-v0.1.3-beta');
  assert.ok(docOnly('Aether-Plugins-TMDb/README.md'));
  assert.ok(!docOnly('Aether-Plugins-TMDb/src/lib.rs'));
});
