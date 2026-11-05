#!/usr/bin/env node
// One-off migration of the legacy catalogue (apiVersion 1, latest version only, packages
// committed in repository/packages) to the versioned catalogue (apiVersion 2) listing
// every historical version.
//
//   node scripts/migrate-catalogue-v2.cjs [--releases releases.json] [--output repository/manifest.json]
//
// Every version comes from a signed file in repository/packages; its checksum is that
// file's SHA-256. A version published as a GitHub release asset points at the release
// (whose digest must match the local file). A version that was never released points at
// the immutable commit-pinned raw URL of the committed file. Nothing is invented: a
// version without a local signed file is left out. `--releases` is the JSON of
// GET /repos/OWNER/REPO/releases; without it the public API is read (read-only).
'use strict';
const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const c = require('./catalogue.cjs');

const git = (...args) => execFileSync('git', args, { cwd: c.root, encoding: 'utf8' }).trim();

function pluginDirectories() {
  const result = new Map();
  for (const dir of fs.readdirSync(c.root)) {
    const cargo = path.join(c.root, dir, 'Cargo.toml');
    if (!dir.startsWith('Aether-Plugins-') || !fs.existsSync(cargo)) continue;
    const id = fs.readFileSync(cargo, 'utf8').match(/\[package\.metadata\.aether\][^[]*?\bid\s*=\s*"([^"]+)"/s)?.[1];
    if (id) result.set(id, dir);
  }
  return result;
}

async function releases(file) {
  if (file) return JSON.parse(fs.readFileSync(file, 'utf8'));
  const all = [];
  for (let page = 1; ; page++) {
    const response = await fetch(`https://api.github.com/repos/${c.OWNER_REPO}/releases?per_page=100&page=${page}`,
      { headers: { 'User-Agent': 'aether-catalogue-migration', Accept: 'application/vnd.github+json' } });
    if (!response.ok) throw new Error(`GitHub API returned ${response.status}`);
    const batch = await response.json();
    all.push(...batch);
    if (batch.length < 100) return all;
  }
}

async function main() {
  const argv = process.argv.slice(2);
  const option = name => { const i = argv.indexOf(name); return i >= 0 ? argv[i + 1] : undefined; };
  const output = option('--output') || path.join(c.root, 'repository', 'manifest.json');
  const assets = new Map();
  for (const release of await releases(option('--releases'))) {
    if (release.draft) continue;
    for (const asset of release.assets || []) {
      assets.set(asset.name, { tag: release.tag_name, digest: asset.digest, published: release.published_at });
    }
  }
  const keys = new Map([
    ['maelmoreau21', c.readPublicKey(path.join(c.root, 'repository', 'publisher-public.txt'))],
    ['poster-rotator', c.readPublicKey(path.join(c.root, 'repository', 'poster-rotator-public.txt'))],
  ]);
  const dirs = pluginDirectories();
  const packages = path.join(c.root, 'repository', 'packages');
  const found = fs.readdirSync(packages).filter(name => name.endsWith('.package.json')).map(name => {
    const file = path.join(packages, name);
    const bytes = fs.readFileSync(file);
    const pkg = JSON.parse(bytes);
    c.validatePackage(pkg, keys.get(pkg.publisherId));
    const relative = `repository/packages/${name}`;
    // The last commit that wrote this file holds exactly these bytes (some packages
    // were rewritten after they were first added).
    const commit = git('log', '--format=%H', '-1', '--', relative);
    if (git('rev-parse', `${commit}:${relative}`) !== git('hash-object', relative)) {
      throw new Error(`${relative} differs from its last commit; commit or restore it first`);
    }
    return { name, bytes, pkg, relative, commit };
  });
  found.sort((a, b) => a.pkg.manifest.id.localeCompare(b.pkg.manifest.id) || c.compareVersions(a.pkg.manifest.version, b.pkg.manifest.version));
  const catalogue = { apiVersion: c.API_VERSION, plugins: [] };
  for (const { name, bytes, pkg, relative, commit } of found) {
    const id = pkg.manifest.id, checksum = c.sha256(bytes), release = assets.get(name);
    let sourceUrl, timestamp;
    if (release) {
      if (release.digest && release.digest !== `sha256:${checksum}`) throw new Error(`Release asset ${name} differs from ${relative}`);
      sourceUrl = c.releaseUrl(release.tag, name);
      timestamp = release.published;
    } else {
      sourceUrl = `https://raw.githubusercontent.com/${c.OWNER_REPO}/${commit}/${relative}`;
      timestamp = new Date(git('log', '-1', '--format=%cI', commit)).toISOString().replace(/\.\d{3}Z$/, 'Z');
    }
    const dir = dirs.get(id);
    const banner = dir && fs.existsSync(path.join(c.root, dir, 'assets', 'banner.svg'))
      ? `https://raw.githubusercontent.com/${c.OWNER_REPO}/main/${dir}/assets/banner.svg` : undefined;
    c.addVersion(catalogue, {
      pkg, publicKey: keys.get(pkg.publisherId), sourceUrl, checksum, timestamp, imageUrl: banner,
    });
  }
  // Historical releases carried no changelog, and none is invented here. One signed
  // package saved its description with a mis-decoded accent; the catalogue copy is fixed.
  for (const plugin of catalogue.plugins) plugin.description = c.repairText(plugin.description);
  c.writeCatalogue(output, catalogue);
  const total = catalogue.plugins.reduce((sum, plugin) => sum + plugin.versions.length, 0);
  console.log(`${output}: ${catalogue.plugins.length} plugins, ${total} versions`);
}

main().catch(error => { console.error(error.message); process.exitCode = 1; });
