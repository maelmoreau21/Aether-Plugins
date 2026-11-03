// Validates consistency between Cargo.toml workspaces, plugin packages, and repository/manifest.json

const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');

const root = path.resolve(__dirname, '..');
const allowUnpublished = process.argv.slice(2).includes('--allow-unpublished');
const repoDir = path.join(root, 'repository');
const manifestPath = path.join(repoDir, 'manifest.json');

console.log('Verifying repository manifest consistency...');
assert(fs.existsSync(manifestPath), 'repository/manifest.json not found');

const manifestContent = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
assert.equal(manifestContent.apiVersion, 1, 'manifest apiVersion must be 1');
assert(Array.isArray(manifestContent.extensions), 'manifest extensions must be an array');

const pluginDirs = [
  { dir: 'Aether-Plugins-Discogs', id: 'discogs' },
  { dir: 'Aether-Plugins-fanart.tv', id: 'fanart-tv' },
  { dir: 'Aether-Plugins-IMDb', id: 'imdb' },
  { dir: 'Aether-Plugins-LrcLib', id: 'lrclib' },
  { dir: 'Aether-Plugins-MusicBrainz', id: 'musicbrainz' },
  { dir: 'Aether-Plugins-OMDb', id: 'omdb' },
  { dir: 'Aether-Plugins-OpenSubtitles', id: 'opensubtitles', optionalInRepository: true },
  { dir: 'Aether-Plugins-TVDB', id: 'tvdb', optionalInRepository: true },
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

  // Sources may contain the next unpublished patch. The catalogue's signed
  // artifacts remain immutable until publication; never rewrite them to make
  // a local development version appear published.
  if (ext.manifest.version !== expectedVersion) {
    assert(allowUnpublished,
      `Version mismatch for '${id}': source ${expectedVersion}, catalogue ${ext.manifest.version}. Use --allow-unpublished only for local preparation; publication is pending.`);
    const stable = version => {
      assert(/^\d+\.\d+\.\d+$/.test(version), `Unsupported version: ${version}`);
      return version.split('.').map(Number);
    };
    const source = stable(expectedVersion), published = stable(ext.manifest.version);
    const firstDifference = source.findIndex((value, index) => value !== published[index]);
    assert(firstDifference >= 0 && source[firstDifference] > published[firstDifference],
      `Source version for '${id}' (${expectedVersion}) must not precede published ${ext.manifest.version}`);
    console.log(`  - ${id}: source ${expectedVersion}, catalogue publié ${ext.manifest.version} (publication en attente)`);
  }

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

  console.log(`  ✓ ${id}: source ${expectedVersion}, catalogue ${ext.manifest.version} (${dir} -> ${packageFileName})`);
}

console.log('Plugin sources and published catalogue are consistent; pending versions are reported above.');
