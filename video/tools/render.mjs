// Deterministic frame renderer. Several headless browsers each open the film page; every frame is drawn by the
// page's own seek(t) (a function of time, never of wall-clock playback: the picture at t is the same at 30, 60 and
// 120 frames a second), captured over the DevTools protocol, put back in order and piped into one FFmpeg process that
// writes every output file of the run (it splits the picture, scales each copy and encodes it).
//
//   node tools/render.mjs <film> [--fps 60] [--scale 2] [--from s] [--to s] [--workers 4] --out file.mp4 [--w 1920 --h 1080]
//
// The page is drawn at 1920 x 1080 CSS pixels times the scale; the frames are JPEGs (quality 97 at 3840 x 2160, 98 at
// 1920 x 1080: a PNG takes 3 to 7 times as long to come out of the browser and the files are ten times the size), and a
// 1080p file can be made from the 2160p frames by a Lanczos downscale, which also gives it a cleaner edge than a page drawn at 1080p.
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { once } from 'node:events';
import { pathToFileURL, fileURLToPath } from 'node:url';
import { launchPage, openAndWait, VIDEO_DIR } from './cdp.mjs';

export const FFMPEG = process.env.PGR_FFMPEG || path.join(VIDEO_DIR, 'bin', process.platform === 'win32' ? 'ffmpeg.exe' : 'ffmpeg');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const COLOR = ['-colorspace', 'bt709', '-color_primaries', 'bt709', '-color_trc', 'bt709', '-color_range', 'tv'];

// Encoder presets. x264 for the files every browser plays; the card's HEVC encoder (or x265) for the best version.
// the limits of H.264 levels: 1080p60 fits level 4.2 (avc1.64002a), 2160p60 needs level 5.2 (avc1.640034)
export const ENC = {
  // generous: the graphics are flat, so this is near lossless at a few tens of Mbit/s. o: level, maxrate, bufsize, preset
  x264: (crf = 14, o = {}) => ['-c:v', 'libx264', '-preset', o.preset || 'slow', '-profile:v', 'high', ...(o.level ? ['-level', o.level] : []), '-crf', String(crf), '-maxrate', o.maxrate || '60M', '-bufsize', o.bufsize || '120M', '-pix_fmt', 'yuv420p', '-bf', '3', '-aq-mode', '3'],
  // Main profile, High tier, level 5.2: the level 4K at 120 frames a second needs, and what a hardware decoder of 2160p120 lists
  nvenc: (cq = 18, maxrate = '150M') => ['-c:v', 'hevc_nvenc', '-preset', 'p7', '-tune', 'hq', '-rc', 'vbr', '-cq', String(cq), '-b:v', '0', '-maxrate', maxrate, '-bufsize', '300M',
    '-profile:v', 'main', '-tier', 'high', '-level', '5.2', '-pix_fmt', 'yuv420p', '-tag:v', 'hvc1', '-bf', '4', '-spatial_aq', '1', '-temporal_aq', '1', '-rc-lookahead', '32'],
  x265: (crf = 14) => ['-c:v', 'libx265', '-preset', 'medium', '-crf', String(crf), '-pix_fmt', 'yuv420p', '-tag:v', 'hvc1', '-x265-params', 'log-level=error:aq-mode=3'],
};
ENC.hd = { level: '4.2', maxrate: '50M', bufsize: '62M' };
ENC.uhd = { level: '5.2', maxrate: '100M', bufsize: '200M', preset: 'medium' };

// outputs: [{ file, w, h, fps, venc: [ffmpeg video args], audio: bool }]
// sceneDir: a copy of scenes/ (with ../build/facts.js and the timeline beside it) to draw from, so that edits made while a long render runs do not reach it
export async function renderFilm({ film, scale = 2, fps = 60, from = 0, to, workers = 4, outputs, audio, quiet = false, sceneDir }) {
  const timeline = JSON.parse(fs.readFileSync(path.join(VIDEO_DIR, 'build', `${film}.timeline.json`), 'utf8'));
  to = to === undefined ? timeline.duration : Math.min(to, timeline.duration);
  const first = Math.round(from * fps), last = Math.round(to * fps), total = last - first;
  const url = pathToFileURL(path.join(sceneDir || path.join(VIDEO_DIR, 'scenes'), `${film}.html`)).href;
  for (const o of outputs) fs.mkdirSync(path.dirname(o.file), { recursive: true });

  // one FFmpeg for every output
  const args = ['-y', '-v', 'error', '-f', 'image2pipe', '-framerate', String(fps), '-c:v', 'mjpeg', '-i', '-'];
  const wantAudio = outputs.some((o) => o.audio) && audio;
  if (wantAudio) args.push('-ss', String(from), '-t', String(to - from), '-i', audio);
  const sc = (o) => `scale=${o.w}:${o.h}:flags=lanczos+accurate_rnd+full_chroma_int:out_color_matrix=bt709:out_range=tv,format=yuv420p,setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv`;
  const parts = outputs.length > 1 ? [`[0:v]split=${outputs.length}${outputs.map((_, i) => `[s${i}]`).join('')}`] : [];
  outputs.forEach((o, i) => parts.push(`${outputs.length > 1 ? `[s${i}]` : '[0:v]'}${o.fps && o.fps !== fps ? `fps=${o.fps},` : ''}${sc(o)}[v${i}]`));
  args.push('-filter_complex', parts.join(';'));
  outputs.forEach((o, i) => {
    args.push('-map', `[v${i}]`);
    if (o.audio && wantAudio) args.push('-map', '1:a:0');
    args.push(...o.venc, ...COLOR, '-r', String(o.fps || fps));
    if (o.audio && wantAudio) args.push('-c:a', 'aac', '-b:a', '192k', '-ar', '48000', '-ac', '2');
    args.push('-movflags', '+faststart', o.file);
  });
  const ff = spawn(FFMPEG, args, { stdio: ['pipe', 'inherit', 'inherit'] });
  const ffDone = new Promise((res, rej) => { ff.on('exit', (c) => (c ? rej(new Error('ffmpeg exited ' + c)) : res())); ff.on('error', rej); });
  ff.stdin.on('error', () => { /* reported by the exit code */ });

  const pages = [];
  let gpu = '';
  for (let k = 0; k < workers; k++) {
    const page = await launchPage({ width: 1920, height: 1080, scale, profile: `render-${film}` });
    await openAndWait(page, url);
    const meta = await page.evaluate('({duration: window.PGR.duration, gpu: window.PGR.gpu})');
    gpu = meta.gpu;
    if (Math.abs(meta.duration - timeline.duration) > 1e-6) throw new Error('page and timeline disagree about the length');
    pages.push(page);
  }

  let next = 0, written = 0;
  const done = new Map();
  let chain = Promise.resolve();
  const flush = () => {
    chain = chain.then(async () => {
      while (done.has(written)) {
        const buf = done.get(written);
        done.delete(written);
        written++;
        if (!ff.stdin.write(buf)) await once(ff.stdin, 'drain');
      }
    });
    return chain;
  };
  const t0 = Date.now();
  let lastLog = 0;
  const work = async (page) => {
    for (;;) {
      const i = next++;
      if (i >= total) return;
      while (i - written > workers * 6) await sleep(4);
      await page.evaluate(`window.PGR.seek(${((first + i) / fps).toFixed(6)})`);
      const r = await page.send('Page.captureScreenshot', { format: 'jpeg', quality: scale > 1 ? 97 : 98, fromSurface: true });
      done.set(i, Buffer.from(r.data, 'base64'));
      await flush();
      const now = Date.now();
      if (!quiet && now - lastLog > 20000) {
        lastLog = now;
        const rate = written / ((now - t0) / 1000);
        console.log(`  ${film} ${1920 * scale}x${1080 * scale} ${fps} fps: frame ${written}/${total}  ${rate.toFixed(1)} frames/s  eta ${Math.round((total - written) / Math.max(rate, 0.01))} s`);
      }
    }
  };
  try {
    await Promise.all(pages.map(work));
    await flush();
    ff.stdin.end();
    await ffDone;
  } finally {
    for (const p of pages) { if (p.logs.length && !quiet) console.log(p.logs.slice(0, 8).join('\n')); await p.close(); }
  }
  const seconds = (Date.now() - t0) / 1000;
  const stats = { film, frames: total, seconds: Math.round(seconds * 10) / 10, drawnFps: Math.round(total / seconds * 100) / 100, scale, fps, from, to, workers, gpu };
  if (!quiet) console.log(`  ${film}: ${total} frames in ${seconds.toFixed(0)} s (${stats.drawnFps} frames/s drawn)`);
  return stats;
}

// One frame of a film as an image file (a poster, a check).
export async function grab({ film, t, scale = 2, out, format = 'png' }) {
  const page = await launchPage({ width: 1920, height: 1080, scale, profile: `grab-${film}` });
  try {
    await openAndWait(page, pathToFileURL(path.join(VIDEO_DIR, 'scenes', `${film}.html`)).href);
    await page.evaluate(`window.PGR.seek(${t})`);
    const r = await page.send('Page.captureScreenshot', format === 'jpeg' ? { format: 'jpeg', quality: 98, fromSurface: true } : { format: 'png', fromSurface: true });
    fs.mkdirSync(path.dirname(out), { recursive: true });
    fs.writeFileSync(out, Buffer.from(r.data, 'base64'));
  } finally { await page.close(); }
  return out;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const a = process.argv.slice(2);
  const film = a.shift();
  const o = { film, scale: 2, fps: 60, workers: 4 };
  let out, w = 1920, h = 1080, enc = 'x264';
  for (let i = 0; i < a.length; i++) {
    if (a[i] === '--scale') o.scale = Number(a[++i]);
    else if (a[i] === '--fps') o.fps = Number(a[++i]);
    else if (a[i] === '--from') o.from = Number(a[++i]);
    else if (a[i] === '--to') o.to = Number(a[++i]);
    else if (a[i] === '--workers') o.workers = Number(a[++i]);
    else if (a[i] === '--out') out = path.resolve(a[++i]);
    else if (a[i] === '--w') w = Number(a[++i]);
    else if (a[i] === '--h') h = Number(a[++i]);
    else if (a[i] === '--enc') enc = a[++i];
  }
  o.outputs = [{ file: out, w, h, fps: o.fps, venc: ENC[enc]() }];
  renderFilm(o).catch((e) => { console.error(e); process.exit(1); });
}
