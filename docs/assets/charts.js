/* The charts. Every function takes the rows to draw, so the page decides the
   filter and the charts never read a method id. Exposed on window.Charts.

   Colours are CSS custom properties. The model tiers always use --s1 (C),
   --s2 (B), --s3 (A); the token segments always use --s1 to --s4 in the order
   start-up, re-read, added, output. */
(function () {
  'use strict';
  var S = window.Site;

  var TIER_COLOR = { C: '--s1', B: '--s2', A: '--s3' };
  var SEGMENTS = [
    ['start', 'Start-up context', '--s1'],
    ['reread', 'Re-read on later turns', '--s2'],
    ['added', 'New context added', '--s3'],
    ['output', 'Output', '--s4']
  ];
  var PART_COLORS = ['--s1', '--s2', '--s3', '--s4', '--s5'];
  var EMPTY = 'No runs were measured for this selection.';
  var GRID_MIN = 260, GRID_GAP = 24;   // .small-multiples in site.css: minmax(260px, 1fr) and gap 24px

  /* ---------- scale and shape helpers (pure) ---------- */
  function tierColor(id) { return TIER_COLOR[id] || '--s5'; }
  function positive(values) { return values.filter(function (v) { return typeof v === 'number' && isFinite(v) && v > 0; }); }
  function niceMax(v) {
    if (!(v > 0) || !isFinite(v)) return 1;
    var p = Math.pow(10, Math.floor(Math.log10(v))), m = v / p;
    return (m <= 1 ? 1 : m <= 2 ? 2 : m <= 4 ? 4 : m <= 5 ? 5 : m <= 8 ? 8 : 10) * p;
  }
  function niceTicks(lo, hi, count) {
    if (!isFinite(lo) || !isFinite(hi)) { lo = 0; hi = 1; }
    if (!(hi > lo)) hi = lo + (Math.abs(lo) || 1);
    var raw = (hi - lo) / count, p = Math.pow(10, Math.floor(Math.log10(raw))), m = raw / p;
    var step = (m <= 1 ? 1 : m <= 2 ? 2 : m <= 5 ? 5 : 10) * p, ticks = [];
    for (var v = Math.floor(lo / step) * step; v <= Math.ceil(hi / step) * step + step / 1e6; v += step) ticks.push(+v.toFixed(6));
    return ticks;
  }
  // A log scale over whole decades. Values that are not positive are ignored.
  function logScale(values, lo, hi) {
    var good = positive(values);
    var min = good.length ? Math.pow(10, Math.floor(Math.log10(Math.min.apply(null, good)))) : 0.001;
    var max = good.length ? Math.pow(10, Math.ceil(Math.log10(Math.max.apply(null, good)))) : 1;
    if (!(max > min)) max = min * 10;
    var span = Math.log10(max) - Math.log10(min);
    var scale = function (v) { return lo + (Math.log10(v) - Math.log10(min)) / span * (hi - lo); };
    var ticks = [];
    for (var v = min; v <= max * 1.0001; v *= 10) ticks.push(v);
    return { scale: scale, ticks: ticks, min: min, max: max };
  }
  // A bar that is square at the baseline and rounded at the data end.
  function barPath(x, y, w, h, r) {
    r = Math.max(0, Math.min(r, w, h / 2));
    return 'M' + x + ',' + y + 'h' + (w - r) + 'a' + r + ',' + r + ' 0 0 1 ' + r + ',' + r + 'v' + (h - 2 * r) +
      'a' + r + ',' + r + ' 0 0 1 ' + (-r) + ',' + r + 'h' + (-(w - r)) + 'z';
  }
  // Bubble area grows with the share answered correctly, from `floor` to 1.
  function qualityFloor(points) {
    var q = points.map(function (p) { return p.quality; }).filter(function (v) { return typeof v === 'number' && isFinite(v); });
    var low = q.length ? Math.min.apply(null, q) : 0.7;
    return Math.max(0, Math.min(0.7, Math.floor(low * 20) / 20));
  }
  function radiusFor(floor) {
    return function (q) {
      var t = Math.max(0, Math.min(1, ((typeof q === 'number' ? q : floor) - floor) / (1 - floor)));
      return Math.sqrt(25 + (169 - 25) * t);
    };
  }
  // The bubble under the pointer. Inside one or more bubbles it is the one drawn
  // last, which is the one on top; otherwise the nearest edge within `reach`.
  function pickBubble(marks, px, py, reach) {
    var inside = null, best = null, bestD = reach == null ? 48 : reach;
    marks.forEach(function (mark) {
      var d = Math.hypot(mark.cx - px, mark.cy - py) - mark.r;
      if (d <= 0) inside = mark;
      if (d < bestD) { bestD = d; best = mark; }
    });
    return inside || best;
  }
  // Where a bubble's code can go without touching another bubble or another
  // label: right, left, above, then below. Null when no place is free, and the
  // tooltip and the table carry the name instead.
  function labelSpot(mark, marks, placed, w, h, area) {
    var gap = 3;
    var spots = [
      { x: mark.cx + mark.r + gap, y: mark.cy - h / 2, anchor: 'start' },
      { x: mark.cx - mark.r - gap - w, y: mark.cy - h / 2, anchor: 'end' },
      { x: mark.cx - w / 2, y: mark.cy - mark.r - gap - h, anchor: 'middle' },
      { x: mark.cx - w / 2, y: mark.cy + mark.r + gap, anchor: 'middle' }
    ];
    for (var i = 0; i < spots.length; i++) {
      var s = spots[i], x0 = s.x, y0 = s.y, x1 = s.x + w, y1 = s.y + h;
      if (x0 < area.left || x1 > area.right || y0 < area.top || y1 > area.bottom) continue;
      var hitsBubble = marks.some(function (k) {
        if (k === mark) return false;
        var nx = Math.max(x0, Math.min(k.cx, x1)), ny = Math.max(y0, Math.min(k.cy, y1));
        return Math.hypot(k.cx - nx, k.cy - ny) < k.r + 2;
      });
      var hitsLabel = placed.some(function (b) { return x0 < b.x1 + 2 && x1 > b.x0 - 2 && y0 < b.y1 && y1 > b.y0; });
      if (!hitsBubble && !hitsLabel) return { x0: x0, y0: y0, x1: x1, y1: y1, anchor: s.anchor };
    }
    return null;
  }
  // How many columns the small-multiples grid gives `cells` charts in a host `full` px wide.
  function gridColumns(full, cells) {
    return Math.max(1, Math.min(cells, Math.floor((full + GRID_GAP) / (GRID_MIN + GRID_GAP))));
  }
  function gridCellWidth(full, cells) {
    var cols = gridColumns(full, cells);
    return Math.max(GRID_MIN, Math.floor((full - GRID_GAP * (cols - 1)) / cols));
  }
  // The value that stretches a scale: the largest, when it is over four times the next one.
  function scaleSetter(rows, key) {
    if (rows.length < 2) return null;
    var sorted = rows.slice().sort(function (a, b) { return b[key] - a[key]; });
    return sorted[0][key] > 4 * sorted[1][key] ? sorted[0] : null;
  }
  // Start-up parts for the bar: one colour each, so anything past the palette is folded into one segment.
  function foldParts(parts, limit) {
    if (parts.length <= limit) return parts;
    var rest = parts.slice(limit - 1);
    return parts.slice(0, limit - 1).concat([{
      name: 'Other parts',
      tokens: rest.reduce(function (sum, p) { return sum + p.tokens; }, 0),
      how: rest.map(function (p) { return p.name; }).join(', ')
    }]);
  }
  function fit(text, px) {
    var room = Math.max(4, Math.floor(px / 6.4));
    return text.length <= room ? text : text.slice(0, room - 1).trimEnd() + '…';
  }
  function byTierThen(key) {
    return function (a, b) { return S.tierOrder(a.tier) - S.tierOrder(b.tier) || (a[key] - b[key]) || S.armOrder(a.arm) - S.armOrder(b.arm); };
  }
  // Methods ordered by their cost on the cheapest tier they share, then by catalogue order.
  function costOrder(points) {
    var tiers = S.tiersIn(points), first = tiers.length ? tiers[0].id : null;
    var ids = [];
    points.forEach(function (p) { if (ids.indexOf(p.arm) < 0) ids.push(p.arm); });
    var cost = function (id) {
      var p = points.find(function (q) { return q.arm === id && q.tier === first && q.cost_warm > 0; });
      return p ? p.cost_warm : Infinity;
    };
    return ids.sort(function (a, b) {
      var ca = cost(a), cb = cost(b);
      if (ca !== cb) return ca < cb ? -1 : 1;
      return S.armOrder(a) - S.armOrder(b);
    });
  }
  // A measured value without padding zeros: whole numbers stay whole, the rest keep one decimal.
  function plain(v) { return S.num(v, Number.isInteger(v) ? 0 : 1); }

  var api = {
    TIER_COLOR: TIER_COLOR, SEGMENTS: SEGMENTS, tierColor: tierColor, niceMax: niceMax, niceTicks: niceTicks,
    logScale: logScale, barPath: barPath, qualityFloor: qualityFloor, radiusFor: radiusFor,
    pickBubble: pickBubble, labelSpot: labelSpot, gridColumns: gridColumns, gridCellWidth: gridCellWidth,
    scaleSetter: scaleSetter, foldParts: foldParts, fit: fit, byTierThen: byTierThen, costOrder: costOrder, plain: plain
  };
  window.Charts = api;
  if (typeof document === 'undefined') return;

  var el = S.el, svgEl = S.svgEl;
  function widthOf(host) { return Math.max(280, host.clientWidth || 0); }
  function empty(hosts) {
    hosts.host.replaceChildren(el('p', { class: 'note' }, EMPTY));
    if (hosts.legend) hosts.legend.replaceChildren();
    if (hosts.table) hosts.table.replaceChildren(el('p', { class: 'note' }, EMPTY));
  }
  function tierLegend(points) {
    return S.tiersIn(points).map(function (t) { return [t.name + ': ' + t.model, tierColor(t.id)]; });
  }
  function chartSvg(width, height, label) {
    return svgEl('svg', { viewBox: '0 0 ' + width + ' ' + height, height: height, role: 'group', 'aria-label': label + '. ' + S.KEYS_HINT });
  }

  /* ---------- cost, efficiency, quality and tier in one view ---------- */
  function drawBubble(hosts, points) {
    var host = hosts.host;
    var all = points.filter(function (p) { return p.cost_warm > 0 && typeof p.efficiency === 'number'; });
    if (!all.length) return empty(hosts);
    host.replaceChildren();
    // The size scale covers every measured point, so a bubble keeps its size when the filter changes.
    var floor = qualityFloor(S.list('points')), radius = radiusFor(floor);
    S.legend(hosts.legend, tierLegend(all), true);
    hosts.legend.appendChild(el('span', null, 'Size: share of the questions answered correctly'));
    var key = svgEl('svg', { viewBox: '0 0 200 28', width: 200, height: 28, role: 'img', 'aria-label': 'Bubble size key' });
    [[1, 14], [(1 + floor) / 2, 82], [floor, 146]].forEach(function (pair) {
      var r = radius(pair[0]);
      key.appendChild(svgEl('circle', { cx: pair[1], cy: 14, r: r, fill: 'none', stroke: 'var(--muted)', 'stroke-width': 1.5 }));
      key.appendChild(svgEl('text', { x: pair[1] + r + 4, y: 18 }, S.pct(pair[0])));
    });
    hosts.legend.appendChild(key);

    var width = widthOf(host);
    var height = Math.min(540, Math.max(360, Math.round(width * 0.62)));
    var m = { left: 54, right: 20, top: 16, bottom: 44 };
    var xs = logScale(all.map(function (p) { return p.cost_warm; }), m.left + 24, width - m.right - 24), x = xs.scale;
    // Position, not length, carries the value here, so the axis need not start at zero.
    var effs = all.map(function (p) { return p.efficiency; });
    var yTicks = niceTicks(Math.min.apply(null, effs) * 0.9, Math.max.apply(null, effs) * 1.04, 5);
    var yMin = yTicks[0], yMax = yTicks[yTicks.length - 1];
    var y = function (v) { return height - m.bottom - (v - yMin) / (yMax - yMin) * (height - m.top - m.bottom); };
    var svg = chartSvg(width, height, 'Cost, efficiency, quality and model tier for each method');
    var whole = yTicks.every(function (v) { return Number.isInteger(v); });
    yTicks.forEach(function (v, i) {
      svg.appendChild(svgEl('line', { class: i ? 'grid' : 'axis', x1: m.left, x2: width - m.right, y1: y(v), y2: y(v) }));
      svg.appendChild(svgEl('text', { class: 'tick', x: m.left - 8, y: y(v) + 4, 'text-anchor': 'end' }, S.num(v, whole ? 0 : 1)));
    });
    xs.ticks.forEach(function (v) {
      svg.appendChild(svgEl('line', { class: 'grid', x1: x(v), x2: x(v), y1: m.top, y2: height - m.bottom }));
      svg.appendChild(svgEl('text', { class: 'tick', x: x(v), y: height - m.bottom + 16, 'text-anchor': 'middle' }, S.usd(v)));
    });
    svg.appendChild(svgEl('text', { x: (m.left + width - m.right) / 2, y: height - 6, 'text-anchor': 'middle' },
      width < 480 ? 'Cost per run, cache warm (log scale)' : 'Cost per run, cache warm (log scale). Left is cheaper.'));
    var midY = (m.top + height - m.bottom) / 2;
    svg.appendChild(svgEl('text', { x: 12, y: midY, 'text-anchor': 'middle', transform: 'rotate(-90 12 ' + midY + ')' }, 'Correct answers per million tokens'));

    // Big bubbles first, so a small one is never hidden under a large one.
    var drawn = all.slice().sort(function (a, b) { return (b.quality || 0) - (a.quality || 0); });
    var marks = drawn.map(function (p) {
      var mark = { p: p, cx: x(p.cost_warm), cy: y(p.efficiency), r: radius(p.quality), group: svgEl('g', { class: 'mark' }) };
      mark.group.appendChild(svgEl('circle', { cx: mark.cx, cy: mark.cy, r: mark.r, fill: 'var(' + tierColor(p.tier) + ')', stroke: 'var(--surface)', 'stroke-width': 2 }));
      svg.appendChild(mark.group);
      return mark;
    });
    // A code is printed only where it touches no other bubble and no other label.
    var area = { left: m.left, right: width - m.right, top: m.top, bottom: height - m.bottom }, placed = [];
    marks.forEach(function (mark) {
      var code = String(mark.p.arm), spot = labelSpot(mark, marks, placed, code.length * 6.8, 12, area);
      if (!spot) return;
      placed.push(spot);
      var tx = spot.anchor === 'start' ? spot.x0 : spot.anchor === 'end' ? spot.x1 : (spot.x0 + spot.x1) / 2;
      mark.group.appendChild(svgEl('text', { class: 'bubble-label', x: tx, y: spot.y0 + 10, 'text-anchor': spot.anchor }, code));
    });
    // Methods that land on the same spot are named in each other's tooltip.
    var sameSpot = function (mark) {
      return marks.filter(function (k) { return k !== mark && Math.hypot(k.cx - mark.cx, k.cy - mark.cy) < 4; })
        .map(function (k) { return k.p.arm + ' on ' + S.tier(k.p.tier).name; });
    };
    var rowsFor = function (mark) {
      var p = mark.p, t = S.tier(p.tier), others = sameSpot(mark);
      var rows = [
        { color: 'var(' + tierColor(p.tier) + ')', value: S.usd(p.cost_warm), label: 'cost per run, cache warm' },
        { value: S.num(p.efficiency, 1), label: 'correct answers per million tokens' },
        { value: S.pct(p.quality), label: 'answered correctly, ' + S.plural(p.n, 'run', 'runs') },
        { value: t.name, label: t.model },
        { value: S.int(p.tokens), label: 'tokens, ' + S.turnsText(p) }];
      if (others.length) rows.push({ value: '+' + others.length, label: 'at the same spot: ' + others.join(', ') });
      return rows;
    };
    var title = function (p) { return S.armLabel(S.arm(p.arm)); };
    var overlay = svgEl('rect', { class: 'hit', x: m.left, y: m.top, width: width - m.left - m.right, height: height - m.top - m.bottom });
    var focusOn = function (target) { marks.forEach(function (k) { k.group.classList.toggle('dim', !!target && k !== target); }); };
    var point = function (e) {
      var box = svg.getBoundingClientRect(), k = box.width ? width / box.width : 1;
      var best = pickBubble(marks, (e.clientX - box.left) * k, (e.clientY - box.top) * k);
      focusOn(best);
      if (best) S.showTip(title(best.p), rowsFor(best), e.clientX, e.clientY); else S.hideTip();
    };
    overlay.addEventListener('pointermove', point);
    overlay.addEventListener('pointerdown', point);   // a tap sends no pointermove
    // A finger leaves the chart as soon as it lifts: the tooltip stays until the next tap or scroll.
    overlay.addEventListener('pointerleave', function (e) { if (e && e.pointerType === 'touch') return; focusOn(null); S.hideTip(); });
    svg.appendChild(overlay);
    // Keyboard order follows cost, left to right.
    var hits = marks.slice().sort(function (a, b) { return a.cx - b.cx || a.cy - b.cy; }).map(function (mark) {
      var hit = svgEl('circle', { class: 'hit', cx: mark.cx, cy: mark.cy, r: Math.max(12, mark.r), 'pointer-events': 'none', role: 'img',
        'aria-label': title(mark.p) + ', ' + S.tier(mark.p.tier).name + ': ' + S.usd(mark.p.cost_warm) + ' per run, ' + S.pct(mark.p.quality) + ' correct' });
      S.bindTip(hit, title(mark.p), function () { return rowsFor(mark); }, function () { focusOn(mark); }, function () { focusOn(null); });
      svg.appendChild(hit);
      return hit;
    });
    S.roving(svg, hits);
    host.appendChild(svg);

    var questions = S.questionColumns();
    S.table(hosts.table,
      [['Method'], ['Tier'], ['Cost per run, cache warm', 1], ['Correct answers per million tokens', 1], ['Correct', 1]]
        .concat(questions.map(function (q) { return [q.label, 1]; })).concat([['Runs', 1]]),
      all.slice().sort(byTierThen('cost_warm')).map(function (p) {
        return [S.armLabel(S.arm(p.arm)), S.tier(p.tier).name, S.usd(p.cost_warm), S.num(p.efficiency, 1), S.pct(p.quality)]
          .concat(questions.map(function (q) { return S.score(p.scores, q.key); })).concat([p.n]);
      }), 'Cost, efficiency and quality by method and tier');
  }

  /* ---------- tokens per run: stacked bars, one panel per tier ---------- */
  function drawTokens(hosts, points) {
    var host = hosts.host;
    var all = points.filter(function (p) { return p.tokens > 0; });
    if (!all.length) return empty(hosts);
    host.replaceChildren();
    S.legend(hosts.legend, SEGMENTS.map(function (s) { return [s[1], s[2]]; }));
    var width = widthOf(host), narrow = width < 640;
    var gutter = narrow ? 0 : Math.min(300, Math.round(width * 0.3)), right = 56, rowH = narrow ? 46 : 30, bar = 16;
    var max = niceMax(Math.max.apply(null, all.map(function (p) { return p.tokens; })));
    var plotW = Math.max(60, width - gutter - right);
    var x = function (v) { return (typeof v === 'number' && v > 0 ? v : 0) / max * plotW; };
    S.tiersIn(all).forEach(function (t) {
      var rows = all.filter(function (p) { return p.tier === t.id; }).sort(byTierThen('tokens'));
      var title = el('div', { class: 'panel-title' });
      title.appendChild(el('strong', null, t.name));
      title.appendChild(document.createTextNode('  ' + t.model + '. Fewest tokens first.'));
      host.appendChild(title);
      var plotH = rows.length * rowH, height = plotH + 26;
      var svg = chartSvg(width, height, 'Tokens per run by method, ' + t.name);
      for (var i = 0; i <= 4; i++) {
        var gx = gutter + plotW * i / 4;
        svg.appendChild(svgEl('line', { class: i ? 'grid' : 'axis', x1: gx, x2: gx, y1: 0, y2: plotH }));
        svg.appendChild(svgEl('text', { class: 'tick', x: gx, y: plotH + 16, 'text-anchor': i ? 'middle' : 'start' }, S.tok(max * i / 4)));
      }
      var hits = rows.map(function (p, r) {
        var a = S.arm(p.arm), label = S.armLabel(a), top = r * rowH + (narrow ? 20 : (rowH - bar) / 2);
        var group = svgEl('g', { class: 'mark' });
        svg.appendChild(svgEl('text', narrow ? { x: 0, y: r * rowH + 13 } : { x: gutter - 10, y: top + bar / 2 + 4, 'text-anchor': 'end' },
          fit(label, narrow ? width : gutter - 14)));
        // The rounded end goes on the last segment wide enough to draw, which is not always the output.
        var widths = SEGMENTS.map(function (seg) { return x(p[seg[0]]); }), end = -1;
        widths.forEach(function (w, s) { if (w > 0.5) end = s; });
        var cursor = gutter;
        widths.forEach(function (w, s) {
          var seg = SEGMENTS[s], size = s === end ? w : w - 2;   // the 2px gap that separates segments
          if (s === end) group.appendChild(svgEl('path', { d: barPath(cursor, top, size, bar, 4), fill: 'var(' + seg[2] + ')' }));
          else if (s < end && size > 0.5) group.appendChild(svgEl('rect', { x: cursor, y: top, width: size, height: bar, fill: 'var(' + seg[2] + ')' }));
          cursor += w;
        });
        svg.appendChild(group);
        svg.appendChild(svgEl('text', { class: 'strong', x: cursor + 8, y: top + bar / 2 + 4 }, S.tok(p.tokens)));
        var hit = svgEl('rect', { class: 'hit', x: 0, y: r * rowH, width: width, height: rowH, role: 'img', 'aria-label': label + ', ' + t.name + ': ' + S.int(p.tokens) + ' tokens' });
        svg.appendChild(hit);
        return S.bindTip(hit, label + ', ' + t.name, function () {
          return SEGMENTS.map(function (seg) { return { color: 'var(' + seg[2] + ')', value: S.int(p[seg[0]]), label: seg[1] }; }).concat([
            { value: S.int(p.tokens), label: 'tokens in total' },
            { value: S.num(p.requests, 1), label: 'turns, ' + S.num(p.tool_calls, 1) + ' tool calls' },
            { value: S.pct(p.quality), label: 'answered correctly, ' + S.plural(p.n, 'run', 'runs') }]);
        });
      });
      S.roving(svg, hits);
      host.appendChild(svg);
    });
    S.table(hosts.table,
      [['Method'], ['Tier'], ['Runs', 1], ['Turns', 1], ['Tool calls', 1], ['Start-up', 1], ['Re-read', 1], ['Added', 1], ['Output', 1], ['Total tokens', 1], ['Correct', 1]],
      all.slice().sort(byTierThen('tokens')).map(function (p) {
        return [S.armLabel(S.arm(p.arm)), S.tier(p.tier).name, p.n, S.num(p.requests, 1), S.num(p.tool_calls, 1), S.int(p.start), S.int(p.reread),
          S.int(p.added), S.int(p.output), S.int(p.tokens), S.pct(p.quality)];
      }), 'Tokens per run by method and tier');
  }

  /* ---------- cost per run: dot plot on a log axis ---------- */
  function drawCost(hosts, points) {
    var host = hosts.host;
    var all = points.filter(function (p) { return p.cost_warm > 0; });
    if (!all.length) return empty(hosts);
    host.replaceChildren();
    S.legend(hosts.legend, tierLegend(all), true);
    var width = widthOf(host), narrow = width < 640;
    var gutter = narrow ? 0 : Math.min(300, Math.round(width * 0.3)), right = 28, rowH = narrow ? 44 : 30;
    var ids = costOrder(all);
    var xs = logScale(all.map(function (p) { return p.cost_warm; }), gutter + 12, width - right), scale = xs.scale;
    var plotH = ids.length * rowH, height = plotH + 26;
    var svg = chartSvg(width, height, 'Cost per run by method and model tier');
    xs.ticks.forEach(function (v) {
      svg.appendChild(svgEl('line', { class: 'grid', x1: scale(v), x2: scale(v), y1: 0, y2: plotH }));
      svg.appendChild(svgEl('text', { class: 'tick', x: scale(v), y: plotH + 16, 'text-anchor': 'middle' }, S.usd(v)));
    });
    var hits = [];
    ids.forEach(function (id, r) {
      var a = S.arm(id), label = S.armLabel(a), cy = r * rowH + (narrow ? 30 : rowH / 2);
      svg.appendChild(svgEl('text', narrow ? { x: 0, y: r * rowH + 13 } : { x: gutter - 10, y: cy + 4, 'text-anchor': 'end' }, fit(label, narrow ? width : gutter - 14)));
      svg.appendChild(svgEl('line', { class: 'grid', x1: gutter, x2: width - right, y1: cy, y2: cy }));
      all.filter(function (q) { return q.arm === id; }).sort(function (p, q) { return p.cost_warm - q.cost_warm; }).forEach(function (p) {
        var cx = scale(p.cost_warm), t = S.tier(p.tier), color = 'var(' + tierColor(p.tier) + ')';
        svg.appendChild(svgEl('circle', { class: 'mark', cx: cx, cy: cy, r: 5, fill: color, stroke: 'var(--surface)', 'stroke-width': 2 }));
        var hit = svgEl('circle', { class: 'hit', cx: cx, cy: cy, r: 13, role: 'img', 'aria-label': label + ', ' + t.name + ': ' + S.usd(p.cost_warm) + ' per run' });
        hits.push(S.bindTip(hit, label + ', ' + t.name, function () {
          return [
            { color: color, value: S.usd(p.cost_warm), label: 'per run, cache warm' },
            { value: S.usd(p.cost_cold), label: 'per run, cache cold' },
            { value: S.usd(p.cost), label: 'as it happened to run' },
            { value: S.int(p.tokens), label: 'tokens' },
            { value: S.pct(p.quality), label: 'answered correctly, ' + S.plural(p.n, 'run', 'runs') }];
        }));
        svg.appendChild(hit);
      });
    });
    S.roving(svg, hits);
    host.appendChild(svg);
    var rank = {};
    ids.forEach(function (id, i) { rank[id] = i; });
    // The orchestrator's share exists only in a setup that has an orchestrator.
    var orchestrated = all.some(function (p) { return typeof p.orchestrator_cost_warm === 'number'; });
    var extra = orchestrated ? [['Of which orchestrator', 1]] : [];
    S.table(hosts.table,
      [['Method'], ['Tier'], ['Cache warm', 1], ['Cache cold', 1], ['As run', 1]].concat(extra, [['Total tokens', 1], ['Correct', 1]]),
      all.slice().sort(function (a, b) { return rank[a.arm] - rank[b.arm] || S.tierOrder(a.tier) - S.tierOrder(b.tier); }).map(function (p) {
        return [S.armLabel(S.arm(p.arm)), S.tier(p.tier).name, S.usd(p.cost_warm), S.usd(p.cost_cold), S.usd(p.cost)]
          .concat(orchestrated ? [S.usd(p.orchestrator_cost_warm)] : [], [S.int(p.tokens), S.pct(p.quality)]);
      }), 'Cost per run by method and tier');
  }

  /* ---------- what the start-up context is made of: one stacked bar ---------- */
  function drawContext(hosts, parts) {
    var host = hosts.host;
    parts = (parts || []).filter(function (p) { return p.tokens > 0; });
    if (!parts.length) return empty(hosts);
    host.replaceChildren();
    var total = parts.reduce(function (sum, p) { return sum + p.tokens; }, 0);
    var shown = foldParts(parts, PART_COLORS.length);
    S.legend(hosts.legend, shown.map(function (p, i) { return [p.name, PART_COLORS[i]]; }));
    var width = widthOf(host), bar = 20, height = 46;
    var svg = chartSvg(width, height, 'What the start-up context is made of');
    var cursor = 0;
    var hits = shown.map(function (p, i) {
      var w = p.tokens / total * width, last = i === shown.length - 1, size = Math.max(0, w - (last ? 0 : 2));
      svg.appendChild(last
        ? svgEl('path', { class: 'mark', d: barPath(cursor, 4, size, bar, 4), fill: 'var(' + PART_COLORS[i] + ')' })
        : svgEl('rect', { class: 'mark', x: cursor, y: 4, width: size, height: bar, fill: 'var(' + PART_COLORS[i] + ')' }));
      if (size > 64) svg.appendChild(svgEl('text', { class: 'tick', x: cursor + 2, y: 40 }, S.tok(p.tokens)));
      var hitW = Math.max(w, 24);
      var hit = svgEl('rect', { class: 'hit', x: Math.min(cursor, width - hitW), y: 0, width: hitW, height: 32, role: 'img', 'aria-label': p.name + ': ' + S.int(p.tokens) + ' tokens' });
      svg.appendChild(hit);
      cursor += w;
      return S.bindTip(hit, p.name, function () {
        return [{ color: 'var(' + PART_COLORS[i] + ')', value: S.int(p.tokens), label: 'tokens, ' + S.pct(p.tokens / total) + ' of the start' }, { value: p.how || '', label: '' }];
      });
    });
    S.roving(svg, hits);
    host.appendChild(svg);
    S.table(hosts.table, [['Part of the start-up context'], ['Tokens', 1], ['Share', 1], ['How it was counted', 0, 'wide'], ['Who controls it', 0, 'wide']],
      parts.map(function (p) { return [p.name, S.int(p.tokens), S.pct(p.tokens / total), p.how, p.owner]; }), 'Parts of the start-up context');
  }

  /* ---------- index build time and disk: three small charts, three scales ---------- */
  function drawIndex(hosts, stacks) {
    var host = hosts.host;
    stacks = stacks || [];
    if (!stacks.length) return empty(hosts);
    host.replaceChildren();
    var charts = [['cold', 'First build, seconds'], ['one_file', 'After one file changes, seconds'], ['disk', 'On disk, megabytes']]
      .map(function (chart) { return { key: chart[0], title: chart[1], rows: stacks.filter(function (s) { return typeof s[chart[0]] === 'number'; }) }; })
      .filter(function (chart) { return chart.rows.length; });
    // The same column count the CSS grid arrives at, so each chart is drawn at the width of its cell.
    var width = gridCellWidth(host.clientWidth || GRID_MIN, charts.length);
    charts.forEach(function (chart) {
      var key = chart.key, title = chart.title, rows = chart.rows;
      var cell = el('div');
      var long = scaleSetter(rows, key);
      cell.appendChild(el('div', { class: 'panel-title' }, long ? title + '. ' + long.name + ' sets the scale, so the other bars are short.' : title));
      var rowH = 40, bar = 14, right = 50;
      var max = niceMax(Math.max.apply(null, rows.map(function (s) { return s[key]; })));
      var height = rows.length * rowH;
      var svg = chartSvg(width, height, title);
      svg.appendChild(svgEl('line', { class: 'axis', x1: 0, x2: 0, y1: 0, y2: height }));
      var hits = rows.map(function (s, r) {
        var w = Math.max(2, s[key] / max * (width - right));
        svg.appendChild(svgEl('text', { x: 4, y: r * rowH + 13 }, fit(s.name, width - 8)));
        svg.appendChild(svgEl('path', { class: 'mark', d: barPath(0, r * rowH + 19, w, bar, 4), fill: 'var(--s1)' }));
        svg.appendChild(svgEl('text', { class: 'strong', x: w + 6, y: r * rowH + 19 + bar / 2 + 4 }, plain(s[key])));
        var hit = svgEl('rect', { class: 'hit', x: 0, y: r * rowH, width: width, height: rowH, role: 'img', 'aria-label': s.name + ': ' + plain(s[key]) + ', ' + title.toLowerCase() });
        svg.appendChild(hit);
        return S.bindTip(hit, s.name, function () { return [{ color: 'var(--s1)', value: plain(s[key]), label: title.toLowerCase() }, { value: s.detail || '', label: '' }]; });
      });
      S.roving(svg, hits);
      cell.appendChild(svg);
      host.appendChild(cell);
    });
    var cellOf = function (v) { return typeof v === 'number' ? plain(v) : 'not measured'; };
    S.table(hosts.table,
      [['Index stack'], ['What it is', 0, 'wide'], ['First build, s', 1], ['Nothing changed, s', 1], ['One file changed, s', 1], ['Disk, MB', 1], ['Disk by engine, MB', 0, 'wide']],
      stacks.map(function (s) {
        var parts = Object.keys(s.parts || {}).map(function (k) { return k + ' ' + s.parts[k]; }).join(', ');
        return [s.name, s.detail, cellOf(s.cold), cellOf(s.unchanged), cellOf(s.one_file), cellOf(s.disk),
          [parts, s.note].filter(Boolean).join('. ')];
      }), 'Index build time and size by stack');
  }

  api.drawBubble = drawBubble; api.drawTokens = drawTokens; api.drawCost = drawCost;
  api.drawContext = drawContext; api.drawIndex = drawIndex;
})();
