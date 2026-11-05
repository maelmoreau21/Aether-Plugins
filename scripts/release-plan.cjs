#!/usr/bin/env node
// Resolve what the release workflow publishes, from a plugin id (manual run) or a
// pushed tag `<prefix>-v<x.y.z>`. The version always comes from the plugin's
// Cargo.toml: bump it in a normal commit first, then release.
//
//   PLUGIN=tmdb [VERSION=0.1.4] node scripts/release-plan.cjs
//   TAG=fanart-v0.1.4 node scripts/release-plan.cjs
//
// Prints KEY=VALUE lines (also appended to $GITHUB_OUTPUT when set).
'use strict';
const fs = require('node:fs');
const path = require('node:path');
const c = require('./catalogue.cjs');

function plugins() {
  const catalogue = c.readCatalogue(path.join(c.root, 'repository', 'manifest.json'));
  const result = [];
  for (const dir of fs.readdirSync(c.root).sort()) {
    const file = path.join(c.root, dir, 'Cargo.toml');
    if (!dir.startsWith('Aether-Plugins-') || !fs.existsSync(file)) continue;
    const cargo = fs.readFileSync(file, 'utf8');
    const id = cargo.match(/\[package\.metadata\.aether\][^[]*?\bid\s*=\s*"([^"]+)"/s)?.[1];
    const crate = cargo.match(/\[package\][^[]*?\bname\s*=\s*"([^"]+)"/s)?.[1];
    const version = cargo.match(/\[package\][^[]*?\bversion\s*=\s*"([^"]+)"/s)?.[1];
    if (!id || !crate || !version) continue;
    const listed = catalogue.plugins.find(plugin => plugin.id === id);
    // Keep the established asset prefix (fanart-tv publishes `fanart-<version>`).
    const asset = listed?.versions
      .map(entry => decodeURIComponent(path.posix.basename(new URL(entry.sourceUrl).pathname)))
      .find(name => name.endsWith('.package.json'));
    const prefix = asset ? asset.replace(/-\d+\.\d+\.\d+\.package\.json$/, '') : id;
    result.push({ id, crate, dir, version, prefix, listed });
  }
  return result;
}

function plan(env = process.env) {
  const all = plugins();
  let plugin, version;
  if (env.TAG) {
    const match = env.TAG.match(/^(.+)-v(\d+\.\d+\.\d+)$/);
    if (!match) throw new Error(`Tag must look like <plugin>-v<x.y.z>: ${env.TAG}`);
    plugin = all.find(entry => entry.prefix === match[1] || entry.id === match[1]);
    version = match[2];
  } else {
    plugin = all.find(entry => entry.id === env.PLUGIN);
    version = env.VERSION || plugin?.version;
  }
  if (!plugin) throw new Error(`Unknown plugin. Known: ${all.map(entry => entry.id).join(', ')}`);
  if (version !== plugin.version) {
    throw new Error(`${plugin.dir}/Cargo.toml has version ${plugin.version}, not ${version}. Commit the version bump first.`);
  }
  const asset = `${plugin.prefix}-${version}.package.json`;
  const tag = `${plugin.prefix}-v${version}`;
  // Every plugin, Poster Rotator included since 0.1.1, is signed with one publisher key.
  return {
    id: plugin.id, crate: plugin.crate, dir: plugin.dir, version, tag, asset,
    source_url: c.releaseUrl(tag, asset, env.GITHUB_REPOSITORY || c.OWNER_REPO),
    publisher: 'maelmoreau21', public_key_file: 'repository/publisher-public.txt',
    listed: String(Boolean(plugin.listed?.versions.some(entry => entry.version === version))),
  };
}

if (require.main === module) {
  try {
    const lines = Object.entries(plan()).map(([key, value]) => `${key}=${value}`).join('\n') + '\n';
    process.stdout.write(lines);
    if (process.env.GITHUB_OUTPUT) fs.appendFileSync(process.env.GITHUB_OUTPUT, lines);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
module.exports = { plan, plugins };
