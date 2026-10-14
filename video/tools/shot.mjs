// Single frames of a film, to look at.
//   node tools/shot.mjs <film> 3.2 12.5 [--scale 1|2] [--n 4] [--out scratch/shots] [--format png|jpeg]
// Writes <out>/<film>-<t>.<ext> at 1920 x 1080 times the scale. --n sets the sub-frames of the shutter (default: what the film asks for).
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { launchPage, openAndWait, VIDEO_DIR } from './cdp.mjs';

const a = process.argv.slice(2);
const film = a.shift();
const times = [];
const o = { scale: 1, out: path.join(VIDEO_DIR, 'scratch', 'shots'), format: 'png', n: 0 };
for (let i = 0; i < a.length; i++) {
  if (a[i] === '--scale') o.scale = Number(a[++i]);
  else if (a[i] === '--out') o.out = path.resolve(a[++i]);
  else if (a[i] === '--format') o.format = a[++i];
  else if (a[i] === '--n') o.n = Number(a[++i]);
  else times.push(Number(a[i]));
}
fs.mkdirSync(o.out, { recursive: true });
const page = await launchPage({ width: 1920, height: 1080, scale: o.scale, profile: 'shot' });
try {
  await openAndWait(page, pathToFileURL(path.join(VIDEO_DIR, 'scenes', film + '.html')).href);
  console.log('gpu:', await page.evaluate('window.PGR.gpu'), ' float targets:', await page.evaluate('window.PGR.floatTargets'));
  for (const t of times) {
    const t0 = Date.now();
    const n = await page.evaluate(`window.PGR.seek(${t}, ${o.n ? `{n: ${o.n}}` : '{}'})`);
    const tDraw = Date.now() - t0;
    const r = await page.send('Page.captureScreenshot', o.format === 'jpeg' ? { format: 'jpeg', quality: 97, fromSurface: true } : { format: 'png', fromSurface: true });
    const file = path.join(o.out, `${film}-${t.toFixed(2)}${o.scale > 1 ? '@' + o.scale + 'x' : ''}.${o.format === 'jpeg' ? 'jpg' : 'png'}`);
    fs.writeFileSync(file, Buffer.from(r.data, 'base64'));
    console.log(`t=${t} sub-frames ${n}  draw ${tDraw} ms  total ${Date.now() - t0} ms  ${file}`);
  }
  if (page.logs.length) console.log(page.logs.slice(0, 12).join('\n'));
} finally { await page.close(); }
