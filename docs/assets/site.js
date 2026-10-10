/* Shared code for every page: data lookups, number formatters, element
   builders, the table builder, the tooltip, the header, the footer and the
   theme toggle. Everything is exposed on window.Site.

   The data lookups and formatters touch no DOM, so a script can load this
   file with only a global `window` and call them. */
(function () {
  'use strict';

  var REPO_URL = 'https://github.com/Charlesganu2004/Global-Ai-Setup-Benchmark';
  var SITE_NAME = 'Local index benchmark';
  // The pages in reading order, in the two groups the rail shows them under.
  var PAGES = [
    { id: 'home', href: 'index.html', name: 'Home', group: 'Findings' },
    { id: 'results', href: 'results.html', name: 'Results', group: 'Findings' },
    { id: 'orchestration', href: 'orchestration.html', name: 'Orchestration', group: 'Findings' },
    { id: 'hooks', href: 'hooks.html', name: 'Hooks', group: 'Findings' },
    { id: 'methods', href: 'methods.html', name: 'Methods', group: 'Method and setup' },
    { id: 'setup', href: 'setup.html', name: 'Setup', group: 'Method and setup' },
    { id: 'how', href: 'how.html', name: 'How it was measured', group: 'Method and setup' }
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
  // One row per method for a setup and a tier, fewest tokens first. A rerun made to test
  // token-goat's hooks is not a method: those are compared on the hooks page.
  function leaderboard(setupId, tierId) {
    var rows = pointsFor(setupId).filter(function (p) { return p.tier === tierId && typeof p.tokens === 'number' && arm(p.arm).group !== 'hooks'; });
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

  /* ---------- what a table cell sorts as (pure) ----------
     '1,199,654', '$0.0018', '97%', '4.5 times', '130k', '1.00M' and '-34%' sort as numbers,
     '2 of 3 runs' as the share. A cell with no number in it sorts last: null. */
  function sortValue(cell, numeric) {
    var text = (isNode(cell) ? cell.textContent : cell == null ? '' : String(cell)).trim();
    if (!numeric) return text.toLowerCase();
    var clean = text.replace(/[$,]/g, '');
    var of = /(-?\d+(?:\.\d+)?)\s+of\s+(\d+(?:\.\d+)?)/.exec(clean);
    if (of) return +of[2] ? +of[1] / +of[2] : null;
    var m = /[-+]?\d+(?:\.\d+)?/.exec(clean);
    if (!m) return null;
    var unit = clean.charAt(m.index + m[0].length);
    return +m[0] * (unit === 'k' ? 1e3 : unit === 'M' ? 1e6 : 1);
  }

  /* ---------- the jump list (pure): pages, the sections of every page, every method ---------- */
  function jumpIndex() {
    var out = [];
    PAGES.forEach(function (p) { out.push({ kind: 'Page', title: p.name, href: p.href, where: '' }); });
    list('sections').forEach(function (s) {
      var p = PAGES.find(function (x) { return x.id === s.page; });
      if (p) out.push({ kind: 'Section', title: s.title, href: p.href + '#' + s.id, where: p.name });
    });
    list('arms').forEach(function (a) {
      out.push({ kind: 'Method', title: a.id + '  ' + a.name, href: 'methods.html#m-' + a.id, where: 'Methods' });
    });
    return out;
  }
  // Every typed word must be in the title, the page name or the kind. A title that starts with the text comes first.
  function jumpMatches(index, typed, limit) {
    var words = String(typed || '').trim().toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return [];
    var hits = index.filter(function (item) {
      var hay = (item.title + ' ' + item.where + ' ' + item.kind).toLowerCase();
      return words.every(function (w) { return hay.indexOf(w) >= 0; });
    });
    var first = words.join(' ');
    hits.sort(function (x, y) {
      return (y.title.toLowerCase().indexOf(first) === 0) - (x.title.toLowerCase().indexOf(first) === 0);
    });
    return hits.slice(0, limit || 8);
  }

  var api = {
    sortValue: sortValue, jumpIndex: jumpIndex, jumpMatches: jumpMatches,
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
     head: [[label, numeric, className, hideLabel], ...]; rows: arrays of strings or nodes.
     opts, all optional:
       sort     false keeps the given order; otherwise every labelled column sorts
       key      indexes of the columns of the short view, shown first when the table is wide
       primary  index of the column that answers the question, drawn a little stronger
       filter   true, or the indexes of the columns to search: adds a box that hides the rows
                which do not hold every typed word. A typed word matches the start of a word in
                those columns; one or two characters must be a whole word, so 'C' finds Tier C
                and not every row with a c in it.
       limit    show this many rows first, with a button for the rest
       noun     what one row is, for the counts: 'run', 'method'
       rowHeader  index of the cell that names the row (default 0); it is a th with scope="row"
     A table over 15 rows scrolls inside its own frame, under a header row and beside a first
     column that stay put. The sort, the filter text and the column view survive a redraw of
     the same table, so changing a page filter does not undo them. */
  function table(host, head, rows, label, opts) {
    opts = opts || {};
    if (!rows.length) {
      host.replaceChildren(el('p', { class: 'note' }, 'Nothing was measured for this selection.'));
      return;
    }
    var signature = head.map(function (h) { return h[0]; }).join('|');
    var state = host.tableState && host.tableState.signature === signature ? host.tableState
      : { signature: signature, sort: null, words: '', all: false, open: false };
    host.tableState = state;
    var noun = opts.noun || 'row', nouns = noun + 's';
    var key = Array.isArray(opts.key) && opts.key.length < head.length ? opts.key : null;
    var phone = !!(window.matchMedia && window.matchMedia('(max-width: 520px)').matches);
    var shortView = !!key && (head.length > 9 || phone);
    var canSort = opts.sort !== false;

    var frame = el('div', { class: 'table-frame' }), bar = el('div', { class: 'table-bar' });
    var wrap = el('div', { class: 'scroll', tabindex: '0', role: 'region', 'aria-label': label || 'Table' });
    var t = el('table'), thead = el('thead'), tr = el('tr'), body = el('tbody');
    var said = el('span', { class: 'visually-hidden', 'aria-live': 'polite' });
    if (label) t.appendChild(el('caption', { class: 'visually-hidden' }, label));

    var WORD_BREAK = /[^a-z0-9.+-]+/;   // what separates two words, in a cell and in the filter box alike
    var pin = head.length > 5;   // a wide table keeps its first column in view
    var rowHeader = typeof opts.rowHeader === 'number' ? opts.rowHeader : 0;
    // The classes of one column. The column's own class (a width, a bar) is for its cells, not its header.
    function classes(i, header) {
      var h = head[i] || [], out = [];
      if (h[1]) out.push('num');
      if (h[2] && !header) out.push(h[2]);
      if (key && key.indexOf(i) < 0) out.push('xcol');
      if (opts.primary === i) out.push('primary');
      if (pin && i === 0) out.push('pin');
      return out.join(' ');
    }
    var ths = head.map(function (h, i) {
      var th = el('th', { scope: 'col' });
      var cls = classes(i, true);
      if (cls) th.className = cls;
      if (h[3]) th.appendChild(el('span', { class: 'visually-hidden' }, h[0]));
      else if (!canSort) th.textContent = h[0];
      else {
        var button = el('button', { type: 'button', class: 'sort' }, h[0]);
        button.appendChild(el('span', { class: 'mark', 'aria-hidden': 'true' }));
        button.addEventListener('click', function () {
          var same = state.sort && state.sort.col === i;
          state.sort = { col: i, dir: same && state.sort.dir === 'ascending' ? 'descending' : 'ascending' };
          arrange();
          said.textContent = 'Sorted by ' + h[0] + ', ' + state.sort.dir + '.';
        });
        th.appendChild(button);
        th.setAttribute('aria-sort', 'none');
        th.sortMark = button.lastChild;
      }
      tr.appendChild(th);
      return th;
    });
    thead.appendChild(tr);

    var items = rows.map(function (row, index) {
      var line = el('tr');
      row.forEach(function (cell, i) {
        var td = i === rowHeader ? el('th', { scope: 'row' }) : el('td'), cls = classes(i);
        if (cls) td.className = cls;
        if (isNode(cell)) td.appendChild(cell); else td.textContent = cell == null ? '' : String(cell);
        line.appendChild(td);
      });
      return { tr: line, cells: row, index: index, words: null };
    });
    // The words of the cells the filter searches, lower case.
    function wordsOf(item) {
      var from = Array.isArray(opts.filter) ? opts.filter : item.cells.map(function (c, i) { return i; });
      return from.map(function (i) { var c = item.cells[i]; return isNode(c) ? c.textContent : c == null ? '' : String(c); })
        .join(' ').toLowerCase().split(WORD_BREAK).filter(Boolean)
        .reduce(function (all, word) { return all.concat(word.indexOf('-') > 0 ? [word].concat(word.split('-').filter(Boolean)) : [word]); }, []);
    }

    /* ---- controls above the table ---- */
    var count = el('span', { class: 'count', 'aria-live': 'polite' }), more = null, columns = null;
    if (opts.filter) {
      var id = 'filter-' + (host.id || Math.random().toString(36).slice(2));
      var box = el('input', { class: 'search', type: 'search', id: id, autocomplete: 'off', spellcheck: 'false' });
      box.value = state.words;
      box.addEventListener('input', function () { state.words = box.value; arrange(); });
      bar.appendChild(el('label', { for: id }, 'Filter ' + nouns));
      bar.appendChild(box);
      bar.appendChild(count);
    }
    if (key) {
      columns = el('button', { type: 'button', class: 'btn' }, 'All ' + head.length + ' columns');
      columns.addEventListener('click', function () { state.all = !state.all; view(); });
      bar.appendChild(columns);
    }
    if (opts.limit && rows.length > opts.limit) {
      more = el('button', { type: 'button', class: 'btn table-more' });
      more.addEventListener('click', function () { state.open = !state.open; arrange(); });
    }
    var hint = el('p', { class: 'table-hint', 'aria-hidden': 'true' }, 'Scroll sideways for more columns.');
    hint.hidden = true;

    // The short view is the key columns; the button beside the filter brings the rest back.
    function view() {
      var short = shortView && !state.all;
      t.classList.toggle('cols-key', short);
      if (columns) {
        columns.hidden = !shortView;
        columns.setAttribute('aria-pressed', String(!short));
      }
      fit();
    }
    // Order, filter and cut the rows, then say how many are showing.
    function arrange() {
      var s = state.sort, ordered = items.slice();
      if (s && head[s.col]) {
        var numeric = !!head[s.col][1];
        ordered.sort(function (a, b) {
          var x = sortValue(a.cells[s.col], numeric), y = sortValue(b.cells[s.col], numeric);
          var noX = x === null || x === '', noY = y === null || y === '';
          if (noX || noY) return noX && noY ? a.index - b.index : noX ? 1 : -1;   // an empty cell is last either way
          var d = numeric ? x - y : x.localeCompare(y, undefined, { numeric: true, sensitivity: 'base' });
          return (s.dir === 'descending' ? -d : d) || a.index - b.index;
        });
      }
      var words = state.words.toLowerCase().split(WORD_BREAK).filter(Boolean), shown = 0, matched = 0;
      ordered.forEach(function (item) {
        if (item.words === null) item.words = wordsOf(item);
        var hit = words.every(function (w) {
          return item.words.some(function (have) { return w.length < 3 ? have === w : have.indexOf(w) === 0; });
        });
        if (hit) matched++;
        var cut = more && !state.open && !words.length && matched > opts.limit;
        item.tr.hidden = !hit || cut;
        if (!item.tr.hidden) shown++;
        body.appendChild(item.tr);
      });
      ths.forEach(function (th, i) {
        if (!th.sortMark) return;
        var mine = s && s.col === i;
        th.setAttribute('aria-sort', mine ? s.dir : 'none');
        th.sortMark.textContent = mine ? (s.dir === 'ascending' ? '↑' : '↓') : '';
      });
      if (opts.filter) count.textContent = matched + ' of ' + plural(rows.length, noun, nouns);
      if (more) {
        more.hidden = !!words.length;
        more.setAttribute('aria-expanded', String(state.open));
        more.textContent = state.open ? 'Show the first ' + opts.limit : 'Show all ' + plural(rows.length, noun, nouns);
      }
      wrap.classList.toggle('tall', shown > 15);
      fit();
    }
    // Tell the reader when columns are off to the side, in words and in the region's name.
    function fit() {
      var over = wrap.scrollWidth > wrap.clientWidth + 1;
      hint.hidden = !over;
      wrap.setAttribute('aria-label', (label || 'Table') + (over ? ', scrolls sideways' : ''));
    }
    frame.fit = fit;

    t.appendChild(thead); t.appendChild(body);
    wrap.appendChild(t);
    frame.appendChild(bar); frame.appendChild(wrap); frame.appendChild(hint);
    if (more) frame.appendChild(more);
    frame.appendChild(said);
    host.replaceChildren(frame);
    arrange();
    view();
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

  /* ---------- the shell: top bar tools, the rail, the footer ----------
     The brand and the list of pages are plain HTML in every page, so they are
     there with JavaScript off. With it on, the list is rebuilt from PAGES (the
     two cannot drift), the sections of the current page are listed under it,
     and the bar gets its tools. */
  var WIDE = '(min-width: 1200px)';   // the rail is a column from here up, a row of links below
  function isWide() { return !!(window.matchMedia && window.matchMedia(WIDE).matches); }

  function buildNav(nav, current) {
    var groups = [];
    PAGES.forEach(function (page) {
      var group = groups.find(function (g) { return g.name === page.group; });
      if (!group) groups.push(group = { name: page.group, pages: [] });
      group.pages.push(page);
    });
    var sections = el('ul', { class: 'nav-sections', 'aria-label': 'On this page' });
    nav.replaceChildren();
    groups.forEach(function (group, i) {
      var id = 'nav-g' + (i + 1), list = el('ul', { 'aria-labelledby': id });
      nav.appendChild(el('p', { class: 'nav-group', id: id }, group.name));
      group.pages.forEach(function (page) {
        var item = el('li'), link = el('a', { href: page.href }, page.name);
        item.appendChild(link);
        if (page.id === current) { link.setAttribute('aria-current', 'page'); item.appendChild(sections); }
        list.appendChild(item);
      });
      nav.appendChild(list);
    });
    var out = el('p', { class: 'nav-out' });
    out.appendChild(el('a', { href: REPO_URL, rel: 'noopener' }, 'Repository'));
    nav.appendChild(out);
    return sections;
  }

  function buildTools(tools, nav, current) {
    var page = PAGES.find(function (p) { return p.id === current; });
    // On a narrow screen the brand gives way to the name of the page and of the section in view.
    var where = el('span', { class: 'where', 'aria-hidden': 'true' }, page ? page.name : '');
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

    var jump = buildJump();
    tools.replaceChildren(theme);
    tools.parentNode.insertBefore(where, tools);
    // The jump box heads the rail where there is one, and sits in the bar where there is not.
    function place() {
      if (isWide()) nav.insertBefore(jump.box, nav.firstChild);
      else tools.insertBefore(jump.box, tools.firstChild);
    }
    place();
    if (window.matchMedia) {
      var media = window.matchMedia(WIDE);
      if (media.addEventListener) media.addEventListener('change', place);
    }
    return { where: where, jump: jump, page: page };
  }

  /* ---------- jump box: a list of places, not a text search ----------
     Pages, the sections of every page, and every method. Ctrl+K (Command+K) focuses it:
     a shortcut with a modifier, so typing a letter anywhere never triggers it. */
  function buildJump() {
    var box = el('div', { class: 'jump' });
    var input = el('input', { type: 'text', id: 'jump', role: 'combobox', autocomplete: 'off', spellcheck: 'false',
      placeholder: 'Jump to', title: 'Jump to a page, a section or a method', 'aria-expanded': 'false', 'aria-controls': 'jump-list', 'aria-autocomplete': 'list' });
    var listbox = el('ul', { id: 'jump-list', role: 'listbox', 'aria-label': 'Places' });
    var said = el('span', { class: 'visually-hidden', 'aria-live': 'polite' });
    var index = null, hits = [], at = -1;
    listbox.hidden = true;
    box.appendChild(el('label', { class: 'visually-hidden', for: 'jump' }, 'Jump to'));
    box.appendChild(input);
    box.appendChild(el('kbd', { 'aria-hidden': 'true' }, 'Ctrl K'));
    box.appendChild(listbox);
    box.appendChild(said);

    function shut() {
      listbox.hidden = true; at = -1;
      input.setAttribute('aria-expanded', 'false');
      input.removeAttribute('aria-activedescendant');
    }
    function mark() {
      Array.prototype.forEach.call(listbox.children, function (row, i) { row.setAttribute('aria-selected', String(i === at)); });
      if (at >= 0) input.setAttribute('aria-activedescendant', 'jump-' + at); else input.removeAttribute('aria-activedescendant');
    }
    function draw() {
      if (!index) index = jumpIndex();
      hits = jumpMatches(index, input.value, 8);
      listbox.replaceChildren();
      hits.forEach(function (hit, i) {
        var row = el('li', { role: 'option', id: 'jump-' + i, 'aria-selected': 'false' });
        row.appendChild(el('span', { class: 'kind' }, hit.kind));
        row.appendChild(el('span', null, hit.title));
        if (hit.where && hit.kind !== 'Method') row.appendChild(el('span', { class: 'in' }, hit.where));
        row.addEventListener('mousedown', function (e) { e.preventDefault(); go(hit); });
        listbox.appendChild(row);
      });
      at = hits.length ? 0 : -1;
      var open = !!input.value.trim();
      listbox.hidden = !open || !hits.length;
      input.setAttribute('aria-expanded', String(!listbox.hidden));
      said.textContent = !open ? '' : hits.length ? plural(hits.length, 'place', 'places') : 'No match';
      mark();
    }
    function go(hit) { shut(); input.value = ''; window.location.href = hit.href; }
    input.addEventListener('input', draw);
    input.addEventListener('keydown', function (e) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        // After the box lost the focus the list is closed: an arrow opens it again on the typed text.
        if (listbox.hidden) { if (input.value.trim()) { e.preventDefault(); draw(); } return; }
        if (!hits.length) return;
        e.preventDefault();
        at = (at + (e.key === 'ArrowDown' ? 1 : hits.length - 1)) % hits.length;
        mark();
      } else if (e.key === 'Home' && hits.length && !listbox.hidden) { e.preventDefault(); at = 0; mark(); }
      else if (e.key === 'End' && hits.length && !listbox.hidden) { e.preventDefault(); at = hits.length - 1; mark(); }
      else if (e.key === 'Enter' && at >= 0 && !listbox.hidden) { e.preventDefault(); go(hits[at]); }
      else if (e.key === 'Escape') { e.stopPropagation(); input.value = ''; shut(); said.textContent = ''; }
    });
    input.addEventListener('blur', shut);
    document.addEventListener('keydown', function (e) {
      if ((e.key !== 'k' && e.key !== 'K') || !(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey) return;
      if (!box.offsetParent) return;   // hidden on a phone, where the page links are all in view
      e.preventDefault();
      input.focus();
    });
    return { box: box, input: input };
  }

  /* ---------- the sections of this page: the rail list, the anchors, the place in the bar ---------- */
  // A heading's own words, without the '#' link that follows them.
  function titleOf(h) {
    return Array.prototype.filter.call(h.childNodes, function (n) { return !(n.classList && n.classList.contains('anchor')); })
      .map(function (n) { return n.textContent; }).join('').replace(/\s+/g, ' ').trim();
  }
  // The '#' link after a heading. Page scripts call this for headings they build after load.
  function anchor(h) {
    if (!h || !h.id || h.querySelector('.anchor') || h.closest('.verdict')) return;
    h.appendChild(el('a', { class: 'anchor', href: '#' + h.id, 'aria-label': 'Link to section: ' + titleOf(h) }, '#'));
  }
  function buildSections(holder, shell) {
    var main = byId('main');
    if (!main) return;
    // The rail lists every h2, and an h3 that asks for it with data-toc.
    var heads = Array.prototype.slice.call(main.querySelectorAll('h2[id], h3[id][data-toc]'));
    Array.prototype.forEach.call(main.querySelectorAll('h2[id], h3[id]'), anchor);
    // The same list twice: in the rail, and for a narrow screen in a closed block above the first section.
    var folded = el('ul'), block = null;
    var links = heads.map(function (h) {
      var name = h.getAttribute('data-toc') || titleOf(h), sub = h.tagName === 'H3';
      [holder, folded].forEach(function (list) {
        if (!list) return;
        var item = el('li'), link = el('a', { href: '#' + h.id }, name);
        if (sub) item.className = 'sub';
        item.appendChild(link);
        list.appendChild(item);
      });
      return holder ? holder.lastChild.firstChild : folded.lastChild.firstChild;
    });
    if (heads.length > 1) {
      block = el('details', { class: 'onpage' });
      block.appendChild(el('summary', null, 'On this page'));
      var inner = el('nav', { 'aria-label': 'On this page' });
      inner.appendChild(folded);
      block.appendChild(inner);
      var first = heads[0].closest('.verdict') || heads[0];
      first.parentNode.insertBefore(block, first);
      folded.addEventListener('click', function (e) { if (e.target.closest && e.target.closest('a')) block.open = false; });
    }
    var pageName = shell && shell.page ? shell.page.name : '';
    function setCurrent(i) {
      links.forEach(function (link, n) { if (n === i) link.setAttribute('aria-current', 'location'); else link.removeAttribute('aria-current'); });
      if (shell && shell.where) shell.where.textContent = pageName + (i >= 0 ? ': ' + links[i].textContent : '');
      // Keep the current entry in view inside the rail without moving the page.
      var rail = byId('site-nav'), link = links[i];
      if (link && rail && isWide() && rail.scrollHeight > rail.clientHeight) {
        var top = link.offsetTop;
        if (top < rail.scrollTop || top > rail.scrollTop + rail.clientHeight - 40) rail.scrollTop = Math.max(0, top - rail.clientHeight / 2);
      }
    }
    // The section in view is the last heading that has passed under the bar.
    function locate() {
      var line = (parseFloat(getComputedStyle(document.documentElement).scrollPaddingTop) || 64) + 8, found = -1;
      heads.forEach(function (h, i) { if (h.getBoundingClientRect().top <= line) found = i; });
      if (window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 2 && heads.length) found = heads.length - 1;
      setCurrent(found);
    }
    if (heads.length) {
      var waiting = false;
      window.addEventListener('scroll', function () {
        if (waiting) return;
        waiting = true;
        window.requestAnimationFrame(function () { waiting = false; locate(); });
      }, { passive: true });
      locate();
    }
    // A link to something a page script built, such as a method card, lands once it exists.
    if (window.location.hash.length > 1) {
      var target = document.getElementById(decodeURIComponent(window.location.hash.slice(1)));
      if (target) target.scrollIntoView();
    }
  }

  /* ---------- a copy button on every command block ---------- */
  function buildCopy() {
    if (!navigator.clipboard || !navigator.clipboard.writeText) return;
    var said = el('span', { class: 'visually-hidden', 'aria-live': 'polite' });
    document.body.appendChild(said);
    Array.prototype.forEach.call(document.querySelectorAll('main pre'), function (pre) {
      if (pre.parentNode.classList.contains('copy-wrap') || pre.closest('.method')) return;
      var wrap = el('div', { class: 'copy-wrap' }), button = el('button', { type: 'button', class: 'btn copy', 'aria-label': 'Copy these commands' }, 'Copy');
      pre.parentNode.insertBefore(wrap, pre);
      wrap.appendChild(pre);
      wrap.appendChild(button);
      button.addEventListener('click', function () {
        navigator.clipboard.writeText(pre.textContent).then(function () {
          button.textContent = 'Copied'; said.textContent = 'Copied';
          setTimeout(function () { button.textContent = 'Copy'; said.textContent = ''; }, 2000);
        }, function () { button.textContent = 'Not copied'; });
      });
    });
  }

  // The filter bar's height, so a heading reached by a link clears it.
  function measureChrome() {
    var filters = document.querySelector('.filters.sticky');
    if (filters) document.documentElement.style.setProperty('--filters-h', filters.offsetHeight + 'px');
    Array.prototype.forEach.call(document.querySelectorAll('.table-frame'), function (frame) { if (frame.fit) frame.fit(); });
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
  api.table = table; api.legend = legend; api.segmented = segmented; api.anchor = anchor;
  api.showTip = showTip; api.hideTip = hideTip; api.bindTip = bindTip; api.roving = roving; api.KEYS_HINT = KEYS_HINT; api.onRedraw = onRedraw;

  var navHost = byId('site-nav'), toolsHost = byId('site-tools'), current = document.body.getAttribute('data-page');
  var sectionList = navHost ? buildNav(navHost, current) : null;
  var shell = toolsHost && navHost ? buildTools(toolsHost, navHost, current) : null;
  buildFooter();
  // The page scripts run after this file and add headings and command blocks of their own.
  var finish = function () { buildSections(sectionList, shell); buildCopy(); measureChrome(); };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', finish); else finish();
  onRedraw(measureChrome);
  api.ready = !!(window.BENCH && Array.isArray(window.BENCH.points));
  if (!api.ready) {
    var main = byId('main');
    if (main) main.insertBefore(el('p', { class: 'note', role: 'alert' }, 'The data file data/benchmark.js did not load, so this page has no numbers to show.'), main.firstChild);
  }
})();
