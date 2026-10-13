// Reads docs/data/showcase.js (window.SHOWCASE) and writes the numbers both films use:
//   build/facts.json  for the voice script (spoken words and caption forms of every number)
//   build/facts.js    for the scenes (window.FACTS, on-screen strings and raw values)
// The headline pair is chosen exactly as the site does in docs/assets/app.js (App.headline): the configuration
// flagged best and the first control, both on the light class. Nothing here is typed in by hand.
import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';

const VIDEO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const SHOWCASE_JS = path.resolve(VIDEO, '..', 'docs', 'data', 'showcase.js');
const BUILD = path.join(VIDEO, 'build');

// The same formats as App.tokens, App.share and App.turns on the site.
const showTokens = (v) => (!(v > 0) ? '0' : v >= 1e6 ? (v / 1e6).toFixed(2) + 'M' : Math.round(v / 1e3) + 'k');
const showShare = (v) => (v >= 0.9995 ? '100%' : Math.min(99, Math.floor(v * 100 + 0.5)) + '%');
const showTurns = (v) => v.toFixed(1);

const ONES = ['zero', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine', 'ten', 'eleven', 'twelve',
  'thirteen', 'fourteen', 'fifteen', 'sixteen', 'seventeen', 'eighteen', 'nineteen'];
const TENS = ['', '', 'twenty', 'thirty', 'forty', 'fifty', 'sixty', 'seventy', 'eighty', 'ninety'];
function below1000(n) {
  const out = [];
  if (n >= 100) { out.push(ONES[Math.floor(n / 100)] + ' hundred'); n %= 100; }
  if (n >= 20) { out.push(TENS[Math.floor(n / 10)] + (n % 10 ? '-' + ONES[n % 10] : '')); } else if (n > 0) out.push(ONES[n]);
  return out.join(' ');
}
export function words(n) {
  n = Math.round(n);
  if (n === 0) return 'zero';
  const parts = [];
  for (const [size, name] of [[1e9, 'billion'], [1e6, 'million'], [1e3, 'thousand']]) {
    if (n >= size) { parts.push(below1000(Math.floor(n / size)) + ' ' + name); n %= size; }
  }
  if (n > 0) parts.push(below1000(n));
  return parts.join(' ');
}
// A decimal read digit by digit after the point: 2.7 is "two point seven".
const decimalWords = (text) => text.split('.').map((s, i) => (i === 0 ? words(Number(s)) : s.split('').map((d) => ONES[Number(d)]).join(' '))).join(' point ');
const tokensWords = (v) => (v >= 1e6 ? decimalWords((v / 1e6).toFixed(1)) + ' million' : words(Math.round(v / 1e3)) + ' thousand');

export function load() {
  const code = fs.readFileSync(SHOWCASE_JS, 'utf8');
  const sandbox = { window: {} };
  vm.createContext(sandbox);
  vm.runInContext(code, sandbox, { filename: 'showcase.js' });
  const data = sandbox.window.SHOWCASE;
  if (!data) throw new Error('docs/data/showcase.js did not define window.SHOWCASE');
  return data;
}

const lightPoint = (c) => (c ? c.points.find((p) => p.class === 'light' && p.setup === 'single') || c.points.find((p) => p.class === 'light') || null : null);

export function compute(data) {
  let best = null; let control = null;
  for (const c of data.configs || []) {
    if (c.best) best = c;
    if (c.family === 'control' && !control) control = c;
  }
  const b = lightPoint(best); const c = lightPoint(control);
  if (!best || !control || !b || !c) throw new Error('SHOWCASE has no best configuration or no control on the light class');
  if (!(b.tokens < c.tokens) || !(b.turns < c.turns)) {
    throw new Error(`The narration says the best configuration needs fewer tokens and fewer turns than no index, but SHOWCASE now says ${b.tokens} tokens and ${b.turns} turns against ${c.tokens} and ${c.turns}. Rewrite video/script/*.json before rendering.`);
  }
  const pct = Math.round((1 - b.tokens / c.tokens) * 100);
  const point = (cfg, p) => ({
    name: cfg.name, slug: cfg.slug, tokens: p.tokens, turns: p.turns, correct: p.correct, perfect: p.perfect_runs, runs: p.runs,
    tokText: showTokens(p.tokens), turnsText: showTurns(p.turns), shareText: showShare(p.correct), ofText: `${p.perfect_runs} of ${p.runs}`,
  });
  const rows = (data.configs || []).map((cfg) => {
    const p = lightPoint(cfg);
    return p ? { ...point(cfg, p), family: cfg.family, best: !!cfg.best } : null;
  }).filter(Boolean);
  const study = data.study || {};
  const perCell = study.runs_per_cell || { min: 1, max: 1 };
  const num = (n) => String(n);
  const facts = {
    built: data.built,
    best: point(best, b), control: point(control, c), pct, fewerText: showTokens(c.tokens - b.tokens),
    rows,
    study: {
      agents: study.agents, files: study.repository.files, language: study.repository.language, runs: study.runs,
      configsPublished: rows.length, questions: study.questions, classes: data.classes, runsMin: perCell.min, runsMax: perCell.max,
    },
    install: data.install && data.install.status,
    // Spoken forms (words) and caption forms (digits) of every number the narration may use.
    say: {
      tok_ref: tokensWords(c.tokens), tok_best: tokensWords(b.tokens),
      turns_ref: decimalWords(showTurns(c.turns).replace(/\.0$/, '')), turns_best: decimalWords(showTurns(b.turns).replace(/\.0$/, '')),
      pct: words(pct) + ' percent', agents: words(study.agents), files: words(study.repository.files),
      configs: words(rows.length), perfect_best: words(b.perfect_runs), runs_best: words(b.runs),
      perfect_ref: words(c.perfect_runs), runs_ref: words(c.runs), runs_min: words(perCell.min), runs_max: words(perCell.max),
      questions: words((study.questions || []).length), classes: words((data.classes || []).length),
    },
    cap: {
      tok_ref: showTokens(c.tokens), tok_best: showTokens(b.tokens),
      turns_ref: showTurns(c.turns).replace(/\.0$/, ''), turns_best: showTurns(b.turns).replace(/\.0$/, ''),
      pct: pct + '%', agents: num(study.agents), files: num(study.repository.files),
      configs: num(rows.length), perfect_best: num(b.perfect_runs), runs_best: num(b.runs),
      perfect_ref: num(c.perfect_runs), runs_ref: num(c.runs), runs_min: num(perCell.min), runs_max: num(perCell.max),
      questions: num((study.questions || []).length), classes: num((data.classes || []).length),
    },
  };
  return facts;
}

export function writeFacts() {
  const facts = compute(load());
  fs.mkdirSync(BUILD, { recursive: true });
  fs.writeFileSync(path.join(BUILD, 'facts.json'), JSON.stringify(facts, null, 1));
  fs.writeFileSync(path.join(BUILD, 'facts.js'), '// Generated by tools/facts.mjs from docs/data/showcase.js. Do not edit.\nwindow.FACTS = ' + JSON.stringify(facts) + ';\n');
  return facts;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const f = writeFacts();
  console.log(`facts: best ${f.best.tokText} tokens ${f.best.turnsText} turns (${f.best.name}); no index ${f.control.tokText} ${f.control.turnsText}; ${f.pct}% fewer; ${f.rows.length} rows`);
}
