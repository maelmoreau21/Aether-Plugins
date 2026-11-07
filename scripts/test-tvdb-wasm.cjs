// Execute the actual guest with a deterministic broker; no account or API access is used.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const wasm = fs.readFileSync(path.resolve(__dirname, '../target/wasm32-unknown-unknown/release/aether_plugin_tvdb.wasm'));
function invoke(operation, input, broker) {
  let guest;
  const write = value => {
    const bytes = Buffer.from(JSON.stringify(value));
    const ptr = guest.exports.media_alloc(bytes.length) >>> 0;
    new Uint8Array(guest.exports.memory.buffer, ptr, bytes.length).set(bytes);
    return (BigInt(ptr) << 32n) | BigInt(bytes.length);
  };
  guest = new WebAssembly.Instance(new WebAssembly.Module(wasm), {aether: {aether_host_http(ptr, len) {
    const request = JSON.parse(Buffer.from(guest.exports.memory.buffer, ptr, len).toString('utf8'));
    assert.equal(new URL(request.url).origin, 'https://api4.thetvdb.com');
    assert(request.timeoutSecs <= 6);
    const response = broker(request);
    if (response.hostError) return write({Err: response.hostError});
    return write({Ok: {status: response.status ?? 200, headers: response.headers ?? {}, body: [...Buffer.from(response.raw ?? JSON.stringify(response.body ?? {status:'success',data:response.data}))]}});
  }}});
  const request = write({apiVersion:1, operation, input});
  const output = guest.exports.media_run(Number(request >> 32n), Number(request & 0xffffffffn));
  return JSON.parse(Buffer.from(guest.exports.memory.buffer, Number(output >> 32n), Number(output & 0xffffffffn)).toString('utf8')).result;
}
const config = {api_key:'fixture-project-key', access_model:'licensed', fallback_language:'en'};
const noNetwork = () => { throw new Error('Unexpected HTTP call'); };
const error = code => ({error:code,message:'TheTVDB request could not be completed'});
function login(request, pin) {
  assert.equal(request.url, 'https://api4.thetvdb.com/v4/login');
  assert.equal(request.method,'POST');
  assert.equal(request.headers.Authorization,undefined);
  assert.deepEqual(JSON.parse(Buffer.from(request.body).toString()), pin ? {apikey:config.api_key,pin} : {apikey:config.api_key});
  return {data:{token:'fixture-private-token'}};
}
const manifest=invoke('manifest',{},noNetwork);
assert.deepEqual(manifest.httpHosts,['api4.thetvdb.com','artworks.thetvdb.com']);
assert(manifest.configSchema.some(field=>field.key==='api_key' && field.secret));
assert(manifest.configSchema.some(field=>field.key==='subscriber_pin' && field.secret));
assert.deepEqual(invoke('metadata.details',{id:'42',mediaType:'movie',config:{}},noNetwork),error('missing_api_key'));
assert.deepEqual(invoke('metadata.details',{id:'42',mediaType:'movie',config:{...config,access_model:'user_supported'}},noNetwork),error('missing_subscriber_pin'));
assert.deepEqual(invoke('metadata.search',{title:'Pilot',mediaType:'episode',config},noNetwork),error('episode_identity_required'));

let calls=[];
const localized=invoke('metadata.search',{title:'Wrong title',mediaType:'tv_show',externalIds:{tvdb:'42',imdb:'tt0042'},language:'fr-FR',config},request=>{
  const url=new URL(request.url); calls.push(url.pathname);
  if(url.pathname.endsWith('/login')) return login(request);
  assert.equal(request.headers.Authorization,'Bearer fixture-private-token');
  if(url.pathname==='/v4/series/42/extended') return {data:{id:42,name:'Base title',overview:'Base overview',score:90000,remoteIds:[{sourceName:'IMDB',id:'tt0042'}]}};
  if(url.pathname==='/v4/series/42/translations/fra') return {data:{name:'Titre français'}};
  if(url.pathname==='/v4/series/42/translations/eng') return {data:{name:'English title',overview:'Fallback overview'}};
  throw new Error(`Unexpected endpoint ${url.pathname}`);
});
assert.equal(localized.length,1);
assert.equal(localized[0].id,'42');
assert.equal(localized[0].title,'Titre français');
assert.equal(localized[0].overview,'Fallback overview');
assert(!calls.some(url=>url.includes('/search')),'TVDB identity must precede fuzzy search and IMDb lookup');
assert(!JSON.stringify(localized).includes('fixture-private-token'));

calls=[];
const episode=invoke('metadata.details',{id:'',externalIds:{imdb:'tt0042'},mediaType:'episode',language:'fr',config:{...config,access_model:'user_supported',subscriber_pin:'fixture-personal-pin'}},request=>{
  const url=new URL(request.url);calls.push(url.pathname);
  if(url.pathname.endsWith('/login')) return login(request,'fixture-personal-pin');
  if(url.pathname==='/v4/search/remoteid/tt0042') return {data:[{episode:{id:42}},{movie:{id:999}}]};
  if(url.pathname==='/v4/episodes/42/extended') return {data:{id:42,name:'Episode',aired:'2026-01-02',remoteIds:[{sourceName:'IMDB',id:'tt0042'}]}};
  if(url.pathname.endsWith('/translations/fra')) return {status:404,body:{message:'sensitive'}};
  if(url.pathname.endsWith('/translations/eng')) return {data:{name:'Episode',overview:'English fallback'}};
  throw new Error(`Unexpected endpoint ${url.pathname}`);
});
assert.equal(episode.provider,'tvdb');assert.equal(episode.providerId,'42');assert.equal(episode.externalIds.imdb,'tt0042');
assert.equal(episode.synopsis,'English fallback');assert.equal(episode.year,2026);assert.equal(episode.originalTitle,undefined);
assert(!JSON.stringify(episode).includes('fixture-personal-pin'));

const search=invoke('metadata.search',{title:'A & B',year:2026,mediaType:'movie',language:'fr',config},request=>{
  const url=new URL(request.url);
  if(url.pathname.endsWith('/login')) return login(request);
  assert.equal(url.pathname,'/v4/search');assert.equal(url.searchParams.get('query'),'A & B');assert.equal(url.searchParams.get('type'),'movie');
  return {data:[{tvdb_id:'42',type:'movie',name:'Original',translations:{fra:'Titre'},overviews:{fra:'Résumé'}},{tvdb_id:'42',type:'movie',name:'Duplicate'},{tvdb_id:'99',type:'series',name:'Wrong kind'}]};
});
assert.equal(search.length,1);assert.equal(search[0].title,'Titre');
const bounded=invoke('metadata.search',{title:'Unicode',mediaType:'movie',config},request=>request.url.endsWith('/login')?login(request):{data:Array.from({length:20},(_,i)=>({tvdb_id:String(i+1),type:'movie',name:'🎬'.repeat(128),overview:'🌍'.repeat(1000)}))});
assert(bounded.length>0 && bounded.length<20);
assert(Buffer.byteLength(JSON.stringify(bounded))<=60*1024);

const images=invoke('metadata.images',{id:'42',mediaType:'movie',language:'fr-FR',config},request=>{
  const url=new URL(request.url);
  if(url.pathname.endsWith('/login')) return login(request);
  if(url.pathname==='/v4/movies/42/extended') return {data:{id:42,artworks:[{type:9,image:'https://artworks.thetvdb.com/poster.jpg',language:'fra'},{type:10,image:'/banners/backdrop.jpg'},{type:9,image:'https://evil.example/secret'}]}};
  if(url.pathname==='/v4/artwork/types') return {data:[{id:9,name:'Movie Poster'},{id:10,name:'Movie Background'}]};
  throw new Error(`Unexpected endpoint ${url.pathname}`);
});
assert.equal(images.length,2);assert.equal(images[0].imageType,'poster');assert.equal(images[1].imageType,'backdrop');
for(const [status,code] of [[401,'unauthorized'],[403,'forbidden'],[404,'not_found'],[408,'timeout'],[429,'rate_limited'],[500,'provider_error'],[503,'provider_unavailable'],[504,'provider_unavailable'],[302,'provider_redirect_refused']]) {
  assert.deepEqual(invoke('metadata.details',{id:'42',mediaType:'movie',config},()=>({status,body:{message:'secret-url?key=private'}})),error(code));
}
// A provider throttle: exactly one request reaches the broker (no retry), and the plugin
// reports rate_limited whatever `Retry-After` says (the Core owns the cooldown).
for (const status of [429, 503]) {
  let requests = 0;
  const throttled = invoke('metadata.details',{id:'42',mediaType:'movie',config},request=>{requests++;return {status,headers:{'retry-after':'120'},body:{}};});
  assert.equal(requests, 1, `HTTP ${status} must not be retried`);
  assert.equal(throttled.error, status === 429 ? 'rate_limited' : 'provider_unavailable');
}
// Broker refusals keep their code, lose the message (it can embed the request URL) and a
// DNS timeout reads as a timeout.
for (const [brokerCode, code] of [['quota_exceeded','quota_exceeded'],['response_too_large','provider_response_too_large'],['dns_timeout','timeout'],['provider_rate_limited','rate_limited'],['network_error','network_error']]) {
  const refused = invoke('metadata.details',{id:'42',mediaType:'movie',config},()=>({hostError:{code:brokerCode,message:'error sending request for url (https://api4.thetvdb.com/v4/login?apikey=private)'}}));
  assert.deepEqual(refused,error(code));
}
// Truncated provider documents are told apart from malformed ones.
assert.deepEqual(invoke('metadata.details',{id:'42',mediaType:'movie',config},request=>request.url.endsWith('/login')?login(request):{raw:'{"status":"succ'}),error('truncated_response'));
assert.deepEqual(invoke('metadata.details',{id:'42',mediaType:'movie',config},request=>request.url.endsWith('/login')?login(request):{raw:'<html>maintenance</html>'}),error('invalid_provider_response'));
assert.deepEqual(invoke('metadata.search',{title:'Ignored',externalIds:{imdb:'tt0042'},mediaType:'movie',config},request=>request.url.endsWith('/login')?login(request):{data:[{movie:{id:42}},{movie:{id:43}}]}),error('ambiguous_match'));
assert.deepEqual(invoke('metadata.details',{id:'42',externalIds:{imdb:'tt0042'},mediaType:'movie',config},request=>request.url.endsWith('/login')?login(request):{data:{id:42,remoteIds:[{sourceName:'IMDB',id:'tt9999'}]}}),error('identity_mismatch'));
console.log('TVDB guest passed offline: access models, identity priority/type, translations/fallback, images, ambiguity, auth/rate/partial errors and secret-free output.');
