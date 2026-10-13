/* The four things PGR gives an agent, drawn as outcomes only: a definition with its callers, a search by meaning,
   the reach of a change, findings that persist. Each drawer takes build progress p from 0 to 1 and draws into a
   design box of about 1100 x 640 units centred on (cx, cy) at scale s. Names are made-up examples. */
(function (global) {
  'use strict';
  var L = global.PGRLIB, seg = L.seg, ease = L.ease, mix = L.mix, rgba = L.rgba, ACC = L.ACCENT, HOT = L.HOT, INK = L.INK, INK2 = L.INK2;
  var MONO = '500 30px ui-monospace, "SF Mono", "Cascadia Mono", Consolas, monospace';
  var SANS = '500 32px -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif';

  function begin(ctx, o) { ctx.save(); ctx.translate(o.cx, o.cy); ctx.scale(o.s, o.s); ctx.lineCap = 'round'; ctx.lineJoin = 'round'; }
  function node(ctx, x, y, r, color, a, filled) {
    ctx.beginPath(); ctx.arc(x, y, r, 0, Math.PI * 2);
    if (filled) { ctx.fillStyle = rgba(color, a); ctx.fill(); } else { ctx.strokeStyle = rgba(color, a); ctx.stroke(); }
  }
  function lineTo(ctx, x0, y0, x1, y1, q, color, a, w) {
    if (q <= 0) return;
    ctx.strokeStyle = rgba(color, a); ctx.lineWidth = w;
    ctx.beginPath(); ctx.moveTo(x0, y0); ctx.lineTo(mix(x0, x1, q), mix(y0, y1, q)); ctx.stroke();
  }
  function curve(ctx, x0, y0, cx, cy, x1, y1, q, color, a, w) {
    if (q <= 0) return;
    ctx.strokeStyle = rgba(color, a); ctx.lineWidth = w;
    var n = Math.max(2, Math.round(24 * q)), i, u, x, y;
    ctx.beginPath(); ctx.moveTo(x0, y0);
    for (i = 1; i <= n; i++) {
      u = q * i / n; x = (1 - u) * (1 - u) * x0 + 2 * (1 - u) * u * cx + u * u * x1; y = (1 - u) * (1 - u) * y0 + 2 * (1 - u) * u * cy + u * u * y1;
      ctx.lineTo(x, y);
    }
    ctx.stroke();
  }
  function onCurve(x0, y0, cx, cy, x1, y1, u) {
    return [(1 - u) * (1 - u) * x0 + 2 * (1 - u) * u * cx + u * u * x1, (1 - u) * (1 - u) * y0 + 2 * (1 - u) * u * cy + u * u * y1];
  }
  function label(ctx, text, x, y, color, a, align, font) {
    if (a <= 0.01) return;
    ctx.font = font || MONO; ctx.textAlign = align || 'left'; ctx.textBaseline = 'middle';
    ctx.fillStyle = rgba(color, a); ctx.fillText(text, x, y);
  }
  function glowDot(ctx, x, y, r, color, a) { L.stamp(ctx, color, x, y, r * 5, a * 0.9); }

  /* 1. A definition and what calls it, in one lookup. */
  var CALLERS = ['main', 'run_job', 'reload', 'test_load', 'cli', 'init_app'];
  function definition(ctx, p, o) {
    begin(ctx, o);
    var dx = -330, dy = 0, lw = 1 / Math.max(o.s, 0.3);
    var a = ease.outQ(seg(p, 0, 0.18));
    ctx.lineWidth = 3.5;
    ctx.strokeStyle = rgba(ACC, 1); ctx.beginPath(); ctx.arc(dx, dy, 62, -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * a); ctx.stroke();
    node(ctx, dx, dy, 15 * a, ACC, 1, true); glowDot(ctx, dx, dy, 20, ACC, a * 0.55);
    if (o.labels) {
      label(ctx, 'load_settings(path, strict)', dx, dy - 118, INK, ease.outQ(seg(p, 0.14, 0.2)), 'center');
      label(ctx, 'defined here', dx, dy + 112, INK2, ease.outQ(seg(p, 0.2, 0.26)), 'center', SANS);
    }
    CALLERS.forEach(function (name, k) {
      var ang = (-52 + k * 20.8) * Math.PI / 180, R = 640;
      var x = dx + R * Math.cos(ang), y = dy + R * Math.sin(ang) * 0.36 + 4;
      var t0 = 0.26 + k * 0.075, q = ease.outC(seg(p, t0, t0 + 0.14));
      var mx = (x + dx) / 2, my = (y + dy) / 2 + (k % 2 ? 46 : -46);
      curve(ctx, x, y, mx, my, dx + 64, dy + (y > 0 ? 18 : -18), q, INK, 0.3, lw);
      node(ctx, x, y, 17, INK, q > 0 ? 0.9 * seg(q, 0, 0.3) : 0, false);
      node(ctx, x, y, 7 * seg(q, 0, 0.3), INK, 0.9, true);
      if (o.labels) label(ctx, name, x + 34, y, INK2, seg(q, 0.2, 0.6), 'left');
      // a token travels from the caller to the definition once the edge is drawn
      var u = ease.ioC(seg(p, t0 + 0.14, t0 + 0.3));
      if (u > 0 && u < 1) { var pt = onCurve(x, y, mx, my, dx + 64, dy + (y > 0 ? 18 : -18), u); glowDot(ctx, pt[0], pt[1], 7, ACC, 0.9); node(ctx, pt[0], pt[1], 5, HOT, 1, true); }
    });
    var pulse = seg(p, 0.82, 1);
    if (pulse > 0) { ctx.strokeStyle = rgba(ACC, 0.5 * (1 - pulse)); ctx.lineWidth = 2; ctx.beginPath(); ctx.arc(dx, dy, 62 + 90 * ease.outQ(pulse), 0, Math.PI * 2); ctx.stroke(); }
    ctx.restore();
  }

  /* 2. Search by meaning: a description in words, the function that does the job lights up. */
  var CONS = (function () {
    var r = L.rng(11), pts = [], i;
    for (i = 0; i < 54; i++) {
      var a = r() * Math.PI * 2, d = Math.sqrt(r());
      pts.push({ x: 120 + Math.cos(a) * d * 430, y: 40 + Math.sin(a) * d * 250, r: 3 + r() * 3.5 });
    }
    pts[0] = { x: 230, y: 10, r: 8, hit: true };
    return pts;
  })();
  function meaning(ctx, p, o) {
    begin(ctx, o);
    var lw = 1 / Math.max(o.s, 0.3), pill = ease.outQ(seg(p, 0, 0.14));
    // the description
    ctx.strokeStyle = rgba(INK, 0.3 * pill); ctx.lineWidth = lw;
    var px = -520, py = -250, pw = 560, ph = 64;
    ctx.beginPath(); if (ctx.roundRect) ctx.roundRect(px, py, pw * pill, ph, 32); else ctx.rect(px, py, pw * pill, ph); ctx.stroke();
    if (o.labels) {
      var words = ['save', 'a', 'report', 'to', 'disk'], shown = '';
      words.forEach(function (w, i) { if (p > 0.12 + i * 0.045) shown += (shown ? ' ' : '') + w; });
      label(ctx, shown, px + 30, py + ph / 2, INK, pill, 'left');
    }
    var wave = ease.outC(seg(p, 0.34, 0.62));
    CONS.forEach(function (c, i) {
      var d = Math.hypot(c.x - (px + pw), c.y - py), lit = wave * 900 > d ? 1 : 0;
      var near = c.hit ? 1 : 0;
      var base = 0.5 * seg(p, 0.02 + (i % 9) * 0.01, 0.2);
      var a = c.hit ? Math.max(base, seg(p, 0.5, 0.64)) : mix(base, base * 0.35, seg(p, 0.5, 0.7));
      if (lit && !c.hit) a = Math.max(a, 0.55 * (1 - seg(p, 0.6, 0.78) * 0.7));
      node(ctx, c.x, c.y, c.r * (c.hit ? mix(1, 1.9, ease.outQ(seg(p, 0.55, 0.7))) : 1), c.hit ? ACC : INK, a, true);
      if (near) glowDot(ctx, c.x, c.y, 12, ACC, seg(p, 0.55, 0.72) * 0.9);
    });
    if (wave > 0 && wave < 1) { ctx.strokeStyle = rgba(ACC, 0.35 * (1 - wave)); ctx.lineWidth = 2; ctx.beginPath(); ctx.arc(px + pw, py + ph / 2, 900 * wave, 0, Math.PI * 2); ctx.stroke(); }
    var q = ease.outC(seg(p, 0.62, 0.8));
    if (q > 0) {
      curve(ctx, px + pw, py + ph / 2, 120, -230, CONS[0].x, CONS[0].y - 14, q, ACC, 0.8, lw * 1.6);
      if (o.labels) label(ctx, 'write_report', CONS[0].x + 34, CONS[0].y + 6, INK, seg(q, 0.5, 1), 'left', '600 40px ui-monospace, "Cascadia Mono", Consolas, monospace');
    }
    ctx.restore();
  }

  /* 3. What a change reaches: rings of dependents light up hop by hop, the rest stay dark. */
  var REACH = (function () {
    var r = L.rng(5), hops = [[5, 190, 0.2], [10, 360, 0.5], [8, 520, 0.9]], out = [];
    hops.forEach(function (h, k) {
      for (var i = 0; i < h[0]; i++) {
        var a = (i / h[0]) * Math.PI * 2 + k * 0.4 + (r() - 0.5) * 0.3, rad = h[1] * (0.94 + r() * 0.12);
        out.push({ x: Math.cos(a) * rad * 1.28, y: Math.sin(a) * rad * 0.62, hop: k + 1, r: 6 + r() * 3, parent: -1 });
      }
    });
    out.forEach(function (n, i) {
      if (n.hop === 1) { n.px = 0; n.py = 0; return; }
      var best = 1e9, bi = 0;
      out.forEach(function (m, j) { if (m.hop === n.hop - 1) { var d = Math.hypot(m.x - n.x, m.y - n.y); if (d < best) { best = d; bi = j; } } });
      n.px = out[bi].x; n.py = out[bi].y;
    });
    return out;
  })();
  function reach(ctx, p, o) {
    begin(ctx, o);
    var lw = 1 / Math.max(o.s, 0.3), core = ease.outQ(seg(p, 0, 0.1));
    var wave = [ease.outC(seg(p, 0.14, 0.38)), ease.outC(seg(p, 0.36, 0.64)), 0];
    REACH.forEach(function (n) {
      var k = n.hop, w = k <= 2 ? wave[k - 1] : 0, ew = k <= 2 ? w : 0;
      if (ew > 0) lineTo(ctx, n.px, n.py, n.x, n.y, ew, INK, 0.22, lw);
      if (k === 3) { node(ctx, n.x, n.y, n.r, INK, 0.16 * seg(p, 0.1, 0.3), true); return; }
      var hot = w > 0 ? 1 - seg(p, 0.7, 0.95) * 0.55 : 0;
      node(ctx, n.x, n.y, n.r * (0.6 + 0.4 * w), INK, 0.16 + 0.8 * w * 0.7, true);
      if (w > 0 && hot > 0.01) glowDot(ctx, n.x, n.y, n.r, ACC, w * hot * 0.6);
    });
    [190, 360].forEach(function (rad, i) {
      var w = wave[i];
      if (w > 0 && w < 1) { ctx.strokeStyle = rgba(ACC, 0.4 * (1 - w) + 0.1); ctx.lineWidth = lw * 1.5; ctx.beginPath(); ctx.ellipse(0, 0, rad * 1.28 * w, rad * 0.62 * w, 0, 0, Math.PI * 2); ctx.stroke(); }
    });
    glowDot(ctx, 0, 0, 22, ACC, core * 0.9);
    node(ctx, 0, 0, 22 * core, ACC, 1, true);
    node(ctx, 0, 0, 40 * core + 14 * Math.max(0, 1 - seg(p, 0.1, 0.4)), ACC, 0.7 * core, false);
    if (o.labels) {
      label(ctx, 'the change', 0, -76, INK, seg(p, 0.06, 0.14), 'center', SANS);
    }
    ctx.restore();
  }

  /* 4. Findings that stay: what one session worked out is already there in the next. */
  var GRID = (function () {
    var out = [], c, r;
    for (r = 0; r < 4; r++) for (c = 0; c < 5; c++) out.push({ found: [2, 6, 8, 13, 16, 19].indexOf(out.length) >= 0 });
    return out;
  })();
  function memory(ctx, p, o) {
    begin(ctx, o);
    var lw = 1 / Math.max(o.s, 0.3), panel = ease.outQ(seg(p, 0, 0.16)), PW = 470, PH = 400;
    [-1, 1].forEach(function (side) {
      var x0 = side < 0 ? -PW - 56 : 56, y0 = -PH / 2 - 10;
      ctx.strokeStyle = rgba(INK, 0.22 * panel); ctx.lineWidth = lw;
      ctx.beginPath(); if (ctx.roundRect) ctx.roundRect(x0, y0, PW, PH, 22); else ctx.rect(x0, y0, PW, PH); ctx.stroke();
      GRID.forEach(function (g, i) {
        var gx = x0 + 60 + (i % 5) * 88, gy = y0 + 70 + Math.floor(i / 5) * 90;
        var lit;
        if (side < 0) lit = g.found ? ease.outC(seg(p, 0.16 + GRID.filter(function (z, j) { return j < i && z.found; }).length * 0.06, 0.3 + GRID.filter(function (z, j) { return j < i && z.found; }).length * 0.06)) : 0;
        else lit = g.found ? ease.outC(seg(p, 0.66 + GRID.filter(function (z, j) { return j < i && z.found; }).length * 0.02, 0.78 + GRID.filter(function (z, j) { return j < i && z.found; }).length * 0.02)) : 0;
        node(ctx, gx, gy, 9, INK, (0.2 + 0.0) * panel, true);
        if (lit > 0) { node(ctx, gx, gy, 9 + 5 * (1 - lit), ACC, lit, true); glowDot(ctx, gx, gy, 10, ACC, lit * 0.6); }
      });
      if (o.labels) label(ctx, side < 0 ? 'one session' : 'the next', x0 + PW / 2, y0 + PH + 52, INK2, panel, 'center', SANS);
    });
    // the findings cross over
    GRID.forEach(function (g, i) {
      if (!g.found) return;
      var k = GRID.filter(function (z, j) { return j < i && z.found; }).length, t0 = 0.5 + k * 0.025;
      var u = ease.ioC(seg(p, t0, t0 + 0.18));
      if (u > 0 && u < 1) {
        var ax = -PW - 56 + 60 + (i % 5) * 88, ay = -PH / 2 - 10 + 70 + Math.floor(i / 5) * 90, bx = 56 + 60 + (i % 5) * 88;
        var pt = onCurve(ax, ay, (ax + bx) / 2, ay - 90, bx, ay, u);
        glowDot(ctx, pt[0], pt[1], 7, ACC, 0.9); node(ctx, pt[0], pt[1], 4.5, HOT, 1, true);
      }
    });
    ctx.restore();
  }

  global.PGRGFX = { definition: definition, meaning: meaning, reach: reach, memory: memory, list: [definition, meaning, reach, memory] };
})(window);
