#!/usr/bin/env node
// Append a published version to repository/manifest.json, keeping every previous
// version (newest first). Idempotent: re-running with the same asset changes nothing.
//
//   node scripts/add-catalogue-version.cjs --package PKG.package.json --asset ASSET \
//     --source-url URL --public-key repository/publisher-public.txt \
//     [--manifest repository/manifest.json] [--changelog TEXT | --changelog-file FILE] \
//     [--timestamp ISO-8601] [--image-url URL]
//
// --asset is the exact file uploaded to the release (the package JSON or a zip that
// contains it); its SHA-256 becomes the catalogue checksum.
'use strict';
const fs = require('node:fs');
const path = require('node:path');
const c = require('./catalogue.cjs');

function options(argv) {
  const result = {};
  for (let i = 0; i < argv.length; i += 2) {
    const key = argv[i];
    if (!key.startsWith('--') || argv[i + 1] === undefined) throw new Error(`Invalid argument: ${key}`);
    result[key.slice(2)] = argv[i + 1];
  }
  return result;
}

function main() {
  const o = options(process.argv.slice(2));
  for (const key of ['package', 'source-url', 'public-key']) if (!o[key]) throw new Error(`--${key} is required`);
  const manifestFile = o.manifest || path.join(c.root, 'repository', 'manifest.json');
  const pkg = JSON.parse(fs.readFileSync(o.package, 'utf8'));
  const publicKey = c.readPublicKey(o['public-key']);
  c.validatePackage(pkg, publicKey);
  const asset = fs.readFileSync(o.asset || o.package);
  const changelog = o['changelog-file'] ? fs.readFileSync(o['changelog-file'], 'utf8').trim() : o.changelog?.trim();
  const catalogue = fs.existsSync(manifestFile) ? c.readCatalogue(manifestFile) : { apiVersion: c.API_VERSION, plugins: [] };
  const changed = c.addVersion(catalogue, {
    pkg, publicKey, sourceUrl: o['source-url'], checksum: c.sha256(asset),
    timestamp: o.timestamp || new Date().toISOString().replace(/\.\d{3}Z$/, 'Z'),
    changelog: changelog || undefined, imageUrl: o['image-url'],
  });
  if (changed) c.writeCatalogue(manifestFile, catalogue);
  console.log(`${pkg.manifest.id}@${pkg.manifest.version}: ${changed ? 'added to' : 'already in'} ${manifestFile}`);
}

if (require.main === module) {
  try { main(); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
