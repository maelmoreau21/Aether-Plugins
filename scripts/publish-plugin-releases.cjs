// Reconcile GitHub releases and immutable signed assets before retrying writes.
const fs = require('node:fs');
const path = require('node:path');
const { createHash } = require('node:crypto');
const { execFileSync } = require('node:child_process');
const { discover } = require('./discover-plugin-releases.cjs');

const sha256 = bytes => `sha256:${createHash('sha256').update(bytes).digest('hex')}`;
const delay = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));

class HttpError extends Error {
  constructor(status, method) {
    super(`GitHub ${method} returned HTTP ${status || 'network/timeout/invalid-response'}`);
    this.status = status;
  }
}

function transient(error) {
  return error instanceof HttpError && (error.status === 0 || error.status === 429 || error.status >= 500);
}

function githubClient(repository, token, fetchImpl = fetch) {
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository)) throw new Error('Invalid GitHub repository');
  if (!token) throw new Error('GH_TOKEN is required');
  const base = `https://api.github.com/repos/${repository}`;
  return async function request(method, endpoint, body, binary = false) {
    const url = new URL(endpoint.startsWith('/') ? base + endpoint : endpoint);
    if (!['https://api.github.com', 'https://uploads.github.com'].includes(url.origin) ||
        !url.pathname.startsWith(`/repos/${repository}/`) || url.username || url.password) {
      throw new Error('Unexpected GitHub API URL');
    }
    const upload = Buffer.isBuffer(body);
    const headers = {
      Authorization: `Bearer ${token}`,
      Accept: binary ? 'application/octet-stream' : 'application/vnd.github+json',
      'X-GitHub-Api-Version': '2022-11-28',
      'User-Agent': 'aether-plugin-release',
    };
    if (body !== undefined) headers['Content-Type'] = 'application/json';
    let response;
    try {
      response = await fetchImpl(url, {
        method, headers, body: upload ? body : body === undefined ? undefined : JSON.stringify(body),
        signal: AbortSignal.timeout(30_000),
      });
    } catch {
      throw new HttpError(0, method);
    }
    if (!response.ok) throw new HttpError(response.status, method);
    try {
      return binary ? Buffer.from(await response.arrayBuffer()) : await response.json();
    } catch {
      // The server may already have accepted a write whose response was lost.
      throw new HttpError(0, method);
    }
  };
}

function publisher(request, { attempts = 5, sleep = delay, log = console.log } = {}) {
  const backoff = attempt => sleep(Math.min(30_000, 2000 * (2 ** attempt)));

  async function read(endpoint, binary = false) {
    for (let attempt = 0; ; attempt++) {
      try {
        return await request('GET', endpoint, undefined, binary);
      } catch (error) {
        if (!transient(error) || attempt + 1 >= attempts) throw error;
        await backoff(attempt);
      }
    }
  }

  async function reconcile(observe, write) {
    for (let attempt = 0; ; attempt++) {
      const existing = await observe(); // An unavailable lookup never means absent.
      if (existing) return existing;
      try {
        return await write();
      } catch (error) {
        if (!transient(error) && error.status !== 422) throw error;
        // Never repeat an ambiguous POST before checking its committed effect.
        const committed = await observe();
        if (committed) return committed;
        if (!transient(error) || attempt + 1 >= attempts) throw error;
        await backoff(attempt);
      }
    }
  }

  return async function publish(item, bytes) {
    const tagPath = `/releases/tags/${encodeURIComponent(item.tag)}`;
    const release = await reconcile(async () => {
      try { return await read(tagPath); }
      catch (error) { if (error.status === 404) return null; throw error; }
    }, async () => {
      // GitHub ignores target_commitish when the tag already exists. Do not
      // attach a new release to an unexpected commit or move that existing tag.
      let reference;
      try { reference = await read(`/git/ref/tags/${encodeURIComponent(item.tag)}`); }
      catch (error) { if (error.status !== 404) throw error; }
      if (reference) {
        let object = reference.object;
        for (let depth = 0; object.type === 'tag' && depth < 8; depth++) {
          object = (await read(`/git/tags/${object.sha}`)).object;
        }
        if (object.type !== 'commit' || object.sha !== item.target) {
          throw new Error(`Existing tag targets another commit, refusing mutation: ${item.tag}`);
        }
      }
      return request('POST', '/releases', {
        tag_name: item.tag, target_commitish: item.target, name: item.title,
        prerelease: item.prerelease, draft: false,
        ...(item.notes ? { body: item.notes } : { generate_release_notes: true }),
      });
    });
    if (!Number.isSafeInteger(release.id) || release.tag_name !== item.tag ||
        release.draft || release.prerelease !== item.prerelease) {
      throw new Error(`Release identity or publication state mismatch: ${item.tag}`);
    }
    const name = path.basename(item.pkgFile);
    const digest = sha256(bytes);

    async function verify(asset) {
      if (asset.name !== name || asset.state !== 'uploaded' || asset.size !== bytes.length) {
        throw new Error(`Incomplete or conflicting asset, refusing replacement: ${item.tag}/${name}`);
      }
      const actual = asset.digest || sha256(await read(`/releases/assets/${asset.id}`, true));
      if (actual !== digest) throw new Error(`Asset SHA-256 conflict, refusing replacement: ${item.tag}/${name}`);
      return asset;
    }

    await reconcile(async () => {
      for (let page = 1; ; page++) {
        const assets = await read(`/releases/${release.id}/assets?per_page=100&page=${page}`);
        if (!Array.isArray(assets)) throw new Error('Invalid GitHub asset listing');
        const matching = assets.filter(asset => asset.name === name);
        if (matching.length > 1) throw new Error(`Duplicate assets: ${item.tag}/${name}`);
        if (matching.length) return verify(matching[0]);
        if (assets.length < 100) return null;
      }
    }, async () => {
      const upload = new URL(release.upload_url.split('{')[0]);
      upload.searchParams.set('name', name);
      return verify(await request('POST', upload.href, bytes));
    });
    log(`Verified release ${item.tag}: ${name} (${digest})`);
  };
}

function publicationPlan(items, channel, target) {
  if (!['main', 'develop'].includes(channel)) throw new Error('Expected main or develop channel');
  return items.map(item => ({
    ...item, target: target(item.pkgFile), prerelease: channel === 'develop',
    tag: item.tag + (channel === 'develop' ? '-beta' : ''),
    title: channel === 'develop' ? item.tag + '-beta' : `${item.name} ${item.version}`,
    notes: channel === 'main' ? `## Extension ${item.name} — Version ${item.version}\n\n` +
      `- **Identifiant** : \`${item.id}\`\n- **Version** : \`${item.version}\`\n` +
      `- **Tag Git** : \`${item.tag}\`\n- **Paquet** : \`${item.pkgFile}\`\n` +
      '- **Signature** : Ed25519 vérifiée avec succès.' : undefined,
  }));
}

async function main() {
  const [channel, input, fixedTarget] = process.argv.slice(2);
  const repository = process.env.GITHUB_REPOSITORY;
  const items = input ? JSON.parse(fs.readFileSync(input, 'utf8')) : discover(
    JSON.parse(fs.readFileSync('repository/manifest.json', 'utf8')),
    `https://raw.githubusercontent.com/${repository}/${channel}/repository/packages/`, 'repository/packages');
  const plan = publicationPlan(items, channel, file => fixedTarget || execFileSync(
    'git', ['log', '-1', '--format=%H', '--', file], { encoding: 'utf8' }).trim());
  // Preflight every local asset before any remote writes. No rebuild or signing.
  const packages = plan.map(item => {
    if (!/^[0-9a-f]{40}$/.test(item.target)) throw new Error('Expected exact release commit SHA');
    const bytes = fs.readFileSync(item.pkgFile);
    const committed = execFileSync('git', ['show', `${item.target}:${item.pkgFile}`]);
    if (!bytes.equals(committed)) throw new Error(`Package differs from release commit: ${item.pkgFile}`);
    return bytes;
  });
  const publish = publisher(githubClient(repository, process.env.GH_TOKEN));
  for (let index = 0; index < plan.length; index++) await publish(plan[index], packages[index]);
}

if (require.main === module) main().catch(error => { console.error(error.message); process.exitCode = 1; });
module.exports = { HttpError, githubClient, publisher, publicationPlan, sha256 };
