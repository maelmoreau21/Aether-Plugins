// Verification of compiled WebAssembly modules:
// - Valid WebAssembly bytecode
// - Hard size budget <= 1 MiB (media-extension-sdk MAX_MODULE, extension ABI 1.1); a module over
//   256 KiB must declare targetAbi 1.1 (checked by catalogue.cjs on the signed package)
// - ABI exports: memory, media_alloc, media_run
// - Explicit host HTTP and versioned private storage imports only

const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');

const wasmDir = path.resolve(process.argv[2] || path.join(__dirname, '../target/wasm32-unknown-unknown/release'));
const MAX_MODULE_BYTES = 1024 * 1024; // 1,048,576 bytes

const expectedPlugins = [
  'aether_plugin_discogs',
  'aether_plugin_fanart',
  'aether_plugin_imdb',
  'aether_plugin_intro_skipper',
  'aether_plugin_lrclib',
  'aether_plugin_musicbrainz',
  'aether_plugin_omdb',
  'aether_plugin_opensubtitles',
  'aether_plugin_poster_rotator',
  'aether_plugin_requests',
  'aether_plugin_reports',
  'aether_plugin_tmdb',
  'aether_plugin_tvdb',
];

// CI requires all modules; release jobs explicitly select one crate.
const requested = process.argv.slice(3).map(name => name.replaceAll('-', '_'));
for (const name of requested) assert(expectedPlugins.includes(name), `Unknown plugin: ${name}`);
const plugins = requested.length ? requested : expectedPlugins;
console.log(`Verifying WebAssembly modules in: ${wasmDir}`);
assert(fs.existsSync(wasmDir), `Directory not found: ${wasmDir}`);

for (const pluginName of plugins) {
  const wasmPath = path.join(wasmDir, `${pluginName}.wasm`);
  assert(fs.existsSync(wasmPath), `Missing required WebAssembly module: ${wasmPath}`);

  const bytes = fs.readFileSync(wasmPath);
  console.log(`Checking ${pluginName}.wasm (${bytes.length} bytes)...`);

  // 1. Hard size limit
  assert(
    bytes.length <= MAX_MODULE_BYTES,
    `Plugin ${pluginName} exceeds size limit: ${bytes.length} > ${MAX_MODULE_BYTES} bytes`
  );

  // 2. WebAssembly bytecode validation
  assert(WebAssembly.validate(bytes), `Invalid WebAssembly binary: ${pluginName}`);
  const module = new WebAssembly.Module(bytes);

  // 3. ABI export verification
  const exports = WebAssembly.Module.exports(module);
  const exportNames = new Set(exports.map(e => e.name));

  assert(exportNames.has('memory'), `${pluginName} missing exported memory`);
  assert(exportNames.has('media_alloc'), `${pluginName} missing media_alloc export`);
  assert(exportNames.has('media_run'), `${pluginName} missing media_run export`);

  for (const item of WebAssembly.Module.imports(module)) {
    const allowed = (item.module === 'aether' && item.name === 'aether_host_http') ||
      (item.module === 'media' && ['media_host_storage_v1', 'media_host_objects_v1'].includes(item.name));
    assert(allowed && item.kind === 'function',
      `${pluginName}: forbidden import ${item.module}:${item.name}`);
  }
  const instance = new WebAssembly.Instance(module, {
    aether: { aether_host_http() { throw new Error('Smoke request must not access the network'); } },
    media: {
      media_host_storage_v1() { throw new Error('Unsupported operation smoke must not access storage'); },
      media_host_objects_v1() { throw new Error('Unsupported operation smoke must not access objects'); },
    },
  });
  const request = Buffer.from(JSON.stringify({apiVersion: 1, operation: 'ci.unsupported', input: {}}));
  const {memory, media_alloc, media_run} = instance.exports;
  const ptr = media_alloc(request.length) >>> 0;
  assert(ptr > 0, `${pluginName}: allocation failed`);
  new Uint8Array(memory.buffer, ptr, request.length).set(request);
  const result = media_run(ptr, request.length);
  const resultPtr = Number(result >> 32n);
  const resultLen = Number(result & 0xffffffffn);
  assert(resultLen > 0 && resultLen <= 65536, `${pluginName}: invalid response size`);
  const response = JSON.parse(Buffer.from(memory.buffer, resultPtr, resultLen).toString('utf8'));
  assert.equal(response.apiVersion, 1, `${pluginName}: invalid response API version`);
  assert.equal(response.result.error, 'unsupported_operation', `${pluginName}: invalid smoke response`);

  if (['aether_plugin_requests', 'aether_plugin_reports'].includes(pluginName)) {
    const invoke = (operation, input) => {
      const bytes = Buffer.from(JSON.stringify({apiVersion: 1, operation, input}));
      const pointer = media_alloc(bytes.length) >>> 0;
      new Uint8Array(memory.buffer, pointer, bytes.length).set(bytes);
      const encoded = media_run(pointer, bytes.length);
      const start = Number(encoded >> 32n), size = Number(encoded & 0xffffffffn);
      assert(size > 0 && size <= 65536, `${pluginName}: invalid domain response size`);
      const output = JSON.parse(Buffer.from(memory.buffer, start, size).toString('utf8'));
      assert.equal(output.apiVersion, 1);
      return output.result;
    };
    if (pluginName === 'aether_plugin_requests') {
      const film = {kind: 'film', title: 'Œuvre de test', externalId: '42', externalProvider: 'tmdb'};
      assert.deepEqual(invoke('requests.validate', film), {valid: true});
      const planned = invoke('requests.plan', film);
      assert.equal(planned.service, 'radarr');
      assert.equal(planned.title, film.title);
      assert.equal(planned.monitorFuture, false);
      assert.equal(planned.scope, 'all');
      const series = invoke('requests.plan', {kind: 'series', title: 'Série', externalId: '7', externalProvider: 'tvdb',
        seasons: [2, 1], episodes: [{season: 1, episode: 4}, {season: 3, episode: 2}]});
      assert.deepEqual(series.seasons, [1, 2]);
      assert.deepEqual(series.episodes, [{season: 3, episode: 2}]);
      assert.equal(series.scope, 'seasons_and_episodes');
      assert.equal(invoke('requests.validate', {kind: 'artist', title: 'Artiste', externalProvider: 'musicbrainz',
        externalId: '01234567-89ab-cdef-0123-456789abcdef'}).error, 'empty_selection');
      assert.deepEqual(invoke('requests.plan', {kind: 'track', title: 'Morceau', externalProvider: 'musicbrainz',
        externalId: '01234567-89ab-cdef-0123-456789abcdef'}), {error: 'track_not_supported'});
    } else {
      const report = {category: 'subtitles', description: 'Décalage après 12 secondes', workId: 'work-1',
        positionSeconds: 12, endSeconds: 20};
      assert.deepEqual(invoke('reports.validate', report), {valid: true});
      assert.deepEqual(invoke('reports.classify', report),
        {category: 'subtitles', code: 'media.subtitles', priority: 'normal',
          checks: ['subtitle_selection', 'subtitle_timing'], automationEligible: false});
      const missing = invoke('reports.classify', {category: 'missing', description: 'Ne démarre pas', workId: 'work-1'});
      assert.equal(missing.priority, 'high');
      assert.equal(missing.automationEligible, false);
      assert.equal(invoke('reports.validate', {...report, endSeconds: 1}).error, 'invalid_time_range');
    }
  }

  console.log(`  ✓ ${pluginName}: valid bytecode, exports, imports, smoke test ok (${bytes.length} octets)`);
}

console.log('WebAssembly verification complete!');
