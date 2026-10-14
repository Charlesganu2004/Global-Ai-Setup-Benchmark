// One command for a film: numbers -> voice -> frames -> encode -> captions, posters, site list, checks.
//
//   node tools/build.mjs trailer|explainer                  review render: 1080p, 60 fps, H.264 at a generous bit rate (docs/media)
//   node tools/build.mjs trailer|explainer --stretch [s]    s seconds (default 10) at 2160p, 120 fps, HEVC, into renders/ (proof of the full path)
//   node tools/build.mjs trailer|explainer --finals         the final files (see below)
//   node tools/build.mjs trailer|explainer --derive         the finals again from the 2160p120 file that is already there (no new frames)
//
// The finals. The frames are drawn once, at 3840 x 2160 and 120 per second, in chunks of --chunk seconds (default 10), each chunk
// encoded by the graphics card's HEVC encoder at a quality that is visually lossless on these graphics, so a render that stops can
// start again at the first chunk that is missing (--finals keeps the chunks in build/chunks/<film>). The chunks are joined, the voice
// is added, and that is the best file (renders/pgr-<film>-2160p120.mp4, too large for git: it goes to a release). Every other file
// is made from the best file, so each is encoded with its size in mind and no frame is drawn twice:
//   docs/media/pgr-<film>-1080p60.mp4        H.264 High, level 4.2: the file every browser plays
//   pgr-<film>-2160p60.mp4                   H.264 High, level 5.2: docs/media when it is under 95 MB, else renders/ (release)
//   docs/media/pgr-<film>-2160p120-small.mp4 HEVC, 2160p at 120 fps, under 95 MB
// GitHub refuses a file over 100 MB, so nothing 95 MB or more goes in docs/media.
//
// Options: --skip-voice  --reuse (review mode: keep the render that is there, redo posters, list and checks)  --workers N
//          --primary af_heart|am_michael  --from s (the stretch starts there)  --chunk s  --only 1080p60,2160p60,small (derive only these)
// Nothing is typed in by hand: the numbers are read from docs/data/showcase.js (tools/facts.mjs), the models from
// benchmark/data/models.json, the install commands from benchmark/data/install.json.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { writeFacts } from './facts.mjs';
import { renderFilm, grab, ENC, FFMPEG } from './render.mjs';
import { VIDEO_DIR } from './cdp.mjs';

const ROOT = path.resolve(VIDEO_DIR, '..');
const MEDIA = path.join(ROOT, 'docs', 'media');
const BUILD = path.join(VIDEO_DIR, 'build');
const RENDERS = path.join(VIDEO_DIR, 'renders');
const PY = process.env.PGR_PYTHON || path.join(VIDEO_DIR, '.venv', process.platform === 'win32' ? 'Scripts/python.exe' : 'bin/python');
const TITLES = { explainer: 'PeterGanuRusty explained', trailer: 'PeterGanuRusty in thirty seconds' };
const STRETCH_FROM = { trailer: 15.2, explainer: 14.6 };      // the race of the two agents and the claim: the most text, the most dots and the fastest counters
const LIMIT = 95e6;                                            // every file in docs/media stays under this
const TENS = { 10: 'ten', 20: 'twenty', 30: 'thirty', 40: 'forty', 50: 'fifty', 60: 'sixty' };

const args = process.argv.slice(2);
const film = args.shift();
if (!TITLES[film]) { console.error('usage: node tools/build.mjs trailer|explainer [--stretch [s]] [--finals] [--derive] ...'); process.exit(2); }
const opt = { mode: 'review', stretch: 10, workers: 4, primary: 'af_heart', voices: 'af_heart,am_michael', skipVoice: false, from: undefined, chunk: 10, only: null };
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (a === '--stretch') { opt.mode = 'stretch'; if (args[i + 1] && /^[\d.]+$/.test(args[i + 1])) opt.stretch = Number(args[++i]); }
  else if (a === '--finals') opt.mode = 'finals';
  else if (a === '--derive') opt.mode = 'derive';
  else if (a === '--workers') opt.workers = Number(args[++i]);
  else if (a === '--primary') opt.primary = args[++i];
  else if (a === '--from') opt.from = Number(args[++i]);
  else if (a === '--chunk') opt.chunk = Number(args[++i]);
  else if (a === '--only') opt.only = args[++i].split(',');
  else if (a === '--skip-voice') opt.skipVoice = true;
  else if (a === '--reuse') opt.reuse = true;
  else throw new Error('unknown option ' + a);
}
const run = (cmd, argv, what, stdio) => {
  const r = spawnSync(cmd, argv, { stdio: stdio || ['ignore', 'inherit', 'inherit'], encoding: 'utf8', cwd: VIDEO_DIR });
  if (r.status) throw new Error(`${what} failed (exit ${r.status})`);
  return r;
};
const probe = (file) => {
  const r = spawnSync(PY, ['-I', path.join('tools', 'ffprobe.py'), file], { encoding: 'utf8', cwd: VIDEO_DIR });
  if (r.status) throw new Error('ffprobe.py failed: ' + r.stderr);
  return JSON.parse(r.stdout);
};
const step = (s) => console.log('\n== ' + s);
const mb = (f) => (fs.statSync(f).size / 1e6).toFixed(1);

step('numbers from docs/data/showcase.js, models from benchmark/data/models.json');
const facts = writeFacts();
console.log(`  best ${facts.best.tokText} tokens, ${facts.best.turnsText} turns; no index ${facts.control.tokText}, ${facts.control.turnsText}; ${facts.pct}% fewer; models ${Object.values(facts.models).map((m) => m.label).join(' | ')}; install commands from ${facts.installers.source}`);

if (!opt.skipVoice && opt.mode !== 'derive') {
  step('voice, timeline, captions, mastered audio');
  run(PY, ['-I', path.join('tools', 'voice.py'), film, '--primary', opt.primary, '--voices', opt.voices], 'voice step');
}
const timeline = JSON.parse(fs.readFileSync(path.join(BUILD, `${film}.timeline.json`), 'utf8'));
const audio = path.join(BUILD, `${film}.${timeline.voice}.master.wav`);
const vtt = path.join(BUILD, `${film}.${timeline.voice}.en.vtt`);
fs.mkdirSync(MEDIA, { recursive: true }); fs.mkdirSync(RENDERS, { recursive: true });
fs.copyFileSync(path.join(BUILD, 'facts.json'), path.join(RENDERS, `pgr-${film}.facts.json`));   // the figures this build draws, for tools/check-fresh.mjs
const sizeCeiling = LIMIT * 0.97;
const dur = timeline.duration;

// the length spoken in the title ("in thirty seconds") is the length the site prints from the file: it must be the same
function titleFor(seconds) {
  const title = TITLES[film], m = /\bin (\w+) seconds\b/.exec(title);
  if (m) {
    const want = TENS[Math.round(seconds)];
    if (!want || want !== m[1]) throw new Error(`the title of the ${film} says "${m[0]}" but the film is ${seconds.toFixed(1)} s: ${want ? `change the title to "in ${want} seconds"` : 'a spoken length needs a film of 10, 20, 30, 40, 50 or 60 s'}`);
  }
  return title;
}

// posters, 3840 x 2160 and 1920 x 1080, from the frame the script names
async function makePosters() {
  step('posters');
  const at = timeline.poster || dur / 2;
  const png = path.join(BUILD, `${film}-poster-2160-src.jpg`);   // a PNG of 3840 x 2160 never comes out of the browser here; a JPEG at quality 98 does
  await grab({ film, t: at, scale: 2, out: png, format: 'jpeg' });
  run(FFMPEG, ['-y', '-v', 'error', '-i', png, '-frames:v', '1', '-q:v', '1', '-pix_fmt', 'yuvj444p', path.join(MEDIA, `pgr-${film}-poster-2160.jpg`)], 'poster 2160');
  run(FFMPEG, ['-y', '-v', 'error', '-i', png, '-vf', 'scale=1920:1080:flags=lanczos+accurate_rnd+full_chroma_int', '-frames:v', '1', '-q:v', '2', '-pix_fmt', 'yuvj444p', path.join(MEDIA, `pgr-${film}-poster.jpg`)], 'poster 1080');
  fs.rmSync(png, { force: true });
  console.log(`  posters from t=${at.toFixed(2)} s`);
}
// the captions, scanned by the site's own guard (it refuses a model named in a way the site does not)
function guardCaptions() {
  const tmp = path.join(BUILD, 'guard-' + film, 'media');
  fs.rmSync(path.join(BUILD, 'guard-' + film), { recursive: true, force: true }); fs.mkdirSync(tmp, { recursive: true });
  fs.copyFileSync(path.join(MEDIA, `pgr-${film}.en.vtt`), path.join(tmp, `pgr-${film}.en.vtt`));
  const r = spawnSync(process.platform === 'win32' ? 'python' : 'python3', [path.join(ROOT, 'benchmark', 'harness', 'build_site.py'), '--guard', path.join(BUILD, 'guard-' + film)], { encoding: 'utf8', cwd: ROOT });
  console.log('  ' + ((r.stdout || '') + (r.stderr || '')).trim().split('\n').slice(-2).join('\n  '));
  if (r.status) throw new Error('the site guard refuses the captions');
}

if (opt.mode === 'review') {
  step(`review render: ${film} 1920x1080, 60 fps, ${Math.round(dur * 60)} frames`);
  const out = path.join(MEDIA, `pgr-${film}-1080p60.mp4`);
  if (opt.reuse && fs.existsSync(out)) console.log('  --reuse: keeping the render that is already there');
  else {
    const stats = await renderFilm({ film, scale: 1, fps: 60, workers: opt.workers, audio, outputs: [{ file: out, w: 1920, h: 1080, fps: 60, venc: ENC.x264(14, ENC.hd), audio: true }] });
    fs.writeFileSync(path.join(RENDERS, `${film}-review.stats.json`), JSON.stringify(stats, null, 1));
  }
  fs.copyFileSync(vtt, path.join(MEDIA, `pgr-${film}.en.vtt`));
  guardCaptions();
  await makePosters();
  writeSiteList([probe(out)]);
  reportProbe(out);
} else if (opt.mode === 'stretch') {
  const from = opt.from === undefined ? STRETCH_FROM[film] : opt.from, to = Math.min(dur, from + opt.stretch);
  step(`stretch: ${film} ${from}-${to} s at 3840x2160, 120 fps (${Math.round((to - from) * 120)} frames), HEVC`);
  const out = path.join(RENDERS, `pgr-${film}-2160p120-stretch.mp4`);
  const stats = await renderFilm({ film, scale: 2, fps: 120, from, to, workers: opt.workers, audio, outputs: [{ file: out, w: 3840, h: 2160, fps: 120, venc: ENC.nvenc(18), audio: true }] });
  fs.writeFileSync(path.join(RENDERS, `${film}-stretch.stats.json`), JSON.stringify({ stats, probe: probe(out) }, null, 1));
  console.log(`  ${out}  ${mb(out)} MB`);
} else {
  const best = path.join(RENDERS, `pgr-${film}-2160p120.mp4`);
  if (opt.mode === 'finals') await drawBest(best);
  if (!fs.existsSync(best)) throw new Error('there is no ' + best + ': run --finals first');
  fs.copyFileSync(vtt, path.join(MEDIA, `pgr-${film}.en.vtt`));
  guardCaptions();
  const files = deriveFiles(best);
  await makePosters();
  // best first. The file under 95 MB goes after the 1080p60 file when its worst second is far from the best file (a browser that cannot
  // reach the release then gets the clean 1080p60 rather than a starved 2160p): its floor is the lowest PSNR of any second of the opening
  // six seconds and of the six seconds round the poster, against the best file.
  const floor = psnrFloor(files.small.file, best, [0, Math.max(0, (timeline.poster || dur / 2) - 3)]);
  console.log(`  the 2160p120 file under 95 MB: lowest second ${floor.db.toFixed(1)} dB PSNR (luma) against the best file, at ${floor.at.toFixed(0)} s; ${floor.db < 38 ? 'listed after the 1080p60 file' : 'listed before the 1080p60 file'}`);
  const order = (floor.db < 38 ? ['best', '2160p60', '1080p60', 'small'] : ['best', '2160p60', 'small', '1080p60']).filter((k) => files[k]);
  writeSiteList(order.map((k) => probe(files[k].file)), order.map((k) => files[k]));
  reportProbe(files['1080p60'].file);
  step('files');
  for (const k of order) console.log(`  ${files[k].where.padEnd(10)} ${path.basename(files[k].file).padEnd(36)} ${mb(files[k].file)} MB`);
  for (const f of fs.readdirSync(MEDIA)) if (fs.statSync(path.join(MEDIA, f)).size >= LIMIT) throw new Error(`${f} in docs/media is ${mb(path.join(MEDIA, f))} MB: the limit is 95 MB`);
}

// ---- the best file: drawn in chunks, each encoded by the card, then joined with the voice
async function drawBest(best) {
  const dir = path.join(BUILD, 'chunks', film);
  fs.mkdirSync(dir, { recursive: true });
  // the pages the chunks are drawn from are copied first (scenes, facts, timeline): edits made while the render runs do not reach it
  const snap = path.join(BUILD, 'snap', film);
  fs.rmSync(snap, { recursive: true, force: true });
  fs.cpSync(path.join(VIDEO_DIR, 'scenes'), path.join(snap, 'scenes'), { recursive: true });
  fs.mkdirSync(path.join(snap, 'build'), { recursive: true });
  for (const f of ['facts.js', `${film}.timeline.js`]) fs.copyFileSync(path.join(BUILD, f), path.join(snap, 'build', f));
  const total = Math.round(dur * 120), per = Math.round(opt.chunk * 120), n = Math.ceil(total / per);
  step(`finals: ${film}, ${total} frames at 3840x2160, 120 fps, ${n} chunks of ${opt.chunk} s`);
  const t0 = Date.now();
  for (let i = 0; i < n; i++) {
    const a = i * per, b = Math.min(total, a + per), file = path.join(dir, String(i).padStart(3, '0') + '.mp4');
    if (fs.existsSync(file) && probe(file).frames === b - a) { console.log(`  chunk ${i + 1}/${n}: there already (${b - a} frames)`); continue; }
    for (let attempt = 1; ; attempt++) {
      try {
        console.log(`  chunk ${i + 1}/${n}: frames ${a}-${b} (${(a / 120).toFixed(1)}-${(b / 120).toFixed(1)} s), attempt ${attempt}`);
        await renderFilm({ film, scale: 2, fps: 120, from: a / 120, to: b / 120, workers: opt.workers, quiet: false, sceneDir: path.join(snap, 'scenes'), outputs: [{ file, w: 3840, h: 2160, fps: 120, venc: ENC.nvenc(18) }] });
        const got = probe(file).frames;
        if (got !== b - a) throw new Error(`chunk ${i} has ${got} frames, expected ${b - a}`);
        break;
      } catch (e) {
        console.log('  chunk failed: ' + e.message);
        fs.rmSync(file, { force: true });
        if (attempt >= 3) throw e;
      }
    }
    console.log(`  elapsed ${Math.round((Date.now() - t0) / 60000)} min`);
  }
  step('join the chunks and the voice');
  const list = path.join(dir, 'list.txt');
  fs.writeFileSync(list, Array.from({ length: n }, (_, i) => `file '${path.join(dir, String(i).padStart(3, '0') + '.mp4').replace(/\\/g, '/')}'`).join('\n') + '\n');
  const joined = path.join(dir, 'joined.mp4');
  run(FFMPEG, ['-y', '-v', 'error', '-f', 'concat', '-safe', '0', '-i', list, '-c', 'copy', '-tag:v', 'hvc1', joined], 'join');
  run(FFMPEG, ['-y', '-v', 'error', '-i', joined, '-i', audio, '-map', '0:v:0', '-map', '1:a:0', '-c:v', 'copy', '-c:a', 'aac', '-b:a', '192k', '-ar', '48000', '-ac', '2', '-shortest',
    '-colorspace', 'bt709', '-color_primaries', 'bt709', '-color_trc', 'bt709', '-color_range', 'tv', '-movflags', '+faststart', best], 'mux');
  fs.rmSync(joined, { force: true });
  const p = probe(best);
  console.log(`  ${best}  ${mb(best)} MB  ${p.frames} frames of ${total} expected  ${p.codec_string}  total ${Math.round((Date.now() - t0) / 60000)} min`);
  if (p.frames !== total) throw new Error(`the best file has ${p.frames} frames, expected ${total}`);
  fs.rmSync(dir, { recursive: true, force: true });                  // the chunks are in the joined file now
  fs.rmSync(snap, { recursive: true, force: true });
}

// ---- every other file, made from the best one
function deriveFiles(best) {
  const files = { best: { file: best, where: 'release' } };
  const want = (k) => !opt.only || opt.only.includes(k);
  const fromBest = (out, vf, venc, what) => {
    step(what);
    const t0 = Date.now();
    run(FFMPEG, ['-y', '-v', 'error', '-i', best, '-map', '0:v:0', '-map', '0:a:0', '-vf', vf, ...venc, '-c:a', 'copy', '-colorspace', 'bt709', '-color_primaries', 'bt709', '-color_trc', 'bt709', '-color_range', 'tv',
      '-movflags', '+faststart', out], what);
    console.log(`  ${path.basename(out)}  ${mb(out)} MB  in ${Math.round((Date.now() - t0) / 1000)} s`);
  };
  const scaleTo = (w, h, fps) => `${fps ? `fps=${fps},` : ''}scale=${w}:${h}:flags=lanczos+accurate_rnd+full_chroma_int:out_color_matrix=bt709:out_range=tv,format=yuv420p,setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv`;
  // 1080p60: CRF 14, raised until the file is under the limit
  const hd = path.join(MEDIA, `pgr-${film}-1080p60.mp4`);
  if (want('1080p60') || !fs.existsSync(hd)) {
    for (let crf = 14; ; crf += 2) { fromBest(hd, scaleTo(1920, 1080, 60), ENC.x264(crf, ENC.hd), `1080p60 from the best file, CRF ${crf}`); if (fs.statSync(hd).size < sizeCeiling || crf >= 24) break; }
  }
  files['1080p60'] = { file: hd, where: 'docs/media' };
  // 2160p60: CRF 16; under the limit it goes in docs/media, otherwise to renders/ for the release
  if (want('2160p60')) {
    const tmp = path.join(RENDERS, `pgr-${film}-2160p60.mp4`), inMedia = path.join(MEDIA, `pgr-${film}-2160p60.mp4`);
    fs.rmSync(inMedia, { force: true });
    fromBest(tmp, scaleTo(3840, 2160, 60), ENC.x264(16, ENC.uhd), '2160p60 from the best file, CRF 16');
    if (fs.statSync(tmp).size < sizeCeiling) { fs.renameSync(tmp, inMedia); files['2160p60'] = { file: inMedia, where: 'docs/media' }; } else files['2160p60'] = { file: tmp, where: 'release' };
  } else for (const [f, where] of [[path.join(MEDIA, `pgr-${film}-2160p60.mp4`), 'docs/media'], [path.join(RENDERS, `pgr-${film}-2160p60.mp4`), 'release']]) if (fs.existsSync(f)) files['2160p60'] = { file: f, where };
  // the 2160p120 file under the limit: HEVC, two passes, at the bit rate the limit leaves after the audio and the container
  const small = path.join(MEDIA, `pgr-${film}-2160p120-small.mp4`);
  if (want('small') || !fs.existsSync(small)) smallEncode(best, small);
  files.small = { file: small, where: 'docs/media' };
  return files;
}

// The 2160p120 file under the limit. The card's HEVC encoder in two passes (-multipass fullres). The rate the limit leaves after the audio and the
// container is the hard ceiling (-maxrate: the encoder's buffer constraint then keeps the whole file under the limit; with a looser maxrate the card
// overshot its target by a quarter on the hardest scenes), the target starts at 88 percent of it and is steered up or down over up to three tries
// until the file lands between 88 and 100 percent of the limit. PGR_SMALL=x265 uses x265 in two passes instead (about ten times slower here).
function smallEncode(best, out) {
  const cap = Math.floor((sizeCeiling * 8 / dur - 192000 - 40000) / 1000);
  let kbps = Math.floor(cap * 0.88), kept = null;
  const vf = 'scale=3840:2160:flags=lanczos+accurate_rnd+full_chroma_int:out_color_matrix=bt709:out_range=tv,format=yuv420p,setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv';
  const colour = ['-colorspace', 'bt709', '-color_primaries', 'bt709', '-color_trc', 'bt709', '-color_range', 'tv', '-movflags', '+faststart'];
  const useX265 = process.env.PGR_SMALL === 'x265', statsFile = path.join(BUILD, film + '-x265.log').split(path.sep).join('/');
  const tmp = out + '.try.mp4';
  for (let attempt = 1; attempt <= 3; attempt++) {
    step(`2160p120 under 95 MB from the best file: ${useX265 ? 'x265' : 'hevc_nvenc two passes'} at ${kbps} kbit/s (attempt ${attempt})`);
    const t0 = Date.now();
    if (useX265) {
      const xp = (pass) => `log-level=error:pass=${pass}:stats=${statsFile}:aq-mode=3:vbv-maxrate=${Math.round(kbps * 1.7)}:vbv-bufsize=${Math.round(kbps * 3.4)}:level-idc=5.2:high-tier=1:colorprim=bt709:transfer=bt709:colormatrix=bt709`;
      const enc = ['-vf', vf, '-c:v', 'libx265', '-preset', process.env.PGR_X265_PRESET || 'fast', '-b:v', kbps + 'k', '-tag:v', 'hvc1'];
      run(FFMPEG, ['-y', '-v', 'error', '-i', best, '-map', '0:v:0', ...enc, '-x265-params', xp(1), '-an', '-f', 'null', process.platform === 'win32' ? 'NUL' : '/dev/null'], 'x265 pass 1');
      run(FFMPEG, ['-y', '-v', 'error', '-i', best, '-map', '0:v:0', '-map', '0:a:0', ...enc, '-x265-params', xp(2), '-c:a', 'copy', ...colour, tmp], 'x265 pass 2');
      for (const e of fs.readdirSync(BUILD)) if (e.startsWith(`${film}-x265`)) fs.rmSync(path.join(BUILD, e), { force: true });
    } else {
      const enc = ['-vf', vf, '-c:v', 'hevc_nvenc', '-preset', 'p7', '-tune', 'hq', '-multipass', 'fullres', '-rc', 'vbr', '-b:v', kbps + 'k', '-maxrate', Math.round(cap * 0.985) + 'k', '-bufsize', Math.round(cap * 2) + 'k',
        '-profile:v', 'main', '-tier', 'high', '-level', '5.2', '-pix_fmt', 'yuv420p', '-tag:v', 'hvc1', '-bf', '4', '-b_ref_mode', 'middle', '-spatial_aq', '1', '-temporal_aq', '1', '-aq-strength', '8', '-rc-lookahead', '32'];
      run(FFMPEG, ['-y', '-v', 'error', '-i', best, '-map', '0:v:0', '-map', '0:a:0', ...enc, '-c:a', 'copy', ...colour, tmp], 'nvenc two passes');
    }
    const size = fs.statSync(tmp).size;
    console.log(`  ${path.basename(out)}  ${(size / 1e6).toFixed(1)} MB (${(100 * size / sizeCeiling).toFixed(0)}% of the limit)  in ${Math.round((Date.now() - t0) / 60000)} min`);
    if (size < sizeCeiling && (!kept || size > kept.size)) { if (kept) fs.rmSync(kept.file, { force: true }); kept = { file: tmp + '.kept', size }; fs.copyFileSync(tmp, kept.file); }
    if (size < sizeCeiling && size > 0.88 * sizeCeiling) break;
    kbps = Math.min(cap, Math.floor(kbps * (sizeCeiling / size) * (size > sizeCeiling ? 0.95 : 0.99)));
  }
  fs.rmSync(tmp, { force: true });
  if (!kept) throw new Error('the 2160p120 file is still over the limit');
  fs.renameSync(kept.file, out);
}

// the lowest PSNR of the luma, second by second, of a file against the best file, over six seconds from each start in starts
function psnrFloor(file, best, starts) {
  let low = { db: Infinity, at: 0 };
  for (const from of starts) {
    const stats = path.join(BUILD, `${film}-psnr.txt`);
    const r = spawnSync(FFMPEG, ['-v', 'error', '-ss', String(from), '-t', '6', '-i', file, '-ss', String(from), '-t', '6', '-i', best, '-lavfi', `[0:v]format=yuv420p[a];[1:v]format=yuv420p[b];[a][b]psnr=stats_file=build/${film}-psnr.txt`, '-f', 'null', process.platform === 'win32' ? 'NUL' : '/dev/null'], { encoding: 'utf8', cwd: VIDEO_DIR });   // a relative path: a drive letter's colon breaks the filter option
    if (r.status || !fs.existsSync(stats)) throw new Error('psnr failed: ' + r.stderr);
    const ys = fs.readFileSync(stats, 'utf8').trim().split('\n').map((l) => Number((/psnr_y:(\S+)/.exec(l) || [])[1])).map((v) => (Number.isFinite(v) ? Math.min(v, 100) : 100));
    fs.rmSync(stats, { force: true });
    const per = 120;       // frames of a second at 120 per second
    for (let i = 0; i + per <= ys.length; i += per) { const m = ys.slice(i, i + per).reduce((a, b) => a + b, 0) / per; if (m < low.db) low = { db: m, at: from + i / per }; }
  }
  return low;
}

function reportProbe(file) {
  const rep = path.join(BUILD, `${film}-1080p60.probe.json`);
  spawnSync(PY, ['-I', path.join('tools', 'probe.py'), film, '--video', file, '--vtt', path.join(MEDIA, `pgr-${film}.en.vtt`), '--voice', timeline.voice, '--report', rep], { encoding: 'utf8', cwd: VIDEO_DIR });
  const info = JSON.parse(fs.readFileSync(rep, 'utf8'));
  console.log(`  loudness ${info.loudness.integrated_lufs} LUFS, true peak ${info.loudness.true_peak_db} dB, faststart ${info.faststart}; captions ${info.captions.cues} cues`);
  if (info.problems.length) { console.log('  PROBLEMS:\n   - ' + info.problems.join('\n   - ')); process.exitCode = 1; } else console.log('  probe checks passed');
}

// benchmark/data/video.json: one entry per film. The old top-level fields (src, poster, captions, width, height,
// seconds, title) point at the 1080p60 file so the site keeps working until it is rebuilt; sources lists every file, best first.
function writeSiteList(probes, entries) {
  const listPath = path.join(ROOT, 'benchmark', 'data', 'video.json');
  let list = { public: [] };
  try { list = JSON.parse(fs.readFileSync(listPath, 'utf8')); } catch { /* first film */ }
  const rel = (f) => 'media/' + path.basename(f);
  const seconds = Math.round(dur * 10) / 10;
  const src = (p, e) => ({ src: e && e.where === 'release' ? 'release:' + p.file : 'media/' + p.file, width: p.width, height: p.height, fps: p.fps, type: p.type, megabytes: p.megabytes, mbps: p.mbps });
  const sources = probes.map((p, i) => src(p, entries && entries[i]));
  const entry = {
    kind: film, title: titleFor(seconds), seconds,
    src: rel(path.join(MEDIA, `pgr-${film}-1080p60.mp4`)), poster: `media/pgr-${film}-poster.jpg`, captions: `media/pgr-${film}.en.vtt`, width: 1920, height: 1080,
    posters: [{ src: `media/pgr-${film}-poster-2160.jpg`, width: 3840, height: 2160 }, { src: `media/pgr-${film}-poster.jpg`, width: 1920, height: 1080 }],
    sources,
  };
  const order = { trailer: 0, explainer: 1 };
  list.public = [...list.public.filter((e) => e.kind !== film && order[e.kind] !== undefined), entry].sort((a, b) => order[a.kind] - order[b.kind]);
  fs.writeFileSync(listPath, JSON.stringify(list, null, 2) + '\n');
  console.log('  ' + listPath);
}
