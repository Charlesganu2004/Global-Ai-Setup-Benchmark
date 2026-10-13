// One command for a whole film: numbers -> voice -> frames -> encode -> captions, poster, site list, checks.
//
//   node tools/build.mjs explainer            1080p into docs/media, everything it needs rebuilt
//   node tools/build.mjs ad
//   node tools/build.mjs explainer --scale 2  the 2160p version (same picture, drawn at twice the pixels)
//
// Options: --primary <voice>   the voice that is muxed (default af_heart)
//          --voices a,b        voices to synthesise (default af_heart,am_michael); the others stay in renders/
//          --workers n         browsers drawing frames at once (default 3)
//          --skip-voice        reuse build/ audio and timeline
//          --skip-render       reuse the master video in build/
//          --sheets dir        where the contact sheets go (default build/sheets)
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { writeFacts } from './facts.mjs';
import { renderMaster, FFMPEG } from './render.mjs';
import { VIDEO_DIR } from './cdp.mjs';

const ROOT = path.resolve(VIDEO_DIR, '..');
const MEDIA = path.join(ROOT, 'docs', 'media');
const BUILD = path.join(VIDEO_DIR, 'build');
const PY = process.env.PGR_PYTHON || path.join(VIDEO_DIR, '.venv', process.platform === 'win32' ? 'Scripts/python.exe' : 'bin/python');
// Size ceilings in bytes (decimal megabytes, a little under the limits in the brief)
const CEILING = { explainer: { 1080: 24.0e6, 2160: 58.0e6 }, ad: { 1080: 11.4e6, 2160: 28.5e6 } };
const TITLES = { explainer: 'PeterGanuRusty explained', ad: 'PeterGanuRusty in twenty seconds' };

const args = process.argv.slice(2);
const film = args.shift();
if (!CEILING[film]) { console.error('usage: node tools/build.mjs explainer|ad [--scale 2] ...'); process.exit(2); }
const opt = { scale: 1, workers: 3, primary: 'af_heart', voices: 'af_heart,am_michael', skipVoice: false, skipRender: false, sheets: path.join(BUILD, 'sheets') };
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (a === '--scale') opt.scale = Number(args[++i]);
  else if (a === '--workers') opt.workers = Number(args[++i]);
  else if (a === '--primary') opt.primary = args[++i];
  else if (a === '--voices') opt.voices = args[++i];
  else if (a === '--sheets') opt.sheets = path.resolve(args[++i]);
  else if (a === '--skip-voice') opt.skipVoice = true;
  else if (a === '--skip-render') opt.skipRender = true;
  else throw new Error('unknown option ' + a);
}
const H = 1080 * opt.scale;
const run = (cmd, argv, what) => {
  const r = spawnSync(cmd, argv, { stdio: ['ignore', 'inherit', 'inherit'], encoding: 'utf8' });
  if (r.status) throw new Error(`${what} failed (exit ${r.status})`);
};
const step = (s) => console.log('\n== ' + s);

step('numbers from docs/data/showcase.js');
const facts = writeFacts();
console.log(`  best ${facts.best.tokText} tokens, ${facts.best.turnsText} turns; no index ${facts.control.tokText}, ${facts.control.turnsText}; ${facts.pct}% fewer`);

if (!opt.skipVoice) {
  step('voice, timeline, captions, mastered audio');
  run(PY, ['-I', path.join('tools', 'voice.py'), film, '--primary', opt.primary, '--voices', opt.voices], 'voice step');
}
const timeline = JSON.parse(fs.readFileSync(path.join(BUILD, `${film}.timeline.json`), 'utf8'));
const primary = timeline.voice;

step(`frames at ${1920 * opt.scale}x${H}, ${timeline.fps} fps, ${Math.round(timeline.duration * timeline.fps)} frames`);
const master = path.join(BUILD, `${film}-${H}p.master.mp4`);
if (!opt.skipRender) await renderMaster({ film, scale: opt.scale, workers: opt.workers, out: master });

step('final encode: two passes to the size ceiling, voice muxed');
fs.mkdirSync(MEDIA, { recursive: true });
const out = path.join(MEDIA, `pgr-${film}-${H}p.mp4`);
const audio = path.join(BUILD, `${film}.${primary}.master.wav`);
const target = CEILING[film][H];
const vbps = Math.floor((target * 8 * 0.985) / timeline.duration - 160000);
const log = path.join(BUILD, `${film}-${H}p.x264`);
const enc = ['-c:v', 'libx264', '-preset', 'slow', '-profile:v', 'high', '-b:v', String(vbps), '-maxrate', String(Math.round(vbps * 2.4)), '-bufsize', String(Math.round(vbps * 3)),
  '-g', String(timeline.fps * 2), '-bf', '3', '-aq-mode', '3', '-pix_fmt', 'yuv420p', '-colorspace', 'bt709', '-color_primaries', 'bt709', '-color_trc', 'bt709', '-color_range', 'tv', '-passlogfile', log];
run(FFMPEG, ['-y', '-v', 'error', '-i', master, ...enc, '-pass', '1', '-an', '-f', 'null', '-'], 'pass 1');
run(FFMPEG, ['-y', '-v', 'error', '-i', master, '-i', audio, '-map', '0:v:0', '-map', '1:a:0', ...enc, '-pass', '2', '-c:a', 'aac', '-b:a', '160k', '-ar', '48000', '-ac', '2',
  '-movflags', '+faststart', out], 'pass 2');
for (const f of fs.readdirSync(BUILD)) if (f.startsWith(path.basename(log))) fs.rmSync(path.join(BUILD, f), { force: true });
console.log(`  ${out}  ${(fs.statSync(out).size / 1e6).toFixed(2)} MB (ceiling ${(target / 1e6).toFixed(1)} MB)`);

step('captions, poster, list for the site');
const vtt = path.join(MEDIA, `pgr-${film}.en.vtt`);
fs.copyFileSync(path.join(BUILD, `${film}.${primary}.en.vtt`), vtt);
const poster = path.join(MEDIA, `pgr-${film}-poster.jpg`);
if (opt.scale === 1) run(FFMPEG, ['-y', '-v', 'error', '-ss', String(timeline.poster || timeline.duration / 2), '-i', out, '-frames:v', '1', '-q:v', '2', poster], 'poster');
// the other voice stays out of the site: audio only, in renders/
fs.mkdirSync(path.join(VIDEO_DIR, 'renders'), { recursive: true });
for (const v of opt.voices.split(',')) {
  const src = path.join(BUILD, `${film}.${v}.master.wav`);
  if (!fs.existsSync(src)) continue;
  run(FFMPEG, ['-y', '-v', 'error', '-i', src, '-c:a', 'aac', '-b:a', '160k', path.join(VIDEO_DIR, 'renders', `pgr-${film}-${v}${v === primary ? '.muxed' : ''}.m4a`)], 'audio copy');
}
fs.copyFileSync(master, path.join(VIDEO_DIR, 'renders', `pgr-${film}-${H}p.master.mp4`));
if (opt.scale === 1) {
  const listPath = path.join(ROOT, 'benchmark', 'data', 'video.json');
  let list = { public: [] };
  try { list = JSON.parse(fs.readFileSync(listPath, 'utf8')); } catch { /* first film */ }
  const entry = { kind: film, src: `media/pgr-${film}-1080p.mp4`, poster: `media/pgr-${film}-poster.jpg`, captions: `media/pgr-${film}.en.vtt`, width: 1920, height: 1080, seconds: Math.round(timeline.duration * 10) / 10, title: TITLES[film] };
  list.public = [...list.public.filter((e) => e.kind !== film), entry].sort((a, b) => (a.kind === 'ad' ? -1 : 1) - (b.kind === 'ad' ? -1 : 1));
  fs.writeFileSync(listPath, JSON.stringify(list, null, 2) + '\n');
  console.log('  ' + listPath);
}

step('contact sheet and checks');
fs.mkdirSync(opt.sheets, { recursive: true });
const every = film === 'ad' ? 1 : 2;
const frames = Math.ceil(timeline.duration / every), cols = 6, rows = Math.ceil(frames / cols);
run(FFMPEG, ['-y', '-v', 'error', '-i', out, '-vf', `fps=1/${every},scale=480:-1,tile=${cols}x${rows}`, '-frames:v', '1', '-q:v', '3', path.join(opt.sheets, `${film}-${H}p-contact.jpg`)], 'contact sheet');
const report = path.join(BUILD, `${film}-${H}p.probe.json`);
const pr = spawnSync(PY, ['-I', path.join('tools', 'probe.py'), film, '--video', out, '--vtt', vtt, '--voice', primary, '--report', report], { encoding: 'utf8', cwd: VIDEO_DIR });
const info = JSON.parse(fs.readFileSync(report, 'utf8'));
console.log(`  ${info.video.width}x${info.video.height} ${info.video.profile} ${info.video.pix_fmt} ${info.video.fps} fps, ${info.seconds} s, ${info.mb} MB, ${info.overall_kbps} kb/s total; audio ${info.audio.codec} ${info.audio.channels} ch ${info.audio.sample_rate} Hz`);
console.log(`  loudness ${info.loudness.integrated_lufs} LUFS, true peak ${info.loudness.true_peak_db} dB, faststart ${info.faststart}; captions ${info.captions.cues} cues, max ${info.captions.max_lines} lines of ${info.captions.max_line_chars} chars`);
if (info.problems.length) { console.log('  PROBLEMS:\n   - ' + info.problems.join('\n   - ')); process.exitCode = 1; } else console.log('  checks passed');
if (info.bytes > target) { console.log(`  OVER THE SIZE CEILING: ${info.bytes} > ${target}`); process.exitCode = 1; }
