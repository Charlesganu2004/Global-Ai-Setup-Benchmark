// Tiles PNG/JPEG frames into one image with FFmpeg's xstack. A review aid, not part of a render.
//   node tools/montage.mjs out.png --cols 2 --width 960 a.png b.png c.png d.png
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const VIDEO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const FFMPEG = process.env.PGR_FFMPEG || path.join(VIDEO, 'bin', 'ffmpeg.exe');
const args = process.argv.slice(2);
const out = args.shift();
let cols = 2, width = 960;
const files = [];
for (let i = 0; i < args.length; i++) {
  if (args[i] === '--cols') cols = Number(args[++i]);
  else if (args[i] === '--width') width = Number(args[++i]);
  else files.push(args[i]);
}
const n = files.length, height = Math.round(width * 9 / 16);
const inputs = files.flatMap((f) => ['-i', f]);
const scaled = files.map((_, i) => `[${i}:v]scale=${width}:${height}:flags=lanczos[s${i}]`).join(';');
const layout = files.map((_, i) => `${(i % cols) * width}_${Math.floor(i / cols) * height}`).join('|');
const graph = `${scaled};${files.map((_, i) => `[s${i}]`).join('')}xstack=inputs=${n}:layout=${layout}:fill=black[v]`;
const r = spawnSync(FFMPEG, ['-y', '-v', 'error', ...inputs, '-filter_complex', graph, '-map', '[v]', '-frames:v', '1', out], { encoding: 'utf8' });
if (r.status) { console.error(r.stderr); process.exit(1); }
console.log(out);
