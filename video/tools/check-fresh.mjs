// Fails when a delivered film file may state figures the site no longer states. A film holds its numbers and labels in
// the picture, so a file older than docs/data/showcase.js is checked against the figures it was drawn from
// (renders/pgr-<film>-<height>p.facts.json, written by tools/build.mjs): the site's build rewrites showcase.js often
// without changing a figure, and only a change in what the films read makes a file stale.
//   node tools/check-fresh.mjs
// Prints each file in docs/media with its time and the verdict, and the time of showcase.js; exit code 1 when any film
// file is older than showcase.js and was drawn from figures that differ from today's.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { load, compute } from './facts.mjs';

const VIDEO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const ROOT = path.resolve(VIDEO, '..');
const DATA = path.join(ROOT, 'docs', 'data', 'showcase.js');
const MEDIA = path.join(ROOT, 'docs', 'media');
const stamp = (d) => d.toLocaleString('sv-SE').replace(' ', 'T');

const dataTime = fs.statSync(DATA).mtime;
const now = JSON.stringify(compute(load()));
const drawnFrom = (film) => {
  try { return JSON.stringify(JSON.parse(fs.readFileSync(path.join(VIDEO, 'renders', `pgr-${film}.facts.json`), 'utf8'))); } catch { return null; }
};
console.log(`showcase.js  ${stamp(dataTime)}`);
let stale = 0;
for (const f of fs.readdirSync(MEDIA).sort()) {
  const m = /^pgr-(trailer|explainer)[.-]/.exec(f);
  if (!m || !/\.(mp4|jpg|vtt)$/.test(f)) continue;
  const t = fs.statSync(path.join(MEDIA, f)).mtime;
  let verdict = 'ok     ';
  let note = '';
  if (t < dataTime) {
    const was = drawnFrom(m[1]);   // every file of a film comes from the same build
    if (was === null) { verdict = 'OLDER  '; note = '  no record of the figures it was drawn from'; stale++; }
    else if (was !== now) { verdict = 'STALE  '; note = '  drawn from figures that differ from showcase.js today'; stale++; }
    else note = '  older than showcase.js, drawn from the same figures and labels';
  }
  console.log(`${verdict}${f.padEnd(40)} ${stamp(t)}${note}`);
}
console.log(stale ? `${stale} file(s) in docs/media were drawn from other figures than showcase.js holds today: build the film again` : 'every film file was drawn from the figures and labels showcase.js holds today');
process.exitCode = stale ? 1 : 0;
