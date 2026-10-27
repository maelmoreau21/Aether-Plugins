// Offline integrity/signature verification of the exact files served by the repository URL.
const fs=require('node:fs');const path=require('node:path');const assert=require('node:assert/strict');const crypto=require('node:crypto');
const root=path.resolve(process.argv[2] || path.join(__dirname,'../repository'));
const base=process.argv[3] || 'https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/repository/packages/';
const bytes=fs.readFileSync(path.join(root,'manifest.json'));
assert(bytes.length<=256*1024);const catalogue=JSON.parse(bytes);assert.equal(catalogue.apiVersion,1);
assert(catalogue.extensions.length>0 && catalogue.extensions.length<=100);
const publicKeys=new Map([
  ['maelmoreau21',fs.readFileSync(path.join(root,'publisher-public.txt'),'utf8').trim()],
  ['poster-rotator',fs.readFileSync(path.join(root,'poster-rotator-public.txt'),'utf8').trim()],
]);
const unique=new Set();
for(const entry of catalogue.extensions){
  const identity=`${entry.manifest.id}@${entry.manifest.version}`;assert(!unique.has(identity));unique.add(identity);
  const publicKey=publicKeys.get(entry.publisherId);assert(publicKey,`Unknown publisher: ${entry.publisherId}`);
  assert.equal(entry.publicKey,publicKey);
  const spki=Buffer.concat([Buffer.from('302a300506032b6570032100','hex'),Buffer.from(publicKey,'base64')]);
  const key=crypto.createPublicKey({key:spki,format:'der',type:'spki'});
  const url=new URL(entry.packageUrl);
  assert.equal(url.protocol,'https:');assert.equal(url.username,'');assert.equal(url.password,'');assert.equal(url.hash,'');
  const filename=decodeURIComponent(path.posix.basename(url.pathname));assert(/^[a-z0-9][a-z0-9._-]*\.package\.json$/.test(filename));
  const file=path.join(root,'packages',filename);
  assert(path.basename(file).endsWith(`-${entry.manifest.version}.package.json`));
  assert.equal(url.href,new URL(encodeURIComponent(filename),base).href);
  const raw=fs.readFileSync(file);assert(raw.length<=400000);
  assert.equal(crypto.createHash('sha256').update(raw).digest('hex'),entry.packageSha256);
  const pkg=JSON.parse(raw);assert.deepEqual(pkg.manifest,entry.manifest);assert.equal(pkg.publisherId,entry.publisherId);
  const wasm=Buffer.from(pkg.wasm,'base64');assert(wasm.length<=256*1024);
  assert.equal(crypto.createHash('sha256').update(wasm).digest('hex'),pkg.sha256);
  const signed=Buffer.from(JSON.stringify(['media-extension-package-v1',pkg.manifest,pkg.publisherId,pkg.sha256]));
  assert(crypto.verify(null,signed,key,Buffer.from(pkg.signature,'base64')),`Signature invalide : ${identity}`);
  assert(Array.isArray(pkg.manifest.permissions));
  assert(!/[├┬�]|Ã[©¨ª«¢¤®¯´¶¹»¼§ƒ]|Â[©®°·]/u.test(JSON.stringify(pkg.manifest)), `Texte mal encodé : ${identity}`);
  console.log(`${identity}: signature, SHA-256, URL et limites valides (${raw.length} octets)`);
}
assert.equal(fs.readdirSync(path.join(root,'packages')).filter(name=>name.endsWith('.package.json')).length,catalogue.extensions.length);
console.log(`Catalogue valide : ${catalogue.extensions.length} paquets. Aucun secret ou clé privée requis.`);
