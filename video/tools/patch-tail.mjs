// Draws the end of a film again and joins it to the part of the best file that did not change, without drawing the rest.
//
//   node tools/patch-tail.mjs trailer|explainer <from-frame>
//
// Use it when only the last seconds of a film changed (a longer tail, a faster card): <from-frame> is the first frame that differs, at
// 120 frames a second. The best file (renders/pgr-<film>-2160p120.mp4) is cut at its last keyframe at or before that frame (a copy of the
// packets, nothing is encoded again), the frames from there to the new end are drawn and encoded the way the render draws them, and the
// two are joined with the voice that build/<film>.timeline.json and the master audio now hold. The old file is kept as
// renders/pgr-<film>-2160p120.before-patch.mp4 until you delete it. Run tools/build.mjs <film> --derive after it for the other files.
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { renderFilm, ENC, FFMPEG } from './render.mjs';
import { VIDEO_DIR } from './cdp.mjs';

const film = process.argv[2], fromFrame = Number(process.argv[3]);
if (!film || !(fromFrame > 0)) { console.error('usage: node tools/patch-tail.mjs trailer|explainer <from-frame>'); process.exit(2); }
const PY = path.join(VIDEO_DIR, '.venv', process.platform === 'win32' ? 'Scripts/python.exe' : 'bin/python');
const best = path.join(VIDEO_DIR, 'renders', `pgr-${film}-2160p120.mp4`), old = best.replace('.mp4', '.before-patch.mp4');
const dir = path.join(VIDEO_DIR, 'build', 'patch-' + film);
fs.rmSync(dir, { recursive: true, force: true }); fs.mkdirSync(dir, { recursive: true });
const tl = JSON.parse(fs.readFileSync(path.join(VIDEO_DIR, 'build', `${film}.timeline.json`), 'utf8'));
const audio = path.join(VIDEO_DIR, 'build', `${film}.${tl.voice}.master.wav`);
const total = Math.round(tl.duration * 120);

// the keyframes of the best file: the cut goes at the last one at or before the first frame that differs
const kf = spawnSync(PY, ['-I', '-c', `import av,json
c=av.open(r'${best}'); v=c.streams.video[0]
print(json.dumps([int(round(float(p.pts*v.time_base)*120)) for p in c.demux(v) if p.size and p.is_keyframe]))`], { encoding: 'utf8' });
if (kf.status) throw new Error(kf.stderr);
const cut = Math.max(...JSON.parse(kf.stdout).filter((k) => k <= fromFrame));
console.log(`cut at keyframe ${cut} (${(cut / 120).toFixed(3)} s); drawing frames ${cut}-${total} of ${total}`);

const part1 = path.join(dir, 'part1.mp4'), part2 = path.join(dir, 'part2.mp4');
let r = spawnSync(FFMPEG, ['-y', '-v', 'error', '-i', best, '-map', '0:v:0', '-c:v', 'copy', '-frames:v', String(cut), '-an', part1], { stdio: 'inherit' });
if (r.status) throw new Error('cut failed');
await renderFilm({ film, scale: 2, fps: 120, from: cut / 120, to: tl.duration, workers: Number(process.env.PGR_WORKERS || 3), outputs: [{ file: part2, w: 3840, h: 2160, fps: 120, venc: ENC.nvenc(18) }] });
const list = path.join(dir, 'list.txt');
fs.writeFileSync(list, [part1, part2].map((f) => `file '${f.replace(/\\/g, '/')}'`).join('\n') + '\n');
const joined = path.join(dir, 'joined.mp4');
r = spawnSync(FFMPEG, ['-y', '-v', 'error', '-f', 'concat', '-safe', '0', '-i', list, '-c', 'copy', '-tag:v', 'hvc1', joined], { stdio: 'inherit' });
if (r.status) throw new Error('join failed');
fs.renameSync(best, old);
r = spawnSync(FFMPEG, ['-y', '-v', 'error', '-i', joined, '-i', audio, '-map', '0:v:0', '-map', '1:a:0', '-c:v', 'copy', '-c:a', 'aac', '-b:a', '192k', '-ar', '48000', '-ac', '2', '-shortest',
  '-colorspace', 'bt709', '-color_primaries', 'bt709', '-color_trc', 'bt709', '-color_range', 'tv', '-movflags', '+faststart', best], { stdio: 'inherit' });
if (r.status) { fs.renameSync(old, best); throw new Error('mux failed, the old file is back'); }
const p = JSON.parse(spawnSync(PY, ['-I', path.join(VIDEO_DIR, 'tools', 'ffprobe.py'), best], { encoding: 'utf8' }).stdout);
console.log(`${best}  ${p.megabytes} MB  ${p.frames} frames of ${total} expected  ${p.codec_string}`);
if (p.frames !== total) throw new Error(`the patched file has ${p.frames} frames, expected ${total}`);
fs.rmSync(dir, { recursive: true, force: true });
