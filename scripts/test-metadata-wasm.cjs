// Execute the real guest modules against deterministic broker responses, offline.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const root = path.resolve(__dirname, '../target/wasm32-unknown-unknown/release');

function invoke(provider, operation, input, broker) {
  const module = new WebAssembly.Module(fs.readFileSync(path.join(root, `aether_plugin_${provider}.wasm`)));
  let guest;
  const write = value => {
    const bytes = Buffer.from(JSON.stringify(value));
    const ptr = guest.exports.media_alloc(bytes.length) >>> 0;
    new Uint8Array(guest.exports.memory.buffer, ptr, bytes.length).set(bytes);
    return (BigInt(ptr) << 32n) | BigInt(bytes.length);
  };
  guest = new WebAssembly.Instance(module, { aether: { aether_host_http(ptr, len) {
    const request = JSON.parse(Buffer.from(guest.exports.memory.buffer, ptr, len).toString('utf8'));
    const body = broker(request);
    return write({ Ok: { status: 200, headers: {}, body: [...Buffer.from(typeof body === 'string' ? body : JSON.stringify(body))] } });
  } } });
  const packed = write({apiVersion: 1, operation, input});
  const result = guest.exports.media_run(Number(packed >> 32n), Number(packed & 0xffffffffn));
  return JSON.parse(Buffer.from(guest.exports.memory.buffer, Number(result >> 32n), Number(result & 0xffffffffn)).toString('utf8')).result;
}

const config = {api_key: 'synthetic', timeout_secs: '15', include_adult: true};
for (const collection of [null, {id: 7, name: 'Fixture collection'}]) {
  const result = invoke('tmdb', 'metadata.details', {id: '42', mediaType: 'movie', config}, request => {
    const url = new URL(request.url);
    assert.equal(url.pathname, '/3/movie/42');
    return {id: 42, title: 'Fixture film', belongs_to_collection: collection};
  });
  if (collection) {
    assert.deepEqual(result.collection, {id: 'tmdb:7', name: 'Fixture collection'});
  } else {
    assert.equal(result.collection, undefined, 'Legacy metadata must not acquire a made-up collection');
  }
}
for (const externalIds of [{tmdb: '42'}, {imdb: 'tt0000042'}]) {
  const requests = [];
  const result = invoke('tmdb', 'metadata.search', {title: 'Incorrect local title', year: 1900, externalIds, config}, request => {
    const url = new URL(request.url);
    requests.push(url.pathname);
    assert.equal(url.searchParams.get('api_key'), 'synthetic');
    assert.equal(request.timeoutSecs, 15);
    if (url.pathname === '/3/find/tt0000042') return {movie_results: [{id: 42}]};
    assert.equal(url.pathname, '/3/movie/42');
    return {id: 42, title: 'Titre français', release_date: '1999-01-01'};
  });
  assert.equal(result.length, 1);
  assert.equal(result[0].id, '42');
  assert.equal(result[0].title, 'Titre français');
  assert(!requests.some(uri => uri.includes('search/')));
}
let searches = 0;
const result = invoke('tmdb', 'metadata.search', {title: 'Titre', year: 2000, config}, request => {
  const url = new URL(request.url);
  assert.equal(url.searchParams.get('include_adult'), 'true');
  searches++;
  return {results: url.searchParams.has('year') ? [] : [{id: 42, title: 'Titre'}]};
});
assert.equal(searches, 2);
assert.equal(result[0].id, '42');

for (const provider of ['imdb', 'omdb']) {
  const result = invoke(provider, 'metadata.search', {title: 'Wrong title', year: 1900, externalIds: {imdb: 'tt0000042'}, config}, request => {
    const url = new URL(request.url);
    assert.equal(request.timeoutSecs, 15);
    if (provider === 'imdb') {
      assert.equal(url.pathname, '/title/tt0000042/');
      return '<script type="application/ld+json">{"@type":"Movie","name":"Titre français","datePublished":"1999-01-01"}</script>';
    }
    assert.equal(url.searchParams.get('i'), 'tt0000042');
    assert(!url.searchParams.has('s'));
    return {Title: 'Titre français', imdbID: 'tt0000042', Year: '1999', Response: 'True'};
  });
  assert.equal(result.length, 1);
  assert.equal(result[0].id, 'tt0000042');
  assert.equal(result[0].title, 'Titre français');
}
console.log('Metadata Wasm: TMDb optional collection/IDs/title fallback/settings, IMDb and OMDb ID lookup passed (offline broker fixtures).');

for (const provider of ['fanart', 'discogs', 'musicbrainz', 'lrclib']) {
  const manifest = invoke(provider, 'manifest', {}, () => { throw new Error('Manifest must not fetch HTTP'); });
  assert(manifest.configSchema.some(field => field.key === 'timeout_secs' && field.fieldType === 'number'));
  const input = provider === 'fanart'
    ? {id: '42', config: {...config, user_key: 'personal-fixture'}}
    : {title: 'Fixture album', mediaType: 'music', config};
  let calls = 0;
  const result = invoke(provider, provider === 'fanart' ? 'metadata.images' : 'metadata.search', input, request => {
    calls++;
    assert.equal(request.timeoutSecs, 15);
    if (provider === 'fanart') {
      assert.equal(request.headers['user-key'], 'personal-fixture');
      assert(manifest.configSchema.some(field => field.key === 'user_key' && field.secret && !field.required));
      return {name: 'Fixture movie', tmdb_id: '42'};
    }
    if (provider === 'discogs') return {results: []};
    if (provider === 'musicbrainz') return {releases: []};
    return [];
  });
  assert(calls > 0, `${provider}: expected HTTP request`);
  assert.deepEqual(result, []);
}
console.log('Provider settings Wasm: fanart personal key and four provider timeout schemas reach the HTTP broker.');
