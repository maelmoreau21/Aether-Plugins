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
