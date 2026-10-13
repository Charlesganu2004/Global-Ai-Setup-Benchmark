/* The Benchmarks page: the model class control, the leaderboard, the chart of tokens against answers
   correct, and the turn runners. The tables are already in the page for every class. This script shows
   one class at a time: the leaderboard keeps one row for each configuration and moves it to its rank, with
   its bar, ring and numbers following the measured values; the chart is drawn from window.SHOWCASE with one
   axis for each measure, the names written beside the marks, a legend when there are two or more series,
   a table view of the same numbers, a tooltip on hover and on focus. */
(function () {
  'use strict';
  var App = window.App, data = window.SHOWCASE;
  var seg = document.querySelector('.seg');
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
  function itemsFor(cid) {
    var items = [];
    data.configs.forEach(function (c) {
      c.points.forEach(function (p) { if (p.class === cid) items.push({ c: c, p: p }); });
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
    card.setAttribute('role', 'table');
    head.setAttribute('role', 'row');
    list.setAttribute('role', 'rowgroup');
    ['Rank', 'Configuration', 'Tokens per task', 'Turns', 'Answers correct', 'Fully correct runs'].forEach(function (label, i) {
      var h = cell('columnheader');
      if (i === 0) {
        h.appendChild(el('span', null, '#')).setAttribute('aria-hidden', 'true');
        h.appendChild(el('span', 'sr-only', label));
      } else h.textContent = label;
      head.appendChild(h);
    });
    card.appendChild(head);
    card.appendChild(list);
    wrap.appendChild(lead);
    wrap.appendChild(card);

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
    function flip(r, was, now) {
      var node = r.node, opts = { duration: 640, easing: spring }, frames;
      if (was === undefined) frames = [{ opacity: 0, transform: 'translateY(14px)' }, { opacity: 1, transform: 'none' }];
      else if (Math.abs(was - now) > 1) frames = [{ transform: 'translateY(' + (was - now) + 'px)' }, { transform: 'none' }];
      else return;
      node.style.zIndex = '2';
      var a;
      try { a = node.animate(frames, opts); } catch (err) { a = node.animate(frames, { duration: 400, easing: 'ease-out' }); }
      a.onfinish = a.oncancel = function () { node.style.zIndex = ''; };
    }
    var current = null;
    function update(cid) {
      var items = itemsFor(cid), keep = {}, was = {};
      rows.forEach(function (r) { if (!r.node.hidden) was[r.slug] = r.node.getBoundingClientRect().top; });
      items.forEach(function (it) {
        var r = bySlug[it.c.slug];
        keep[r.slug] = true;
        r.node.hidden = false;
        list.appendChild(r.node);
      });
      rows.forEach(function (r) { if (!keep[r.slug]) r.node.hidden = true; });
      card.setAttribute('aria-label', (className[cid] || cid) + ' class leaderboard');
      items.forEach(function (it, i) {
        var r = bySlug[it.c.slug];
        meta(r, it, i);
        r.it = it;
        if (!revealed) return;
        paint(r, it.p, 0);
        if (!App.reduced && r.node.animate && current !== null) flip(r, was[r.slug], r.node.getBoundingClientRect().top);
      });
      current = cid;
    }
    // The first time the board is seen, every bar and ring draws in from nothing, one row after another.
    App.once(card, function () {
      revealed = true;
      var slugs = itemsFor(current).map(function (it) { return it.c.slug; });
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
  function draw(cid, animate) {
    var host = document.querySelector('.chart[data-chart="' + cid + '"]');
    if (!host || !host.clientWidth) return;
    var items = itemsFor(cid);
    if (!items.length) return;
    var card = host.closest('.chart-card');
    var W = Math.max(280, Math.round(host.clientWidth)), H = Math.max(300, Math.round(host.clientHeight));
    drawn[cid] = W;
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
      'aria-label': className[cid] + ' class: tokens against answers correct for each configuration. Use the arrow keys to move between marks.' });
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
      App.once(host, function () { requestAnimationFrame(function () { requestAnimationFrame(function () { host.classList.remove('pre'); }); }); }, 0.35);
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
          placed.push(found.box);
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

  /* ---------- show one class at a time ---------- */
  var status = document.getElementById('seg-status');
  function show(cid, label) {
    Array.prototype.forEach.call(document.querySelectorAll('.panel'), function (panel) {
      panel.classList.toggle('on', panel.getAttribute('data-class') === cid);
    });
    App.tip.hide();
    draw(cid, true);
    if (board) board.update(cid);
    if (status && label) status.textContent = 'Showing the ' + label.toLowerCase() + ' class.';
  }
  var first = seg.querySelector('[role="radio"]');
  App.segmented(seg, show);
  show(first ? first.getAttribute('data-class') : data.classes[0].id);

  // Redraw when the width of the visible chart changes: a rotation, a window resize, the sidebar appearing.
  if ('ResizeObserver' in window) {
    var watcher = new ResizeObserver(function () {
      Object.keys(drawn).forEach(function (cid) {
        var host = document.querySelector('.chart[data-chart="' + cid + '"]');
        if (host && host.clientWidth && Math.round(host.clientWidth) !== drawn[cid]) draw(cid, false);
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
    var note = fig.querySelector('.runner-status'), again = fig.querySelector('.replay');
    var runs = Array.prototype.slice.call(fig.querySelectorAll('.runner')), timers = [], STEP = 480;
    if (App.reduced) { if (note) note.hidden = false; return; }
    function play() {
      timers.forEach(clearTimeout);
      timers = [];
      fig.classList.remove('run');
      runs.forEach(function (r) { r.classList.remove('done'); });
      void fig.offsetWidth;
      fig.classList.add('run');
      // A run is done when its last dot has arrived: the delay of the last dot plus the three steps it travels.
      runs.forEach(function (r) {
        var turns = r.querySelectorAll('.dot').length;
        timers.push(setTimeout(function () { r.classList.add('done'); }, (turns + 2) * STEP));
      });
    }
    fig.classList.add('armed');
    App.once(fig, play, 0.6);
    if (again) again.addEventListener('click', play);
  })();
})();
