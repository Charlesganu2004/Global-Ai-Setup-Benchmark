// Checks the final files of a film, from the files themselves.
//
//   node tools/check-finals.mjs trailer|explainer [--psnr]
//
// For every file: codec, profile, level, pixel format, size, frame rate, frame count against duration x fps, bit rate, size in MB,
// fast start, colour tags and the codec string a browser needs. Then:
//  - nothing in docs/media is 95 MB or more (GitHub refuses a file over 100 MB);
//  - the closing card is held, complete and at full brightness, for at least 3 seconds before the fade (measured on the picture);
//  - across a stretch of fast motion no two frames of the 2160p120 file are the same picture (no dropped or doubled frame);
//  - with --psnr, how close each smaller file is to the best one (PSNR of the luma over 10 s of the film's hardest stretch).
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { VIDEO_DIR } from './cdp.mjs';
import { FFMPEG } from './render.mjs';

const film = process.argv[2];
const psnr = process.argv.includes('--psnr');
const ROOT = path.resolve(VIDEO_DIR, '..');
const PY = process.env.PGR_PYTHON || path.join(VIDEO_DIR, '.venv', process.platform === 'win32' ? 'Scripts/python.exe' : 'bin/python');
const tl = JSON.parse(fs.readFileSync(path.join(VIDEO_DIR, 'build', `${film}.timeline.json`), 'utf8'));
const MEDIA = path.join(ROOT, 'docs', 'media'), RENDERS = path.join(VIDEO_DIR, 'renders');
const candidates = [
  ['best', path.join(RENDERS, `pgr-${film}-2160p120.mp4`)], ['2160p60', path.join(MEDIA, `pgr-${film}-2160p60.mp4`)], ['2160p60', path.join(RENDERS, `pgr-${film}-2160p60.mp4`)],
  ['small', path.join(MEDIA, `pgr-${film}-2160p120-small.mp4`)], ['1080p60', path.join(MEDIA, `pgr-${film}-1080p60.mp4`)],
].filter((c) => fs.existsSync(c[1]));
let bad = 0;
const fail = (m) => { bad++; console.log('  FAIL ' + m); };

const info = {};
for (const [k, f] of candidates) {
  const r = spawnSync(PY, ['-I', path.join(VIDEO_DIR, 'tools', 'ffprobe.py'), f], { encoding: 'utf8' });
  const p = info[k] = JSON.parse(r.stdout);
  const expected = Math.round(tl.duration * p.fps);
  console.log(`${k.padEnd(8)} ${path.basename(f)}  (${path.relative(ROOT, path.dirname(f)).replace(/\\/g, '/')})`);
  console.log(`         ${p.codec} ${p.profile} level ${p.level}  ${p.pix_fmt}  ${p.width}x${p.height}  ${p.fps} fps  ${p.frames} frames (duration x fps = ${expected}, duration ${p.duration} s, film ${tl.duration} s)  ${p.megabytes} MB  ${p.mbps} Mbit/s  faststart ${p.faststart}`);
  console.log(`         ${p.type}  colour ${p.colorspace} ${p.color_primaries} ${p.color_trc} ${p.color_range}  audio ${p.audio && p.audio.codec} ${p.audio && p.audio.channels} ch ${p.audio && p.audio.sample_rate} Hz`);
  if (!p.frame_count_matches || Math.abs(p.frames - expected) > 1) fail(`${k}: ${p.frames} frames, expected ${expected}`);
  if (!p.faststart) fail(`${k}: no fast start`);
  // PyAV prints the enum numbers: 1 is BT.709 for the matrix, the primaries and the transfer, and 1 is the limited (tv, mpeg) range
  const is709 = (v) => /bt709|(^|[^0-9])1$/.test(String(v).trim()) && !/unspec|^0$|2$/.test(String(v).trim());
  if (!is709(p.colorspace) || !is709(p.color_primaries) || !is709(p.color_trc) || !/tv|mpeg|1$/.test(String(p.color_range))) fail(`${k}: colour tags are not BT.709 tv (${p.colorspace} ${p.color_primaries} ${p.color_trc} ${p.color_range})`);
  if (f.startsWith(MEDIA) && p.bytes >= 95e6) fail(`${k}: ${p.megabytes} MB in docs/media`);
}

// the hold of the closing card: mean luma of the terminal over the last seconds, at 20 per second
const hdFile = (candidates.find((c) => c[0] === '1080p60') || candidates[0])[1];
{
  const from = tl.duration - 7, w = 160, h = 100;
  const r = spawnSync(FFMPEG, ['-v', 'error', '-ss', String(from), '-i', hdFile, '-vf', `fps=20,crop=1700:560:110:380,scale=${w}:${h}:flags=area,format=gray`, '-f', 'rawvideo', '-'], { maxBuffer: 1 << 28 });
  const raw = r.stdout, n = Math.floor(r.stdout.length / (w * h)), series = [];
  for (let i = 0; i < n; i++) { let s = 0; for (let j = 0; j < w * h; j++) s += r.stdout[i * w * h + j]; series.push(s / (w * h)); }
  const at = (t) => Math.min(n - 1, Math.max(0, Math.round((t - from) * 20)));
  // the card is complete after the last frame, before the fade, whose mean luma is still more than 3 percent off the plateau it settles on
  // (typing adds light to the picture, 10 percent and more; the slow push of the camera and the drift of the lights behind the glass move it by 1 or 2)
  const hi = at(tl.duration - 0.45);
  const plateau = series[at(tl.duration - 1.0)];
  let done = 0;
  for (let i = 0; i < hi; i++) if (Math.abs(series[i] - plateau) > 0.03 * plateau) done = i + 1;
  const complete = from + done / 20, fadeStart = tl.duration - 0.3, hold = fadeStart - complete;
  console.log(`closing card: complete at ${complete.toFixed(2)} s, fade from ${fadeStart.toFixed(2)} s: held ${hold.toFixed(2)} s at full brightness (plateau luma ${plateau.toFixed(1)})`);
  if (hold < 3.0) fail(`the closing card is held for ${hold.toFixed(2)} s, not 3.0 s`);
}

// no doubled or dropped frame across fast motion, in the best file (or the largest there is)
{
  const k = info.best ? 'best' : info.small ? 'small' : '1080p60', file = candidates.find((c) => c[0] === k)[1], fps = info[k].fps, w = 192, h = 108;
  const from = film === 'trailer' ? 15.2 : 14.6, secs = 4;
  const r = spawnSync(FFMPEG, ['-v', 'error', '-ss', String(from), '-t', String(secs), '-i', file, '-vf', `scale=${w}:${h}:flags=area,format=gray`, '-f', 'rawvideo', '-'], { maxBuffer: 1 << 28 });
  const n = Math.floor(r.stdout.length / (w * h));
  let same = 0, big = 0;
  for (let i = 1; i < n; i++) { let d = 0; for (let j = 0; j < w * h; j++) d += Math.abs(r.stdout[i * w * h + j] - r.stdout[(i - 1) * w * h + j]); if (d === 0) same++; if (d / (w * h) > 6) big++; }
  console.log(`motion stretch ${from}-${from + secs} s of the ${k} file at ${fps} fps: ${n} frames decoded (expected ${secs * fps}), ${same} equal to the one before, ${big} with a large jump`);
  if (Math.abs(n - secs * fps) > 1) fail(`the stretch has ${n} frames, expected ${secs * fps}`);
  if (same > 0) fail(`${same} frames are the same picture as the one before in a stretch of motion`);
}

if (psnr && info.best) {
  const mid = Math.max(0, Math.round(((tl.poster || tl.duration / 2) - 5) * 30) / 30);       // on the 1/30 s grid, so every file has a frame at it
  for (const k of ['small', '2160p60', '1080p60']) {
    if (!info[k]) continue;
    const f = candidates.find((c) => c[0] === k)[1], p = info[k], fps = Math.min(p.fps, info.best.fps);
    const sc = `${info.best.fps !== p.fps ? `fps=${p.fps},` : ''}scale=${p.width}:${p.height}:flags=lanczos+accurate_rnd+full_chroma_int,format=yuv420p`;
    const r = spawnSync(FFMPEG, ['-v', 'info', '-ss', String(mid), '-t', '10', '-i', f, '-ss', String(mid), '-t', '10', '-i', candidates.find((c) => c[0] === 'best')[1],
      '-lavfi', `[0:v]format=yuv420p[a];[1:v]${sc}[b];[a][b]psnr`, '-f', 'null', '-'], { encoding: 'utf8', maxBuffer: 1 << 26 });
    const m = /PSNR y:([\d.]+) u:([\d.]+) v:([\d.]+) average:([\d.]+) min:([\d.]+) max:([\d.]+)/.exec(r.stderr || '');
    console.log(`${k.padEnd(8)} against the best file, ${mid.toFixed(1)}-${(mid + 10).toFixed(1)} s: ${m ? `PSNR y ${m[1]} dB, average of the planes ${m[4]} dB, worst frame ${m[5]} dB` : 'no PSNR line: ' + (r.stderr || '').slice(-300)}`);
  }
}
console.log(bad ? `${bad} failure(s)` : `${film}: every final file checks out`);
process.exitCode = bad ? 1 : 0;
