// Resolve release assets from the validated catalogue, not from plugin IDs.
const fs = require('node:fs');
const path = require('node:path');

function discover(catalogue, baseUrl, packageDir) {
  const base = new URL(baseUrl);
  const releases = [];
  const tags = new Set();
  for (const entry of catalogue.extensions || []) {
    const { id, name, version } = entry.manifest;
    const url = new URL(entry.packageUrl);
    const filename = decodeURIComponent(path.posix.basename(url.pathname));
    const suffix = `-${version}.package.json`;
    if (url.href !== new URL(encodeURIComponent(filename), base).href ||
        !/^[a-z0-9][a-z0-9._-]*\.package\.json$/.test(filename) ||
        !filename.endsWith(suffix)) {
      throw new Error(`Invalid package URL for ${id}@${version}`);
    }
    const prefix = filename.slice(0, -suffix.length);
    if (!prefix) throw new Error(`Missing release prefix for ${id}@${version}`);
    const pkgFile = path.posix.join(packageDir, filename);
    if (!fs.existsSync(pkgFile)) throw new Error(`Missing package for ${id}@${version}: ${pkgFile}`);
    const tag = `${prefix}-v${version}`;
    if (tags.has(tag)) throw new Error(`Duplicate release tag: ${tag}`);
    tags.add(tag);
    releases.push({ id, version, tag, pkgFile, name });
  }
  return releases;
}

if (require.main === module) {
  try {
    const catalogue = JSON.parse(fs.readFileSync('repository/manifest.json', 'utf8'));
    const baseUrl = process.argv[2];
    if (!baseUrl) throw new Error('Expected package base URL');
    process.stdout.write(`${JSON.stringify(discover(catalogue, baseUrl, 'repository/packages'), null, 2)}\n`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}

module.exports = { discover };
