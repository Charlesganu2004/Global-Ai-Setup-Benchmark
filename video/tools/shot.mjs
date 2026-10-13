// Developer check: render a few frames of a film to PNG files, to look at them.
//   node tools/shot.mjs explainer 3.2 12.5 40 [--scale 1] [--out dir]
import { launchPage, openAndWait, VIDEO_DIR } from './cdp.mjs';
import path from 'node:path';
import fs from 'node:fs';
import { pathToFileURL } from 'node:url';

const args = process.argv.slice(2);
const film = args.shift();
const opt = { scale: 1, out: path.join(VIDEO_DIR, 'scratch', 'shots') };
const times = [];
for (let i = 0; i < args.length; i++) {
  if (args[i] === '--scale') opt.scale = Number(args[++i]);
  else if (args[i] === '--out') opt.out = path.resolve(args[++i]);
  else times.push(Number(args[i]));
}
fs.mkdirSync(opt.out, { recursive: true });
const page = await launchPage({ width: 1920, height: 1080, scale: opt.scale });
try {
  await openAndWait(page, pathToFileURL(path.join(VIDEO_DIR, 'scenes', film + '.html')).href);
  for (const t of times) {
    await page.evaluate(`window.PGR.seek(${t})`);
    const r = await page.send('Page.captureScreenshot', { format: 'png', fromSurface: true });
    const f = path.join(opt.out, `${film}-${String(t).replace('.', '_')}.png`);
    fs.writeFileSync(f, Buffer.from(r.data, 'base64'));
    console.log(f);
  }
  if (page.logs.length) console.log(page.logs.join('\n'));
} finally { await page.close(); }
