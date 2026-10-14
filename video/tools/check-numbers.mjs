// Checks the words and the numbers of a film against the site: what is written on screen, spoken, in the script and in the captions.
//
//   node tools/check-numbers.mjs trailer|explainer       the check
//   node tools/check-numbers.mjs selftest                plants every kind of mistake and says whether the check catches it
//
// 1. Numbers. The film is sought every half second, at the end of every cue, a second after every cue starts, in the middle of every scene
//    and at the poster frame. The page logs every string it draws (window.PGR.drawn()). Every figure must be a number of
//    docs/data/showcase.js (build/facts.json, the file the narration is filled from). The one exception is a numeral that is counting up
//    towards a figure (the page tags it with the figure it is counting to): a value short of that figure passes, the figure itself is
//    checked like any other, so a wrong headline number fails even though counters are in motion by design.
// 2. Words. The models are named as benchmark/data/models.json names them ("Claude Haiku 5.5, low effort"): exactly those names,
//    and "<effort> effort", are let through. Any other model or vendor name, "private preview", "ad", a model class ("light class",
//    "mid", "three model classes") and a tier letter fail, in the script, the narration, the captions and on screen.
// 3. Layout. Display type (40 px and up) ends left of x 1800 while the camera is at rest, and no string drawn ends in an ellipsis.
// With PGR_MUTATE=pct or range the page is loaded with a wrong headline figure; the check must then fail (selftest does that).
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { pathToFileURL, fileURLToPath } from 'node:url';
import { launchPage, openAndWait, VIDEO_DIR } from './cdp.mjs';

const read = (p) => JSON.parse(fs.readFileSync(p, 'utf8'));
const MODELS = read(path.resolve(VIDEO_DIR, '..', 'benchmark', 'data', 'models.json'));

// ---- the words
const BANNED = /claude|anthropic|openai|gpt|haiku|sonnet|opus|gemini|codex|\beffort\b|private preview|\bads?\b/i;
const CLASSY = /\b(?:light|mid|middle|top)[ -](?:model[ -])?(?:class(?:es)?|tier)\b|\bmodel classes\b|\bclasses:\s*light\b|\btier[\s_-]*[abc]\b|^\s*(?:light|mid|top)\s*$|^\s*(?:light|mid|top),/i;
const ELLIPSIS = /(\.\.\.|…)\s*$/;

// The names that are let through: the models of models.json, with their effort, in the caption form and the spoken form (the version in words).
export function allowedNames(facts) {
  const out = [];
  for (const t of MODELS.tiers || []) {
    const cls = { C: 'light', B: 'mid', A: 'top' }[t.tier];
    const said = facts && facts.say && facts.say['name_' + cls];
    for (const n of [t.name, said].filter(Boolean)) out.push(`${n}, ${t.effort} effort`, `${n} at ${t.effort} effort`, n);
    out.push(`${t.effort} effort`);
  }
  return [...new Set(out)].sort((a, b) => b.length - a.length);
}
export const mask = (s, names) => names.reduce((acc, n) => acc.split(n).join(' '), String(s));
export function wordProblems(s, names) {
  const m = mask(s, names), out = [];
  const b = m.match(BANNED); if (b) out.push(`a word that must not be there: "${b[0]}"`);
  const c = m.match(CLASSY); if (c) out.push(`a model class or tier label: "${c[0].trim()}"`);
  return out;
}

// ---- the numbers
const figures = (s) => (String(s).match(/\d[\d.]*\s?[kM%]?/g) || []).map((f) => f.replace(/\s/g, '').replace(/\.+$/, ''));
function buildAllowed(facts) {
  const allowed = new Set();
  const add = (...vals) => vals.forEach((v) => figures(v).forEach((f) => allowed.add(f.replace(/\s/g, ''))));
  [facts.best, facts.control, ...facts.rows].forEach((p) => add(p.tokText, p.turnsText, p.shareText, p.ofText));
  add(facts.pct + '%', facts.fewerText, facts.study.agents, facts.study.files, facts.study.runsMin, facts.study.runsMax, facts.study.configsPublished);
  facts.cells.forEach((q) => add(q.pct + '%', q.perfect, q.runs));
  add(facts.range.min + '%', facts.range.max + '%', facts.range.n, facts.reliable.perfect, facts.reliable.runs);
  add(...Object.values(facts.cap).filter((v) => !/[a-z]{3}/i.test(v)));     // every figure the narration may say, in digits (the model names are words)
  add('100', '100k', '0', facts.modelsNote);                               // "one dot = 100 tokens", the counters' start, the dates of the models note
  for (let i = 1; i <= Math.ceil(facts.control.turns); i++) add(String(i)); // the turn counter shows 1 .. the no index turns
  return allowed;
}
const value = (f) => { const m = /^(\d+(?:\.\d+)?)([kM%]?)/.exec(f); return m ? Number(m[1]) * (m[2] === 'k' ? 1e3 : m[2] === 'M' ? 1e6 : 1) : NaN; };
// a numeral in motion: the page says what it is counting to; anything short of that is on its way
export const onItsWay = (d, f) => d.counting && d.text !== d.counting && value(f) < Math.max(...figures(d.counting).map(value));

// ---- the selftest: every kind of mistake, planted, and whether it is caught
function selfTest() {
  const facts = read(path.join(VIDEO_DIR, 'build', 'facts.json'));
  const names = allowedNames(facts), allowed = buildAllowed(facts);
  let bad = 0;
  const expect = (what, got, want) => { const ok = got === want; if (!ok) bad++; console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${what}: ${got ? 'refused' : 'passed'}${ok ? '' : `, expected ${want ? 'refused' : 'passed'}`}`); };
  const words = (s) => wordProblems(s, names).length > 0;
  const nums = (s) => figures(mask(s, names)).some((f) => !allowed.has(f));
  expect('the light model, as the site names it', words('One comparison: Claude Haiku 5.5, low effort, first question set.'), false);
  expect('the mid model, as the site names it', words('Claude Sonnet 5.5, medium effort'), false);
  expect('the top model, as the site names it', words('Claude Opus 5.5, maximum effort'), false);
  expect('the light model in the spoken form', words('One comparison: Claude Haiku five point five at low effort, first question set.'), false);
  expect('a model of another version', words('Claude Haiku 6, low effort'), true);
  expect('a model of another vendor', words('Measured with GPT five.'), true);
  expect('"light class"', words('One comparison: light class, first question set.'), true);
  expect('"Three model classes"', words('Three model classes: light, mid and top.'), true);
  expect('a bare "Light, original set" label', words('Light, original set'), true);
  expect('"Tier C"', words('Run it on Tier C.'), true);
  expect('"private preview"', words('PeterGanuRusty. Private preview.'), true);
  expect('the word "ad"', words('Watch the ad.'), true);
  expect('a figure that is not on the site (64%)', nums('64% fewer, against no index.'), true);
  expect('a figure that is not on the site (777 agents)', nums('777 agents'), true);
  expect('a figure of the site (54% fewer)', nums('54% fewer, against no index.'), false);
  expect('the version in a model name is not a figure', nums('Claude Haiku 5.5, low effort'), false);
  expect('an ellipsis at the end of a string', ELLIPSIS.test('parameter names and the functions t…'), true);
  const fixed = { text: '40%', counting: '54%' };
  expect('a counter on its way (40% of 54%)', !onItsWay(fixed, '40%'), false);
  expect('a counter past its figure (60% of 54%)', !onItsWay({ text: '60%', counting: '54%' }, '60%'), true);
  expect('a counter that has landed on a wrong figure', !onItsWay({ text: '64%', counting: '64%' }, '64%'), true);
  // the check on the page, with a wrong headline figure
  for (const mut of ['pct', 'range']) {
    const r = spawnSync(process.execPath, [fileURLToPath(import.meta.url), 'explainer'], { env: { ...process.env, PGR_MUTATE: mut }, encoding: 'utf8' });
    const failed = r.status === 1;
    if (!failed) bad++;
    console.log(`  ${failed ? 'ok  ' : 'FAIL'} the film drawn with a wrong ${mut === 'pct' ? 'headline percentage (+10)' : 'top of the range (+1)'}: ${failed ? 'refused' : 'passed'}`);
  }
  console.log(bad ? `${bad} plant(s) not caught` : 'every planted mistake was caught');
  process.exitCode = bad ? 1 : 0;
}

const film = process.argv[2];
if (film === 'selftest') { selfTest(); process.exit(process.exitCode || 0); }

const facts = read(path.join(VIDEO_DIR, 'build', 'facts.json'));
const tl = read(path.join(VIDEO_DIR, 'build', `${film}.timeline.json`));
const names = allowedNames(facts), allowed = buildAllowed(facts);
// the install commands carry a few digits of their own (the account name, the file names); they are the strings of install.json, read as they are
const commands = facts.installers.items.map((i) => i.command);
const isCommandText = (tx) => { const s = tx.trim(); return s.length > 6 && commands.some((c) => c.includes(s)); };
let bad = 0, badWords = 0, badLayout = 0, seen = 0;

// the script, the narration, the captions
const script = fs.readFileSync(path.join(VIDEO_DIR, 'script', film + '.json'), 'utf8');
const spoken = tl.scenes.flatMap((sc) => sc.cues.map((c) => c.spoken + ' ' + (c.text || ''))).join(' ');
const vtts = [path.join(VIDEO_DIR, 'build', `${film}.${tl.voice}.en.vtt`), path.resolve(VIDEO_DIR, '..', 'docs', 'media', `pgr-${film}.en.vtt`)].filter((p) => fs.existsSync(p));
const captions = vtts.map((p) => fs.readFileSync(p, 'utf8').replace(/^\d+$|^[\d:.]+ --> [\d:.]+$|^WEBVTT$/gm, ''));
for (const [what, text] of [['script', script], ['narration', spoken], ...vtts.map((p, i) => ['captions ' + path.basename(p), captions[i]])]) {
  const probs = wordProblems(text, names);
  if (probs.length) { badWords++; console.log(`  ${what}: ${probs.join('; ')}`); }
}
// a caption must not break a model name over two lines: the site's guard refuses "Claude" or "Haiku" alone
for (const [i, p] of vtts.entries()) {
  const broken = captions[i].split('\n').map((l) => l.trim()).filter((l) => /(^|\s)Claude(\s+(Haiku|Sonnet|Opus))?$/.test(l));
  if (broken.length) { badWords++; console.log(`  captions ${path.basename(p)}: a model name is broken over two lines: ${broken.join(' | ')}`); }
}

const times = [];
for (const s of tl.scenes) {
  times.push(s.start + s.dur / 2);
  for (const c of s.cues) { times.push(c.end - 0.05); times.push(c.start + 1.0); }
}
for (let t = 0.5; t < tl.duration; t += 0.5) times.push(t);
if (tl.poster) times.push(tl.poster);
times.push(tl.duration - 0.2);
const page = await launchPage({ width: 1920, height: 1080, scale: 1, profile: 'numbers' });
try {
  const mut = process.env.PGR_MUTATE;
  if (mut) {
    // a wrong headline figure, set before the scenes read it
    await page.send('Page.addScriptToEvaluateOnNewDocument', { source: `(function(){var f;Object.defineProperty(window,'FACTS',{configurable:true,get:function(){return f;},set:function(v){f=v;
      if('${mut}'==='pct'){v.pct+=10;v.cap.pct=v.pct+'%';} else {v.range.max+=1;v.cap.range_max=v.range.max+'%';}}});})();` });
  }
  await openAndWait(page, pathToFileURL(path.join(VIDEO_DIR, 'scenes', film + '.html')).href);
  for (const t of times) {
    await page.evaluate(`window.PGR.seek(${t}, {n: 1})`);
    const drawn = await page.evaluate('window.PGR.drawn()');
    const live = drawn.filter((d) => d.a > 0.3 && d.x > -50 && d.x < 1970 && d.y > -50 && d.y < 1130);
    for (const d of live) {
      const probs = wordProblems(d.text, names);
      if (probs.length) { badWords++; console.log(`  t=${t.toFixed(2)}: on screen: ${probs.join('; ')}   [${d.text}]`); }
      if (ELLIPSIS.test(d.text)) { badLayout++; console.log(`  t=${t.toFixed(2)}: a string drawn cut short with an ellipsis: ${d.text}`); }
    }
    // display type must end left of x 1800 while the camera is at rest (a frame that moves more than 3 output pixels is not at rest)
    const rest = (await page.evaluate(`window.PGR.subframes(${t}).d`)) < 3;
    if (rest) for (const d of live.filter((q) => q.size >= 40 && q.sr !== undefined && q.k > 0.9 && q.k < 1.3)) {
      if (d.sr > 1800 || d.sx < 100) { badLayout++; console.log(`  t=${t.toFixed(2)}: display type outside the margins (${d.sx.toFixed(0)} to ${d.sr.toFixed(0)}): ${d.text}`); }
    }
    const found = live.filter((d) => !isCommandText(d.text)).flatMap((d) => figures(mask(d.text, names)).filter((f) => !allowed.has(f) && !onItsWay(d, f)).map((f) => ({ f, d })));
    seen += live.filter((d) => !isCommandText(d.text)).flatMap((d) => figures(mask(d.text, names))).length;
    if (found.length) { bad++; console.log(`  t=${t.toFixed(2)}: not a site number: ${found.map((x) => x.f).join(', ')}   [${live.map((d) => d.text).join(' | ')}]`); }
  }
  console.log(`${film}: ${times.length} frames read, ${seen} figures, ${bad} frames with a figure that is not in showcase.js; ${badWords} wrong words (a model or vendor name, a class label, private preview, ad); ${badLayout} layout faults (display type past the margins, a cut-short string)${mut ? `   [mutation: ${mut}]` : ''}`);
} finally { await page.close(); }
process.exitCode = bad || badWords || badLayout ? 1 : 0;
