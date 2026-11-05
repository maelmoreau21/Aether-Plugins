#!/usr/bin/env node
// Build and sign an Aether extension package without compiling Aether Core.
//
//   node scripts/sign-package.cjs MANIFEST.json MODULE.wasm KEY.pk8 PUBLISHER_ID PUBLIC_KEY.txt OUTPUT.package.json
//
// KEY.pk8 is the binary PKCS#8 key written by `media-extension-pack keygen`. The
// package is verified against PUBLIC_KEY.txt before it is written, so a wrong
// signing secret fails here instead of at installation.
'use strict';
const fs = require('node:fs');
const crypto = require('node:crypto');
const c = require('./catalogue.cjs');

function sign({ manifest, wasm, privateKey, publisherId }) {
  const digest = c.sha256(wasm);
  const canonical = c.canonicalManifest(manifest);
  const signature = crypto.sign(null, c.signingBytes(canonical, publisherId, digest), privateKey).toString('base64');
  return { manifest: canonical, publisherId, wasm: wasm.toString('base64'), sha256: digest, signature };
}

function main() {
  const args = process.argv.slice(2);
  if (args.length !== 6) {
    throw new Error('Usage: sign-package.cjs MANIFEST.json MODULE.wasm KEY.pk8 PUBLISHER_ID PUBLIC_KEY.txt OUTPUT.package.json');
  }
  const [manifestFile, wasmFile, keyFile, publisherId, publicKeyFile, output] = args;
  const privateKey = c.privateKeyObject(fs.readFileSync(keyFile));
  const publicKey = c.readPublicKey(publicKeyFile);
  if (c.rawPublicKey(privateKey) !== publicKey) {
    throw new Error(`The signing key does not match ${publicKeyFile}. Check the signing secret.`);
  }
  const pkg = sign({
    manifest: JSON.parse(fs.readFileSync(manifestFile, 'utf8')),
    wasm: fs.readFileSync(wasmFile), privateKey, publisherId,
  });
  c.validatePackage(pkg, publicKey, { strictText: true });
  const bytes = Buffer.from(JSON.stringify(pkg, null, 2));
  if (bytes.length > c.MAX_PACKAGE) throw new Error('Package exceeds the Core size limit');
  // Never overwrite: a published version is immutable.
  fs.writeFileSync(output, bytes, { flag: 'wx' });
  console.log(`${pkg.manifest.id}@${pkg.manifest.version} signed (${bytes.length} bytes, sha256 ${c.sha256(bytes)})`);
}

if (require.main === module) {
  try { main(); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
module.exports = { sign };
