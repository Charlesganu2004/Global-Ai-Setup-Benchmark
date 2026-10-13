/* The Benchmarks page: the model class control, the question set control, the leaderboard, the chart of tokens
   and mean score, the engine race and the turn runners. The tables are already in the page for every
   class and every question set. This script shows one class and one set at a time: the leaderboard keeps one row
   for each configuration measured on the set and moves it to its rank, with its bar, ring and numbers following
   the measured values, and a row opens to show its model classes on that set; a configuration not measured on
   the set is not in the view. The turn runners follow both controls. The chart is drawn from window.SHOWCASE as one
   row for each configuration, the fewest tokens first: a bar and a dot for the mean tokens on one scale, the line from
   the lowest to the highest run, a ring for the mean score, the best configuration and the control marked, the names
   written in the rows, and a table view of the same numbers. Until this script has run to its end the tables stay as
   the page wrote them. */
(function () {
  'use strict';
  var App = window.App, data = window.SHOWCASE, root = document.documentElement;
  var seg = document.querySelector('.seg[data-seg="class"]') || document.querySelector('.seg');
  var setSeg = document.querySelector('.seg[data-seg="set"]');
  if (!App || !data || !data.configs || !seg) return;

  var NS = 'http://www.w3.org/2000/svg', NB = '\u00a0';
  // The labels and notes come with the data: the name of the highlighted configuration and the one sentence that says
  // how it is chosen, the note on the control row, and the line under each view.
  var labels = data.labels || {}, viewNotes = data.view_notes || {};
  var BEST = labels.best || 'Most reliable';
  var className = {};
  data.classes.forEach(function (c) { className[c.id] = c.name; });
  var setName = {};
  (data.sets || []).forEach(function (s) { setName[s.id] = s.name; });
  function setLabel(sid) { return (setName[sid] || sid).toLowerCase(); }
  function valueOf(button) { return button.getAttribute('data-value') || button.getAttribute('data-class'); }
  // The view the two controls choose: a model class and a question set.
  var view = { cid: valueOf(seg.querySelector('[aria-checked="true"]') || seg.querySelector('[role="radio"]')),
    sid: setSeg ? valueOf(setSeg.querySelector('[aria-checked="true"]') || setSeg.querySelector('[role="radio"]')) : 'original' };

  function svg(tag, attrs, text) {
    var node = document.createElementNS(NS, tag);
    Object.keys(attrs || {}).forEach(function (key) { node.setAttribute(key, attrs[key]); });
    if (text != null) node.textContent = text;
    return node;
  }
  // A tick step of 1, 2, 2.5, 5 or 10 times a power of ten that is at least the one asked for.
  function niceStep(v) {
    var p = Math.pow(10, Math.floor(Math.log10(v))), m = v / p;
    return (m <= 1 ? 1 : m <= 2 ? 2 : m <= 2.5 ? 2.5 : m <= 5 ? 5 : 10) * p;
  }
  function itemsFor(cid, sid) {
    var items = [];
    data.configs.forEach(function (c) {
      c.points.forEach(function (p) { if (p.class === cid && (p.set || 'original') === sid) items.push({ c: c, p: p }); });
    });
    // The same order as the leaderboard: the highlighted configuration first, then fewest tokens.
    items.sort(function (a, b) { return (b.c.best ? 1 : 0) - (a.c.best ? 1 : 0) || a.p.tokens - b.p.tokens; });
    return items;
  }
  function range(p) {
    var low = App.tokens(p.tokens_low), high = App.tokens(p.tokens_high);
    return low === high ? '' : low + NB + 'to' + NB + high;
  }

  /* ---------- the leaderboard: one row for each configuration, moved to its rank when the class changes ---------- */
  var board = (function () {
    var first = document.querySelector('.board-panel');
    if (!first) return null;
    function el(tag, cls, text) {
      var n = document.createElement(tag);
      if (cls) n.className = cls;
      if (text != null) n.textContent = text;
      return n;
    }
    function cell(role, cls) { var n = el('span', cls); n.setAttribute('role', role); return n; }
    // One scale for every class, so a bar keeps its length for a number wherever it is shown.
    var scaleMax = 0;
    data.configs.forEach(function (c) { c.points.forEach(function (p) { scaleMax = Math.max(scaleMax, p.tokens); }); });

    var wrap = el('div', 'lb-wrap'), card = el('div', 'card lb'), head = el('div', 'lb-head'), list = el('div', 'lb-list');
    var lead = el('p', 'group-title', 'One agent answering three lookups. The ' + BEST.toLowerCase() + ' configuration comes first, then the rest by fewest tokens. ' +
      (labels.rule || ''));
    var viewNote = el('p', 'footnote view-note');
    var empty = el('p', 'note empty');
    empty.style.display = 'none';
    empty.classList.add('note');
    card.setAttribute('role', 'table');
    head.setAttribute('role', 'row');
    list.setAttribute('role', 'rowgroup');
    ['Rank', 'Configuration', 'Tokens per task', 'Turns', 'Mean score', 'Fully correct runs', 'By model class'].forEach(function (label, i) {
      var h = cell('columnheader');
      if (i === 0) h.appendChild(el('span', null, '#')).setAttribute('aria-hidden', 'true');
      if (i === 0 || i === 6) h.appendChild(el('span', 'sr-only', label));
      else h.textContent = label;
      head.appendChild(h);
    });
    card.appendChild(head);
    card.appendChild(list);
    wrap.appendChild(lead);
    wrap.appendChild(card);
    wrap.appendChild(empty);
    wrap.appendChild(viewNote);

    var rows = [], bySlug = {};
    data.configs.forEach(function (c) {
      var r = { slug: c.slug, prev: { tokens: 0, turns: 0, correct: 0 } };
      r.node = el('div', 'lb-row');
      r.node.setAttribute('role', 'row');
      r.rank = r.node.appendChild(cell('cell', 'lb-rank'));
      r.name = r.node.appendChild(cell('rowheader', 'lb-name'));
      r.nameText = r.name.appendChild(document.createTextNode(c.name));
      r.tag = r.name.appendChild(el('span', 'tag', BEST));
      // A short visible note under the name, for the control (its reads were altered by another tool, with a link to the
      // Study) and for a configuration kept from the first session.
      function note(text) {
        var n = r.name.appendChild(el('span', 'sub row-note'));
        n.style.whiteSpace = 'normal';
        n.appendChild(document.createTextNode(text));
        return n;
      }
      if (c.family === 'control' && labels.control_note) {
        var noteNode = note(labels.control_note + ' ');
        var link = noteNode.appendChild(el('a', null, labels.control_link_text || 'Why this matters'));
        link.href = labels.control_link || 'study.html#control';
      }
      if (c.session === 'earlier' && labels.earlier) note(labels.earlier);
      var tok = r.node.appendChild(cell('cell', 'lb-tok')), bar = tok.appendChild(el('span', 'lb-bar')), num = tok.appendChild(el('span', 'lb-num'));
      r.bar = bar.appendChild(document.createElement('i'));
      r.tok = num.appendChild(el('b'));
      r.range = num.appendChild(el('small'));
      function stat(label) {
        var s = cell('cell', 'lb-stat'), body, v;
        s.appendChild(el('span', 'lb-l', label));
        body = s.appendChild(el('span', 'lb-body'));
        v = body.appendChild(el('span', 'lb-v'));
        return { s: s, body: body, v: v, b: v.appendChild(el('b')) };
      }
      var t = stat('Turns'), a = stat('Mean score'), f = stat('Fully correct runs');
      r.node.appendChild(t.s);
      r.turns = t.b;
      var ring = svg('svg', { class: 'ring', viewBox: '0 0 26 26', 'aria-hidden': 'true', focusable: 'false' });
      ring.appendChild(svg('circle', { class: 't', cx: 13, cy: 13, r: 11 }));
      r.ringP = ring.appendChild(svg('circle', { class: 'p', cx: 13, cy: 13, r: 11, pathLength: 1 }));
      a.body.insertBefore(ring, a.v);
      r.node.appendChild(a.s);
      r.share = a.b;
      r.node.appendChild(f.s);
      r.full = f.b;
      r.runs = f.v.appendChild(el('small'));
      // The row opens to show the same configuration on each of the three model classes, on the same scale.
      var open = r.node.appendChild(cell('cell', 'lb-open')), more = open.appendChild(el('button', 'lb-more'));
      var sub = r.node.appendChild(cell('cell', 'lb-sub')), inner = sub.appendChild(el('div', 'lb-sub-in')), classes = inner.appendChild(el('ul', 'lb-classes'));
      var arrow = more.appendChild(svg('svg', { viewBox: '0 0 24 24', 'aria-hidden': 'true', focusable: 'false' }));
      arrow.appendChild(svg('path', { d: 'm6 9 6 6 6-6' }));
      inner.id = 'by-class-' + c.slug;
      more.type = 'button';
      more.setAttribute('aria-expanded', 'false');
      more.setAttribute('aria-controls', inner.id);
      more.setAttribute('aria-label', c.name + ': show each model class');
      r.subs = {};
      r.shown = null;
      // The model classes of this configuration on one question set, written into the open row.
      r.fill = function (sid) {
        if (r.shown === sid) return;
        r.shown = sid;
        classes.replaceChildren();
        classes.setAttribute('aria-label', c.name + ' on each model class, ' + setLabel(sid) + ' questions');
        r.subs = {};
        data.classes.forEach(function (cls) {
          var p = c.points.filter(function (pt) { return pt.class === cls.id && (pt.set || 'original') === sid; })[0], li = classes.appendChild(el('li', 'lb-class'));
          r.subs[cls.id] = li;
          li.appendChild(el('b', null, cls.name));
          if (!p) { li.classList.add('none'); li.appendChild(el('span', null, 'Not measured on this class.')); return; }
          li.appendChild(el('span', 'lb-bar')).appendChild(document.createElement('i')).style.setProperty('--v', (p.tokens / scaleMax).toFixed(4));
          var facts = li.appendChild(el('span', 'lb-facts'));
          facts.appendChild(el('b', null, App.tokens(p.tokens)));
          facts.appendChild(document.createTextNode(' tokens, ' + App.turns(p.turns) + ' turns, ' + App.share(p.correct) + ' correct, ' +
            App.of(p.perfect_runs, p.runs) + (p.runs === 1 ? ' run' : ' runs') + ' fully correct'));
        });
      };
      r.fill('original');
      more.addEventListener('click', function () {
        var on = more.getAttribute('aria-expanded') !== 'true';
        more.setAttribute('aria-expanded', String(on));
        r.node.classList.toggle('open', on);
      });
      r.node.hidden = true;
      list.appendChild(r.node);
      rows.push(r);
      bySlug[c.slug] = r;
    });
    first.parentNode.insertBefore(wrap, first);
    App.reveal(card);

    var revealed = false, spring = App.spring;
    // Counts a number from where it was to where it is, after a delay.
    function tween(node, from, to, fmt, ms, delay) {
      var final = fmt(to);
      if (App.reduced || !(delay > 0)) { App.count(node, from, to, fmt, ms, final); return; }
      node.textContent = fmt(from);
      setTimeout(function () { App.count(node, from, to, fmt, ms, final); }, delay);
    }
    function paint(r, p, delay) {
      r.bar.style.setProperty('--d', delay + 'ms');
      r.ringP.style.setProperty('--d', delay + 'ms');
      r.bar.style.setProperty('--v', (p.tokens / scaleMax).toFixed(4));
      r.ringP.style.strokeDashoffset = String(1 - p.correct);
      tween(r.tok, r.prev.tokens, p.tokens, App.tokens, 800, delay);
      tween(r.turns, r.prev.turns, p.turns, App.turns, 800, delay);
      tween(r.share, r.prev.correct, p.correct, App.share, 800, delay);
      r.prev = { tokens: p.tokens, turns: p.turns, correct: p.correct };
    }
    // What does not move with a number: the rank, the tag, the range and the counts of runs.
    function meta(r, it, i) {
      r.node.classList.toggle('is-best', !!it.c.best);
      r.rank.textContent = String(i + 1);
      r.tag.hidden = !it.c.best;
      r.range.textContent = range(it.p);
      r.full.textContent = App.of(it.p.perfect_runs, it.p.runs);
      r.runs.textContent = it.p.runs + (it.p.runs === 1 ? ' run' : ' runs');
    }
    // A row travels to its new rank with weight: the further it goes the longer it takes, it lifts off the card
    // while it moves, and a row that climbs passes over the ones it overtakes.
    function flip(r, was, now, order) {
      var node = r.node, move = was === undefined ? 0 : was - now, frames;
      if (was === undefined) frames = [{ opacity: 0, transform: 'translateY(14px)' }, { opacity: 1, transform: 'none' }];
      else if (Math.abs(move) > 1) {
        frames = [{ transform: 'translateY(' + move + 'px) scale(1)' }, { transform: 'translateY(' + move * 0.12 + 'px) scale(1.012)', offset: 0.62 },
          { transform: 'none' }];
      } else return;
      var opts = { duration: 560 + Math.min(360, Math.abs(move) * 0.9), delay: order * 26, easing: spring, fill: 'backwards' };
      node.style.zIndex = move > 0 ? '3' : '2';
      node.classList.add('moving');
      var a;
      try { a = node.animate(frames, opts); } catch (err) { a = node.animate(frames, { duration: 400, easing: 'ease-out' }); }
      a.onfinish = a.oncancel = function () { node.style.zIndex = ''; node.classList.remove('moving'); };
    }
    var current = null;
    function update(cid, sid) {
      var items = itemsFor(cid, sid), keep = {}, was = {};
      rows.forEach(function (r) { if (!r.node.hidden) was[r.slug] = r.node.getBoundingClientRect().top; });
      items.forEach(function (it) {
        var r = bySlug[it.c.slug];
        keep[r.slug] = true;
        r.node.hidden = false;
        list.appendChild(r.node);
      });
      rows.forEach(function (r) { if (!keep[r.slug]) r.node.hidden = true; });
      card.setAttribute('aria-label', (className[cid] || cid) + ' class leaderboard, ' + setLabel(sid) + ' questions');
      empty.style.display = items.length ? 'none' : '';
      viewNote.textContent = items.length ? (viewNotes[cid + '|' + sid] || '') : '';
      viewNote.hidden = !viewNote.textContent;
      if (!items.length && App.offer) App.offer(empty);
      card.style.display = items.length ? '' : 'none';
      items.forEach(function (it, i) {
        var r = bySlug[it.c.slug];
        meta(r, it, i);
        r.it = it;
        r.fill(sid);
        Object.keys(r.subs).forEach(function (id) { r.subs[id].classList.toggle('is-best', id === cid && !!it.c.best); r.subs[id].classList.toggle('now', id === cid); });
        if (!revealed) return;
        paint(r, it.p, 0);
        if (!App.reduced && r.node.animate && current !== null) flip(r, was[r.slug], r.node.getBoundingClientRect().top, i);
      });
      current = { cid: cid, sid: sid };
    }
    // The first time the board is seen, every bar and ring draws in from nothing, one row after another.
    App.once(card, function () {
      revealed = true;
      var slugs = itemsFor(current.cid, current.sid).map(function (it) { return it.c.slug; });
      rows.forEach(function (r) { var n = slugs.indexOf(r.slug); if (n >= 0) paint(r, r.it.p, n * 70); });
    }, 0.2);
    return { update: update };
  })();

  /* ---------- the chart: one row for each configuration, the fewest tokens first ---------- */
  var drawn = {};
  function chartHost(cid, sid) { return document.querySelector('.chart[data-chart="' + cid + '"][data-set="' + sid + '"]'); }
  function node(tag, cls, text) {
    var n = document.createElement(tag);
    if (cls) n.className = cls;
    if (text != null) n.textContent = text;
    return n;
  }
  function draw(cid, sid, animate) {
    var host = chartHost(cid, sid);
    if (!host || drawn[cid + '/' + sid]) return;
    var items = itemsFor(cid, sid).slice().sort(function (a, b) { return a.p.tokens - b.p.tokens; });
    if (!items.length) return;
    drawn[cid + '/' + sid] = true;
    // One scale from zero, so a length reads as a length; the scale ends on the first tick past the highest run.
    var top = Math.max.apply(null, items.map(function (it) { return it.p.tokens_high; })), step = niceStep(top / 5);
    var steps = Math.ceil(top / step - 1e-9), xMax = steps * step;
    var ref = items.filter(function (it) { return it.c.family === 'control'; })[0] || null;
    var chart = node('div', 'rk'), head = node('div', 'rk-head'), list = node('ol', 'rk-list'), axis = node('span', 'rk-track');
    chart.style.setProperty('--n', steps);
    head.setAttribute('aria-hidden', 'true');
    head.appendChild(node('span'));
    head.appendChild(node('span', null, 'Mean tokens per task'));
    for (var t = 0; t <= steps; t += 1) {
      var tick = node('i', null, t ? App.tokens(step * t) : '0');
      tick.style.left = (t / steps * 100) + '%';
      axis.appendChild(tick);
    }
    head.appendChild(axis);
    head.appendChild(node('span'));
    head.appendChild(node('span', null, 'Mean score'));
    items.forEach(function (it, i) {
      var p = it.p, li = node('li', 'rk-row' + (it.c.best ? ' best' : '') + (it.c.family === 'control' ? ' ctrl' : ''));
      li.style.setProperty('--i', i);
      li.appendChild(node('span', 'rk-no', String(i + 1))).setAttribute('aria-hidden', 'true');
      var name = li.appendChild(node('span', 'rk-name', it.c.name));
      if (it.c.best) name.appendChild(node('span', 'tag', BEST));
      var track = li.appendChild(node('span', 'rk-track')), bar = node('i', 'rk-bar'), whisker = node('i', 'rk-range'), dot = node('i', 'rk-dot');
      track.setAttribute('aria-hidden', 'true');
      track.style.setProperty('--x', (p.tokens / xMax).toFixed(4));
      bar.style.setProperty('--x', (p.tokens / xMax).toFixed(4));
      dot.style.setProperty('--x', (p.tokens / xMax).toFixed(4));
      whisker.style.setProperty('--lo', (p.tokens_low / xMax).toFixed(4));
      whisker.style.setProperty('--hi', (p.tokens_high / xMax).toFixed(4));
      if (ref) { var line = node('i', 'rk-ref'); line.style.setProperty('--ref', (ref.p.tokens / xMax).toFixed(4)); track.appendChild(line); }
      track.appendChild(bar);
      if (p.tokens_high > p.tokens_low) track.appendChild(whisker);
      track.appendChild(dot);
      var tok = li.appendChild(node('span', 'rk-tok')), spread = range(p);
      tok.setAttribute('aria-hidden', 'true');
      tok.appendChild(node('b', null, App.tokens(p.tokens)));
      if (spread) tok.appendChild(node('small', null, spread));
      var score = li.appendChild(node('span', 'rk-score')), ring = svg('svg', { class: 'ring', viewBox: '0 0 26 26', focusable: 'false' });
      score.setAttribute('aria-hidden', 'true');
      ring.appendChild(svg('circle', { class: 't', cx: 13, cy: 13, r: 11 }));
      ring.appendChild(svg('circle', { class: 'p', cx: 13, cy: 13, r: 11, pathLength: 1, style: '--v:' + p.correct.toFixed(4) }));
      score.appendChild(ring);
      score.appendChild(node('b', null, App.share(p.correct)));
      li.appendChild(node('span', 'sr-only', (i + 1) + '. ' + it.c.name + (it.c.best ? ', ' + BEST.toLowerCase() : '') + ': ' + App.tokens(p.tokens) +
        ' tokens' + (spread ? ' (' + spread + ')' : '') + ', ' + p.turns.toFixed(1) + ' turns, mean score ' + App.share(p.correct) + ', ' +
        App.of(p.perfect_runs, p.runs) + ' runs fully correct'));
      list.appendChild(li);
    });
    list.setAttribute('aria-label', (className[cid] || cid) + ' class, ' + setLabel(sid) + ' questions: tokens and mean score for each configuration');
    chart.appendChild(head);
    chart.appendChild(list);
    host.replaceChildren(chart);
    // Held back until the chart is in view, so the bars are seen drawing and not spent off screen.
    if (animate && !App.reduced) {
      chart.classList.add('pre');
      App.once(host, function () { requestAnimationFrame(function () { requestAnimationFrame(function () { chart.classList.remove('pre'); }); }); }, 0.15);
    }
  }

  /* ---------- show one class and one question set at a time ---------- */
  var status = document.getElementById('seg-status'), note = document.getElementById('set-note');
  var runners = null;   // set by the turn runners below
  function show(spoken) {
    Array.prototype.forEach.call(document.querySelectorAll('.panel'), function (panel) {
      panel.classList.toggle('on', panel.getAttribute('data-class') === view.cid && (panel.getAttribute('data-set') || 'original') === view.sid);
    });
    // The line under the controls says what the chosen set is; a page without a script shows both lines.
    if (note) Array.prototype.forEach.call(note.querySelectorAll('[data-set]'), function (line) { line.hidden = line.getAttribute('data-set') !== view.sid; });
    var measured = itemsFor(view.cid, view.sid).length > 0;
    Array.prototype.forEach.call(document.querySelectorAll('[aria-labelledby="chart"], [aria-labelledby="turns"]'), function (section) {
      section.classList.toggle('gone', !measured);
    });
    draw(view.cid, view.sid, true);
    if (board) board.update(view.cid, view.sid);
    if (runners) runners.change();
    if (status && spoken) status.textContent = 'Showing the ' + spoken.toLowerCase() + '.';
  }
  var classCtl = App.segmented(seg, function (value, label) { view.cid = value; show(label + ' class, ' + setLabel(view.sid) + ' questions'); });
  var setCtl = setSeg ? App.segmented(setSeg, function (value, label) { view.sid = value; show((className[view.cid] || view.cid) + ' class, ' + label + ' questions'); }) : null;
  // Where the chosen pair has no measurements, one line says so and a button moves to the nearest pair that has some.
  function nearest() {
    var sets = (data.sets || []).map(function (st) { return st.id; });
    for (var n = 0; n < sets.length; n += 1) if (itemsFor(view.cid, sets[n]).length) return { cid: view.cid, sid: sets[n] };
    for (var m = 0; m < data.classes.length; m += 1) if (itemsFor(data.classes[m].id, view.sid).length) return { cid: data.classes[m].id, sid: view.sid };
    return null;
  }
  App.offer = function (host) {
    var go = nearest();
    host.replaceChildren(document.createTextNode('No configuration was measured on the ' + (className[view.cid] || view.cid).toLowerCase() + ' class with the ' + setLabel(view.sid) + ' questions.'));
    if (!go || !setCtl) return;
    var button = node('button', 'btn quiet', 'Show the ' + (className[go.cid] || go.cid).toLowerCase() + ' class, ' + setLabel(go.sid) + ' questions');
    button.type = 'button';
    button.addEventListener('click', function () {
      var ci = data.classes.map(function (c) { return c.id; }).indexOf(go.cid), si = (data.sets || []).map(function (st) { return st.id; }).indexOf(go.sid);
      if (ci >= 0 && ci !== data.classes.map(function (c) { return c.id; }).indexOf(view.cid)) classCtl.select(ci);
      if (si >= 0 && go.sid !== view.sid) setCtl.select(si);
    });
    host.appendChild(button);
  };
  show();
  // Everything above ran: the page may now hide its static tables and show the controls.
  root.classList.add('ready');

  /* ---------- turn runners: one dot is one turn; the task ends when the last turn ends ---------- */
  (function () {
    var fig = document.getElementById('runners');
    if (!fig) return;
    var status = fig.querySelector('.runner-status'), again = fig.querySelector('.replay');
    var all = Array.prototype.slice.call(fig.querySelectorAll('.runner')), runs = all, timers = [], STEP = 480, seen = false;
    if (App.reduced) { if (status) status.hidden = false; return; }
    // The runs of the panel the controls have chosen; the others are not on screen.
    function current() {
      runs = Array.prototype.slice.call(fig.querySelectorAll('.panel.on .runner'));
      if (!runs.length && !fig.querySelector('.panel')) runs = all;
    }
    function play() {
      timers.forEach(clearTimeout);
      timers = [];
      current();
      fig.classList.remove('run');
      all.forEach(function (r) { r.classList.remove('done'); });
      void fig.offsetWidth;
      fig.classList.add('run');
      // A run is done when its last dot has arrived: the delay of the last dot plus the three steps it travels.
      runs.forEach(function (r) {
        var turns = r.querySelectorAll('.dot').length;
        timers.push(setTimeout(function () { r.classList.add('done'); }, (turns + 2) * STEP));
      });
    }
    fig.classList.add('armed');
    App.once(fig, function () { seen = true; play(); }, 0.6);
    if (again) again.addEventListener('click', play);
    // A new class or question set shows another panel: its runs play again, once the figure has been seen.
    runners = { change: function () { current(); if (seen) play(); } };
  })();

  /* ---------- the engine race: every bar runs at one speed for its measured time, so the quickest stops first ---------- */
  (function () {
    var fig = document.getElementById('race');
    if (!fig) return;
    var rows = Array.prototype.slice.call(fig.querySelectorAll('.race-row')), note = fig.querySelector('.race-status'), again = fig.querySelector('.replay');
    var most = Math.max.apply(null, rows.map(function (row) { return parseFloat(row.getAttribute('data-ms')) || 0; })), stops = [], LONGEST = 2600;
    if (App.reduced || !(most > 0)) { if (note && App.reduced) note.hidden = false; return; }
    function play() {
      stops.forEach(function (stop) { stop(); });
      stops = rows.map(function (row) {
        var ms = parseFloat(row.getAttribute('data-ms')) || 0, bar = row.querySelector('.race-track i'), value = row.querySelector('.race-val b');
        var final = value.getAttribute('data-final') || value.textContent, places = (final.split('.')[1] || '').length;
        value.setAttribute('data-final', final);
        row.classList.remove('done');
        // No easing: here a bar is a clock.
        return App.run(LONGEST * ms / most, function (p) {
          bar.style.transform = 'scaleX(' + (ms / most * p).toFixed(4) + ')';
          value.textContent = p >= 1 ? final : (ms * p).toFixed(places);
        }, function () { row.classList.add('done'); });
      });
    }
    fig.classList.add('armed');
    App.once(fig, play, 0.6);
    if (again) again.addEventListener('click', play);
  })();
})();
