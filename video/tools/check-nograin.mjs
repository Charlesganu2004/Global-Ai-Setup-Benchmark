// Fails if any scene still uses grain, noise or a dither, in the code or in the picture.
//   static: the scenes, the stylesheet and the pages hold no noise texture, no turbulence filter, no random numbers and no dither
//           (comments are not counted)
//   picture: a grid of 12 patches (64 x 40 px) of three frames of each film is read back from the canvas. For each pixel the
//           difference from the mean of its four neighbours is taken; the median of those differences over a patch is 0 on a
//           flat patch or a slow gradient up to 0.5 (a hairline of the floor or the edge of a letter changes a few pixels only) and several
//           levels on grain, which changes every pixel. The median of the 12 patches' medians must stay under 1.0 of 255 (the 8-bit steps of the vignette give up to 0.5).
//   node tools/check-nograin.mjs [trailer|explainer ...]
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { launchPage, openAndWait, VIDEO_DIR } from './cdp.mjs';

const BANNED = /feTurbulence|feTile|fractalNoise|\bgrain\b|\bnoise\b|\bdither|Math\.random|perlin|simplex|film-grain|bluenoise/i;
const stripComments = (s) => s.replace(/\/\*[\s\S]*?\*\//g, '').replace(/(^|[^:'"\\])\/\/.*$/gm, '$1').replace(/<!--[\s\S]*?-->/g, '');
let bad = 0;
for (const f of fs.readdirSync(path.join(VIDEO_DIR, 'scenes'))) {
  if (!/\.(js|css|html)$/.test(f)) continue;
  const code = stripComments(fs.readFileSync(path.join(VIDEO_DIR, 'scenes', f), 'utf8'));
  const m = code.match(BANNED);
  console.log(`  ${m ? 'FAIL' : 'ok  '} scenes/${f}${m ? '  uses ' + m[0] : ''}`);
  if (m) bad++;
}
const films = process.argv.slice(2).length ? process.argv.slice(2) : ['trailer', 'explainer'];
const page = await launchPage({ width: 1920, height: 1080, scale: 1, profile: 'grain' });
try {
  for (const film of films) {
    const tl = JSON.parse(fs.readFileSync(path.join(VIDEO_DIR, 'build', `${film}.timeline.json`), 'utf8'));
    await openAndWait(page, pathToFileURL(path.join(VIDEO_DIR, 'scenes', film + '.html')).href);
    for (const t of [tl.duration * 0.2, tl.duration * 0.5, tl.duration * 0.8]) {
      await page.evaluate(`window.PGR.seek(${t}, {n: 1})`);
      const all = await page.evaluate('(function () { var r = []; for (var gy = 0; gy < 3; gy++) for (var gx = 0; gx < 4; gx++) r.push(window.PGR.roughness(40 + gx * 470, 40 + gy * 480 + 40, 64, 40)); return r; })()');
      const meds = all.map((w) => w.median).sort((p, q) => p - q), m = (meds[5] + meds[6]) / 2, rough = all.map((w) => w.rms).sort((p, q) => p - q);
      const verdict = m < 1.0 ? 'ok  ' : 'FAIL';
      if (m >= 1.0) bad++;
      console.log(`  ${verdict} ${film} t=${t.toFixed(1)}: 12 patches, median of the neighbour differences ${m.toFixed(2)} levels of 255 (rms of the quietest patch ${rough[0].toFixed(2)}, of the noisiest ${rough[11].toFixed(2)})`);
    }
  }
} finally { await page.close(); }
console.log(bad ? `${bad} failure(s)` : 'no grain, no noise, no dither');
process.exitCode = bad ? 1 : 0;
