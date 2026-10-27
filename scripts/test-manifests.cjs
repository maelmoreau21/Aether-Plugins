// Validates consistency between Cargo.toml workspaces, plugin packages, and repository/manifest.json

const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');

const root = path.resolve(__dirname, '..');
const repoDir = path.join(root, 'repository');
const manifestPath = path.join(repoDir, 'manifest.json');

console.log('Verifying repository manifest consistency...');
assert(fs.existsSync(manifestPath), 'repository/manifest.json not found');

const manifestContent = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
assert.equal(manifestContent.apiVersion, 1, 'manifest apiVersion must be 1');
assert(Array.isArray(manifestContent.extensions), 'manifest extensions must be an array');

const pluginDirs = [
  { dir: 'Aether-Plugins-Avatar', id: 'avatars' },
  { dir: 'Aether-Plugins-Discogs', id: 'discogs' },
  { dir: 'Aether-Plugins-fanart.tv', id: 'fanart-tv' },
  { dir: 'Aether-Plugins-IMDb', id: 'imdb' },
  { dir: 'Aether-Plugins-LrcLib', id: 'lrclib' },
  { dir: 'Aether-Plugins-MusicBrainz', id: 'musicbrainz' },
  { dir: 'Aether-Plugins-OMDb', id: 'omdb' },
  { dir: 'Aether-Plugins-TMDb', id: 'tmdb' },
  { dir: 'Aether-Plugins-IntroSkipper', id: 'intro-skipper', optionalInRepository: true },
  { dir: 'Aether-Plugins-PosterRotator', id: 'poster-rotator', optionalInRepository: true },
];

const extensionsMap = new Map();
for (const ext of manifestContent.extensions) {
  assert(ext.manifest, 'Extension entry must contain manifest');
  assert(ext.manifest.id, 'Extension manifest must contain id');
  assert(ext.packageUrl, 'Extension must have packageUrl');
  assert(ext.packageSha256, 'Extension must have packageSha256');
  assert(ext.publicKey, 'Extension must have publicKey');
  extensionsMap.set(ext.manifest.id, ext);
}

for (const { dir, id, optionalInRepository } of pluginDirs) {
  const cargoPath = path.join(root, dir, 'Cargo.toml');
  assert(fs.existsSync(cargoPath), `Cargo.toml not found for ${dir}`);
  const cargoText = fs.readFileSync(cargoPath, 'utf8');

  // Extract version from Cargo.toml
  const versionMatch = cargoText.match(/version\s*=\s*"([^"]+)"/);
  assert(versionMatch, `Could not find version in ${cargoPath}`);
  const expectedVersion = versionMatch[1];

  const ext = extensionsMap.get(id);
  if (!ext && optionalInRepository) {
    console.log(`  - ${id}@${expectedVersion} (${dir}): plugin présent dans le workspace, en attente de première publication signée`);
    continue;
  }
  assert(ext, `Extension '${id}' (from ${dir}) missing in repository/manifest.json`);

  assert.equal(
    ext.manifest.version,
    expectedVersion,
    `Version mismatch for '${id}': Cargo.toml has ${expectedVersion}, manifest.json has ${ext.manifest.version}`
  );

  // Validate permissions and hosts
  assert(
    Array.isArray(ext.manifest.permissions) && ext.manifest.permissions.length > 0,
    `Extension '${id}' must have at least one permission`
  );

  if (ext.manifest.httpHosts) {
    for (const host of ext.manifest.httpHosts) {
      assert(
        !host.includes('/') && !host.includes(':'),
        `Invalid httpHost '${host}' in '${id}': must be bare hostname without protocol or path`
      );
    }
  }

  // Validate package file existence from packageUrl
  const url = new URL(ext.packageUrl);
  const packageFileName = decodeURIComponent(path.posix.basename(url.pathname));
  const pkgFile = path.join(repoDir, 'packages', packageFileName);
  assert(
    fs.existsSync(pkgFile),
    `Corresponding package file not found: ${pkgFile}`
  );

  console.log(`  ✓ ${id}@${expectedVersion} (${dir} -> ${packageFileName}): workspace and repository synchronized`);
}

console.log('All plugins and manifest entries are strictly synchronized!');
