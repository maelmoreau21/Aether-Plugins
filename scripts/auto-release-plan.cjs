#!/usr/bin/env node
// Decide which plugins a push must publish, without any manual version bump.
//
//   BRANCH=develop node scripts/auto-release-plan.cjs
//
// A plugin is published when its sources (its folder or the shared crates) differ
// from the commit of its latest version in this branch's catalogue. Documentation-only
// changes are ignored. The next version is the patch after the highest version ever
// published for the plugin (git tags of both branches included, so develop and main
// never reuse a number), or the Cargo.toml version when it is higher (manual
// minor/major bump).
//
// The Core accepts numeric x.y.z plugin versions only, so a beta is a channel, not a
// suffix: develop publishes GitHub pre-releases (`<prefix>-v0.1.5-beta`) into the
// develop catalogue; merging develop into main brings those catalogue entries along
// and the workflow marks their releases stable (`promote`).
//
// Prints {"release":[{plugin,version}],"promote":[tag]} (and appends `plan=` to
// $GITHUB_OUTPUT when set).
'use strict';
const fs = require('node:fs');
const { execFileSync } = require('node:child_process');
const { plugins } = require('./release-plan.cjs');
const c = require('./catalogue.cjs');

const SHARED = ['crates'];
const git = args => execFileSync('git', args, { cwd: c.root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
const tryGit = args => { try { return git(args); } catch { return null; } };
const parse = v => v.split('.').map(Number);
const compare = (a, b) => { const x = parse(a), y = parse(b); for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] - y[i]; return 0; };
const docOnly = file => /\.(md|txt)$/i.test(file) || file.split('/').includes('docs');

function tagOf(entry) {
  const parts = new URL(entry.sourceUrl).pathname.split('/');
  const index = parts.indexOf('download');
  return index > 0 ? decodeURIComponent(parts[index + 1]) : null;
}

/** Commit holding the sources of a published version. */
function sourceCommit(plugin, entry) {
  const tag = tagOf(entry);
  const tagged = tag && tryGit(['rev-list', '-n', '1', tag]);
  if (tagged) return tagged;
  // Legacy entries point at a frozen URL: use the catalogue commit that listed them.
  return tryGit(['log', '-1', '--format=%H', `-S"version": "${entry.version}"`, '--', 'repository/manifest.json']);
}

function changedSince(commit, dir) {
  const files = git(['diff', '--name-only', `${commit}..HEAD`, '--', dir, ...SHARED]).split('\n').filter(Boolean);
  // The workspace lockfile and root manifest change with unrelated plugins; only
  // shared crates and this plugin's own files are release-relevant.
  return files.filter(file => !docOnly(file) && (file.startsWith(`${dir}/`) || file.startsWith('crates/')));
}

function highestPublished(plugin) {
  let highest = '0.0.0';
  for (const entry of plugin.listed?.versions ?? []) if (compare(entry.version, highest) > 0) highest = entry.version;
  for (const tag of (tryGit(['tag', '--list', `${plugin.prefix}-v*`]) ?? '').split('\n')) {
    const version = tag.match(/-v(\d+\.\d+\.\d+)(?:-beta)?$/)?.[1];
    if (version && compare(version, highest) > 0) highest = version;
  }
  return highest;
}

function plan(branch) {
  const release = [], promote = [], notes = [];
  for (const plugin of plugins()) {
    const versions = [...(plugin.listed?.versions ?? [])].sort((a, b) => compare(b.version, a.version));
    if (!versions.length) { notes.push(`${plugin.id}: not in the catalogue yet, publish its first version manually`); continue; }
    const latest = versions[0];
    if (branch === 'main' && tagOf(latest)?.endsWith('-beta')) promote.push(tagOf(latest));
    const commit = sourceCommit(plugin, latest);
    if (!commit) { notes.push(`${plugin.id}: source commit of ${latest.version} not found, skipped`); continue; }
    const changed = changedSince(commit, plugin.dir);
    if (!changed.length) continue;
    const [major, minor, patch] = parse(highestPublished(plugin));
    const next = `${major}.${minor}.${patch + 1}`;
    const version = compare(plugin.version, next) > 0 ? plugin.version : next;
    // Commit subjects touching this plugin become the changelog shown in Aether.
    const subjects = (tryGit(['log', '--no-merges', '--format=%s', `${commit}..HEAD`, '--', plugin.dir, ...SHARED]) ?? '')
      .split('\n').filter(line => line && !/\[skip (ci|release)\]|^chore\(catalogue\)/i.test(line));
    const changelog = [...new Set(subjects)].slice(0, 12).map(line => `- ${line}`).join('\n');
    release.push({ plugin: plugin.id, version, changelog });
    notes.push(`${plugin.id}: ${latest.version} -> ${version} (${changed.length} file(s) changed)`);
  }
  return { release, promote, notes };
}

if (require.main === module) {
  try {
    const branch = process.env.BRANCH || git(['rev-parse', '--abbrev-ref', 'HEAD']);
    if (!['main', 'develop'].includes(branch)) throw new Error(`Automatic releases run on main or develop, not ${branch}`);
    const result = plan(branch);
    for (const note of result.notes) console.error(note);
    const json = JSON.stringify({ release: result.release, promote: result.promote });
    process.stdout.write(json + '\n');
    if (process.env.GITHUB_OUTPUT) fs.appendFileSync(process.env.GITHUB_OUTPUT, `plan=${json}\n`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
module.exports = { plan, compare, tagOf, docOnly };
