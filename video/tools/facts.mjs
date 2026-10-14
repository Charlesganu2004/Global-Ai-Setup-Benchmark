// Reads docs/data/showcase.js (window.SHOWCASE) and writes the numbers both films use:
//   build/facts.json  for the voice script (spoken words and caption forms of every number)
//   build/facts.js    for the scenes (window.FACTS, on-screen strings and raw values)
// The headline pair is chosen exactly as the site does in docs/assets/app.js (App.headline): the configuration
// flagged best and the first control, both on the light class and the first question set. It also works out the
// four comparisons the site states as a range (two models by two question sets), and checks the sentence the
// films say about the highlighted configuration (fully correct in every run). Nothing here is typed in by hand.
import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';

const VIDEO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const SHOWCASE_JS = path.resolve(VIDEO, '..', 'docs', 'data', 'showcase.js');
const BUILD = path.join(VIDEO, 'build');
const INSTALL_JSON = path.resolve(VIDEO, '..', 'benchmark', 'data', 'install.json');
const MODELS_JSON = path.resolve(VIDEO, '..', 'benchmark', 'data', 'models.json');
const FALLBACK_INSTALL = 'irm https://github.com/Charlesganu2004/Global-Ai-Setup-Benchmark/releases/latest/download/install.ps1 | iex';

// The install commands the closing card shows: the two of benchmark/data/install.json, or the Windows line when
// that file is not there yet (source says which).
function readInstallers() {
  try {
    const j = JSON.parse(fs.readFileSync(INSTALL_JSON, 'utf8'));
    const items = ((j.public || j).installers || []).filter((i) => i && i.command).map((i) => ({ id: i.id, system: i.system, shell: i.shell, command: i.command }));
    if (items.length) return { source: 'benchmark/data/install.json', items };
  } catch { /* not written yet, or not valid yet */ }
  return { source: 'fallback', items: [{ id: 'windows', system: 'Windows', shell: 'PowerShell', command: FALLBACK_INSTALL }] };
}

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

// The films name the models the runs used, as benchmark/data/models.json does (the site does the same): the light class is
// tier C, the mid class tier B, the top class tier A. A class is never shown or spoken by that word. The label is
// "Claude Haiku 5.5, low effort"; it must equal the class name the site data carries (docs/data/showcase.js).
const TIER_OF = { light: 'C', mid: 'B', top: 'A' };
export function readModels() {
  const j = JSON.parse(fs.readFileSync(MODELS_JSON, 'utf8'));
  const byTier = {};
  for (const t of j.tiers || []) byTier[t.tier] = t;
  const out = {};
  for (const [id, tier] of Object.entries(TIER_OF)) {
    const t = byTier[tier];
    if (!t || !t.name || !t.effort) throw new Error(`benchmark/data/models.json has no name and effort for tier ${tier}`);
    out[id] = { id, tier, name: t.name, effort: t.effort, label: `${t.name}, ${t.effort} effort`, at: `${t.name} at ${t.effort} effort` };
  }
  const dates = String(j.how || '').match(/(\d{4}-\d{2}-\d{2}) to (\d{4}-\d{2}-\d{2})/);
  return { models: out, from: dates && dates[1], to: dates && dates[2] };
}
// The spoken form of a model name: the version read as words ("Claude Haiku five point five").
const sayName = (name) => name.replace(/(\d+)\.(\d+)/g, (m, a, b) => `${words(Number(a))} point ${b.split('').map((d) => ONES[Number(d)]).join(' ')}`);
// spoken without the comma of the caption form, so the voice does not stop in the middle of a name: "Claude Haiku five point five at low effort"
const sayAt = (m) => `${sayName(m.name)} at ${m.effort} effort`;
const classLabel = (models, c) => {
  const m = models[c && c.id];
  if (!m) throw new Error(`models.json has no model for the class "${c && c.id}"`);
  if (c.name && c.name !== m.label) throw new Error(`the site names the class "${c.id}" "${c.name}" but benchmark/data/models.json says "${m.label}"`);
  return m.label;
};

// The headline comparison: the light class (the smallest model), first question set (the site's own headline cell).
const lightPoint = (c, firstSet) => (c
  ? (firstSet && c.points.find((p) => p.class === 'light' && p.set === firstSet && p.setup === 'single'))
    || c.points.find((p) => p.class === 'light' && p.setup === 'single') || c.points.find((p) => p.class === 'light') || null
  : null);

export function compute(data) {
  const { models, from: modelsFrom, to: modelsTo } = readModels();
  let best = null; let control = null;
  for (const c of data.configs || []) {
    if (c.best) best = c;
    if (c.family === 'control' && !control) control = c;
  }
  const firstSet = (data.sets && data.sets[0] && data.sets[0].id) || null;
  const b = lightPoint(best, firstSet); const c = lightPoint(control, firstSet);
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
    const p = lightPoint(cfg, firstSet);
    return p ? { ...point(cfg, p), family: cfg.family, best: !!cfg.best } : null;
  }).filter(Boolean);
  // The comparisons behind the range the site states: the highlighted configuration against no index, one cell for
  // each model class and question set that both were run on.
  const cells = [];
  for (const cls of data.classes || []) {
    for (const set of data.sets || []) {
      const pick = (cfg) => cfg.points.find((q) => q.class === cls.id && q.set === set.id && q.setup === 'single');
      const pb = pick(best); const pc = pick(control);
      if (!pb || !pc || !(pc.tokens > 0)) continue;
      cells.push({
        class: cls.id, className: classLabel(models, cls), set: set.id, setName: set.name,
        bestTokens: pb.tokens, controlTokens: pc.tokens, pct: Math.round((1 - pb.tokens / pc.tokens) * 100),
        bestTurns: pb.turns, controlTurns: pc.turns, perfect: pb.perfect_runs, runs: pb.runs, headline: pb === b,
      });
    }
  }
  if (cells.length < 2) throw new Error('SHOWCASE has fewer than two comparisons of the best configuration against no index');
  if (cells.some((q) => !(q.bestTokens < q.controlTokens))) {
    throw new Error('In at least one comparison the best configuration no longer needs fewer tokens than no index; the range sentence in video/script/*.json must be rewritten.');
  }
  const pcts = cells.map((q) => q.pct);
  const range = { min: Math.min(...pcts), max: Math.max(...pcts), n: cells.length };
  // The site states the same range in data.versus (low and high as shares). When that field is there, the films must
  // say what it says: a disagreement means one of the two is stale, and the narration must not go out.
  const versus = data.versus;
  if (versus && typeof versus.low === 'number' && typeof versus.high === 'number') {
    const vmin = Math.round(versus.low * 100); const vmax = Math.round(versus.high * 100);
    if (vmin !== range.min || vmax !== range.max || (Array.isArray(versus.cells) && versus.cells.length !== cells.length)) {
      throw new Error(`The site states ${vmin}% to ${vmax}% over ${(versus.cells || []).length} comparisons, but the points give ${range.min}% to ${range.max}% over ${cells.length}. Rebuild the site data before rendering.`);
    }
  }
  // The films say the highlighted configuration was fully correct in every run on both question sets. Say it only while it is true.
  const reliable = { perfect: cells.reduce((a, q) => a + q.perfect, 0), runs: cells.reduce((a, q) => a + q.runs, 0), sets: new Set(cells.map((q) => q.set)).size };
  if (reliable.perfect !== reliable.runs || reliable.sets < 2) {
    throw new Error(`The narration says the highlighted configuration was fully correct in every run on both question sets, but SHOWCASE says ${reliable.perfect} of ${reliable.runs} runs on ${reliable.sets} sets. Rewrite video/script/*.json before rendering.`);
  }
  // The word on the highlighted row: the site's field when it has one, else the wording the site uses today.
  const bestLabel = (data.labels && data.labels.best) || data.best_label || 'Most reliable';
  const study = data.study || {};
  const perCell = study.runs_per_cell || { min: 1, max: 1 };
  const num = (n) => String(n);
  const facts = {
    models, modelsNote: modelsFrom ? `The runs asked for a class of model by alias. These are the models those aliases meant from ${modelsFrom} to ${modelsTo}.` : '',
    best: point(best, b), control: point(control, c), pct, fewerText: showTokens(c.tokens - b.tokens),
    rows, cells, range, reliable, bestLabel,
    labels: { classLabel: classLabel(models, (data.classes || [])[0]), firstSet: 'first question set', secondSet: 'held-out question set' },
    study: {
      agents: study.agents, files: study.repository.files, language: study.repository.language, runs: study.runs,
      configsPublished: rows.length, questions: study.questions, classes: (data.classes || []).map((c) => ({ id: c.id, name: classLabel(models, c), model: models[c.id].name, effort: models[c.id].effort })), runsMin: perCell.min, runsMax: perCell.max,
    },
    install: data.install && data.install.status,
    installers: readInstallers(),
    // Spoken forms (words) and caption forms (digits) of every number the narration may use.
    say: {
      tok_ref: tokensWords(c.tokens), tok_best: tokensWords(b.tokens),
      turns_ref: decimalWords(showTurns(c.turns).replace(/\.0$/, '')), turns_best: decimalWords(showTurns(b.turns).replace(/\.0$/, '')),
      pct: words(pct) + ' percent', agents: words(study.agents), files: words(study.repository.files),
      configs: words(rows.length), perfect_best: words(b.perfect_runs), runs_best: words(b.runs),
      perfect_ref: words(c.perfect_runs), runs_ref: words(c.runs), runs_min: words(perCell.min), runs_max: words(perCell.max),
      questions: words((study.questions || []).length), classes: words((data.classes || []).length),
      range_min: words(range.min) + ' percent', range_max: words(range.max) + ' percent', cells: words(range.n),
      perfect_all: words(reliable.perfect), runs_all: words(reliable.runs),
      model_light: sayAt(models.light), model_mid: sayAt(models.mid), model_top: sayAt(models.top),
      name_light: sayName(models.light.name), name_mid: sayName(models.mid.name), name_top: sayName(models.top.name),
    },
    cap: {
      tok_ref: showTokens(c.tokens), tok_best: showTokens(b.tokens),
      turns_ref: showTurns(c.turns).replace(/\.0$/, ''), turns_best: showTurns(b.turns).replace(/\.0$/, ''),
      pct: pct + '%', agents: num(study.agents), files: num(study.repository.files),
      configs: num(rows.length), perfect_best: num(b.perfect_runs), runs_best: num(b.runs),
      perfect_ref: num(c.perfect_runs), runs_ref: num(c.runs), runs_min: num(perCell.min), runs_max: num(perCell.max),
      questions: num((study.questions || []).length), classes: num((data.classes || []).length),
      range_min: range.min + '%', range_max: range.max + '%', cells: num(range.n),
      perfect_all: num(reliable.perfect), runs_all: num(reliable.runs),
      model_light: models.light.label, model_mid: models.mid.label, model_top: models.top.label,
      name_light: models.light.name, name_mid: models.mid.name, name_top: models.top.name,
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
  console.log(`facts: ${f.range.n} comparisons, ${f.range.min}% to ${f.range.max}% fewer (${f.cells.map((q) => `${q.className}, ${q.setName} ${q.pct}%`).join('; ')}); ${f.reliable.perfect} of ${f.reliable.runs} runs fully correct; label "${f.bestLabel}"`);
}
