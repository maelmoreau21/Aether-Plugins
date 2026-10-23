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
  'aether_plugin_lrclib',
  'aether_plugin_musicbrainz',
  'aether_plugin_omdb',
  'aether_plugin_tmdb',
];

console.log(`Verifying WebAssembly modules in: ${wasmDir}`);
assert(fs.existsSync(wasmDir), `Directory not found: ${wasmDir}`);

for (const pluginName of expectedPlugins) {
  const wasmPath = path.join(wasmDir, `${pluginName}.wasm`);
  assert(fs.existsSync(wasmPath), `Missing wasm binary for: ${pluginName}`);

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

  // 4. Host capability imports verification
  const imports = WebAssembly.Module.imports(module);
  const importsMap = new Map();
  for (const imp of imports) {
    if (!importsMap.has(imp.module)) importsMap.set(imp.module, []);
    importsMap.get(imp.module).push(imp.name);
  }

  // Network plugins must import host broker HTTP function
  if (pluginName !== 'aether_plugin_avatar') {
    const aetherImports = importsMap.get('aether') || [];
    assert(
      aetherImports.includes('aether_host_http'),
      `${pluginName} must import aether:aether_host_http for brokered network requests`
    );
  }

  // 5. Instantiation smoke test with mock host functions
  let allocOffset = 1024;
  const mockMemory = new WebAssembly.Memory({ initial: 2, maximum: 16 });
  const mockImports = {
    aether: {
      aether_host_http: () => 0,
    },
    env: {
      memory: mockMemory,
    },
  };

  try {
    const instance = new WebAssembly.Instance(module, mockImports);
    assert(typeof instance.exports.media_alloc === 'function');
    assert(typeof instance.exports.media_run === 'function');

    // Test media_alloc
    const ptr = instance.exports.media_alloc(64);
    assert(typeof ptr === 'number' && ptr > 0, `media_alloc returned invalid pointer: ${ptr}`);
  } catch (err) {
    assert.fail(`Failed to instantiate ${pluginName}: ${err.message}`);
  }

  console.log(`  ✓ ${pluginName}: valid bytecode, exports, imports, smoke test ok (${bytes.length} octets)`);
}

console.log(`All ${expectedPlugins.length} WebAssembly plugins successfully verified!`);
