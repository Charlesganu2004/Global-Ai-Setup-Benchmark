/* The Benchmarks page: the model class control, the question set control, the leaderboard, the chart of tokens
   against answers correct, the engine race and the turn runners. The tables are already in the page for every
   class and every question set. This script shows one class and one set at a time: the leaderboard keeps one row
   for each configuration measured on the set and moves it to its rank, with its bar, ring and numbers following
   the measured values, and a row opens to show its model classes on that set; a configuration not measured on
   the set is not in the view. The turn runners follow both controls. The chart is drawn from window.SHOWCASE
   with one axis for each measure, every mark travelling from
   the no-index value to its own, the names written beside the marks, a legend when there are two or more
   series, a table view of the same numbers, a tooltip on hover and on focus. Whatever the data holds is
   shown: a new family of configurations or an engine speed table needs no change here. */
(function () {
  'use strict';
  var App = window.App, data = window.SHOWCASE;
  var seg = document.querySelector('.seg[data-seg="class"]') || document.querySelector('.seg');
  var setSeg = document.querySelector('.seg[data-seg="set"]');
  if (!App || !data || !data.configs || !seg) return;

  var NS = 'http://www.w3.org/2000/svg';
  // Each series has a colour and its own marker shape, so colour is never the only signal.
  var STYLE = {
    'control': { color: '--c-control', shape: 'circle' },
    'tool': { color: '--c-tool', shape: 'square' },
    'engine-v1': { color: '--c-engine1', shape: 'diamond' },
    'engine-v2': { color: '--c-engine2', shape: 'triangle' }
  };
  var familyName = {}, className = {};
  (data.families || []).forEach(function (f) { familyName[f.id] = f.name; });
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
  function marker(kind, r) {
    if (kind === 'square') return svg('rect', { x: -r, y: -r, width: 2 * r, height: 2 * r });
    if (kind === 'diamond') { var d = r * 1.4; return svg('polygon', { points: '0,' + (-d) + ' ' + d + ',0 0,' + d + ' ' + (-d) + ',0' }); }
    if (kind === 'triangle') { var h = r * 1.35; return svg('polygon', { points: '0,' + (-h) + ' ' + h + ',' + (h * 0.8) + ' ' + (-h) + ',' + (h * 0.8) }); }
    return svg('circle', { r: r });
  }
  function niceMax(v) {
    var p = Math.pow(10, Math.floor(Math.log10(v))), m = v / p;
    return (m <= 1 ? 1 : m <= 2 ? 2 : m <= 2.5 ? 2.5 : m <= 5 ? 5 : 10) * p;
  }
  function itemsFor(cid, sid) {
    var items = [];
    data.configs.forEach(function (c) {
      c.points.forEach(function (p) { if (p.class === cid && (p.set || 'original') === sid) items.push({ c: c, p: p }); });
    });
    // The same order as the leaderboard: the best first, then fewest tokens.
    items.sort(function (a, b) { return (b.c.best ? 1 : 0) - (a.c.best ? 1 : 0) || a.p.tokens - b.p.tokens; });
    return items;
  }
  function overlap(a, b) { return a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1; }
  function range(p) {
    var low = App.tokens(p.tokens_low), high = App.tokens(p.tokens_high);
    return low === high ? '' : low + ' to ' + high;
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
    var lead = el('p', 'group-title', 'One agent answering three lookups. The best configuration first, then fewest tokens.');
    var empty = el('p', 'note empty');
    empty.style.display = 'none';
    card.setAttribute('role', 'table');
    head.setAttribute('role', 'row');
    list.setAttribute('role', 'rowgroup');
    ['Rank', 'Configuration', 'Tokens per task', 'Turns', 'Answers correct', 'Fully correct runs', 'By model class'].forEach(function (label, i) {
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

    var rows = [], bySlug = {};
    data.configs.forEach(function (c) {
      var r = { slug: c.slug, prev: { tokens: 0, turns: 0, correct: 0 } };
      r.node = el('div', 'lb-row');
      r.node.setAttribute('role', 'row');
      r.rank = r.node.appendChild(cell('cell', 'lb-rank'));
      r.name = r.node.appendChild(cell('rowheader', 'lb-name'));
      r.nameText = r.name.appendChild(document.createTextNode(c.name));
      r.tag = r.name.appendChild(el('span', 'tag', 'Best'));
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
      var t = stat('Turns'), a = stat('Answers correct'), f = stat('Fully correct runs');
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
      empty.textContent = 'No configuration was measured on the ' + (className[cid] || cid).toLowerCase() + ' class with the ' + setLabel(sid) + ' questions.';
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

  /* ---------- the legend: the series that are in this class, when there are two or more ---------- */
  function fillLegend(card, items) {
    var list = card.querySelector('.legend');
    if (!list) return;
    // In the order the data lists the series, not the order of the marks.
    var seen = (data.families || []).map(function (f) { return f.id; }).filter(function (id) {
      return items.some(function (it) { return it.c.family === id; });
    });
    list.replaceChildren();
    if (seen.length < 2) { list.hidden = true; return; }
    list.hidden = false;
    seen.forEach(function (family) {
      var style = STYLE[family] || STYLE.control, li = document.createElement('li');
      var icon = svg('svg', { viewBox: '-8 -8 16 16', 'aria-hidden': 'true', focusable: 'false' });
      var shape = marker(style.shape, 5);
      shape.setAttribute('fill', 'var(' + style.color + ')');
      icon.appendChild(shape);
      li.appendChild(icon);
      li.appendChild(document.createTextNode(familyName[family] || family));
      list.appendChild(li);
    });
  }

  /* ---------- the chart: the ranges draw in and the marks settle one after another when it comes into view ---------- */
  var drawn = {};
  function chartHost(cid, sid) { return document.querySelector('.chart[data-chart="' + cid + '"][data-set="' + sid + '"]'); }
  function draw(cid, sid, animate) {
    var host = chartHost(cid, sid);
    if (!host || !host.clientWidth) return;
    var items = itemsFor(cid, sid);
    if (!items.length) return;
    var card = host.closest('.chart-card');
    var W = Math.max(280, Math.round(host.clientWidth)), H = Math.max(300, Math.round(host.clientHeight));
    drawn[cid + '/' + sid] = W;
    var narrow = W < 560, m = { l: 60, r: 20, t: 22, b: 52 };
    var plot = { x0: m.l, x1: W - m.r, y0: m.t, y1: H - m.b };

    // x: tokens from zero, so a length reads as a length. y: the share of answers correct, from a stated floor.
    var xMax = niceMax(Math.max.apply(null, items.map(function (it) { return it.p.tokens_high; })) * 1.06);
    var xStep = xMax / (xMax >= 5e5 ? 5 : 4);
    var xTicks = [];
    for (var t = 0; t <= xMax * 1.0001; t += xStep) xTicks.push(t);
    var lowest = Math.min.apply(null, items.map(function (it) { return it.p.correct; }));
    var yStep = lowest > 0.7 ? 0.05 : 0.1;
    var yLo = Math.max(0, Math.floor((lowest - 0.02) / yStep + 1e-9) * yStep);
    var yTicks = [];
    for (var v = yLo; v <= 1.0001; v += yStep) yTicks.push(+v.toFixed(4));
    var x = function (tokens) { return plot.x0 + tokens / xMax * (plot.x1 - plot.x0); };
    var y = function (share) { return plot.y1 - (share - yLo) / (1 - yLo) * (plot.y1 - plot.y0 - 8); };

    var root = svg('svg', { viewBox: '0 0 ' + W + ' ' + H, role: 'group',
      'aria-label': className[cid] + ' class, ' + setLabel(sid) + ' questions: tokens against answers correct for each configuration. Use the arrow keys to move between marks.' });
    yTicks.forEach(function (tick, i) {
      root.appendChild(svg('line', { class: i ? 'grid' : 'axis', x1: plot.x0, x2: plot.x1, y1: y(tick), y2: y(tick) }));
      root.appendChild(svg('text', { class: 'tick', x: plot.x0 - 8, y: y(tick) + 4, 'text-anchor': 'end' }, Math.round(tick * 100) + '%'));
    });
    xTicks.forEach(function (tick, i) {
      root.appendChild(svg('line', { class: i ? 'grid' : 'axis', x1: x(tick), x2: x(tick), y1: plot.y0, y2: plot.y1 }));
      root.appendChild(svg('text', { class: 'tick', x: x(tick), y: plot.y1 + 18, 'text-anchor': i === 0 ? 'start' : 'middle' }, App.tokens(tick)));
    });
    root.appendChild(svg('text', { class: 'axis-title', x: (plot.x0 + plot.x1) / 2, y: H - 10, 'text-anchor': 'middle' },
      narrow ? 'Tokens per task (mean)' : 'Tokens per task (mean). Further left is fewer.'));
    var midY = (plot.y0 + plot.y1) / 2;
    root.appendChild(svg('text', { class: 'axis-title', x: 14, y: midY, 'text-anchor': 'middle', transform: 'rotate(-90 14 ' + midY + ')' },
      yLo > 0 ? 'Answers correct (axis from ' + Math.round(yLo * 100) + '%)' : 'Answers correct'));

    // The lowest to the highest run, behind the marks.
    var marks = items.map(function (it, i) {
      var style = STYLE[it.c.family] || STYLE.control, cx = x(it.p.tokens), cy = y(it.p.correct);
      if (it.p.tokens_high > it.p.tokens_low) {
        root.appendChild(svg('line', { class: 'range', pathLength: 1, style: '--n:' + i, x1: x(it.p.tokens_low), x2: x(it.p.tokens_high), y1: cy, y2: cy, stroke: 'var(' + style.color + ')' }));
      }
      return { it: it, i: i, cx: cx, cy: cy, r: 7, style: style };
    });

    // Every mark starts on the no-index value and travels to its own. The thin line it leaves behind is the
    // difference, in tokens and in answers correct, between that configuration and no index.
    var ref = marks.filter(function (k) { return k.it.c.family === 'control'; })[0] || null;
    if (ref) {
      marks.forEach(function (k) {
        if (k === ref || Math.hypot(k.cx - ref.cx, k.cy - ref.cy) < 14) return;
        root.appendChild(svg('line', { class: 'trail', pathLength: 1, style: '--n:' + k.i, x1: ref.cx, y1: ref.cy, x2: k.cx, y2: k.cy }));
      });
    }
    function travel() {
      host.classList.remove('pre');
      if (!ref) return;
      marks.forEach(function (k) {
        if (k === ref || !k.node.animate) return;
        try {
          k.node.animate([{ transform: 'translate(' + ref.cx + 'px,' + ref.cy + 'px)' }, { transform: 'translate(' + k.cx + 'px,' + k.cy + 'px)' }],
            { duration: 900, delay: 120 + k.i * 90, easing: 'cubic-bezier(.23,1,.32,1)', fill: 'backwards' });
        } catch (err) { /* the mark is already in place */ }
      });
    }

    var order = marks.slice().sort(function (a, b) { return a.cx - b.cx || a.cy - b.cy; });
    marks.forEach(function (k) {
      var it = k.it, p = it.p;
      k.node = svg('g', { class: 'mark', transform: 'translate(' + k.cx + ',' + k.cy + ')', tabindex: k === order[0] ? '0' : '-1', role: 'img',
        'aria-label': (k.i + 1) + '. ' + it.c.name + (it.c.best ? ', best' : '') + ': ' + App.tokens(p.tokens) + ' tokens, ' + p.turns.toFixed(1) +
          ' turns, ' + App.share(p.correct) + ' of answers correct, ' + App.of(p.perfect_runs, p.runs) + ' runs fully correct' });
      var pop = svg('g', { class: 'pop', style: '--n:' + k.i });
      pop.appendChild(svg('circle', { class: 'ring', r: 15 }));
      var shape = marker(k.style.shape, k.r);
      shape.setAttribute('class', 'shape');
      shape.setAttribute('fill', 'var(' + k.style.color + ')');
      pop.appendChild(shape);
      k.node.appendChild(pop);
      root.appendChild(k.node);
    });

    // Names beside the marks where there is room; a number from the key where there is not. A name never
    // covers another mark, another name, or the line that runs from the lowest to the highest run.
    var placed = marks.map(function (k) { return { x0: k.cx - k.r - 3, x1: k.cx + k.r + 3, y0: k.cy - k.r - 3, y1: k.cy + k.r + 3 }; });
    marks.forEach(function (k) {
      if (k.it.p.tokens_high > k.it.p.tokens_low) {
        placed.push({ x0: x(k.it.p.tokens_low), x1: x(k.it.p.tokens_high), y0: k.cy - 3, y1: k.cy + 3 });
      }
    });
    var measure = svg('text', { class: 'name', visibility: 'hidden' }, '');
    root.appendChild(measure);
    host.replaceChildren(root);
    // Hidden at once, released when the chart is in view, so the draw-in is seen and not spent off screen.
    host.classList.toggle('pre', !!animate && !App.reduced);
    if (animate && !App.reduced) {
      App.once(host, function () { requestAnimationFrame(function () { requestAnimationFrame(travel); }); }, 0.35);
    }
    var keyed = narrow;
    marks.forEach(function (k) {
      var words = [narrow ? null : k.it.c.name, String(k.i + 1)];
      for (var w = 0; w < words.length; w++) {
        if (!words[w]) continue;
        measure.textContent = words[w];
        var width = measure.getComputedTextLength();
        var spots = [
          { x: k.cx + k.r + 6, y: k.cy + 4, a: 'start' }, { x: k.cx - k.r - 6, y: k.cy + 4, a: 'end' },
          { x: k.cx, y: k.cy - k.r - 8, a: 'middle' }, { x: k.cx, y: k.cy + k.r + 16, a: 'middle' },
          { x: k.cx - k.r, y: k.cy - k.r - 8, a: 'start' }, { x: k.cx - k.r, y: k.cy + k.r + 16, a: 'start' },
          { x: k.cx + k.r, y: k.cy - k.r - 8, a: 'end' }, { x: k.cx + k.r, y: k.cy + k.r + 16, a: 'end' }];
        var found = null;
        for (var s = 0; s < spots.length && !found; s++) {
          var sp = spots[s];
          var box = { x0: sp.a === 'start' ? sp.x : sp.a === 'end' ? sp.x - width : sp.x - width / 2, y0: sp.y - 11, y1: sp.y + 3 };
          box.x1 = box.x0 + width;
          var inside = box.x0 >= plot.x0 - 4 && box.x1 <= W - 4 && box.y0 >= 0 && box.y1 <= plot.y1 + 6;
          if (inside && !placed.some(function (other, idx) { return idx !== k.i && overlap(box, other); })) {
            found = { spot: sp, box: box };
          }
        }
        if (found) {
          placed.push({ x0: found.box.x0 - 5, x1: found.box.x1 + 5, y0: found.box.y0 - 1, y1: found.box.y1 + 1 });
          root.appendChild(svg('text', { class: 'name', x: found.spot.x, y: found.spot.y, 'text-anchor': found.spot.a }, words[w]));
          if (w === 1) keyed = true;
          return;
        }
      }
      keyed = true;
    });
    root.removeChild(measure);
    card.classList.toggle('keyed', keyed);
    fillLegend(card, items);

    // Hover, tap and focus show the same tooltip.
    function rowsFor(k) {
      var p = k.it.p, low = App.tokens(p.tokens_low), high = App.tokens(p.tokens_high);
      var rows = [['Tokens per task', App.tokens(p.tokens)]];
      if (low !== high) rows.push(['Lowest to highest', low + ' to ' + high]);
      rows.push(['Turns', p.turns.toFixed(1)], ['Answers correct', App.share(p.correct)],
        ['Fully correct runs', App.of(p.perfect_runs, p.runs)], ['Wall time', App.seconds(p.seconds)]);
      return rows;
    }
    function titleFor(k) { return (k.i + 1) + '. ' + k.it.c.name + (k.it.c.best ? ' (best)' : ''); }
    function heat(target) { marks.forEach(function (k) { k.node.classList.toggle('hot', k === target); }); }
    function nearest(e) {
      var box = root.getBoundingClientRect(), scale = box.width ? W / box.width : 1;
      var px = (e.clientX - box.left) * scale, py = (e.clientY - box.top) * scale, best = null, bestD = 36;
      marks.forEach(function (k) {
        var d = Math.hypot(k.cx - px, k.cy - py);
        if (d < bestD) { bestD = d; best = k; }
      });
      return best;
    }
    function point(e) {
      var k = nearest(e);
      heat(k);
      if (k) App.tip.show(titleFor(k), rowsFor(k), e.clientX, e.clientY); else App.tip.hide();
    }
    root.addEventListener('pointermove', point);
    root.addEventListener('pointerdown', point);
    root.addEventListener('pointerleave', function (e) { if (e.pointerType === 'touch') return; heat(null); App.tip.hide(); });
    marks.forEach(function (k) {
      k.node.addEventListener('focus', function () {
        var box = k.node.getBoundingClientRect();
        heat(k);
        App.tip.show(titleFor(k), rowsFor(k), box.left + box.width / 2, box.top + box.height / 2);
      });
      k.node.addEventListener('blur', function () { heat(null); App.tip.hide(); });
      k.node.addEventListener('keydown', function (e) {
        var at = order.indexOf(k), next = -1;
        if (e.key === 'ArrowRight' || e.key === 'ArrowDown') next = Math.min(order.length - 1, at + 1);
        else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') next = Math.max(0, at - 1);
        else if (e.key === 'Home') next = 0;
        else if (e.key === 'End') next = order.length - 1;
        if (next < 0) return;
        e.preventDefault();
        order.forEach(function (o) { o.node.setAttribute('tabindex', o === order[next] ? '0' : '-1'); });
        order[next].node.focus();
      });
    });
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
    App.tip.hide();
    draw(view.cid, view.sid, true);
    if (board) board.update(view.cid, view.sid);
    if (runners) runners.change();
    if (status && spoken) status.textContent = 'Showing the ' + spoken.toLowerCase() + '.';
  }
  App.segmented(seg, function (value, label) { view.cid = value; show(label + ' class, ' + setLabel(view.sid) + ' questions'); });
  if (setSeg) App.segmented(setSeg, function (value, label) { view.sid = value; show((className[view.cid] || view.cid) + ' class, ' + label + ' questions'); });
  show();

  // Redraw when the width of the visible chart changes: a rotation, a window resize, the sidebar appearing.
  if ('ResizeObserver' in window) {
    var watcher = new ResizeObserver(function () {
      Object.keys(drawn).forEach(function (key) {
        var parts = key.split('/'), host = chartHost(parts[0], parts[1]);
        if (host && host.clientWidth && Math.round(host.clientWidth) !== drawn[key]) draw(parts[0], parts[1], false);
      });
    });
    Array.prototype.forEach.call(document.querySelectorAll('.chart'), function (host) { watcher.observe(host); });
  }
  document.addEventListener('pointerdown', function (e) {
    if (!e.target.closest || !e.target.closest('.chart')) App.tip.hide();
  });

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
