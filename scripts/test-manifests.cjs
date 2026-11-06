// Consistency between the plugin sources (Cargo.toml) and repository/manifest.json.
//
//   node scripts/test-manifests.cjs [--allow-unpublished]
//
// A Cargo.toml version may lag behind the catalogue: auto-release publishes the next
// patch without a bump commit. Without --allow-unpublished (main), a source must not be
// ahead of the newest published version; with it (development), it may be: that
// version simply has not been released yet.
'use strict';
const assert = require('node:assert/strict');
const path = require('node:path');
const c = require('./catalogue.cjs');
const { plugins } = require('./release-plan.cjs');

const allowUnpublished = process.argv.slice(2).includes('--allow-unpublished');
const catalogue = c.readCatalogue(path.join(c.root, 'repository', 'manifest.json'));
c.validateCatalogue(catalogue);

console.log('Verifying plugin sources against the versioned catalogue...');
for (const { id, dir, version, listed } of plugins()) {
  if (!listed) {
    console.log(`  - ${id}@${version} (${dir}): not published yet`);
    continue;
  }
  const latest = listed.versions[0].version;
  const order = c.compareVersions(version, latest);
  // Automatic releases take the next patch from the published history and never
  // commit a Cargo.toml bump: a source behind the catalogue is the normal state.
  if (order < 0) {
    console.log(`  ✓ ${id}: published ${latest} (source ${version}, versioned by auto-release)`);
    continue;
  }
  if (order > 0) {
    assert(allowUnpublished, `${id}: source ${version} is not published (catalogue ${latest}). Run the release workflow, or use --allow-unpublished outside main.`);
    console.log(`  - ${id}: source ${version}, published ${latest} (release pending)`);
    continue;
  }
  console.log(`  ✓ ${id}: ${version} (${listed.versions.length} versions published)`);
}
const known = new Set(plugins().map(plugin => plugin.id));
for (const plugin of catalogue.plugins) assert(known.has(plugin.id), `Catalogue plugin without sources: ${plugin.id}`);
console.log('Plugin sources and catalogue are consistent.');
