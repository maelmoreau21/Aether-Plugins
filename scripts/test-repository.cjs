// Offline integrity/signature verification of the exact files served by the repository URL.
const fs=require('node:fs');const path=require('node:path');const assert=require('node:assert/strict');const crypto=require('node:crypto');
const args=process.argv.slice(2).filter(a=>a!=='--allow-development');
const allowDevelopment=process.argv.includes('--allow-development');
const root=path.resolve(args[0] || path.join(__dirname,'../repository'));
const base=args[1] || 'https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/repository/packages/';
const bytes=fs.readFileSync(path.join(root,'manifest.json'));
assert(bytes.length<=256*1024);const catalogue=JSON.parse(bytes);assert.equal(catalogue.apiVersion,1);
assert(catalogue.extensions.length>0 && catalogue.extensions.length<=100);

const defaultPublicKeyPath=path.join(root,'publisher-public.txt');
const defaultPublicKey=fs.existsSync(defaultPublicKeyPath)?fs.readFileSync(defaultPublicKeyPath,'utf8').trim():'';
const publicKeys=new Map();
if(defaultPublicKey)publicKeys.set('maelmoreau21',defaultPublicKey);
const posterKeyPath=path.join(root,'poster-rotator-public.txt');
if(fs.existsSync(posterKeyPath))publicKeys.set('poster-rotator',fs.readFileSync(posterKeyPath,'utf8').trim());

function getVerifierKey(publisherId){
  const pk=publicKeys.get(publisherId)||defaultPublicKey;
  assert(pk,`Unknown publisher or missing public key: ${publisherId}`);
  const spki=Buffer.concat([Buffer.from('302a300506032b6570032100','hex'),Buffer.from(pk,'base64')]);
  return {publicKeyString:pk,key:crypto.createPublicKey({key:spki,format:'der',type:'spki'})};
}

const unique=new Set();
const referencedFiles=new Set();
for(const entry of catalogue.extensions){
  const identity=`${entry.manifest.id}@${entry.manifest.version}`;assert(!unique.has(identity));unique.add(identity);
  const {publicKeyString,key}=getVerifierKey(entry.publisherId);
  assert.equal(entry.publicKey,publicKeyString);
  const url=new URL(entry.packageUrl);
  assert.equal(url.protocol,'https:');assert.equal(url.username,'');assert.equal(url.password,'');assert.equal(url.hash,'');
  const filename=decodeURIComponent(path.posix.basename(url.pathname));assert(/^[a-z0-9][a-z0-9._-]*\.package\.json$/.test(filename));
  referencedFiles.add(filename);
  const file=path.join(root,'packages',filename);
  assert(path.basename(file).endsWith(`-${entry.manifest.version}.package.json`));
  const allowed=[new URL(encodeURIComponent(filename),base).href];
  if(allowDevelopment)allowed.push(new URL(encodeURIComponent(filename),base.replace('/main/repository/packages/','/develop/repository/packages/')).href);
  assert(allowed.includes(url.href),'Unexpected package URL');
  const raw=fs.readFileSync(file);assert(raw.length<=400000);
  assert.equal(crypto.createHash('sha256').update(raw).digest('hex'),entry.packageSha256);
  const pkg=JSON.parse(raw);assert.deepEqual(pkg.manifest,entry.manifest);assert.equal(pkg.publisherId,entry.publisherId);
  const wasm=Buffer.from(pkg.wasm,'base64');assert(wasm.length<=256*1024);
  assert.equal(crypto.createHash('sha256').update(wasm).digest('hex'),pkg.sha256);
  const signed=Buffer.from(JSON.stringify(['media-extension-package-v1',pkg.manifest,pkg.publisherId,pkg.sha256]));
  assert(crypto.verify(null,signed,key,Buffer.from(pkg.signature,'base64')),`Signature invalide : ${identity}`);
  assert(Array.isArray(pkg.manifest.permissions));
  assert(!/[├┬]|Ã[©¨ª«¢¤®¯´¶¹»¼§ƒ]|Â[©®°·]/u.test(JSON.stringify(pkg.manifest)), `Texte mal encodé : ${identity}`);
  console.log(`${identity}: signature, SHA-256, URL et limites valides (${raw.length} octets)`);
}
// Published packages are immutable: old versions remain downloadable after the
// catalogue advances. Verify their signatures too, without requiring catalogue entries.
const archivedFiles=fs.readdirSync(path.join(root,'packages')).filter(name=>name.endsWith('.package.json')&&!referencedFiles.has(name));
for(const filename of archivedFiles){
  assert(/^[a-z0-9][a-z0-9._-]*\.package\.json$/.test(filename));
  const raw=fs.readFileSync(path.join(root,'packages',filename));assert(raw.length<=400000);
  const pkg=JSON.parse(raw);
  const identity=`${pkg.manifest.id}@${pkg.manifest.version}`;
  assert(!unique.has(identity),`Duplicate signed identity: ${identity}`);unique.add(identity);
  assert(filename.endsWith(`-${pkg.manifest.version}.package.json`));
  const wasm=Buffer.from(pkg.wasm,'base64');assert(wasm.length<=256*1024);
  assert.equal(crypto.createHash('sha256').update(wasm).digest('hex'),pkg.sha256);
  if(pkg.manifest.checksum)assert.equal(pkg.manifest.checksum,pkg.sha256);
  const signed=Buffer.from(JSON.stringify(['media-extension-package-v1',pkg.manifest,pkg.publisherId,pkg.sha256]));
  const {key}=getVerifierKey(pkg.publisherId);
  assert(crypto.verify(null,signed,key,Buffer.from(pkg.signature,'base64')),`Signature invalide : ${identity}`);
  assert(Array.isArray(pkg.manifest.permissions));
  assert(!/[├┬]|Ã[©¨ª«¢¤®¯´¶¹»¼§ƒ]|Â[©®°·]/u.test(JSON.stringify(pkg.manifest)), `Texte mal encodé : ${identity}`);
  console.log(`${identity}: ancienne version, signature et SHA-256 valides`);
}
console.log(`Catalogue valide : ${catalogue.extensions.length} paquets actifs, ${archivedFiles.length} anciennes versions conservees et verifiees.`);
