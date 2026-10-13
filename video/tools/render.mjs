// Deterministic frame renderer. Several headless browsers each open the film page; every frame is drawn by the
// page's own seek(t) (a function of time, never of wall-clock playback), captured as a PNG over the DevTools
// protocol, put back in order and piped into FFmpeg, which writes a near-lossless master.
//
//   node tools/render.mjs <film> [--scale 1|2] [--workers 3] [--out file.mp4] [--only t1,t2,...]
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { once } from 'node:events';
import { pathToFileURL, fileURLToPath } from 'node:url';
import { launchPage, openAndWait, VIDEO_DIR } from './cdp.mjs';

export const FFMPEG = process.env.PGR_FFMPEG || path.join(VIDEO_DIR, 'bin', process.platform === 'win32' ? 'ffmpeg.exe' : 'ffmpeg');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export async function renderMaster({ film, scale = 1, workers = 3, out, quiet = false }) {
  const timeline = JSON.parse(fs.readFileSync(path.join(VIDEO_DIR, 'build', `${film}.timeline.json`), 'utf8'));
  const fps = timeline.fps;
  const total = Math.round(timeline.duration * fps);
  const url = pathToFileURL(path.join(VIDEO_DIR, 'scenes', `${film}.html`)).href;
  const W = 1920 * scale, H = 1080 * scale;
  out = out || path.join(VIDEO_DIR, 'build', `${film}-${H}p.master.mp4`);
  fs.mkdirSync(path.dirname(out), { recursive: true });

  const ff = spawn(FFMPEG, [
    '-y', '-v', 'error', '-f', 'image2pipe', '-framerate', String(fps), '-c:v', scale > 1 ? 'mjpeg' : 'png', '-i', '-',
    '-vf', 'scale=flags=lanczos+accurate_rnd+full_chroma_int:out_color_matrix=bt709:out_range=tv,format=yuv420p',
    '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '8', '-profile:v', 'high', '-r', String(fps),
    '-colorspace', 'bt709', '-color_primaries', 'bt709', '-color_trc', 'bt709', '-color_range', 'tv', out,
  ], { stdio: ['pipe', 'inherit', 'inherit'] });
  const ffDone = new Promise((res, rej) => { ff.on('exit', (c) => (c ? rej(new Error('ffmpeg exited ' + c)) : res())); ff.on('error', rej); });
  ff.stdin.on('error', () => { /* reported by the exit code */ });

  const pages = [];
  for (let k = 0; k < workers; k++) {
    const page = await launchPage({ width: 1920, height: 1080, scale, profile: `render-${film}` });
    await openAndWait(page, url);
    const meta = await page.evaluate('({fps: window.PGR.fps, duration: window.PGR.duration})');
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
      await page.evaluate(`window.PGR.seek(${(i / fps).toFixed(6)})`);
      const r = await page.send('Page.captureScreenshot', scale > 1 ? { format: 'jpeg', quality: 97, fromSurface: true } : { format: 'png', fromSurface: true });
      done.set(i, Buffer.from(r.data, 'base64'));
      await flush();
      const now = Date.now();
      if (!quiet && now - lastLog > 15000) {
        lastLog = now;
        const rate = written / ((now - t0) / 1000);
        console.log(`  ${film} ${W}x${H}: frame ${written}/${total}  ${rate.toFixed(1)} fps  eta ${Math.round((total - written) / Math.max(rate, 0.01))} s`);
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
  if (!quiet) console.log(`  ${film}: ${total} frames in ${((Date.now() - t0) / 1000).toFixed(0)} s -> ${out}`);
  return { out, total, fps, width: W, height: H, duration: timeline.duration };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const a = process.argv.slice(2);
  const film = a.shift();
  const o = { film };
  for (let i = 0; i < a.length; i++) {
    if (a[i] === '--scale') o.scale = Number(a[++i]);
    else if (a[i] === '--workers') o.workers = Number(a[++i]);
    else if (a[i] === '--out') o.out = path.resolve(a[++i]);
  }
  renderMaster(o).catch((e) => { console.error(e); process.exit(1); });
}
