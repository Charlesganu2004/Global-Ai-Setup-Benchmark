// The picture must not depend on the frame rate. This renders the same second of a film three times through the real renderer
// (the browser, the capture, the encoder), at 30, 60 and 120 frames a second, losslessly, and compares the decoded frames at the
// times all three share (every 1/30 s): the PSNR of the 30 fps file against the 60 and the 120 fps file. tools/check-fps.mjs asks the
// page for the same time three times; this one also covers the render and the encode, which a regression in them would change.
//   node tools/check-rate.mjs trailer|explainer [from-seconds]
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { renderFilm, FFMPEG } from './render.mjs';
import { VIDEO_DIR } from './cdp.mjs';

const film = process.argv[2];
const from = Number(process.argv[3] || (film === 'trailer' ? 16.0 : 16.0));
const dir = path.join(VIDEO_DIR, 'build', 'rate');
fs.mkdirSync(dir, { recursive: true });
const lossless = ['-c:v', 'libx264', '-preset', 'veryfast', '-crf', '0', '-pix_fmt', 'yuv420p'];
const files = {};
for (const fps of [30, 60, 120]) {
  files[fps] = path.join(dir, `${film}-${fps}.mp4`);
  await renderFilm({ film, scale: 1, fps, from, to: from + 1, workers: 3, quiet: true, outputs: [{ file: files[fps], w: 1920, h: 1080, fps, venc: lossless }] });
}
let bad = 0;
for (const fps of [60, 120]) {
  // every (fps / 30)th frame of the faster file is the frame at a time the 30 fps file has: pair them in order
  const r = spawnSync(FFMPEG, ['-v', 'info', '-i', files[30], '-i', files[fps], '-lavfi',
    `[0:v]setpts=N/(30*TB),format=yuv420p[a];[1:v]select='not(mod(n,${fps / 30}))',setpts=N/(30*TB),format=yuv420p[b];[a][b]psnr`, '-f', 'null', '-'], { encoding: 'utf8', maxBuffer: 1 << 26 });
  const m = /PSNR y:([\d.]+|inf) u:\S+ v:\S+ average:([\d.]+|inf) min:([\d.]+|inf) max:\S+/.exec(r.stderr || '');
  const num = (v) => (v === 'inf' ? Infinity : Number(v));
  const y = m ? num(m[1]) : 0, worst = m ? num(m[3]) : 0, ok = y >= 50 && worst >= 40;
  if (!ok) bad++;
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${film} ${from}-${from + 1} s: the 30 fps render against the ${fps} fps render, 30 frames: PSNR y ${m ? m[1] : 'n/a'} dB, worst frame ${m ? m[3] : 'n/a'} dB`);
}
fs.rmSync(dir, { recursive: true, force: true });
console.log(bad ? `${bad} rate(s) differ` : `${film}: 30, 60 and 120 frames a second draw the same picture through the renderer`);
process.exitCode = bad ? 1 : 0;
