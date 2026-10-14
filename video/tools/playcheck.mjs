// Asks the browser (Edge, over the DevTools protocol, as the render uses it) whether it can play each file's type and
// whether decoding it is smooth and power efficient, with the file's real width, height, frame rate and bit rate:
// HTMLMediaElement.canPlayType and navigator.mediaCapabilities.decodingInfo. A headed window cannot be opened here, so
// this is the question the player's own code would ask, not a playback.
//   node tools/playcheck.mjs file.mp4 [more files ...]
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { launchPage, VIDEO_DIR } from './cdp.mjs';

const PY = process.env.PGR_PYTHON || path.join(VIDEO_DIR, '.venv', process.platform === 'win32' ? 'Scripts/python.exe' : 'bin/python');
const files = process.argv.slice(2);
if (!files.length) { console.error('usage: node tools/playcheck.mjs file.mp4 ...'); process.exit(2); }
const page = await launchPage({ width: 1280, height: 720, scale: 1, profile: 'play' });
let any = 0;
try {
  await page.send('Page.navigate', { url: 'about:blank' });
  await new Promise((r) => setTimeout(r, 400));
  console.log('browser:', await page.evaluate('navigator.userAgent'));
  for (const f of files) {
    const r = spawnSync(PY, ['-I', path.join(VIDEO_DIR, 'tools', 'ffprobe.py'), f], { encoding: 'utf8' });
    if (r.status) { console.log(`${f}: ffprobe failed`); any++; continue; }
    const p = JSON.parse(r.stdout);
    const q = { type: p.type, width: p.width, height: p.height, framerate: p.fps, bitrate: Math.round((p.video_bit_rate_kbps || p.mbps * 1000) * 1000) };
    const res = await page.evaluate(`(async () => {
      const q = ${JSON.stringify(q)}, out = { canPlayType: document.createElement('video').canPlayType(q.type) || 'no' };
      try {
        const d = await navigator.mediaCapabilities.decodingInfo({ type: 'file', video: { contentType: q.type, width: q.width, height: q.height, bitrate: q.bitrate, framerate: q.framerate } });
        out.supported = d.supported; out.smooth = d.smooth; out.powerEfficient = d.powerEfficient;
      } catch (e) { out.error = String(e); }
      return out;
    })()`);
    if (!res.supported) any++;
    console.log(`${path.basename(f)}  ${p.width}x${p.height} ${p.fps} fps ${(q.bitrate / 1e6).toFixed(1)} Mbit/s  ${p.type}`);
    console.log(`   canPlayType: ${res.canPlayType}   supported: ${res.supported}   smooth: ${res.smooth}   power efficient: ${res.powerEfficient}${res.error ? '   error: ' + res.error : ''}`);
  }
} finally { await page.close(); }
process.exitCode = any ? 1 : 0;
