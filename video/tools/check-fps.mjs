// The picture at a time must not depend on the frame rate. For 12 timestamps (multiples of 1/30 s, so each is a frame at
// 30, 60 and 120 fps) this draws the frame the way a render at each rate does (index / fps), in a different order for each
// rate, and compares every pixel of the picture with the one drawn at 30 fps. The pictures must match to within the
// rounding of the graphics card's rasteriser: at most 1000 colour channels (of about 6 million, 0.016 percent) may be more than 16 levels away
// (the card draws the antialiased edge of a shape or of a glyph slightly differently once it has been drawn many times, and moves
// a few edge pixels by up to a few tens of levels; the most seen is 527 channels, in the 230 x 100 px badge while it springs in), and the mean difference must stay under 0.01 of 255. Anything the film
// counts in frames, or any state that leaks from one frame into the next, shows up here as a large difference.
//   node tools/check-fps.mjs trailer|explainer [--scale 1|2]
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { launchPage, openAndWait, VIDEO_DIR } from './cdp.mjs';

const a = process.argv.slice(2);
const film = a.shift();
const scale = a[0] === '--scale' ? Number(a[1]) : 1;
const tl = JSON.parse(fs.readFileSync(path.join(VIDEO_DIR, 'build', `${film}.timeline.json`), 'utf8'));
const stamps = [];
for (let k = 0; k < 12; k++) stamps.push(Math.round((tl.duration * (k + 0.5) / 12) * 30) / 30);
const page = await launchPage({ width: 1920, height: 1080, scale, profile: 'fps' });
let bad = 0;
try {
  await openAndWait(page, pathToFileURL(path.join(VIDEO_DIR, 'scenes', film + '.html')).href);
  await page.evaluate('window.PGR.forget()');
  const orders = { 30: stamps.slice(), 60: stamps.slice().reverse(), 120: stamps.slice().sort((x, y) => (Math.round(x * 30) * 7919) % 12 - (Math.round(y * 30) * 7919) % 12) };
  const worst = {};
  for (const fps of [30, 60, 120]) {
    for (const t of orders[fps]) {
      const i = Math.round(t * fps), tt = i / fps;
      await page.evaluate(`window.PGR.seek(${tt.toFixed(9)})`);
      if (fps === 30) await page.evaluate(`window.PGR.snap('${t}')`);
      else (worst[t] = worst[t] || {})[fps] = await page.evaluate(`window.PGR.diff('${t}')`);
    }
  }
  for (const t of stamps) {
    const d = worst[t], m = Math.max(d[60].max, d[120].max), o2 = Math.max(d[60].over2, d[120].over2), o16 = Math.max(d[60].over16, d[120].over16), mean = Math.max(d[60].mean, d[120].mean);
    const same = o16 <= 1000 && mean < 0.01;
    if (!same) bad++;
    console.log(`  ${same ? 'ok  ' : 'FAIL'} t=${t.toFixed(3)}  against 30 fps: 60 fps max ${d[60].max} levels, 120 fps max ${d[120].max} levels, mean ${mean.toExponential(1)}, ${o2} channels over 2 levels, ${o16} over 16${!same && d[60].box ? ', differs inside x ' + d[60].box[0] + '-' + d[60].box[2] + ', y ' + d[60].box[1] + '-' + d[60].box[3] : ''}`);
  }
} finally { await page.close(); }
console.log(`${film}: ${stamps.length} timestamps drawn at 30, 60 and 120 fps settings, ${bad} differ`);
process.exitCode = bad ? 1 : 0;
