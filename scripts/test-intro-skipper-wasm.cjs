// Drives the real IntroSkipper Wasm guest with real Chromaprint fingerprints (the corpus under
// Aether-Plugins-IntroSkipper/tests/corpus/intro) the way the Core does: one JSON request of the
// shape built by `crates/core/src/extensions/automated.rs`, base64 little-endian words.
//
//   node scripts/test-intro-skipper-wasm.cjs [path/to/aether_plugin_intro_skipper.wasm]
//
// Checks the ABI limits the Core enforces (request and response under MAX_JSON), the markers
// against the ground truth, and prints the guest memory and the time taken. V8 is not wasmi (the
// Core's interpreter): the time is an upper bound on the work, not the Core's latency.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');

const MAX_JSON = 65536; // media_extension_sdk::MAX_JSON: requests to an ABI 1.0 package
const MAX_INPUT_JSON = 262144; // media_extension_sdk::MAX_INPUT_JSON: requests to ABI 1.1
const wasmPath = process.argv[2] ||
  path.join(__dirname, '../target/wasm32-unknown-unknown/release/aether_plugin_intro_skipper.wasm');
const corpus = path.join(__dirname, '../Aether-Plugins-IntroSkipper/tests/corpus/intro');
const manifest = JSON.parse(fs.readFileSync(path.join(corpus, 'manifest.json'), 'utf8'));

const bytes = fs.readFileSync(wasmPath);
assert(bytes.length <= 1048576, `module too large: ${bytes.length}`);
const instance = new WebAssembly.Instance(new WebAssembly.Module(bytes), {
  aether: { aether_host_http() { throw new Error('no network'); } },
  media: {
    media_host_storage_v1() { throw new Error('no storage'); },
    media_host_objects_v1() { throw new Error('no objects'); },
  },
});
const { memory, media_alloc, media_run } = instance.exports;

function invoke(operation, input) {
  const request = Buffer.from(JSON.stringify({ apiVersion: 1, operation, input }));
  const size = request.length;
  const started = process.hrtime.bigint();
  const ptr = media_alloc(size) >>> 0;
  new Uint8Array(memory.buffer, ptr, size).set(request);
  const packed = media_run(ptr, size);
  const ms = Number(process.hrtime.bigint() - started) / 1e6;
  const at = Number(packed >> 32n);
  const len = Number(packed & 0xffffffffn);
  const response = JSON.parse(Buffer.from(memory.buffer, at, len).toString('utf8'));
  return { result: response.result, requestBytes: size, responseBytes: len, ms, memoryBytes: memory.buffer.byteLength };
}

function episodes(scenario, count, rate) {
  return scenario.episodes.slice(0, count).map((ep, i) => {
    const out = {
      id: ep.id,
      seriesId: 'series-corpus',
      seasonNumber: 1,
      episodeNumber: i + 1,
      duration: manifest.duration,
      fingerprintsCompact: fs.readFileSync(path.join(corpus, ep.head)).toString('base64'),
      fingerprintRate: rate,
    };
    if (ep.tail) {
      out.tailFingerprintsCompact = fs.readFileSync(path.join(corpus, ep.tail)).toString('base64');
      out.tailStart = manifest.duration - manifest.tailSeconds;
    }
    return out;
  });
}

const near = (found, truth, tolerance = 3) =>
  found && Math.abs(found.start - truth.start) <= tolerance && Math.abs(found.end - truth.end) <= tolerance;

const standard = manifest.scenarios.find((s) => s.name === 'standard');
const cold = manifest.scenarios.find((s) => s.name === 'cold_open');
const none = manifest.scenarios.find((s) => s.name === 'no_intro');

// 1. Three episodes: the most the 64 KiB request allows with a full head and tail each.
let run = invoke('intro.detect', { episodes: episodes(standard, 3, manifest.legacyRate) });
assert(run.requestBytes <= MAX_JSON, `request ${run.requestBytes}`);
assert(run.responseBytes <= MAX_JSON);
assert.equal(run.result.length, 3);
run.result.forEach((r, i) => {
  assert(near(r.intro, standard.episodes[i].intro), `intro ${JSON.stringify(r.intro)}`);
  assert(near(r.credits, standard.episodes[i].credits), `credits ${JSON.stringify(r.credits)}`);
});
console.log(`standard x3: request ${run.requestBytes} B, response ${run.responseBytes} B, ${run.ms.toFixed(1)} ms, guest memory ${(run.memoryBytes / 1048576).toFixed(1)} MiB`);

// 2. The legacy rate (older Node) and the real one give the same markers.
const real = invoke('intro.detect', { episodes: episodes(standard, 3, manifest.sampleRate) });
assert.deepEqual(real.result, run.result);

// 3. Cold open at different offsets, no tail.
run = invoke('intro.detect', { episodes: episodes(cold, 3, manifest.sampleRate) });
run.result.forEach((r, i) => assert(near(r.intro, cold.episodes[i].intro), `cold ${i}`));
console.log(`cold_open x3: request ${run.requestBytes} B, ${run.ms.toFixed(1)} ms`);

// 4. No opening: nothing is invented.
run = invoke('intro.detect', { episodes: episodes(none, 3, manifest.sampleRate) });
run.result.forEach((r) => assert.equal(r.intro, undefined));

// 5. Four episodes with full tails exceed what an ABI 1.0 package takes, not what 1.1 takes.
const four = JSON.stringify({ apiVersion: 1, operation: 'intro.detect', input: { episodes: episodes(standard, 4, manifest.sampleRate) } });
assert(four.length > MAX_JSON && four.length <= MAX_INPUT_JSON, `four episodes are ${four.length} B`);
console.log(`standard x4: ${four.length} B, over ${MAX_JSON} (ABI 1.0) and within ${MAX_INPUT_JSON} (ABI 1.1)`);

console.log('IntroSkipper Wasm corpus check complete');
