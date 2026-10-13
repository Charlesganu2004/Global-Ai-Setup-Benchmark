/* Shared pieces for both films. Everything here is a pure function of time: a frame is drawn from t alone, so any
   frame can be rendered on its own and in any order. The look is the site's: near-black, one rust accent,
   system fonts, hairlines, grain. */
(function (global) {
  'use strict';
  var W = 1920, H = 1080;
  var clamp = function (v, a, b) { a = a === undefined ? 0 : a; b = b === undefined ? 1 : b; return v < a ? a : v > b ? b : v; };
  var mix = function (a, b, k) { return a + (b - a) * k; };
  var seg = function (t, a, b) { return b === a ? (t >= b ? 1 : 0) : clamp((t - a) / (b - a)); };

  // The spring the site uses for things that settle (docs/assets/app.css --spring), sampled.
  var SPRING = [0, 0.0394, 0.1371, 0.2666, 0.4079, 0.5468, 0.6738, 0.7833, 0.873, 0.9427, 0.9938, 1.0287, 1.05, 1.0606, 1.0633,
    1.0604, 1.054, 1.0457, 1.0367, 1.028, 1.02, 1.0132, 1.0076, 1.0033, 1.0002, 0.998, 0.9968, 0.9961, 1];
  var ease = {
    lin: function (p) { return p; },
    outQ: function (p) { return 1 - Math.pow(1 - p, 4); },
    outC: function (p) { return 1 - Math.pow(1 - p, 3); },
    outE: function (p) { return p >= 1 ? 1 : 1 - Math.pow(2, -10 * p); },
    inQ: function (p) { return p * p * p * p; },
    inC: function (p) { return p * p * p; },
    ioC: function (p) { return p < 0.5 ? 4 * p * p * p : 1 - Math.pow(-2 * p + 2, 3) / 2; },
    ioQ: function (p) { return p < 0.5 ? 8 * p * p * p * p : 1 - Math.pow(-2 * p + 2, 4) / 2; },
    spring: function (p) {
      var f = clamp(p) * (SPRING.length - 1), i = Math.min(SPRING.length - 2, Math.floor(f));
      return mix(SPRING[i], SPRING[i + 1], f - i);
    }
  };
  // Deterministic random numbers: the same seed gives the same particles in every frame and every render.
  function rng(seed) {
    var a = seed >>> 0;
    return function () {
      a = (a + 0x6D2B79F5) >>> 0;
      var t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }

  var ACCENT = [255, 122, 61], HOT = [255, 181, 148], INK = [244, 244, 246], INK2 = [184, 184, 193], CONTROL = [143, 143, 154];
  var rgba = function (c, a) { return 'rgba(' + c[0] + ',' + c[1] + ',' + c[2] + ',' + (a < 0 ? 0 : a > 1 ? 1 : a).toFixed(3) + ')'; };
  var lerpC = function (a, b, k) { return [mix(a[0], b[0], k), mix(a[1], b[1], k), mix(a[2], b[2], k)]; };

  // The canvas: a CSS size of 1920x1080 and a backing store of that times the device scale (1 for 1080p, 2 for 2160p).
  function makeCanvas(id) {
    var c = document.getElementById(id), dpr = global.devicePixelRatio || 1;
    c.width = Math.round(W * dpr); c.height = Math.round(H * dpr);
    c.style.width = W + 'px'; c.style.height = H + 'px';
    var ctx = c.getContext('2d');
    return { c: c, ctx: ctx, dpr: dpr, begin: function () { ctx.setTransform(dpr, 0, 0, dpr, 0, 0); ctx.globalCompositeOperation = 'source-over'; ctx.globalAlpha = 1; } };
  }

  // A soft round light, drawn once and stamped. Tinted per colour.
  var sprites = {};
  function glow(color) {
    var key = color.join(',');
    if (sprites[key]) return sprites[key];
    var dpr = global.devicePixelRatio || 1, S = Math.round(96 * dpr), c = document.createElement('canvas');
    c.width = c.height = S;
    var g = c.getContext('2d'), grad = g.createRadialGradient(S / 2, S / 2, 0, S / 2, S / 2, S / 2);
    grad.addColorStop(0, rgba(color, 0.95)); grad.addColorStop(0.18, rgba(color, 0.5)); grad.addColorStop(0.5, rgba(color, 0.12)); grad.addColorStop(1, rgba(color, 0));
    g.fillStyle = grad; g.fillRect(0, 0, S, S);
    return (sprites[key] = c);
  }
  function stamp(ctx, color, x, y, r, a) {
    if (a <= 0.003) return;
    ctx.globalAlpha = a > 1 ? 1 : a;
    ctx.drawImage(glow(color), x - r, y - r, r * 2, r * 2);
    ctx.globalAlpha = 1;
  }

  // The background: off-black with a faint pool of light and a vignette, so a ground reads as a surface.
  var bgCache = null;
  function background(ctx, dpr, shiftX, shiftY) {
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.globalCompositeOperation = 'source-over'; ctx.globalAlpha = 1;
    ctx.fillStyle = '#0a0a0c'; ctx.fillRect(0, 0, W, H);
    var cx = W * 0.38 + (shiftX || 0), cy = H * 0.42 + (shiftY || 0);
    var g = ctx.createRadialGradient(cx, cy, 0, cx, cy, 1250);
    g.addColorStop(0, 'rgba(255,255,255,0.05)'); g.addColorStop(0.55, 'rgba(255,255,255,0.016)'); g.addColorStop(1, 'rgba(255,255,255,0)');
    ctx.fillStyle = g; ctx.fillRect(0, 0, W, H);
    var v = ctx.createRadialGradient(W / 2, H / 2, 520, W / 2, H / 2, 1180);
    v.addColorStop(0, 'rgba(0,0,0,0)'); v.addColorStop(1, 'rgba(0,0,0,0.55)');
    ctx.fillStyle = v; ctx.fillRect(0, 0, W, H);
  }

  // A hairline: 1 px on a 1080p stage, which is 2 device pixels on a 2160p render (the site draws .5 px at 2x).
  function hair(ctx, a) { ctx.lineWidth = 1; ctx.strokeStyle = 'rgba(255,255,255,' + (a === undefined ? 0.14 : a) + ')'; }

  // ---------------- DOM text helpers ----------------
  function el(tag, cls, html, parent) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (html !== undefined) e.innerHTML = html;
    if (parent) parent.appendChild(e);
    return e;
  }
  var shown = new WeakMap();
  function vis(e, on) {
    if (shown.get(e) === on) return;
    shown.set(e, on); e.style.visibility = on ? 'visible' : 'hidden';
  }
  // Place and fade an element. Rotation and scale around its own origin; o is opacity.
  function put(e, o) {
    var x = o.x || 0, y = o.y || 0, s = o.s === undefined ? 1 : o.s, op = o.o === undefined ? 1 : o.o;
    if (op <= 0.001) { vis(e, false); return; }
    vis(e, true);
    e.style.opacity = op >= 0.999 ? '' : op.toFixed(3);
    e.style.transform = 'translate3d(' + x.toFixed(2) + 'px,' + y.toFixed(2) + 'px,0)' + (s === 1 ? '' : ' scale(' + s.toFixed(4) + ')');
  }
  // A line of type that rises out of its own mask. Returns {root, inner}; rise(line, p) moves it.
  function maskLine(parent, text, cls, style) {
    var root = el('div', 'mask ' + (cls || ''), '', parent), inner = el('span', 'rise', text, root);
    if (style) for (var k in style) root.style[k] = style[k];
    return { root: root, inner: inner };
  }
  function rise(line, p) {
    if (p <= 0) { vis(line.root, false); return; }
    vis(line.root, true);
    line.inner.style.transform = 'translate3d(0,' + ((1 - p) * 118).toFixed(2) + '%,0)';
  }
  // Words of a headline, each in its own mask, with the uneven gaps the motion rules ask for (frames at 30 fps).
  var GAPS = [0, 3, 2, 2, 2, 2, 3, 2];
  function words(parent, text, cls) {
    var root = el('div', 'words ' + (cls || ''), '', parent), list = [];
    text.split(' ').forEach(function (w, i) {
      var m = el('span', 'wmask', '', root), inner = el('span', 'rise', w, m);
      list.push({ root: m, inner: inner });
      if (i < text.split(' ').length - 1) root.appendChild(document.createTextNode(' '));
    });
    return { root: root, list: list };
  }
  function riseWords(ws, t, t0, dur) {
    var acc = 0, any = false;
    ws.list.forEach(function (w, i) {
      acc += (GAPS[Math.min(i, GAPS.length - 1)] || 2) / 30;
      var p = ease.outQ(seg(t, t0 + acc, t0 + acc + (dur || 0.5)));
      w.inner.style.transform = 'translate3d(0,' + ((1 - p) * 118).toFixed(2) + '%,0)';
      if (p > 0) any = true;
    });
    vis(ws.root, any);
  }

  // The mark: a P with the token it hands over. The path is the site's logo. Returns a drawer for canvas and a DOM svg.
  var MARK_STEM = 'M7 4v16', MARK_BOWL = 'M7 4h6.5a4.25 4.25 0 0 1 0 8.5H7';
  function markSvg(parent, px, cls) {
    var s = '<svg viewBox="0 0 24 24" width="' + px + '" height="' + px + '" aria-hidden="true"><path class="m-stem" d="' + MARK_STEM + '" pathLength="1"/><path class="m-bowl" d="' + MARK_BOWL + '" pathLength="1"/><circle class="m-dot" cx="18.5" cy="18.6" r="1.9"/></svg>';
    var d = el('div', 'mark ' + (cls || ''), s, parent);
    return { root: d, stem: d.querySelector('.m-stem'), bowl: d.querySelector('.m-bowl'), dot: d.querySelector('.m-dot') };
  }
  // p from 0 to 1: the stem is drawn, then the bowl, then the token lands as a dot with a small overshoot (the one spring).
  function markDraw(m, p) {
    var a = ease.outC(seg(p, 0, 0.35)), b = ease.outC(seg(p, 0.3, 0.8)), c = seg(p, 0.72, 1);
    m.stem.style.strokeDasharray = '1 1'; m.stem.style.strokeDashoffset = (1 - a).toFixed(4);
    m.bowl.style.strokeDasharray = '1 1'; m.bowl.style.strokeDashoffset = (1 - b).toFixed(4);
    var s = ease.spring(c);
    m.dot.style.transformBox = 'fill-box'; m.dot.style.transformOrigin = 'center';
    m.dot.style.transform = 'scale(' + (c <= 0 ? 0 : s).toFixed(3) + ')';
    vis(m.root, p > 0);
  }

  // Number formats, the site's (docs/assets/app.js).
  function fmtTokens(v) { return !(v > 0) ? '0' : v >= 1e6 ? (v / 1e6).toFixed(2) + 'M' : Math.round(v / 1e3) + 'k'; }
  function fmtTurns(v) { return v.toFixed(1); }

  // Fonts must be ready before the first frame.
  function ready() {
    var fonts = document.fonts && document.fonts.ready ? document.fonts.ready : Promise.resolve();
    return fonts.then(function () { return new Promise(function (r) { requestAnimationFrame(function () { requestAnimationFrame(r); }); }); });
  }

  // Film grain: one tile of noise shifted every few frames, so it lives but costs the encoder little.
  function grainShift(frame, step) {
    var k = Math.floor(frame / (step || 3));
    return [(k * 53) % 160, (k * 97) % 160];
  }

  global.PGRLIB = {
    W: W, H: H, clamp: clamp, mix: mix, seg: seg, ease: ease, rng: rng,
    ACCENT: ACCENT, HOT: HOT, INK: INK, INK2: INK2, CONTROL: CONTROL, rgba: rgba, lerpC: lerpC,
    makeCanvas: makeCanvas, glow: glow, stamp: stamp, background: background, hair: hair,
    el: el, vis: vis, put: put, maskLine: maskLine, rise: rise, words: words, riseWords: riseWords,
    markSvg: markSvg, markDraw: markDraw, MARK_STEM: MARK_STEM, MARK_BOWL: MARK_BOWL,
    fmtTokens: fmtTokens, fmtTurns: fmtTurns, ready: ready, grainShift: grainShift
  };
})(window);
