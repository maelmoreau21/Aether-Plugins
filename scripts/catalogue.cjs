// Versioned plugin catalogue (repository/manifest.json, apiVersion 2) and signed packages.
//
// The catalogue lists every plugin once with all of its published versions, newest
// first, in the style of a Jellyfin plugin repository. Each version points at an
// immutable GitHub release asset (`sourceUrl`) and its SHA-256 (`checksum`). Trust
// still comes only from the Ed25519 signature inside the package.
'use strict';
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');

const API_VERSION = 2;
const MAX_MANIFEST = 1024 * 1024; // Core limit for a catalogue document.
const MAX_PACKAGE = 2 * 1024 * 1024; // Core limit for an extracted package JSON.
const MAX_MODULE = 262144; // media_extension_sdk::MAX_MODULE
const MAX_VERSIONS = 1000;
const OWNER_REPO = 'maelmoreau21/Aether-Plugins';
const PERMISSIONS = ['metadata.read', 'metadata.write', 'avatars', 'images', 'providers',
  'storage.private', 'playback.markers', 'subtitles', 'requests', 'reports'];

const VERSION = /^\d{1,8}\.\d{1,8}\.\d{1,8}$/;
const IDENTIFIER = /^[a-z0-9._-]{1,80}$/;
const SHA256 = /^[0-9a-f]{64}$/;

function compareVersions(left, right) {
  const a = left.split('.').map(Number), b = right.split('.').map(Number);
  for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] - b[i];
  return 0;
}

const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');

// Exact field order and omission rules of the Rust structs in media-extension-sdk.
// The signature covers serde's compact JSON of these structs, so a package signed
// here must serialize byte for byte like the Core re-serializes it.
const MANIFEST_FIELDS = ['apiVersion', 'id', 'name', 'version', 'description', 'author', 'license',
  'repository', 'permissions', 'httpHosts', 'category', 'targetAbi', 'minServerVersion',
  'maxServerVersion', 'checksum', 'banner', 'changelog', 'releaseDate', 'configSchema',
  'storageMigration'];
const REQUIRED = new Set(['apiVersion', 'id', 'name', 'version', 'description', 'author', 'license',
  'repository', 'permissions']);

function strict(object, allowed, what) {
  if (!object || typeof object !== 'object' || Array.isArray(object)) throw new Error(`${what} must be an object`);
  for (const key of Object.keys(object)) if (!allowed.includes(key)) throw new Error(`Unknown ${what} field: ${key}`);
}

function canonicalField(field) {
  strict(field, ['key', 'label', 'description', 'fieldType', 'defaultValue', 'required', 'secret', 'options'], 'configSchema');
  const out = { key: field.key, label: field.label, description: field.description ?? '', fieldType: field.fieldType };
  if (field.defaultValue != null) out.defaultValue = field.defaultValue;
  out.required = field.required ?? false;
  out.secret = field.secret ?? false;
  if (Array.isArray(field.options) && field.options.length) {
    out.options = field.options.map(option => {
      strict(option, ['label', 'value'], 'option');
      return { label: option.label, value: option.value };
    });
  }
  for (const key of ['key', 'label', 'description', 'fieldType']) {
    if (typeof out[key] !== 'string') throw new Error(`configSchema.${key} must be a string`);
  }
  return out;
}

/** Rebuild a manifest in serde order; unknown fields are an error, not silently dropped. */
function canonicalManifest(manifest) {
  strict(manifest, MANIFEST_FIELDS, 'manifest');
  const out = {};
  for (const key of MANIFEST_FIELDS) {
    const value = manifest[key];
    if (value == null) {
      if (REQUIRED.has(key)) throw new Error(`Missing manifest field: ${key}`);
      continue;
    }
    if (key === 'httpHosts' && Array.isArray(value) && value.length === 0) continue;
    if (key === 'configSchema') out[key] = value.map(canonicalField);
    else if (key === 'storageMigration') {
      strict(value, ['fromVersion', 'keyRenames'], 'storageMigration');
      out[key] = { fromVersion: value.fromVersion, keyRenames: value.keyRenames.map(rename => {
        strict(rename, ['from', 'to'], 'keyRenames');
        return { from: rename.from, to: rename.to };
      }) };
    } else out[key] = value;
  }
  return out;
}

const signingBytes = (manifest, publisherId, digest) =>
  Buffer.from(JSON.stringify(['media-extension-package-v1', canonicalManifest(manifest), publisherId, digest]));

function publicKeyObject(publicKey) {
  const raw = Buffer.from(publicKey, 'base64');
  if (raw.length !== 32) throw new Error('Ed25519 public key must be 32 bytes (base64)');
  return crypto.createPublicKey({ key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), raw]), format: 'der', type: 'spki' });
}

/**
 * Private key from the PKCS#8 file written by `media-extension-pack keygen` (ring,
 * RFC 5958 v2 with the public key) or a plain v1 PKCS#8 Ed25519 key.
 */
function privateKeyObject(der) {
  // ring: SEQUENCE(81) { 1, Ed25519, OCTET STRING { seed }, [1] IMPLICIT public key }.
  const ring = Buffer.from('3051020101300506032b657004220420', 'hex');
  const v1 = Buffer.from('302e020100300506032b657004220420', 'hex');
  let seed;
  if (der.length === 83 && der.subarray(0, 16).equals(ring)) seed = der.subarray(16, 48);
  else if (der.length === 48 && der.subarray(0, 16).equals(v1)) seed = der.subarray(16, 48);
  else throw new Error('Unsupported private key: expected an Ed25519 PKCS#8 key');
  return crypto.createPrivateKey({ key: Buffer.concat([v1, seed]), format: 'der', type: 'pkcs8' });
}

function rawPublicKey(privateKey) {
  return crypto.createPublicKey(privateKey).export({ format: 'der', type: 'spki' }).subarray(-32).toString('base64');
}

/** Same checks as Core `validate_package` that can be made without a Wasm engine. */
function validatePackage(pkg, publicKey, { strictText = false } = {}) {
  strict(pkg, ['manifest', 'publisherId', 'wasm', 'sha256', 'signature'], 'package');
  const m = pkg.manifest;
  const problems = [];
  if (m.apiVersion !== 1) problems.push('apiVersion must be 1');
  if (!IDENTIFIER.test(m.id) || m.id.endsWith('.')) problems.push('invalid id');
  if (!IDENTIFIER.test(pkg.publisherId)) problems.push('invalid publisherId');
  if (!VERSION.test(m.version)) problems.push('version must be x.y.z');
  if (!m.name || m.name.length > 120) problems.push('invalid name');
  if (Buffer.byteLength(m.description || '') > 2048) problems.push('description too long');
  if (!m.author || m.author.length > 120 || !m.license || m.license.length > 80) problems.push('invalid author/license');
  if (!String(m.repository).startsWith('https://') || m.repository.length > 512) problems.push('repository must be https');
  if (!Array.isArray(m.permissions) || !m.permissions.length || m.permissions.some(p => !PERMISSIONS.includes(p))) {
    problems.push(`permissions must be a non-empty subset of ${PERMISSIONS.join(', ')}`);
  }
  if ((m.httpHosts || []).length > 16) problems.push('too many httpHosts');
  const wasm = Buffer.from(pkg.wasm, 'base64');
  if (wasm.length > MAX_MODULE || !wasm.subarray(0, 4).equals(Buffer.from('0061736d', 'hex'))) problems.push('invalid or oversized Wasm module');
  if (sha256(wasm) !== pkg.sha256) problems.push('Wasm SHA-256 mismatch');
  if (m.checksum && m.checksum !== pkg.sha256) problems.push('manifest checksum differs from Wasm SHA-256');
  if (!crypto.verify(null, signingBytes(m, pkg.publisherId, pkg.sha256), publicKeyObject(publicKey), Buffer.from(pkg.signature, 'base64'))) {
    problems.push('invalid Ed25519 signature');
  }
  // `Ǹ` is how a UTF-8 `é` read as CP437/CP1252 was saved by a legacy Discogs package;
  // that signed history stays installable, but new packages must be clean.
  const mojibake = strictText ? /[├┬Ǹ]|Ã[©¨ª«¢¤®¯´¶¹»¼§ƒ]|Â[©®°·]|﻿/u : /[├┬]|Ã[©¨ª«¢¤®¯´¶¹»¼§ƒ]|Â[©®°·]/u;
  if (mojibake.test(JSON.stringify(m))) problems.push('mis-encoded text (mojibake or byte order mark)');
  if (problems.length) throw new Error(`${m.id}@${m.version}: ${problems.join('; ')}`);
}

function readCatalogue(file) {
  const bytes = fs.readFileSync(file);
  if (bytes.length > MAX_MANIFEST) throw new Error(`${file} exceeds ${MAX_MANIFEST} bytes`);
  const catalogue = JSON.parse(bytes);
  if (catalogue.apiVersion !== API_VERSION || !Array.isArray(catalogue.plugins)) {
    throw new Error(`${file}: expected {"apiVersion":2,"plugins":[...]}`);
  }
  return catalogue;
}

/** Schema checks mirroring Core `versioned_items`; returns the number of versions. */
function validateCatalogue(catalogue) {
  const ids = new Set(), seen = new Set();
  let total = 0;
  const https = (value, what) => {
    const url = new URL(value);
    if (url.protocol !== 'https:' || url.username || url.password || url.hash || url.port || value.length > 2048) {
      throw new Error(`${what} must be a plain https URL: ${value}`);
    }
  };
  for (const plugin of catalogue.plugins) {
    const where = plugin.id || '(missing id)';
    if (!IDENTIFIER.test(plugin.id || '') || ids.has(plugin.id)) throw new Error(`Invalid or duplicate plugin id: ${where}`);
    ids.add(plugin.id);
    if (!plugin.name || plugin.name.length > 120) throw new Error(`${where}: invalid name`);
    if ((plugin.description || '').length > 2048) throw new Error(`${where}: description too long`);
    if (plugin.imageUrl) https(plugin.imageUrl, `${where} imageUrl`);
    if (!Array.isArray(plugin.versions) || !plugin.versions.length) throw new Error(`${where}: versions must be a non-empty array`);
    const sorted = [...plugin.versions].sort((a, b) => compareVersions(b.version, a.version));
    if (sorted.some((entry, index) => entry !== plugin.versions[index])) throw new Error(`${where}: versions must be sorted newest first`);
    for (const entry of plugin.versions) {
      const id = `${plugin.id}@${entry.version}`;
      total++;
      if (!VERSION.test(entry.version || '') || seen.has(id)) throw new Error(`Invalid or duplicate version: ${id}`);
      seen.add(id);
      if (!SHA256.test(entry.checksum || '')) throw new Error(`${id}: checksum must be a lowercase SHA-256`);
      if (entry.checksumAlgorithm && entry.checksumAlgorithm.toLowerCase() !== 'sha256') throw new Error(`${id}: only sha256 is accepted`);
      if (entry.targetAbi && !/^\d{1,8}(\.\d{1,8}){0,3}$/.test(entry.targetAbi)) throw new Error(`${id}: invalid targetAbi`);
      for (const key of ['minServerVersion', 'maxServerVersion']) {
        if (entry[key] && !VERSION.test(entry[key])) throw new Error(`${id}: invalid ${key}`);
      }
      https(entry.sourceUrl, `${id} sourceUrl`);
      const publisherId = entry.publisherId || plugin.publisherId;
      const publicKey = entry.publicKey || plugin.publicKey;
      if (!IDENTIFIER.test(publisherId || '')) throw new Error(`${id}: missing publisherId`);
      publicKeyObject(publicKey);
      if (entry.timestamp && Number.isNaN(Date.parse(entry.timestamp))) throw new Error(`${id}: invalid timestamp`);
      if ((entry.changelog || '').length > 8192) throw new Error(`${id}: changelog too long`);
    }
  }
  if (total > MAX_VERSIONS) throw new Error(`Catalogue lists ${total} versions; the Core accepts at most ${MAX_VERSIONS}`);
  return total;
}

function serialize(catalogue) {
  const text = `${JSON.stringify(catalogue, null, 2)}\n`;
  if (Buffer.byteLength(text) > MAX_MANIFEST) throw new Error(`Catalogue would exceed ${MAX_MANIFEST} bytes`);
  return text;
}

function writeCatalogue(file, catalogue) {
  validateCatalogue(catalogue);
  const temporary = `${file}.tmp`;
  fs.writeFileSync(temporary, serialize(catalogue));
  fs.renameSync(temporary, file);
}

const ORDER = ['id', 'name', 'description', 'overview', 'owner', 'category', 'imageUrl', 'publisherId', 'publicKey', 'versions'];
const VERSION_ORDER = ['version', 'targetAbi', 'minServerVersion', 'maxServerVersion', 'sourceUrl', 'checksum',
  'timestamp', 'changelog', 'permissions', 'httpHosts', 'publisherId', 'publicKey'];
const ordered = (object, keys) => Object.fromEntries(keys.filter(key => object[key] != null &&
  !(Array.isArray(object[key]) && !object[key].length && key !== 'versions')).map(key => [key, object[key]]));

/**
 * Add one published version to the catalogue. Previous versions are kept. Adding the
 * same version with the same checksum is a no-op; a different checksum for an existing
 * version is refused, because installed servers rely on immutable release assets.
 * Returns true when the catalogue changed.
 */
function addVersion(catalogue, { pkg, publicKey, sourceUrl, checksum, timestamp, changelog, imageUrl, overview }) {
  const m = pkg.manifest;
  let plugin = catalogue.plugins.find(entry => entry.id === m.id);
  const entry = ordered({
    version: m.version, targetAbi: m.targetAbi, minServerVersion: m.minServerVersion,
    maxServerVersion: m.maxServerVersion, sourceUrl, checksum, timestamp, changelog: changelog || m.changelog,
    permissions: m.permissions, httpHosts: m.httpHosts,
  }, VERSION_ORDER);
  if (plugin) {
    const existing = plugin.versions.find(version => version.version === m.version);
    if (existing) {
      if (existing.checksum !== checksum) {
        throw new Error(`${m.id}@${m.version} is already published with checksum ${existing.checksum}; bump the version instead of replacing it`);
      }
      return false;
    }
  } else {
    plugin = { id: m.id, versions: [] };
    catalogue.plugins.push(plugin);
  }
  const newest = !plugin.versions.length || compareVersions(m.version, plugin.versions[0].version) > 0;
  if (newest && plugin.publisherId && (plugin.publisherId !== pkg.publisherId || plugin.publicKey !== publicKey)) {
    // A key rotation keeps older versions verifiable: they keep their own publisher.
    for (const version of plugin.versions) {
      version.publisherId ??= plugin.publisherId;
      version.publicKey ??= plugin.publicKey;
    }
  }
  plugin.versions.push(entry);
  plugin.versions.sort((a, b) => compareVersions(b.version, a.version));
  if (newest) {
    // Presentation follows the newest release.
    Object.assign(plugin, {
      name: m.name, description: m.description, owner: m.author, category: m.category,
      publisherId: pkg.publisherId, publicKey,
    });
    if (imageUrl) plugin.imageUrl = imageUrl;
    if (overview) plugin.overview = overview;
    for (const version of plugin.versions) {
      if (version.publisherId === plugin.publisherId && version.publicKey === plugin.publicKey) {
        delete version.publisherId; delete version.publicKey;
      }
    }
  } else if (plugin.publisherId !== pkg.publisherId || plugin.publicKey !== publicKey) {
    entry.publisherId = pkg.publisherId; entry.publicKey = publicKey;
  }
  catalogue.plugins.sort((a, b) => a.id.localeCompare(b.id));
  const index = catalogue.plugins.indexOf(plugin);
  catalogue.plugins[index] = ordered(plugin, ORDER);
  catalogue.plugins[index].versions = plugin.versions.map(version => ordered(version, VERSION_ORDER));
  return true;
}

const releaseUrl = (tag, asset, repository = OWNER_REPO) =>
  `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(asset)}`;

function readPublicKey(file) {
  return fs.readFileSync(file, 'utf8').trim();
}

/** Repair the accent mis-decoding found in legacy presentation text. */
const repairText = text => text && text.replace(/Ǹ/g, 'é');

module.exports = {
  API_VERSION, MAX_MANIFEST, MAX_PACKAGE, OWNER_REPO, PERMISSIONS, compareVersions, sha256,
  canonicalManifest, signingBytes, publicKeyObject, privateKeyObject, rawPublicKey, validatePackage,
  readCatalogue, validateCatalogue, writeCatalogue, serialize, addVersion, releaseUrl, readPublicKey, repairText,
  root: path.resolve(__dirname, '..'),
};
