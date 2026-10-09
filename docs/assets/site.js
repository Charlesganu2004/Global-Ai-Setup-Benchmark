/* Shared code for every page: data lookups, number formatters, element
   builders, the table builder, the tooltip, the header, the footer and the
   theme toggle. Everything is exposed on window.Site.

   The data lookups and formatters touch no DOM, so a script can load this
   file with only a global `window` and call them. */
(function () {
  'use strict';

  var REPO_URL = 'https://github.com/Charlesganu2004/Global-Ai-Setup-Benchmark';
  var SITE_NAME = 'Local index token benchmark';
  var PAGES = [
    { id: 'home', href: 'index.html', name: 'Home' },
    { id: 'results', href: 'results.html', name: 'Results' },
    { id: 'methods', href: 'methods.html', name: 'Methods' },
    { id: 'setup', href: 'setup.html', name: 'Setup' },
    { id: 'how', href: 'how.html', name: 'How it was measured' }
  ];
  var HAS_DOM = typeof document !== 'undefined';

  /* ---------- data lookups (pure) ---------- */
  function bench() { return (typeof window !== 'undefined' && window.BENCH) || {}; }
  function list(key) { var v = bench()[key]; return Array.isArray(v) ? v : []; }
  function byIdIn(key, id, fallback) {
    var found = list(key).find(function (x) { return x.id === id; });
    return found || fallback;
  }
  function arm(id) { return byIdIn('arms', id, { id: String(id), name: String(id), group: '', detail: '', commands: [], reads: '' }); }
  function tier(id) { return byIdIn('tiers', id, { id: String(id), name: 'Tier ' + id, model: '' }); }
  function setup(id) { return byIdIn('setups', id, { id: String(id), name: String(id), detail: '' }); }
  function group(id) { return byIdIn('groups', id, { id: String(id || ''), name: id ? String(id) : 'Ungrouped', detail: '' }); }
  function orderIn(key, id) { var i = list(key).findIndex(function (x) { return x.id === id; }); return i < 0 ? 1e9 : i; }
  function armOrder(id) { return orderIn('arms', id); }
  function tierOrder(id) { return orderIn('tiers', id); }
  function armLabel(a) { return a.id + '  ' + a.name; }
  function inGroup(armId, groupId) { return !groupId || groupId === 'all' || arm(armId).group === groupId; }
  function pointsFor(setupId, groupId) {
    return list('points').filter(function (p) { return p.setup === setupId && inGroup(p.arm, groupId); });
  }
  function runsFor(setupId, groupId) {
    return list('runs').filter(function (c) { return c.setup === setupId && inGroup(c.arm, groupId); });
  }
  function point(armId, setupId, tierId) {
    return list('points').find(function (p) { return p.arm === armId && p.setup === setupId && p.tier === tierId; }) || null;
  }
  function tiersIn(points) {
    return list('tiers').filter(function (t) { return points.some(function (p) { return p.tier === t.id; }); });
  }
  // The tier with the lowest input price; the first listed tier when no prices are given.
  function cheapestTier() {
    var price = bench().price || {}, best = null;
    list('tiers').forEach(function (t) {
      var rate = price[t.id] && price[t.id].input;
      if (typeof rate === 'number' && (best === null || rate < best.rate)) best = { id: t.id, rate: rate };
    });
    if (best) return best.id;
    return list('tiers').length ? list('tiers')[0].id : null;
  }
  // The first setup the data file lists.
  function defaultSetup() {
    var s = list('setups');
    return s.length ? s[0].id : null;
  }
  function groupOptions() {
    var used = list('groups').filter(function (g) { return list('arms').some(function (a) { return a.group === g.id; }); });
    return [{ id: 'all', name: 'All methods' }].concat(used.map(function (g) { return { id: g.id, name: g.name }; }));
  }
  // One row per method for a setup and a tier, fewest tokens first.
  function leaderboard(setupId, tierId) {
    var rows = pointsFor(setupId).filter(function (p) { return p.tier === tierId && typeof p.tokens === 'number'; });
    rows.sort(function (a, b) { return a.tokens - b.tokens || armOrder(a.arm) - armOrder(b.arm); });
    var max = rows.reduce(function (m, p) { return Math.max(m, p.tokens); }, 0);
    return rows.map(function (p, i) {
      return { rank: i + 1, point: p, arm: arm(p.arm), share: max > 0 ? p.tokens / max : 0 };
    });
  }
  function searchArms(query, groupId) {
    var q = String(query || '').trim().toLowerCase();
    return list('arms').filter(function (a) {
      if (!inGroup(a.id, groupId)) return false;
      if (!q) return true;
      var hay = [a.id, a.name, a.detail, a.reads, group(a.group).name].concat(a.commands || []).join('\n').toLowerCase();
      return q.split(/\s+/).every(function (word) { return hay.indexOf(word) >= 0; });
    });
  }

  /* ---------- number formatters (pure) ---------- */
  function ok(v) { return typeof v === 'number' && isFinite(v); }
  function tok(n) {
    if (!ok(n)) return '';
    return n >= 1e6 ? (n / 1e6).toFixed(2) + 'M' : n >= 1e3 ? Math.round(n / 1e3) + 'k' : String(Math.round(n));
  }
  function usd(v) {
    if (!ok(v)) return '';
    if (v === 0) return '$0';
    return v >= 1 ? '$' + v.toFixed(2) : v >= 0.1 ? '$' + v.toFixed(3) : v >= 0.001 ? '$' + v.toFixed(4) : '$' + v.toPrecision(2);
  }
  // A share is never rounded up to 100%: 99.6% is shown as 99%.
  function pct(v) { return !ok(v) ? '' : v >= 0.9995 ? '100%' : Math.min(99, Math.round(v * 100)) + '%'; }
  function num(v, digits) { return ok(v) ? v.toFixed(digits == null ? 1 : digits) : ''; }
  function int(v) { return ok(v) ? Math.round(v).toLocaleString('en-US') : ''; }
  function turns(p) { return num(p.requests, p.n > 1 ? 1 : 0); }
  function turnsText(p) { var t = turns(p); return t + (t === '1' ? ' turn' : ' turns'); }
  // The score columns: one per question the data file lists.
  function questionColumns() { return list('questions').map(function (q, i) { return { key: q.id || 'q' + (i + 1), label: 'Q' + (i + 1) }; }); }
  function plural(n, one, many) { return n + ' ' + (n === 1 ? one : many); }
  function score(scores, key) { return scores && ok(scores[key]) ? pct(scores[key]) : ''; }

  var api = {
    REPO_URL: REPO_URL, SITE_NAME: SITE_NAME, PAGES: PAGES, ready: false,
    list: list, arm: arm, tier: tier, setup: setup, group: group, armOrder: armOrder, tierOrder: tierOrder,
    armLabel: armLabel, inGroup: inGroup, pointsFor: pointsFor, runsFor: runsFor, point: point, tiersIn: tiersIn,
    cheapestTier: cheapestTier, defaultSetup: defaultSetup, groupOptions: groupOptions, leaderboard: leaderboard,
    searchArms: searchArms,
    tok: tok, usd: usd, pct: pct, num: num, int: int, turns: turns, turnsText: turnsText, plural: plural, score: score,
    questionColumns: questionColumns
  };
  window.Site = api;
  if (!HAS_DOM) return;

  /* ---------- element builders ---------- */
  var NS = 'http://www.w3.org/2000/svg';
  function el(name, attrs, text) {
    var e = document.createElement(name);
    if (attrs) Object.keys(attrs).forEach(function (k) { if (k === 'class') e.className = attrs[k]; else e.setAttribute(k, attrs[k]); });
    if (text != null) e.textContent = String(text);
    return e;
  }
  function svgEl(name, attrs, text) {
    var e = document.createElementNS(NS, name);
    if (attrs) Object.keys(attrs).forEach(function (k) { e.setAttribute(k, attrs[k]); });
    if (text != null) e.textContent = String(text);
    return e;
  }
  function byId(id) { return document.getElementById(id); }
  function cssVar(name) { return getComputedStyle(document.documentElement).getPropertyValue(name).trim(); }
  function isNode(v) { return !!v && typeof v === 'object' && typeof v.nodeType === 'number'; }
  function setText(id, text) { var host = byId(id); if (host) host.textContent = text == null ? '' : String(text); }
  function fillList(host, lines) {
    host.replaceChildren();
    (lines || []).forEach(function (line) { host.appendChild(el('li', null, line)); });
  }

  /* ---------- table builder ----------
     head: [[label, numeric, className], ...]; rows: arrays of strings or nodes. */
  function table(host, head, rows, label) {
    var wrap = el('div', { class: 'scroll', tabindex: '0', role: 'region', 'aria-label': label || 'Table' });
    if (!rows.length) {
      host.replaceChildren(el('p', { class: 'note' }, 'Nothing was measured for this selection.'));
      return;
    }
    var t = el('table'), thead = el('thead'), tr = el('tr'), body = el('tbody');
    if (label) t.appendChild(el('caption', { class: 'visually-hidden' }, label));
    head.forEach(function (h) {
      var th = el('th', { scope: 'col' });
      if (h[1]) th.className = 'num';
      if (h[3]) th.appendChild(el('span', { class: 'visually-hidden' }, h[0])); else th.textContent = h[0];
      tr.appendChild(th);
    });
    thead.appendChild(tr);
    rows.forEach(function (row) {
      var line = el('tr');
      row.forEach(function (cell, i) {
        var h = head[i] || [];
        var td = el('td');
        var cls = [h[1] ? 'num' : '', h[2] || ''].join(' ').trim();
        if (cls) td.className = cls;
        if (isNode(cell)) td.appendChild(cell); else td.textContent = cell == null ? '' : String(cell);
        line.appendChild(td);
      });
      body.appendChild(line);
    });
    t.appendChild(thead); t.appendChild(body);
    wrap.appendChild(t);
    host.replaceChildren(wrap);
  }

  /* ---------- legend ---------- */
  function legend(host, items, dot) {
    host.replaceChildren();
    items.forEach(function (item) {
      var span = el('span');
      var key = el('span', { class: dot ? 'key dot' : 'key', 'aria-hidden': 'true' });
      key.style.background = 'var(' + item[1] + ')';
      span.appendChild(key);
      span.appendChild(document.createTextNode(item[0]));
      host.appendChild(span);
    });
  }

  /* ---------- segmented buttons ----------
     A redraw with the same options keeps the buttons, so the one that was
     pressed keeps the keyboard focus. */
  function segmented(host, options, current, onChange) {
    host.segChange = onChange;
    var ids = options.map(function (o) { return String(o.id); }).join('\n');
    if (host.segIds !== ids) {
      var had = document.activeElement, at = -1;
      Array.prototype.forEach.call(host.children, function (child, i) { if (child === had) at = i; });
      host.replaceChildren();
      options.forEach(function (o) {
        var button = el('button', { type: 'button' }, o.name);
        button.addEventListener('click', function () { host.segChange(o.id); });
        host.appendChild(button);
      });
      host.segIds = ids;
      if (at >= 0 && host.children.length) host.children[Math.min(at, host.children.length - 1)].focus();
    }
    options.forEach(function (o, i) { host.children[i].setAttribute('aria-pressed', String(o.id === current)); });
  }

  /* ---------- tooltip: values lead, labels follow ---------- */
  var tip = null, focusedTip = null;   // focusedTip redraws the tooltip of the mark that has the keyboard focus
  function ensureTip() {
    if (tip) return tip;
    tip = el('div', { class: 'tip', role: 'status' });
    tip.hidden = true;
    document.body.appendChild(tip);
    // Focusing a mark that is off screen scrolls the page: the tooltip follows the mark instead of closing.
    window.addEventListener('scroll', function () { if (focusedTip) focusedTip(); else hideTip(); }, { passive: true });
    document.addEventListener('keydown', function (e) { if (e.key === 'Escape') hideTip(); });
    return tip;
  }
  function showTip(title, rows, x, y) {
    ensureTip();
    tip.replaceChildren();
    if (title) tip.appendChild(el('div', { class: 'tip-title' }, title));
    rows.forEach(function (row) {
      var line = el('div', { class: 'tip-row' });
      if (row.color) { var key = el('span', { class: 'line' }); key.style.background = row.color; line.appendChild(key); }
      line.appendChild(el('strong', null, row.value));
      if (row.label) line.appendChild(el('span', { class: 'what' }, row.label));
      tip.appendChild(line);
    });
    tip.hidden = false;
    var box = tip.getBoundingClientRect();
    var left = Math.min(Math.max(8, x + 14), window.innerWidth - box.width - 8);
    var top = y + 16 + box.height > window.innerHeight ? Math.max(8, y - box.height - 12) : y + 16;
    tip.style.left = Math.max(8, left) + 'px';
    tip.style.top = top + 'px';
  }
  function hideTip() { if (tip) tip.hidden = true; }
  function bindTip(target, title, rows, onEnter, onLeave) {
    target.setAttribute('tabindex', '0');
    var atMark = function () {
      if (target.isConnected === false) { focusedTip = null; hideTip(); return; }
      var box = target.getBoundingClientRect();
      showTip(title, rows(), box.left + box.width / 2, box.bottom);
    };
    target.addEventListener('pointermove', function (e) { showTip(title, rows(), e.clientX, e.clientY); if (onEnter) onEnter(); });
    target.addEventListener('pointerleave', function () {
      if (focusedTip) focusedTip(); else hideTip();
      if (onLeave && focusedTip !== atMark) onLeave();
    });
    target.addEventListener('focus', function () { focusedTip = atMark; atMark(); if (onEnter) onEnter(); });
    target.addEventListener('blur', function () { if (focusedTip === atMark) focusedTip = null; hideTip(); if (onLeave) onLeave(); });
    return target;
  }

  /* ---------- one tab stop per chart ----------
     Tab reaches the first mark of a chart, the arrow keys, Home and End move
     between its marks, and the next Tab leaves the chart. */
  function roving(container, marks) {
    if (!marks.length) return;
    var at = 0;
    marks.forEach(function (mark, i) {
      mark.setAttribute('tabindex', i ? '-1' : '0');
      mark.addEventListener('focus', function () { marks[at].setAttribute('tabindex', '-1'); at = i; mark.setAttribute('tabindex', '0'); });
    });
    container.addEventListener('keydown', function (e) {
      var to = e.key === 'ArrowRight' || e.key === 'ArrowDown' ? at + 1 : e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? at - 1
        : e.key === 'Home' ? 0 : e.key === 'End' ? marks.length - 1 : null;
      if (to === null) return;
      e.preventDefault();
      marks[Math.max(0, Math.min(marks.length - 1, to))].focus();
    });
  }
  var KEYS_HINT = 'Arrow keys move between the marks.';

  /* ---------- redraw on resize and on theme change ---------- */
  var redraws = [], pending = 0, lastWidth = window.innerWidth;
  function redrawAll() { focusedTip = null; hideTip(); redraws.forEach(function (fn) { fn(); }); }
  function onRedraw(fn) { redraws.push(fn); }
  window.addEventListener('resize', function () {
    if (window.innerWidth === lastWidth) return;   // a phone's address bar changes only the height
    lastWidth = window.innerWidth;
    clearTimeout(pending);
    pending = setTimeout(redrawAll, 120);
  });
  if (window.matchMedia) {
    var media = window.matchMedia('(prefers-color-scheme: dark)');
    if (media.addEventListener) media.addEventListener('change', redrawAll);
  }

  /* ---------- theme: auto, light, dark ---------- */
  var THEMES = ['auto', 'light', 'dark'], THEME_KEY = 'bench-theme';
  function readTheme() {
    try { var v = window.localStorage.getItem(THEME_KEY); return THEMES.indexOf(v) >= 0 ? v : 'auto'; } catch (e) { return 'auto'; }
  }
  function applyTheme(mode) {
    if (mode === 'auto') document.documentElement.removeAttribute('data-theme');
    else document.documentElement.setAttribute('data-theme', mode);
  }
  function saveTheme(mode) { try { window.localStorage.setItem(THEME_KEY, mode); } catch (e) { /* storage is off: the choice lasts for this page */ } }

  /* ---------- header and footer ---------- */
  function buildHeader() {
    var host = byId('site-header');
    if (!host) return;
    var current = document.body.getAttribute('data-page');
    var wrap = el('div', { class: 'wrap' });
    wrap.appendChild(el('a', { class: 'brand', href: 'index.html' }, SITE_NAME));

    var menu = el('button', { type: 'button', class: 'btn menu-btn', 'aria-expanded': 'false', 'aria-controls': 'site-nav' }, 'Menu');
    var nav = el('nav', { class: 'site-nav', id: 'site-nav', 'aria-label': 'Site' });
    PAGES.forEach(function (page) {
      var link = el('a', { href: page.href }, page.name);
      if (page.id === current) link.setAttribute('aria-current', 'page');
      nav.appendChild(link);
    });
    nav.appendChild(el('a', { class: 'repo', href: REPO_URL, rel: 'noopener' }, 'Repository'));
    menu.addEventListener('click', function () {
      var open = nav.classList.toggle('open');
      menu.setAttribute('aria-expanded', String(open));
    });

    // Tab order is the order on screen at every width: brand, theme, menu, then the links.
    var mode = readTheme();
    var theme = el('button', { type: 'button', class: 'btn theme-btn' });
    function label() {
      theme.textContent = 'Theme: ' + mode;
      theme.setAttribute('aria-label', 'Colour theme: ' + mode + '. Press to change.');
    }
    theme.addEventListener('click', function () {
      mode = THEMES[(THEMES.indexOf(mode) + 1) % THEMES.length];
      applyTheme(mode); saveTheme(mode); label(); redrawAll();
    });
    applyTheme(mode); label();

    wrap.appendChild(theme); wrap.appendChild(menu); wrap.appendChild(nav);
    host.replaceChildren(wrap);
  }
  function buildFooter() {
    var host = byId('site-footer');
    if (!host) return;
    var B = bench(), totals = B.totals || {};
    var wrap = el('div', { class: 'wrap' });
    var facts = [];
    if (B.generated) facts.push('Data generated ' + B.generated + '.');
    var counts = [];
    if (typeof totals.agents === 'number') counts.push(plural(totals.agents, 'agent', 'agents'));
    if (typeof totals.cells === 'number') counts.push(plural(totals.cells, 'graded run', 'graded runs'));
    if (typeof totals.tokens === 'number') counts.push(tok(totals.tokens) + ' tokens measured');
    if (typeof totals.start === 'number') counts.push(tok(totals.start) + ' tokens of start-up context per agent');
    if (counts.length) facts.push(counts.join(', ') + '.');
    wrap.appendChild(el('p', null, facts.join(' ')));
    var line = el('p');
    line.appendChild(document.createTextNode('Code, data and the setup itself: '));
    line.appendChild(el('a', { href: REPO_URL, rel: 'noopener' }, REPO_URL.replace('https://', '')));
    wrap.appendChild(line);
    host.replaceChildren(wrap);
  }

  api.el = el; api.svgEl = svgEl; api.byId = byId; api.cssVar = cssVar; api.setText = setText; api.fillList = fillList;
  api.table = table; api.legend = legend; api.segmented = segmented;
  api.showTip = showTip; api.hideTip = hideTip; api.bindTip = bindTip; api.roving = roving; api.KEYS_HINT = KEYS_HINT; api.onRedraw = onRedraw;

  buildHeader();
  buildFooter();
  api.ready = !!(window.BENCH && Array.isArray(window.BENCH.points));
  if (!api.ready) {
    var main = byId('main');
    if (main) main.insertBefore(el('p', { class: 'note', role: 'alert' }, 'The data file data/benchmark.js did not load, so this page has no numbers to show.'), main.firstChild);
  }
})();
