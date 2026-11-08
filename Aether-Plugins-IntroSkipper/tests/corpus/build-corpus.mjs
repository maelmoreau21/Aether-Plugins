// Builds the acoustic test corpus of the IntroSkipper plugin.
//
//   docker run --rm -v "<repo>:/work" -w /work aether-rust-dev \
//     node Aether-Plugins-IntroSkipper/tests/corpus/build-corpus.mjs
//
// It needs ffmpeg with the chromaprint muxer. Every audio signal is synthesised (no copyrighted
// material): a "theme" is a deterministic melody, a "scene" is pink noise over a quiet tonal bed.
// Each episode is turned into a head (300 s) and a tail (200 s) exactly like the Node does
// (`crates/node/src/tasks.rs`: mono, 11025 Hz, `-f chromaprint -fp_format raw`, head cut at 2500
// words, tail cut at 1500 words *keeping the first ones*). The committed .bin files are the source
// of truth: another ffmpeg build may produce slightly different bits.
import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync, existsSync, rmSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const out = join(dirname(fileURLToPath(import.meta.url)), 'intro');
const work = '/tmp/intro-corpus-work';
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
mkdirSync(work, { recursive: true });

const SR = 11025;
const HEAD_SECONDS = 300;
const TAIL_SECONDS = 186; // 1500 items / 8.0769 per second: the window the Node must request so the tail reaches the end of the file
const HEAD_WORDS = 2500;
const TAIL_WORDS = 1500;
const ffmpeg = (args) =>
  execFileSync('ffmpeg', ['-nostdin', '-v', 'error', '-y', ...args], { maxBuffer: 1 << 26 });

// A melody: one note every 1/3 s, chosen by a small integer recipe (a, b, c).
function melody(a, b, c, gain) {
  const n = 'floor(t*3)';
  const k = `mod(${n}*${a}+floor(${n}/4)*${b}+mod(${n}*${n},${c + 5}),14)`;
  const f = `(196*pow(2,${k}/12))`;
  const env = '(0.35+0.65*(1-mod(t*3,1)))';
  return `${gain}*${env}*(0.6*sin(2*PI*${f}*t)+0.3*sin(4*PI*${f}*t)+0.2*sin(3*PI*${f}*t))`;
}
const synth = (expr, dur, file) =>
  ffmpeg(['-f', 'lavfi', '-i', `aevalsrc='${expr}':s=${SR}:d=${dur}`, '-ac', '1', '-c:a', 'pcm_s16le', file]);

const themes = {
  A: [5, 3, 2], // the series opening
  B: [7, 5, 1], // a second opening (changed mid-season)
  C: [3, 7, 0], // the end credits
  LOGO: [11, 2, 1], // a studio bumper shared by episodes
  JINGLE: [9, 4, 2], // a short "previously on" jingle
};
function theme(name, dur, tag = '') {
  const file = join(work, `theme-${name}-${dur}${tag}.wav`);
  if (!existsSync(file)) synth(melody(...themes[name], 0.5), dur, file);
  return file;
}
let sceneCount = 0;
// Pink noise over a quiet melody from the same family as the themes (a hard negative).
function scene(seed, dur) {
  const file = join(work, `scene-${seed}-${dur}.wav`);
  if (existsSync(file)) return file;
  sceneCount++;
  const a = 2 + ((seed * 7 + 3) % 11);
  const b = 1 + ((seed * 5 + 1) % 9);
  const c = 3 + ((seed * 3 + 2) % 13); // themes use c < 3: a scene never repeats a theme
  ffmpeg([
    '-f', 'lavfi', '-i', `anoisesrc=d=${dur}:c=pink:r=${SR}:a=0.25:seed=${seed}`,
    '-f', 'lavfi', '-i', `aevalsrc='${melody(a, b, c, 0.18)}':s=${SR}:d=${dur}`,
    '-filter_complex', '[0][1]amix=inputs=2:normalize=0:duration=first',
    '-ac', '1', '-c:a', 'pcm_s16le', file,
  ]);
  return file;
}
// The theme with dialogue-like noise on top of it.
function noisyTheme(name, dur, noise, seed) {
  const file = join(work, `noisy-${name}-${dur}-${noise}-${seed}.wav`);
  if (!existsSync(file))
    ffmpeg([
      '-i', theme(name, dur),
      '-f', 'lavfi', '-i', `anoisesrc=d=${dur}:c=pink:r=${SR}:a=${noise}:seed=${seed}`,
      '-filter_complex', '[0][1]amix=inputs=2:normalize=0:duration=first',
      '-ac', '1', '-c:a', 'pcm_s16le', file,
    ]);
  return file;
}
// The same theme after a lossy encode (another release of the same episode).
function lossyTheme(name, dur) {
  const file = join(work, `lossy-${name}-${dur}.wav`);
  const aac = join(work, `lossy-${name}-${dur}.m4a`);
  if (!existsSync(file)) {
    ffmpeg(['-i', theme(name, dur), '-c:a', 'aac', '-b:a', '40k', aac]);
    ffmpeg(['-i', aac, '-ac', '1', '-ar', String(SR), '-c:a', 'pcm_s16le', file]);
  }
  return file;
}
function concat(parts, name) {
  const list = join(work, `${name}.txt`);
  writeFileSync(list, parts.map((p) => `file '${p}'`).join('\n'));
  const file = join(work, `${name}.wav`);
  ffmpeg(['-f', 'concat', '-safe', '0', '-i', list, '-c', 'copy', file]);
  return file;
}
function fingerprint(wav, seconds, words, from = 'start') {
  const raw = execFileSync(
    'ffmpeg',
    ['-nostdin', '-v', 'error', '-i', wav, '-map', '0:a:0', '-vn', '-ac', '1', '-ar', String(SR),
      '-t', String(seconds), '-f', 'chromaprint', '-fp_format', 'raw', '-'],
    { maxBuffer: 1 << 26 },
  );
  const all = raw.length / 4;
  const keep = Math.min(words, all);
  // The Node keeps the FIRST words of the window (`Vec::truncate`).
  return from === 'start' ? raw.subarray(0, keep * 4) : raw.subarray((all - keep) * 4);
}

// ---------------------------------------------------------------- scenarios
// head: segments from t=0; `intro` marks the segment the detector must find.
// tail: { credits: dur|null, post: dur } counted back from the end of the episode.
const DURATION = 1440;
const scenarios = [];
const sc = (name, description, withTail, episodes) => scenarios.push({ name, description, withTail, episodes });

sc('standard', 'Opening theme A (60 s) from the first second, end credits C (60 s) at the very end', true,
  [1, 2, 3, 4, 5].map((n) => ({ head: [{ kind: 'theme', name: 'A', dur: 60, intro: true }], tail: { credits: 60, post: 0 } })));

sc('cold_open', 'Cold open of varying length before the opening theme A', false,
  [35, 90, 62, 140, 20].map((cold) => ({ head: [{ kind: 'scene', dur: cold }, { kind: 'theme', name: 'A', dur: 60, intro: true }] })));

sc('recap', 'A "previously on" recap of varying length (sharing a 6 s jingle) before theme A', false,
  [45, 62, 38, 70, 55].map((len) => ({
    head: [{ kind: 'theme', name: 'JINGLE', dur: 6 }, { kind: 'scene', dur: len - 6 }, { kind: 'theme', name: 'A', dur: 60, intro: true }],
  })));

sc('opening_change', 'Eight episodes: opening A then, from episode 5, a different opening B (75 s)', true,
  [10, 55, 20, 40].map((off) => ({ head: [{ kind: 'scene', dur: off }, { kind: 'theme', name: 'A', dur: 60, intro: true }], tail: { credits: 60, post: 0 } }))
    .concat([30, 12, 70, 25].map((off) => ({ head: [{ kind: 'scene', dur: off }, { kind: 'theme', name: 'B', dur: 75, intro: true }], tail: { credits: 60, post: 0 } }))));

sc('no_intro', 'No opening at all (anthology, every episode starts on its own scene)', true,
  [1, 2, 3, 4, 5].map(() => ({ head: [{ kind: 'scene', dur: 300 }], tail: { credits: 60, post: 0 } })));

sc('studio_bumper', 'No opening, but a 20 s studio bumper shared by every episode (a known limit: it IS a repetition)', false,
  [25, 60, 10, 80, 40].map((off) => ({ head: [{ kind: 'scene', dur: off }, { kind: 'theme', name: 'LOGO', dur: 20, intro: true }], expectIntro: false })));

sc('special_episode', 'Episode 3 is a special without opening; the other episodes have theme A', false,
  [0, 1, 2, 3, 4].map((i) => (i === 2
    ? { head: [{ kind: 'scene', dur: 300 }] }
    : { head: [{ kind: 'scene', dur: 10 + 15 * i }, { kind: 'theme', name: 'A', dur: 60, intro: true }] })));

sc('noisy', 'Theme A under growing dialogue/effects noise (amplitude 0.1 to 1.0 against a theme peak of 0.5)', false,
  [0.1, 0.25, 0.5, 0.75, 1.0].map((noise, i) => ({
    noise,
    head: [{ kind: 'scene', dur: 20 }, { kind: 'noisy', name: 'A', dur: 60, noise, intro: true }],
  })));

sc('other_encode', 'The same opening after a lossy AAC 40 kbit/s encode in 3 of the 5 episodes', false,
  [0, 1, 2, 3, 4].map((i) => ({ head: [{ kind: 'scene', dur: 25 }, { kind: i % 2 === 0 ? 'lossy' : 'theme', name: 'A', dur: 60, intro: true }] })));

sc('truncated_intro', 'Episode 3 only plays the first 38 s of the opening (a shortened broadcast cut)', false,
  [0, 1, 2, 3, 4].map((i) => ({ head: [{ kind: 'scene', dur: 20 }, { kind: 'theme', name: 'A', dur: i === 2 ? 38 : 60, intro: true }] })));

sc('credits_variants', 'Credits followed by a post-credits scene (eps 3-4) and no credits at all (eps 5)', true,
  [{ credits: 60, post: 0 }, { credits: 60, post: 0 }, { credits: 60, post: 45 }, { credits: 60, post: 30 }, { credits: null, post: 0 }]
    .map((tail) => ({ head: [{ kind: 'theme', name: 'A', dur: 60, intro: true }], tail })));

// ---------------------------------------------------------------- generation
const manifest = { sampleRate: 11025 / 1365, legacyRate: 8.61328125, headSeconds: HEAD_SECONDS, tailSeconds: TAIL_SECONDS, duration: DURATION, scenarios: [] };
let seed = 100;
for (const scenario of scenarios) {
  const episodes = [];
  scenario.episodes.forEach((spec, index) => {
    const id = `${scenario.name}-${String(index + 1).padStart(2, '0')}`;
    seed += 1;
    // head
    const parts = [];
    let t = 0;
    let intro = null;
    for (const seg of spec.head) {
      let file;
      if (seg.kind === 'scene') file = scene(seed * 10 + parts.length, seg.dur);
      else if (seg.kind === 'theme') file = theme(seg.name, seg.dur);
      else if (seg.kind === 'noisy') file = noisyTheme(seg.name, seg.dur, seg.noise, seed);
      else file = lossyTheme(seg.name, seg.dur);
      parts.push(file);
      if (seg.intro) intro = { start: t, end: t + seg.dur };
      t += seg.dur;
    }
    if (t < HEAD_SECONDS) parts.push(scene(seed * 10 + 9, HEAD_SECONDS - t));
    const head = fingerprint(concat(parts, `${id}-head`), HEAD_SECONDS, HEAD_WORDS);
    writeFileSync(join(out, `${id}.head.bin`), head);
    // tail
    let credits = null;
    let tailFile = null;
    if (scenario.withTail) {
      const tl = spec.tail ?? { credits: 60, post: 0 };
      const body = TAIL_SECONDS - (tl.credits ?? 0) - tl.post;
      const tparts = [scene(seed * 10 + 5, body)];
      if (tl.credits) {
        tparts.push(theme('C', tl.credits));
        credits = { start: DURATION - tl.post - tl.credits, end: DURATION - tl.post };
      }
      if (tl.post) tparts.push(scene(seed * 10 + 6, tl.post));
      const tail = fingerprint(concat(tparts, `${id}-tail`), TAIL_SECONDS, TAIL_WORDS);
      tailFile = `${id}.tail.bin`;
      writeFileSync(join(out, tailFile), tail);
    }
    episodes.push({
      id,
      number: index + 1,
      head: `${id}.head.bin`,
      tail: tailFile,
      intro: spec.expectIntro === false ? null : intro,
      ...(spec.expectIntro === false ? { repeated: intro } : {}),
      credits: scenario.withTail ? credits : undefined,
      ...(spec.noise !== undefined ? { noise: spec.noise } : {}),
    });
  });
  manifest.scenarios.push({ name: scenario.name, description: scenario.description, episodes });
  console.log(`${scenario.name}: ${episodes.length} episodes`);
}
writeFileSync(join(out, 'manifest.json'), JSON.stringify(manifest, null, 1));
console.log(`done (${sceneCount} scenes synthesised)`);
