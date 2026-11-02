const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),crypto=require('node:crypto');
const {spawnSync}=require('node:child_process');
const verifier=path.join(__dirname,'test-repository.cjs');
const base='https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/repository/packages/';
function fixture(change){
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'plugin-history-'));
  try{
    fs.mkdirSync(path.join(root,'packages'));
    const {privateKey,publicKey}=crypto.generateKeyPairSync('ed25519');
    const publicRaw=publicKey.export({format:'der',type:'spki'}).subarray(-32).toString('base64');
    fs.writeFileSync(path.join(root,'publisher-public.txt'),publicRaw);
    function pack(version){
      const wasm=Buffer.from('0061736d01000000','hex');
      const sha256=crypto.createHash('sha256').update(wasm).digest('hex');
      const manifest={apiVersion:1,id:'fixture',name:'Fixture',version,description:'Synthetic',author:'Tests',license:'MIT',repository:'https://example.org',permissions:['metadata.read'],checksum:sha256};
      const publisherId='tests';
      const signature=crypto.sign(null,Buffer.from(JSON.stringify(['media-extension-package-v1',manifest,publisherId,sha256])),privateKey).toString('base64');
      const pkg={manifest,publisherId,wasm:wasm.toString('base64'),sha256,signature};
      const filename=`fixture-${version}.package.json`;
      fs.writeFileSync(path.join(root,'packages',filename),JSON.stringify(pkg));
      return {filename,pkg};
    }
    const old=pack('0.1.0'),current=pack('0.1.1');
    const currentBytes=fs.readFileSync(path.join(root,'packages',current.filename));
    fs.writeFileSync(path.join(root,'manifest.json'),JSON.stringify({apiVersion:1,extensions:[{manifest:current.pkg.manifest,publisherId:'tests',publicKey:publicRaw,packageUrl:base+current.filename,packageSha256:crypto.createHash('sha256').update(currentBytes).digest('hex')}]}));
    change?.(root,old,current);
    return spawnSync(process.execPath,[verifier,root,base],{encoding:'utf8'});
  }finally{assert.equal(path.dirname(path.resolve(root)),path.resolve(os.tmpdir()));assert(path.basename(root).startsWith('plugin-history-'));fs.rmSync(root,{recursive:true,force:true});}
}
test('catalogue advances while the signed previous version stays downloadable',()=>{const r=fixture();assert.equal(r.status,0,r.stderr);assert.match(r.stdout,/1 paquets actifs, 1 anciennes versions/);});
test('a corrupted archived module is rejected',()=>{const r=fixture((root,old)=>{old.pkg.wasm=Buffer.from('corrupted').toString('base64');fs.writeFileSync(path.join(root,'packages',old.filename),JSON.stringify(old.pkg));});assert.notEqual(r.status,0);});
test('an invalid archived signature is rejected',()=>{const r=fixture((root,old)=>{old.pkg.signature=Buffer.alloc(64).toString('base64');fs.writeFileSync(path.join(root,'packages',old.filename),JSON.stringify(old.pkg));});assert.notEqual(r.status,0);});
test('a missing active package is rejected',()=>{const r=fixture((root,old,current)=>fs.unlinkSync(path.join(root,'packages',current.filename)));assert.notEqual(r.status,0);});
