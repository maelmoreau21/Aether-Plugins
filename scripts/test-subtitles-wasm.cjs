// Real guest ABI, deterministic HTTP broker: never contacts OpenSubtitles.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const wasm = fs.readFileSync(path.resolve(__dirname, '../target/wasm32-unknown-unknown/release/aether_plugin_opensubtitles.wasm'));
function invoke(operation, input, broker) {
  let guest;
  const write = value => {
    const data = Buffer.from(JSON.stringify(value));
    const ptr = guest.exports.media_alloc(data.length) >>> 0;
    new Uint8Array(guest.exports.memory.buffer, ptr, data.length).set(data);
    return (BigInt(ptr) << 32n) | BigInt(data.length);
  };
  guest = new WebAssembly.Instance(new WebAssembly.Module(wasm), {aether: {aether_host_http(ptr, len) {
    const request = JSON.parse(Buffer.from(guest.exports.memory.buffer, ptr, len).toString('utf8'));
    const response = broker(request);
    return write({Ok: {status: response.status ?? 200, headers: {}, body: [...Buffer.from(JSON.stringify(response.body))]}});
  }}});
  const packed = write({apiVersion: 1, operation, input});
  const output = guest.exports.media_run(Number(packed >> 32n), Number(packed & 0xffffffffn));
  return JSON.parse(Buffer.from(guest.exports.memory.buffer, Number(output >> 32n), Number(output & 0xffffffffn)).toString('utf8')).result;
}
const config = {api_key: 'fixture-api-key', username: 'fixture-user', password: 'fixture-password'};
let calls = 0;
const result = invoke('subtitles.search', {mediaType: 'movie', title: 'Wrong title', externalIds: {imdb: 'tt0133093'}, language: 'fr', forced: true, config}, request => {
  calls++;
  assert.equal(request.headers['Api-Key'], config.api_key);
  assert.match(request.headers['User-Agent'], /v0\.1\.0/);
  if (calls === 1) {
    assert.equal(request.url, 'https://api.opensubtitles.com/api/v1/login');
    assert.equal(request.method, 'POST');
    assert.deepEqual(JSON.parse(Buffer.from(request.body).toString()), {username: config.username, password: config.password});
    return {body: {token: 'fixture-token', base_url: 'vip-api.opensubtitles.com'}};
  }
  assert.match(request.url, /^https:\/\/vip-api\.opensubtitles\.com\/api\/v1\/subtitles\?/);
  assert.equal(request.headers.Authorization, 'Bearer fixture-token');
  assert.equal(new URL(request.url).searchParams.get('imdb_id'), '133093');
  return {body: {data: [{attributes: {language: 'fr', foreign_parts_only: true, hearing_impaired: true, release: 'Fixture', files: [{file_id: 42}]}}], total_pages: 1}};
});
assert.equal(calls, 2);
assert.equal(result.items[0].id, '42');
assert.equal(result.items[0].forced, true);
assert.equal(result.items[0].sdh, true);
assert.equal(result.items[0].match, 'identity');
assert(!JSON.stringify(result).includes('fixture-token'));
const download = invoke('subtitles.download', {fileId: '42', config: {api_key: 'fixture-api-key'}}, request => {
  assert.equal(request.url, 'https://api.opensubtitles.com/api/v1/download');
  assert.equal(request.method, 'POST');
  assert.deepEqual(JSON.parse(Buffer.from(request.body).toString()), {file_id: 42, sub_format: 'srt'});
  return {body: {link: 'https://dl.opensubtitles.com/download/fixture', remaining: 0, reset_time_utc: '2026-11-04T00:00:00Z'}};
});
assert.equal(download.remaining, 0);
for (const [status, code] of [[401, 'unauthorized'], [403, 'unauthorized'], [404, 'not_found'], [400, 'invalid_request'], [406, 'download_quota_exceeded'], [408, 'timeout'], [429, 'rate_limited'], [503, 'rate_limited'], [502, 'provider_unavailable'], [302, 'provider_redirect_refused'], [500, 'provider_error']]) {
  const output = invoke('subtitles.download', {fileId: '42', config: {api_key: 'fixture-api-key'}}, () => ({status, body: {message: 'provider-secret'}}));
  assert.deepEqual(output, {error: code});
}
// Official hash + series identifiers + regional variant, through the real guest.
let seen;
const hashed = invoke('subtitles.search', {mediaType: 'episode', title: 'Series', externalIds: {imdb: ' '}, parentExternalIds: {imdb: 'tt0903747'}, seasonNumber: 2, episodeNumber: 5, movieHash: '8E245D9679D31E12', language: 'pt-BR', sdh: false, config: {api_key: 'fixture-api-key'}}, request => {
  seen = new URL(request.url);
  return {body: {total_pages: 1, data: [
    {attributes: {language: 'pt-PT', release: 'European', files: [{file_id: 1}]}},
    {attributes: {language: 'pt-BR', release: 'Other cut', files: [{file_id: 2}]}},
    {attributes: {language: 'pt-BR', release: 'Exact cut', moviehash_match: true, files: [{file_id: 3}]}},
    {attributes: {language: 'pt-BR', release: 'For the deaf', hearing_impaired: true, files: [{file_id: 4}]}},
  ]}};
});
assert.equal(seen.searchParams.get('moviehash'), '8e245d9679d31e12');
assert.equal(seen.searchParams.get('languages'), 'pt-br');
assert.equal(seen.searchParams.get('parent_imdb_id'), '903747');
assert.equal(seen.searchParams.get('season_number'), '2');
assert.equal(seen.searchParams.get('episode_number'), '5');
assert.equal(seen.searchParams.get('hearing_impaired'), 'exclude');
assert.equal(seen.searchParams.has('query'), false);
assert.deepEqual(hashed.items.map(i => i.id), ['3', '2']);
assert.equal(hashed.items[0].hashMatch, true);
assert.equal(hashed.items[0].match, 'identity');
// Aether's own fingerprint never reaches the provider.
assert.deepEqual(invoke('subtitles.search', {mediaType: 'movie', title: 'x', language: 'fr', movieHash: 'b3s1:1048576:0123456789abcdef0123456789abcdef', config: {api_key: 'fixture-api-key'}}, () => {throw new Error('invalid hash reached network');}), {error: 'invalid_movie_hash'});
// A refused login is reported as such, costs exactly one request and is not retried.
let loginCalls = 0;
assert.deepEqual(invoke('subtitles.search', {mediaType: 'movie', title: 'x', language: 'fr', config}, () => {loginCalls++; return {status: 401, body: {message: 'provider-secret'}};}), {error: 'invalid_credentials'});
assert.equal(loginCalls, 1);
assert.deepEqual(invoke('subtitles.download', {fileId: '../bad', config}, () => {throw new Error('invalid input reached network');}), {error: 'invalid_file_id'});
assert.deepEqual(invoke('subtitles.download', {fileId: '42', config: {api_key: 'fixture-api-key'}}, () => ({body: {link: 'https://localhost/private'}})), {error: 'download_host_denied'});
console.log('OpenSubtitles guest: search/login/VIP/download/quotas/errors/SSRF contract passed offline.');
