/* The Overview page: the two token streams. One canvas draws two lanes to the same scale: lane length is
   tokens per task, one particle stands for 1k tokens, the stream speed follows turns per task. Every number
   comes from window.SHOWCASE. The lanes, their labels and the difference are also written in the page, so
   without a script, without canvas or with reduced motion the page shows the same measurements as a still. */
(function () {
  'use strict';
  var App = window.App, data = window.SHOWCASE, fig = document.getElementById('stream');
  if (!App || !data || !fig) return;
  var found = App.headline(data);
  if (!found || !found.c) return;
  var host = fig.querySelector('.lanes'), els = [fig.querySelector('.lane-ref'), fig.querySelector('.lane-best')];
  var canvas = document.createElement('canvas'), ctx = canvas.getContext ? canvas.getContext('2d') : null;
  if (!host || !ctx || !els[0] || !els[1]) return;
  canvas.setAttribute('aria-hidden', 'true');
  host.appendChild(canvas);
  var gap = fig.querySelector('.gap');
  if (gap) gap.classList.add('wait');   // before the canvas takes over, so the label never flashes
  fig.classList.add('has-canvas');

  var PX_PER_TURN = 22, EDGE = 0.045, rows = 6;
  // Near, middle and far particles: size, brightness and speed fall with distance.
  var LAYERS = [{ a: 0.95, t: 1.6, s: 1 }, { a: 0.6, t: 1.1, s: 0.82 }, { a: 0.32, t: 0.8, s: 0.64 }];
  var R = Math.max(found.b.tokens, found.c.tokens);
  var rgb = (getComputedStyle(document.documentElement).getPropertyValue('--accent-rgb') || '255,122,61').replace(/\s/g, '');
  var lanes = [found.c, found.b].map(function (p, i) {
    var el = els[i];
    return { el: el, tokens: p.tokens, turns: p.turns, accent: i === 1, ph: [0, 0, 0],
      tokEl: el.querySelector('[data-tok]'), turnEl: el.querySelector('[data-turns]'),
      tokText: el.querySelector('[data-tok]') ? el.querySelector('[data-tok]').textContent : '',
      turnText: el.querySelector('[data-turns]') ? el.querySelector('[data-turns]').textContent : '' };
  });

  // One particle for each 1k tokens of the longest lane, spread evenly along it so the density never clumps.
  var seed = 7;
  function rnd() { seed = (seed * 1664525 + 1013904223) >>> 0; return seed / 4294967296; }
  var N = Math.max(40, Math.round(R / 1000)), P = [];
  for (var i = 0; i < N; i++) {
    var r = rnd();
    P.push({ s: (i + rnd()) / N, row: rnd(), layer: r < 0.2 ? 0 : r < 0.55 ? 1 : 2 });
  }

  var W = 0, H = 0, dpr = 1, tr = [], ticks = [], spacing = 16;
  function measure() {
    // Layout sizes, not bounding boxes: the card may be mid-reveal and scaled when this runs.
    W = host.offsetWidth; H = host.offsetHeight;
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.round(W * dpr);
    canvas.height = Math.round(H * dpr);
    tr = lanes.map(function (lane) {
      var b = lane.el.querySelector('.lane-track');
      return { x: lane.el.offsetLeft + b.offsetLeft, y: lane.el.offsetTop + b.offsetTop, w: b.offsetWidth, h: b.offsetHeight };
    });
    rows = Math.max(5, Math.round(tr[0].h / 8));
    spacing = tr[0].w * rows / N;   // the room a particle has along its row, so a streak never runs into the next one
    ticks = Array.prototype.map.call(fig.querySelectorAll('.scale span'), function (s) { return parseFloat(s.getAttribute('data-v')) / R; })
      .filter(function (f) { return f > 0 && f < 1; });
  }
  function box(x, y, w, h, rad) { ctx.beginPath(); if (ctx.roundRect) ctx.roundRect(x, y, w, h, rad); else ctx.rect(x, y, w, h); }
  function clamp(v) { return v < 0 ? 0 : v > 1 ? 1 : v; }

  // The lane of the best configuration starts as long as the no-index lane and settles on its measured length.
  var intro = -1, DELAY = 0.45, SPAN = 1.5, shownTok = null, shownTurn = null, waiting = true;
  function draw(dt) {
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, W, H);
    var k = intro < 0 ? 0 : App.ease(clamp((intro - DELAY) / SPAN)), hair = dpr >= 2 ? 0.5 : 1;
    var startFrac = lanes[0].tokens / R;
    lanes.forEach(function (lane, n) {
      var t = tr[n], frac = n ? startFrac + (lane.tokens / R - startFrac) * k : lane.tokens / R;
      var turns = n ? lanes[0].turns + (lane.turns - lanes[0].turns) * k : lane.turns;
      var tone = lane.accent ? rgb : '178,178,190', vf = turns * PX_PER_TURN / t.w;
      // the lane itself, then the scale lines inside it
      ctx.globalCompositeOperation = 'source-over';
      ctx.globalAlpha = 1;
      ctx.lineWidth = hair;
      box(t.x + hair / 2, t.y + hair / 2, t.w * frac - hair, t.h - hair, 10);
      ctx.fillStyle = lane.accent ? 'rgba(' + rgb + ',.09)' : 'rgba(255,255,255,.04)';
      ctx.fill();
      ctx.strokeStyle = lane.accent ? 'rgba(' + rgb + ',.6)' : 'rgba(255,255,255,.16)';
      ctx.stroke();
      ctx.fillStyle = 'rgba(255,255,255,.07)';
      for (var m = 0; m < ticks.length; m++) {
        if (ticks[m] < frac - 0.002) ctx.fillRect(Math.round((t.x + ticks[m] * t.w) * dpr) / dpr, t.y + 4, hair, t.h - 8);
      }
      ctx.fillStyle = lane.accent ? 'rgb(' + rgb + ')' : 'rgb(184,184,193)';
      ctx.fillRect(t.x + t.w * frac - 1.5, t.y - 3, 2, t.h + 6);
      // the stream: additive, so overlaps brighten instead of covering
      ctx.globalCompositeOperation = 'lighter';
      ctx.fillStyle = 'rgb(' + tone + ')';
      for (var j = 0; j < 3; j++) lane.ph[j] += vf * LAYERS[j].s * dt;
      for (var q = 0; q < P.length; q++) {
        var p = P[q], L = LAYERS[p.layer], x = p.s + lane.ph[p.layer];
        x -= Math.floor(x);
        if (x >= frac) continue;
        var f = Math.min(1, x / EDGE, (frac - x) / EDGE);
        if (f <= 0) continue;
        var a = L.a * f * f * (3 - 2 * f), len = Math.max(1.5, Math.min(vf * t.w * L.s * 0.11, spacing * 0.6)), px = t.x + x * t.w;
        var yy = Math.round((t.y + (Math.floor(p.row * rows) + 0.5) * t.h / rows) * dpr) / dpr - L.t / 2;
        if (p.layer === 0) {
          // near particles: a faint long tail behind a short bright head
          ctx.globalAlpha = a * 0.3;
          ctx.fillRect(px - len * 2.4, yy, len * 2.4, L.t);
          ctx.globalAlpha = a;
          ctx.fillRect(px - len * 0.6, yy, len * 0.6, L.t);
        } else {
          ctx.globalAlpha = a;
          ctx.fillRect(px - len, yy, len, L.t);
        }
      }
    });
    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = 'source-over';
    // the numbers on the labels follow the lane while it settles, then read exactly as written in the page
    var b = lanes[1];
    var tok = k >= 1 ? b.tokText : App.tokens(startFrac * R + (b.tokens - startFrac * R) * k);
    var trn = k >= 1 ? b.turnText : App.turns(lanes[0].turns + (b.turns - lanes[0].turns) * k);
    if (b.tokEl && tok !== shownTok) { shownTok = tok; b.tokEl.textContent = tok; }
    if (b.turnEl && trn !== shownTurn) { shownTurn = trn; b.turnEl.textContent = trn; }
    if (gap && (k < 1) !== waiting) { waiting = k < 1; gap.classList.toggle('wait', waiting); }
  }

  var visible = false, running = false, last = 0;
  function still() { intro = DELAY + SPAN + 1; draw(0); }
  function tick(now) {
    if (App.reduced) { running = false; still(); return; }
    if (!visible || document.hidden) { running = false; return; }
    // Never draw faster than about 70 times a second, whatever the screen: the motion is timed, not stepped.
    if (last && now - last < 12) { requestAnimationFrame(tick); return; }
    var dt = last ? Math.min(0.05, (now - last) / 1000) : 0;
    last = now;
    if (intro >= 0) intro += dt;
    draw(dt);
    requestAnimationFrame(tick);
  }
  function start() { if (running || App.reduced) return; running = true; last = 0; requestAnimationFrame(tick); }

  measure();
  if (App.reduced) still(); else draw(0);
  if ('ResizeObserver' in window) {
    new ResizeObserver(function () { measure(); if (!running) { if (App.reduced) still(); else draw(0); } }).observe(host);
  }
  if ('IntersectionObserver' in window && !App.reduced) {
    new IntersectionObserver(function (entries) {
      var e = entries[entries.length - 1];
      visible = e.isIntersecting;
      if (visible && e.intersectionRatio >= 0.35 && intro < 0) intro = 0;
      if (visible) start();
    }, { threshold: [0, 0.35] }).observe(host);
    document.addEventListener('visibilitychange', function () { if (!document.hidden && visible) start(); });
  } else if (!App.reduced) {
    visible = true;
    intro = 0;
    start();
  }
})();
