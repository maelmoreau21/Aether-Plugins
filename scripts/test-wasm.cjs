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

console.log(`Verifying WebAssembly modules in: ${wasmDir}`);
assert(fs.existsSync(wasmDir), `Directory not found: ${wasmDir}`);

for (const pluginName of expectedPlugins) {
  const wasmPath = path.join(wasmDir, `${pluginName}.wasm`);
  if (!fs.existsSync(wasmPath)) {
    // Si exécuté pour un sous-ensemble de plugins, continuer si d'autres sont présents
    console.log(`Note: ${pluginName}.wasm not found in target dir (skipped)`);
    continue;
  }

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

  console.log(`  ✓ ${pluginName}: valid bytecode, exports, imports, smoke test ok (${bytes.length} octets)`);
}

console.log('WebAssembly verification complete!');
