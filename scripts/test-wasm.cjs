// Verification of compiled WebAssembly modules:
// - Valid WebAssembly bytecode
// - Hard size budget <= 256 KB (262,144 bytes, defined by media-extension-sdk MAX_MODULE)
// - ABI exports: memory, media_alloc, media_run
// - Host brokered imports: aether:aether_host_http (for plugins requesting network)

const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');

const wasmDir = path.resolve(process.argv[2] || path.join(__dirname, '../target/wasm32-unknown-unknown/release'));
const MAX_MODULE_BYTES = 256 * 1024; // 262,144 bytes

const expectedPlugins = [
  'aether_plugin_avatar',
  'aether_plugin_discogs',
  'aether_plugin_fanart',
  'aether_plugin_imdb',
  'aether_plugin_intro_skipper',
  'aether_plugin_lrclib',
  'aether_plugin_musicbrainz',
  'aether_plugin_omdb',
  'aether_plugin_tmdb',
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
    assert(item.module === 'aether' && item.name === 'aether_host_http' && item.kind === 'function',
      `${pluginName}: forbidden import ${item.module}:${item.name}`);
  }
  const instance = new WebAssembly.Instance(module, {
    aether: { aether_host_http() { throw new Error('Smoke request must not access the network'); } },
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

  console.log(`  ✓ ${pluginName}: valid bytecode, exports, imports, smoke test ok (${bytes.length} octets)`);
}

console.log('WebAssembly verification complete!');
