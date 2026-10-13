// Checks that every number written on screen is a number of docs/data/showcase.js. It seeks the film at the end of
// every cue and in the middle of every scene, reads the text of the visible elements, and compares each figure
// with the set built from build/facts.json (the same file the narration is filled from).
//   node tools/check-numbers.mjs explainer|ad
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { launchPage, openAndWait, VIDEO_DIR } from './cdp.mjs';

const film = process.argv[2];
const facts = JSON.parse(fs.readFileSync(path.join(VIDEO_DIR, 'build', 'facts.json'), 'utf8'));
const tl = JSON.parse(fs.readFileSync(path.join(VIDEO_DIR, 'build', `${film}.timeline.json`), 'utf8'));

const figures = (s) => (String(s).match(/\d[\d.]*\s?[kM%]?/g) || []).map((f) => f.replace(/\s/g, '').replace(/\.+$/, ''));
const allowed = new Set();
const add = (...vals) => vals.forEach((v) => figures(v).forEach((f) => allowed.add(f.replace(/\s/g, ''))));
const pts = [facts.best, facts.control, ...facts.rows];
pts.forEach((p) => add(p.tokText, p.turnsText, p.shareText, p.ofText));
add(facts.pct + '%', facts.fewerText, facts.study.agents, facts.study.files, facts.study.runsMin, facts.study.runsMax, facts.study.configsPublished);
// the range of the four comparisons, each comparison's saving, and the highlighted configuration's own run count
facts.cells.forEach((q) => add(q.pct + '%'));
add(facts.range.min + '%', facts.range.max + '%', facts.range.n, facts.reliable.perfect, facts.reliable.runs);

const times = [];
for (const s of tl.scenes) {
  times.push(s.start + s.dur / 2);
  for (const c of s.cues) times.push(c.end - 0.05);
}
const page = await launchPage({ width: 1920, height: 1080, scale: 1, profile: 'numbers' });
let bad = 0;
try {
  await openAndWait(page, pathToFileURL(path.join(VIDEO_DIR, 'scenes', film + '.html')).href);
  for (const t of times) {
    await page.evaluate(`window.PGR.seek(${t})`);
    const texts = await page.evaluate(`Array.from(document.querySelectorAll('#ui *')).filter(e => e.children.length === 0 && e.textContent.trim()).filter(e => { const r = e.getBoundingClientRect(), cs = getComputedStyle(e); let p = e, op = 1, vis = true; while (p && p.id !== 'ui') { const c = getComputedStyle(p); if (c.visibility === 'hidden') vis = false; op *= parseFloat(c.opacity); p = p.parentElement; } return vis && op > 0.2 && r.width > 0; }).map(e => e.textContent.trim())`);
    const found = texts.flatMap(figures).map((f) => f.replace(/\s/g, ''));
    // a numeral that is counting up (a leaderboard row) or between the no index figure and the best one is in motion by design
    const maxK = Math.round(Math.max(facts.control.tokens, ...facts.rows.map((r) => r.tokens)) / 1000);
    const counting = (f) => /^\d+k$/.test(f) && parseInt(f) <= maxK;
    const unknown = found.filter((f) => !allowed.has(f) && !counting(f));
    if (unknown.length) { bad++; console.log(`  t=${t.toFixed(2)}: not a site number: ${unknown.join(', ')}   [${texts.join(' | ')}]`); }
  }
  console.log(`${film}: ${times.length} frames read, ${bad} with a figure that is not in showcase.js (allowed: ${[...allowed].sort().join(' ')})`);
} finally { await page.close(); }
process.exitCode = bad ? 1 : 0;
