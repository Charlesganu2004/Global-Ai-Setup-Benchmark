/* Shared pieces for both films. Every frame is a pure function of the time t in seconds: nothing counts frames, so the
   same t draws the same picture at 30, 60 or 120 frames a second. The page draws on a 2D canvas (the scene), and the
   engine below averages several sub-frames of it in a float texture (real motion blur: the shutter is a fixed
   1/120 s), adds a soft bloom from the bright parts, and shows the result on a WebGL canvas. No noise, no grain and
   no dither anywhere: smooth gradients are left to the 8-bit limit of the picture and a generous encode.
   The look is the site's: near-black, one rust accent, system fonts, hairlines, the radius scale 6 / 10 / 16. */
(function (global) {
  'use strict';
  var W = 1920, H = 1080, DPR = global.devicePixelRatio || 1;
  var SHUTTER = 1 / 120;      // seconds the virtual shutter stays open, whatever the frame rate
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
    ioE: function (p) { return p <= 0 ? 0 : p >= 1 ? 1 : p < 0.5 ? Math.pow(2, 20 * p - 10) / 2 : 1 - Math.pow(2, -20 * p + 10) / 2; },
    smooth: function (p) { return p * p * (3 - 2 * p); },
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

  var ACCENT = [255, 122, 61], HOT = [255, 181, 148], INK = [244, 244, 246], INK2 = [184, 184, 193], INK3 = [146, 146, 156], CONTROL = [143, 143, 154], BG = [10, 10, 12];
  var rgba = function (c, a) { return 'rgba(' + (c[0] | 0) + ',' + (c[1] | 0) + ',' + (c[2] | 0) + ',' + (a < 0 ? 0 : a > 1 ? 1 : a).toFixed(3) + ')'; };
  var lerpC = function (a, b, k) { return [mix(a[0], b[0], k), mix(a[1], b[1], k), mix(a[2], b[2], k)]; };
  var R_S = 6, R_M = 10, R_L = 16;      // the site's radius scale

  // ------------------------------------------------------------------ text on the canvas
  var FAM = {
    d: '"Segoe UI Variable Display", "SF Pro Display", -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif',
    t: '"Segoe UI Variable Text", "SF Pro Text", -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif',
    m: '"Cascadia Mono", ui-monospace, "SF Mono", Consolas, monospace'
  };
  var drawn = [], logging = false;           // what a frame wrote, for tools/check-numbers.mjs
  // The motion probe. Before a frame is drawn the engine draws the scene twice, at the two ends of the shutter, with TR set:
  // every moving primitive (text, panels, glows, the dots of a cloud) notes where it is on the output, and the largest distance
  // any of them travelled in the shutter, in output pixels, decides how many sub-frames the frame gets. Nothing is noted otherwise.
  var TR = null, quiet = 0;
  function track(ctx, x, y) {
    if (!TR || quiet) return;
    var m = ctx.getTransform();
    TR.p.push(m.a * x + m.c * y + m.e, m.b * x + m.d * y + m.f);
  }
  function setFont(ctx, o) {
    var size = o.size || 40;
    ctx.font = (o.weight || 600) + ' ' + size + 'px ' + FAM[o.font || 't'];
    ctx.letterSpacing = ((o.ls || 0) * size).toFixed(2) + 'px';
    ctx.textBaseline = 'alphabetic';
    ctx.fontKerning = 'normal';
    ctx.textRendering = 'geometricPrecision';
  }
  var widthCache = {};
  function measure(ctx, s, o) {
    setFont(ctx, o);
    var key = ctx.font + '|' + ctx.letterSpacing + '|' + s;
    return widthCache[key] === undefined ? (widthCache[key] = ctx.measureText(s).width) : widthCache[key];
  }
  // One line of text. o: size weight font(d t m) ls(em) color a align(l c r) shadow(0..1 dark halo) glow(0..1)
  function text(ctx, s, x, y, o) {
    o = o || {};
    var a = o.a === undefined ? 1 : o.a;
    if (TR && !quiet) track(ctx, x, y);                 // noted before any early return, so both probes make the same list
    if (a <= 0.003 || s === '') return 0;
    setFont(ctx, o);
    var w = measure(ctx, s, o), ls = (o.ls || 0) * (o.size || 40);
    var x0 = o.align === 'c' ? x - (w - ls) / 2 : o.align === 'r' ? x - (w - ls) : x;
    ctx.fillStyle = rgba(o.color || INK, a);
    if (o.shadow) {
      // a soft shadow under the type where it crosses lights and dots (a hard outline reads as a sticker edge)
      ctx.save();
      ctx.shadowColor = 'rgba(10,10,12,' + (0.6 * o.shadow * a).toFixed(3) + ')'; ctx.shadowBlur = (o.size || 40) * 0.4 * DPR; ctx.shadowOffsetY = (o.size || 40) * 0.05 * DPR;
      ctx.fillText(s, x0, y);
      ctx.restore();
    }
    ctx.fillText(s, x0, y);
    if (logging) {
      var m = ctx.getTransform();
      drawn.push({ text: s, size: o.size || 40, x: x0, y: y, a: a, w: w, sx: (m.a * x0 + m.c * y + m.e) / DPR, sr: (m.a * (x0 + w) + m.c * y + m.e) / DPR, sy: (m.b * x0 + m.d * y + m.f) / DPR, k: m.a / DPR });
    }
    return w;
  }
  // Text that rises out of a mask: p from 0 to 1 (the mask is the line's own box).
  function riseText(ctx, s, x, y, p, o) {
    if (TR && !quiet) track(ctx, x, y + (1 - Math.max(0, Math.min(1, p))) * (o.size || 40) * 1.12);
    if (p <= 0) return 0;
    var size = o.size || 40, w = measure(ctx, s, o);
    var x0 = o.align === 'c' ? x - w / 2 : o.align === 'r' ? x - w : x;
    ctx.save(); ctx.beginPath(); ctx.rect(x0 - size * 0.3, y - size * 0.98, w + size * 0.6, size * 1.3); ctx.clip();
    var o2 = Object.assign({}, o, { align: 'l' });
    quiet++; text(ctx, s, x0, y + (1 - p) * size * 1.12, o2); quiet--;
    ctx.restore();
    return w;
  }
  // The uneven word gaps of the motion rules, in seconds.
  var GAPS = [0, 0.1, 0.067, 0.067, 0.067, 0.067, 0.1, 0.067];
  function wordsText(ctx, s, x, y, t, t0, o, dur) {
    var ws = s.split(' '), space = measure(ctx, ' ', o), tot = 0, i, acc = 0, cx;
    var widths = ws.map(function (w) { return measure(ctx, w, o); });
    widths.forEach(function (w) { tot += w; }); tot += space * (ws.length - 1);
    cx = o.align === 'c' ? x - tot / 2 : o.align === 'r' ? x - tot : x;
    for (i = 0; i < ws.length; i++) {
      acc += GAPS[Math.min(i, GAPS.length - 1)] || 0.067;
      var p = ease.outQ(seg(t, t0 + acc, t0 + acc + (dur || 0.5)));
      riseText(ctx, ws[i], cx, y, p, Object.assign({}, o, { align: 'l' }));
      cx += widths[i] + space;
    }
    return tot;
  }
  // Numerals in fixed cells so a counting number does not shiver sideways.
  function numText(ctx, s, x, y, o) {
    o = o || {};
    setFont(ctx, o);
    var cell = measure(ctx, '0', o), ls = (o.ls || 0) * (o.size || 40), tot = 0, i, ch, ws = [];
    for (i = 0; i < s.length; i++) { ch = s.charAt(i); ws.push(/\d/.test(ch) ? cell : measure(ctx, ch, o)); tot += ws[i]; }
    var cx = o.align === 'c' ? x - (tot - ls) / 2 : o.align === 'r' ? x - (tot - ls) : x, left = cx;
    var a = o.a === undefined ? 1 : o.a;
    if (a <= 0.003) return tot;
    for (i = 0; i < s.length; i++) {
      ch = s.charAt(i);
      var w1 = measure(ctx, ch, o), off = /\d/.test(ch) ? (ws[i] - w1) / 2 : 0;
      ctx.fillStyle = rgba(o.color || INK, a);
      ctx.fillText(ch, cx + off, y); cx += ws[i];
    }
    if (logging) {
      var m = ctx.getTransform();
      // counting: the figure this numeral is counting towards; a value short of it is a number in motion, and the number check lets it pass
      drawn.push({ text: s, size: o.size || 40, x: left, y: y, a: a, w: tot, counting: o.counting, sx: (m.a * left + m.c * y + m.e) / DPR, sr: (m.a * (left + tot) + m.c * y + m.e) / DPR, sy: (m.b * x + m.d * y + m.f) / DPR, k: m.a / DPR });
    }
    return tot;
  }

  // ------------------------------------------------------------------ shapes
  function rr(ctx, x, y, w, h, r) {
    r = Math.min(r, w / 2, h / 2);
    ctx.beginPath(); ctx.moveTo(x + r, y); ctx.arcTo(x + w, y, x + w, y + h, r); ctx.arcTo(x + w, y + h, x, y + h, r);
    ctx.arcTo(x, y + h, x, y, r); ctx.arcTo(x, y, x + w, y, r); ctx.closePath();
  }
  // A pane of dark glass: a faint lift, a 1 px edge that is brighter at the top, a soft shadow and a sheen that
  // slides across it. o: r a(opacity) sheen(0..1) t(time for the sheen) edge(color) glow(color, 0..1 strength)
  function panel(ctx, x, y, w, h, o) {
    o = o || {};
    var r = o.r === undefined ? R_L : o.r, a = o.a === undefined ? 1 : o.a;
    if (TR && !quiet) { track(ctx, x, y); track(ctx, x + w, y + h); }
    if (a <= 0.003) return;
    ctx.save();
    ctx.globalAlpha = a;
    var sh;
    for (sh = 14; sh >= 1; sh--) { rr(ctx, x - sh * 3.2, y - sh * 2.2 + 22, w + sh * 6.4, h + sh * 4.4, r + sh * 3); ctx.fillStyle = 'rgba(0,0,0,' + (0.034 * (1 - sh / 16)).toFixed(4) + ')'; ctx.fill(); }
    rr(ctx, x, y, w, h, r); ctx.fillStyle = 'rgba(15,15,18,' + (o.fill === undefined ? 0.9 : o.fill) + ')'; ctx.fill();
    var g = ctx.createLinearGradient(0, y, 0, y + h);
    g.addColorStop(0, 'rgba(255,255,255,0.075)'); g.addColorStop(0.5, 'rgba(255,255,255,0.025)'); g.addColorStop(1, 'rgba(255,255,255,0.04)');
    rr(ctx, x, y, w, h, r); ctx.fillStyle = g; ctx.fill();
    if (o.glow) {
      var gg = ctx.createRadialGradient(x + w * 0.5, y + h, 0, x + w * 0.5, y + h, Math.max(w, h) * 0.75);
      gg.addColorStop(0, rgba(o.glow, 0.16 * (o.glowK === undefined ? 1 : o.glowK))); gg.addColorStop(1, rgba(o.glow, 0));
      rr(ctx, x, y, w, h, r); ctx.fillStyle = gg; ctx.fill();
    }
    if (o.sheen) {
      ctx.save(); rr(ctx, x, y, w, h, r); ctx.clip();
      var k = ((o.t || 0) * 0.11) % 1.6 - 0.3, sx = x + w * k, sg = ctx.createLinearGradient(sx - w * 0.18, y, sx + w * 0.18, y + h * 0.6);
      sg.addColorStop(0, 'rgba(255,255,255,0)'); sg.addColorStop(0.5, 'rgba(255,255,255,' + (0.055 * o.sheen).toFixed(3) + ')'); sg.addColorStop(1, 'rgba(255,255,255,0)');
      ctx.fillStyle = sg; ctx.fillRect(x, y, w, h); ctx.restore();
    }
    var eg = ctx.createLinearGradient(0, y, 0, y + h);
    eg.addColorStop(0, o.edge ? rgba(o.edge, 0.7) : 'rgba(255,255,255,0.26)'); eg.addColorStop(0.35, o.edge ? rgba(o.edge, 0.3) : 'rgba(255,255,255,0.1)'); eg.addColorStop(1, o.edge ? rgba(o.edge, 0.2) : 'rgba(255,255,255,0.07)');
    rr(ctx, x + 0.5, y + 0.5, w - 1, h - 1, r); ctx.lineWidth = 1; ctx.strokeStyle = eg; ctx.stroke();
    ctx.restore();
  }
  // A desktop terminal window: title bar with the window controls on the right, lines typed by time.
  // lines: [{at, text, kind: 'cmd' | 'out' | 'dim' | 'acc' | 'hot', cps}] with absolute times; a line shows once at has passed.
  function terminal(ctx, x, y, w, h, t, spec) {
    var size = spec.size || 26, lh = Math.round(size * 1.52), top = y + 62, pad = 30, i, ln, n, shown = [], cmdW = measure(ctx, '> ', { size: size, font: 'm', weight: 500 });
    panel(ctx, x, y, w, h, { a: spec.a, sheen: spec.sheen === undefined ? 1 : spec.sheen, t: t, glow: spec.glow, glowK: spec.glowK, edge: spec.edge });
    if ((spec.a === undefined ? 1 : spec.a) <= 0.01) return;
    quiet++;                                    // the lines of a terminal scroll in whole lines: not a motion to blur
    ctx.save(); ctx.globalAlpha = spec.a === undefined ? 1 : spec.a;
    rr(ctx, x, y, w, h, R_L); ctx.clip();
    ctx.fillStyle = 'rgba(255,255,255,0.035)'; ctx.fillRect(x, y, w, 46);
    ctx.fillStyle = 'rgba(255,255,255,0.08)'; ctx.fillRect(x, y + 46, w, 1);
    text(ctx, spec.title || 'Terminal', x + 22, y + (spec.titleSize ? 34 : 31), { size: spec.titleSize || 19, weight: 600, color: spec.titleSize ? INK2 : INK3, font: 't' });
    ctx.strokeStyle = 'rgba(255,255,255,0.4)'; ctx.lineWidth = 1.4;
    var cx0 = x + w - 38; ctx.beginPath(); ctx.moveTo(cx0 - 9, y + 17); ctx.lineTo(cx0 + 9, y + 35); ctx.moveTo(cx0 + 9, y + 17); ctx.lineTo(cx0 - 9, y + 35); ctx.stroke();
    ctx.strokeRect(cx0 - 52, y + 18, 15, 15); ctx.beginPath(); ctx.moveTo(cx0 - 95, y + 26); ctx.lineTo(cx0 - 80, y + 26); ctx.stroke();
    for (i = 0; i < spec.lines.length; i++) if (t >= spec.lines[i].at) shown.push(spec.lines[i]);
    var pb = spec.padBottom || 0, cap = Math.floor((h - 62 - 22 - pb) / lh), first = Math.max(0, shown.length - cap), cursor = null;
    ctx.beginPath(); ctx.rect(x, y + 48, w, h - 48 - pb); ctx.clip();
    for (i = first; i < shown.length; i++) {
      ln = shown[i];
      var chars = ln.cps ? Math.floor((t - ln.at) * ln.cps) : 1e9, str = ln.text.slice(0, Math.max(0, chars));
      var yy = top + (i - first + 1) * lh - lh * 0.3;
      var col = ln.kind === 'acc' ? ACCENT : ln.kind === 'hot' ? HOT : ln.kind === 'dim' ? INK3 : ln.kind === 'cmd' || ln.kind === 'cont' ? INK : INK2;
      var ox = x + pad;
      if (ln.kind === 'cmd') { text(ctx, '>', ox, yy, { size: size, font: 'm', weight: 600, color: ACCENT }); ox += cmdW; } else if (ln.kind === 'cont') ox += cmdW;
      text(ctx, str, ox, yy, { size: size, font: 'm', weight: ln.kind === 'cmd' || ln.kind === 'cont' ? 600 : 500, color: col, a: ln.a === undefined ? 1 : ln.a });
      if (i === shown.length - 1) cursor = { x: ox + measure(ctx, str, { size: size, font: 'm', weight: 500 }), y: yy, typing: chars < ln.text.length };
    }
    if (cursor && spec.cursor !== false && (cursor.typing || Math.floor(t / 0.53) % 2 === 0)) {
      ctx.fillStyle = rgba(ACCENT, 0.95); ctx.fillRect(cursor.x + 3, cursor.y - size * 0.82, size * 0.55, size * 1.05);
    }
    ctx.restore();
    quiet--;
  }

  // A soft round light, drawn once and stamped. Tinted per colour; hard = how much of the disc is solid.
  var sprites = {};
  function glow(color, hard) {
    hard = hard || 0;
    var key = color.join(',') + '|' + hard;
    if (sprites[key]) return sprites[key];
    var S = Math.round(128 * Math.max(1, DPR)), c = document.createElement('canvas');
    c.width = c.height = S;
    var g = c.getContext('2d'), grad = g.createRadialGradient(S / 2, S / 2, 0, S / 2, S / 2, S / 2);
    grad.addColorStop(0, rgba(color, 1)); grad.addColorStop(0.08 + hard * 0.4, rgba(color, 0.78)); grad.addColorStop(0.3 + hard * 0.3, rgba(color, 0.22));
    grad.addColorStop(0.62, rgba(color, 0.05)); grad.addColorStop(1, rgba(color, 0));
    g.fillStyle = grad; g.fillRect(0, 0, S, S);
    return (sprites[key] = c);
  }
  function stamp(ctx, color, x, y, r, a, hard) {
    if (TR && !quiet) track(ctx, x, y);
    if (a <= 0.003) return;
    ctx.globalAlpha = a > 1 ? 1 : a;
    ctx.drawImage(glow(color, hard), x - r, y - r, r * 2, r * 2);
    ctx.globalAlpha = 1;
  }
  // The ground: flat near-black with a faint pool of light that drifts. The vignette is done in the shader, in float.
  function background(ctx, t, shiftX, shiftY, pool) {
    ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
    ctx.globalCompositeOperation = 'source-over'; ctx.globalAlpha = 1;
    var cx = W * 0.4 + (shiftX || 0) + Math.sin(t * 0.37) * 60, cy = H * 0.42 + (shiftY || 0) + Math.cos(t * 0.29) * 40;
    var g = ctx.createRadialGradient(cx, cy, 0, cx, cy, 1300), k = pool === undefined ? 1 : pool;
    function lift(a) { return 'rgb(' + (10 + 245 * a * k).toFixed(2) + ',' + (10 + 245 * a * k).toFixed(2) + ',' + (12 + 243 * a * k).toFixed(2) + ')'; }
    g.addColorStop(0, lift(0.062)); g.addColorStop(0.5, lift(0.02)); g.addColorStop(1, 'rgb(10,10,12)');
    ctx.fillStyle = g; ctx.fillRect(0, 0, W, H);
  }
  // A virtual camera: a layer with parallax p (0 stays on the glass, 1 moves with the world).
  function layer(ctx, cam, p) {
    var s = 1 + (cam.zoom - 1) * p;
    ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
    ctx.translate(W / 2, H / 2); ctx.rotate((cam.rot || 0) * p); ctx.scale(s, s); ctx.translate(-(W / 2 + cam.x * p), -(H / 2 + cam.y * p));
  }
  function hair(ctx, a) { ctx.lineWidth = 1; ctx.strokeStyle = 'rgba(255,255,255,' + (a === undefined ? 0.14 : a) + ')'; }

  // The mark: a P with the token it hands over (the site's logo path). p from 0 to 1.
  var MARK_STEM = new Path2D('M7 4v16'), MARK_BOWL = new Path2D('M7 4h6.5a4.25 4.25 0 0 1 0 8.5H7');
  function markDraw(ctx, x, y, px, p, color, a) {
    if (p <= 0) return;
    var s = px / 24, k1 = ease.outC(seg(p, 0, 0.35)), k2 = ease.outC(seg(p, 0.3, 0.8)), k3 = seg(p, 0.72, 1);
    ctx.save(); ctx.translate(x, y); ctx.scale(s, s);
    ctx.lineCap = 'round'; ctx.lineJoin = 'round'; ctx.lineWidth = 3.2; ctx.strokeStyle = rgba(color || ACCENT, a === undefined ? 1 : a);
    // dashed strokes with a normalised path length are not available on Path2D, so the draw-on is a dash offset in real length
    ctx.setLineDash([16 * k1 + 0.01, 100]); ctx.stroke(MARK_STEM);
    ctx.setLineDash([26 * k2 + 0.01, 100]); ctx.stroke(MARK_BOWL);
    ctx.setLineDash([]);
    if (k3 > 0) { ctx.fillStyle = rgba(color || ACCENT, a === undefined ? 1 : a); ctx.beginPath(); ctx.arc(18.5, 18.6, 1.9 * ease.spring(k3), 0, Math.PI * 2); ctx.fill(); }
    ctx.restore();
  }

  // Number formats, the site's (docs/assets/app.js).
  function fmtTokens(v) { return !(v > 0) ? '0' : v >= 1e6 ? (v / 1e6).toFixed(2) + 'M' : Math.round(v / 1e3) + 'k'; }
  function fmtTurns(v) { return v.toFixed(1); }

  // ------------------------------------------------------------------ the engine: sub-frames, bloom, output
  var VS = '#version 300 es\nout vec2 v; void main(){ vec2 p = vec2(float((gl_VertexID<<1)&2), float(gl_VertexID&2)); v = p; gl_Position = vec4(p*2.-1., 0., 1.); }';
  var FS = {
    acc: '#version 300 es\nprecision highp float; uniform sampler2D t; uniform float w; in vec2 v; out vec4 o; void main(){ o = vec4(texture(t, vec2(v.x, 1.-v.y)).rgb*w, 1.); }',
    pre: '#version 300 es\nprecision highp float; uniform sampler2D t; uniform vec2 px; uniform float thr, knee, first; in vec2 v; out vec4 o;\n' +
      'vec3 s(vec2 d){ return texture(t, v + px*d).rgb; }\n' +
      'void main(){ vec3 r = s(vec2(0))*0.125 + (s(vec2(-2,2))+s(vec2(2,2))+s(vec2(-2,-2))+s(vec2(2,-2)))*0.03125 + (s(vec2(0,2))+s(vec2(-2,0))+s(vec2(2,0))+s(vec2(0,-2)))*0.0625 + (s(vec2(-1,1))+s(vec2(1,1))+s(vec2(-1,-1))+s(vec2(1,-1)))*0.125;\n' +
      ' if (first > 0.5) { float br = max(r.r, max(r.g, r.b)); float rq = clamp(br - thr + knee, 0., 2.*knee); rq = rq*rq/(4.*knee+1e-4); r *= max(rq, br - thr)/max(br, 1e-4); }\n o = vec4(r, 1.); }',
    up: '#version 300 es\nprecision highp float; uniform sampler2D base, low; uniform vec2 px; uniform float k; in vec2 v; out vec4 o;\n' +
      'vec3 L(vec2 d){ return texture(low, v + px*d).rgb; }\n' +
      'void main(){ vec3 t = (L(vec2(-1,-1)) + L(vec2(0,-1))*2. + L(vec2(1,-1)) + L(vec2(-1,0))*2. + L(vec2(0,0))*4. + L(vec2(1,0))*2. + L(vec2(-1,1)) + L(vec2(0,1))*2. + L(vec2(1,1)))/16.;\n o = vec4(texture(base, v).rgb + t*k, 1.); }',
    comp: '#version 300 es\nprecision highp float; uniform sampler2D acc, glow; uniform float bloom, fade, vig, expo; in vec2 v; out vec4 o;\n' +
      'void main(){ vec3 c = texture(acc, v).rgb + texture(glow, v).rgb*bloom; c *= expo;\n' +
      ' vec2 q = (v - 0.5)*vec2(1.7778, 1.); float r = length(q); c *= 1. - vig*smoothstep(0.45, 1.05, r);\n' +
      ' c = mix(c, vec3(10./255., 10./255., 12./255.), fade); o = vec4(c, 1.); }'
  };
  var X = null, PROF = null, SEC = null, secLast = 0;
  // section timer for tools: mark(ctx, 'name') adds the time since the last mark (the canvas is synced first)
  function mark(ctx, name) { if (!SEC) return; ctx.getImageData(0, 0, 1, 1); var n = performance.now(); SEC[name] = (SEC[name] || 0) + n - secLast; secLast = n; }
  function engineInit() {
    // The scene is an OffscreenCanvas: handing its picture to WebGL as a bitmap moves it without a copy, where sharing a
    // canvas costs a full copy of the 4K picture every time it is drawn on again (about 60 ms a frame).
    var out = document.getElementById('out'), USE_OFF = location.hash.indexOf('c') < 0, S;
    if (USE_OFF) S = new OffscreenCanvas(Math.round(W * DPR), Math.round(H * DPR)); else { S = document.createElement('canvas'); S.width = Math.round(W * DPR); S.height = Math.round(H * DPR); }
    var ctx = S.getContext('2d', { alpha: false });
    out.width = S.width; out.height = S.height; out.style.width = W + 'px'; out.style.height = H + 'px';
    var gl = out.getContext('webgl2', { alpha: false, antialias: false, depth: false, stencil: false, preserveDrawingBuffer: true, powerPreference: 'high-performance' });
    if (!gl) throw new Error('WebGL2 is not available');
    var cbf = gl.getExtension('EXT_color_buffer_float') || gl.getExtension('EXT_color_buffer_half_float');
    var fmt = cbf ? { i: gl.RGBA16F, t: gl.HALF_FLOAT } : { i: gl.RGBA8, t: gl.UNSIGNED_BYTE };
    function prog(fs) {
      var p = gl.createProgram();
      [[gl.VERTEX_SHADER, VS], [gl.FRAGMENT_SHADER, fs]].forEach(function (a) {
        var s = gl.createShader(a[0]); gl.shaderSource(s, a[1]); gl.compileShader(s);
        if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error('shader: ' + gl.getShaderInfoLog(s));
        gl.attachShader(p, s);
      });
      gl.linkProgram(p);
      if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error('link: ' + gl.getProgramInfoLog(p));
      var u = {}, n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS), i;
      for (i = 0; i < n; i++) { var nm = gl.getActiveUniform(p, i).name; u[nm] = gl.getUniformLocation(p, nm); }
      return { p: p, u: u };
    }
    var P = { acc: prog(FS.acc), pre: prog(FS.pre), up: prog(FS.up), comp: prog(FS.comp) };
    function target(w, h, type) {
      var tex = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, tex);
      gl.texImage2D(gl.TEXTURE_2D, 0, type === 'u8' ? gl.RGBA8 : fmt.i, w, h, 0, gl.RGBA, type === 'u8' ? gl.UNSIGNED_BYTE : fmt.t, null);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      var fb = gl.createFramebuffer(); gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
      return { tex: tex, fb: fb, w: w, h: h };
    }
    var sceneTex = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, sceneTex);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    var acc = target(S.width, S.height);
    // the bloom chain: halves from the picture down to 30 px wide; the levels wider than 960 only feed the next one
    var down = [], up = [], w = S.width, h = S.height;
    while (w > 30) { w = Math.max(1, Math.round(w / 2)); h = Math.max(1, Math.round(h / 2)); down.push(target(w, h)); if (w <= 960) up.push(target(w, h)); }
    var lowest = down.length - 1, firstUp = down.length - up.length;   // down[firstUp + j] pairs with up[j]
    var vao = gl.createVertexArray(); gl.bindVertexArray(vao);
    function pass(pr, fb, w, h, binds, f) {
      gl.bindFramebuffer(gl.FRAMEBUFFER, fb); gl.viewport(0, 0, w, h); gl.useProgram(pr.p);
      binds.forEach(function (b, i) { gl.activeTexture(gl.TEXTURE0 + i); gl.bindTexture(gl.TEXTURE_2D, b[1]); gl.uniform1i(pr.u[b[0]], i); });
      if (f) f(pr.u);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
    }
    X = {
      S: S, ctx: ctx, gl: gl, floatTargets: !!cbf,
      // draw n sub-frames spread over the shutter around t, average them, add the bloom, show.
      render: function (t, n, draw, post) {
        var k;
        gl.disable(gl.BLEND);
        gl.bindFramebuffer(gl.FRAMEBUFFER, acc.fb); gl.viewport(0, 0, acc.w, acc.h); gl.clearColor(0, 0, 0, 1); gl.clear(gl.COLOR_BUFFER_BIT);
        gl.enable(gl.BLEND); gl.blendFunc(gl.ONE, gl.ONE);
        for (k = 0; k < n; k++) {
          var ts = n === 1 ? t : t - SHUTTER / 2 + SHUTTER * (k + 0.5) / n;
          logging = (k === Math.floor(n / 2)); if (logging) drawn.length = 0;
          var tp0 = PROF ? performance.now() : 0; secLast = tp0;
          draw(ctx, ts, { k: k, n: n, t: t });
          if (PROF) { ctx.getImageData(0, 0, 1, 1); PROF.draw += performance.now() - tp0; tp0 = performance.now(); }
          gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, sceneTex);
          gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false); gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
          if (USE_OFF) { var bm = S.transferToImageBitmap(); gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, bm); bm.close(); }
          else gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, S);
          if (PROF) { gl.finish(); PROF.upload += performance.now() - tp0; tp0 = performance.now(); }
          pass(P.acc, acc.fb, acc.w, acc.h, [['t', sceneTex]], function (u) { gl.uniform1f(u.w, 1 / n); });
          if (PROF) { gl.finish(); PROF.acc += performance.now() - tp0; }
        }
        var tp1 = PROF ? performance.now() : 0;
        logging = false;
        gl.disable(gl.BLEND);
        var bloom = post.bloom === undefined ? 0.18 : post.bloom, i;
        if (bloom > 0) {
          for (i = 0; i < down.length; i++) {
            var src = i === 0 ? acc : down[i - 1];
            pass(P.pre, down[i].fb, down[i].w, down[i].h, [['t', src.tex]], function (u) { gl.uniform2f(u.px, 1 / src.w, 1 / src.h); gl.uniform1f(u.thr, post.thr === undefined ? 0.58 : post.thr); gl.uniform1f(u.knee, 0.22); gl.uniform1f(u.first, i === 0 ? 1 : 0); });
          }
          for (i = up.length - 1; i >= 0; i--) {
            var base = down[firstUp + i], low = i === up.length - 1 ? down[lowest] : up[i + 1];
            pass(P.up, up[i].fb, up[i].w, up[i].h, [['base', base.tex], ['low', low.tex]], function (u) { gl.uniform2f(u.px, 1 / low.w, 1 / low.h); gl.uniform1f(u.k, i === up.length - 1 ? 0.0 : 0.62); });
          }
        }
        pass(P.comp, null, acc.w, acc.h, [['acc', acc.tex], ['glow', bloom > 0 ? up[0].tex : acc.tex]], function (u) {
          gl.uniform1f(u.bloom, bloom > 0 ? bloom : 0); gl.uniform1f(u.fade, post.fade || 0); gl.uniform1f(u.vig, post.vig === undefined ? 0.32 : post.vig); gl.uniform1f(u.expo, post.expo === undefined ? 1 : post.expo);
        });
        gl.finish();
        if (PROF) PROF.post += performance.now() - tp1;
      }
    };
    return X;
  }

  // A film: the page of each film hands over its length, its draw function and what it needs for the shutter and the post pass.
  // film = { duration, fps (the timeline's grid), draw(ctx, t, sub) (sub.t is the time of the frame itself, the middle of the shutter: counters read it so that their digits do not smear), motion(t) -> sub-frame count, post(t) -> {bloom, fade, vig, thr}, poster }
  function film(f) {
    var eng = engineInit(), snaps = {};
    var PGR = global.PGR = {
      fps: f.fps, duration: f.duration, ready: false, shutter: SHUTTER, sceneTimes: f.sceneTimes || [],
      gpu: (function () { var e = eng.gl.getExtension('WEBGL_debug_renderer_info'); return e ? eng.gl.getParameter(e.UNMASKED_RENDERER_WEBGL) : eng.gl.getParameter(eng.gl.RENDERER); })(),
      floatTargets: eng.floatTargets,
      seek: function (t, o) {
        o = o || {};
        t = clamp(t, 0, f.duration);
        var n = o.n || PGR.subframes(t).n;
        eng.render(t, n, f.draw, f.post ? f.post(t) : {});
        return n;
      },
      // How many sub-frames the shutter of the frame at t needs: the scene is drawn at the two ends of the shutter with the motion
      // probe on, and the sub-frames are spaced about 1.5 output pixels apart (28 at most) along the fastest thing on screen (camera, type rising,
      // bars growing, a dot in flight), so no moving edge shows as separate copies. A still frame gets one. {n, d (output px)}
      subframes: function (t) {
        var A, B, i, dx, dy, q, d = 0, ca, cb;
        TR = { p: [], cloud: null }; f.draw(eng.ctx, t - SHUTTER / 2, { k: 0, n: 2, t: t }); A = TR;
        TR = { p: [], cloud: null }; f.draw(eng.ctx, t + SHUTTER / 2, { k: 1, n: 2, t: t }); B = TR;
        TR = null; quiet = 0;
        var m = Math.min(A.p.length, B.p.length);
        for (i = 0; i + 1 < m; i += 2) { dx = A.p[i] - B.p[i]; dy = A.p[i + 1] - B.p[i + 1]; q = dx * dx + dy * dy; if (q > d) d = q; }
        ca = A.cloud; cb = B.cloud;
        if (ca && cb) for (i = 0; i + 1 < ca.length; i += 2) {
          if (ca[i] !== ca[i] || cb[i] !== cb[i]) continue;
          dx = ca[i] - cb[i]; dy = ca[i + 1] - cb[i + 1]; q = dx * dx + dy * dy; if (q > d) d = q;
        }
        d = Math.sqrt(d);
        var n = d < 0.4 ? 1 : Math.max(2, Math.min(f.maxSubframes || 28, Math.ceil(d / 1.5)));
        return { n: n, d: d };
      },
      drawn: function () { return drawn.slice(); },
      // the mean and the standard deviation of a patch of the picture on screen (x, y from the top left, stage px)
      stats: function (x, y, w, h) {
        var gl = eng.gl, k = DPR, px = new Uint8Array(w * k * h * k * 4);
        gl.bindFramebuffer(gl.FRAMEBUFFER, null);
        gl.readPixels(Math.round(x * k), Math.round((H - y - h) * k), w * k, h * k, gl.RGBA, gl.UNSIGNED_BYTE, px);
        var n = w * k * h * k, sum = 0, sq = 0, i, v;
        for (i = 0; i < n; i++) { v = px[i * 4 + 1]; sum += v; sq += v * v; }
        var mean = sum / n;
        return { mean: mean, std: Math.sqrt(Math.max(0, sq / n - mean * mean)) };
      },
      // the roughness of a patch of the picture: for each pixel the difference from the mean of its four neighbours (green channel,
      // levels of 255). median is the median of the absolute differences, rms their root mean square. A flat patch or a slow gradient
      // gives a median of 0 (a hairline or an edge changes a few pixels only), grain changes every pixel and gives several levels.
      roughness: function (x, y, w, h) {
        var gl = eng.gl, k = DPR, pw = w * k, ph = h * k, px = new Uint8Array(pw * ph * 4), r = 0, n = 0, i, j, c, d, hist = new Uint32Array(1100), half, acc = 0, med = 0;
        gl.bindFramebuffer(gl.FRAMEBUFFER, null);
        gl.readPixels(Math.round(x * k), Math.round((H - y - h) * k), pw, ph, gl.RGBA, gl.UNSIGNED_BYTE, px);
        for (j = 1; j < ph - 1; j++) for (i = 1; i < pw - 1; i++) {
          c = (j * pw + i) * 4 + 1;
          d = px[c] - (px[c - 4] + px[c + 4] + px[c - pw * 4] + px[c + pw * 4]) / 4;
          r += d * d; n++; hist[Math.min(1099, Math.round(Math.abs(d) * 4))]++;
        }
        half = n / 2;
        for (i = 0; i < hist.length; i++) { acc += hist[i]; if (acc >= half) { med = i / 4; break; } }
        return { rms: Math.sqrt(r / n), median: med };
      },
      // an FNV-1a hash of every byte of the picture on screen, for tools/check-fps.mjs
      hash: function () {
        var gl = eng.gl, px = new Uint8Array(gl.drawingBufferWidth * gl.drawingBufferHeight * 4), i, h = 2166136261;
        gl.bindFramebuffer(gl.FRAMEBUFFER, null);
        gl.readPixels(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight, gl.RGBA, gl.UNSIGNED_BYTE, px);
        for (i = 0; i < px.length; i++) { h ^= px[i]; h = Math.imul(h, 16777619) >>> 0; }
        return h.toString(16);
      },
      // keep the picture on screen, and later compare another picture with it (for tools/check-fps.mjs): the largest
      // difference of any colour channel, how many channels differ by more than 2 and by more than 16 levels, the mean difference
      snap: function (key) {
        var gl = eng.gl, px = snaps[key] = new Uint8Array(gl.drawingBufferWidth * gl.drawingBufferHeight * 4);
        gl.bindFramebuffer(gl.FRAMEBUFFER, null); gl.readPixels(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight, gl.RGBA, gl.UNSIGNED_BYTE, px);
        return px.length;
      },
      diff: function (key) {
        var gl = eng.gl, ref = snaps[key], px = new Uint8Array(ref.length), i, d, mx = 0, n2 = 0, n16 = 0, sum = 0, gw = gl.drawingBufferWidth, gh = gl.drawingBufferHeight, x0 = gw, y0 = gh, x1 = -1, y1 = -1, q;
        gl.bindFramebuffer(gl.FRAMEBUFFER, null); gl.readPixels(0, 0, gw, gh, gl.RGBA, gl.UNSIGNED_BYTE, px);
        for (i = 0; i < px.length; i++) {
          if (i % 4 === 3) continue; d = Math.abs(px[i] - ref[i]); sum += d; if (d > mx) mx = d; if (d > 2) n2++;
          if (d > 16) { n16++; q = i >> 2; if (q % gw < x0) x0 = q % gw; if (q % gw > x1) x1 = q % gw; q = gh - 1 - Math.floor(q / gw); if (q < y0) y0 = q; if (q > y1) y1 = q; }
        }
        return { max: mx, over2: n2, over16: n16, mean: sum / (px.length * 0.75), box: n16 ? [x0, y0, x1, y1] : null };
      },
      forget: function () { snaps = {}; },
      profile: function (on) { PROF = on ? { draw: 0, upload: 0, acc: 0, post: 0 } : null; SEC = on ? {} : null; secLast = performance.now(); return PROF; },
      sections: function () { return SEC; },
      prof: function () { return PROF; },
      canPaintOnly: false
    };
    document.fonts.ready.then(function () { PGR.seek(0); PGR.ready = true; });
    return PGR;
  }

  global.PGRLIB = {
    W: W, H: H, DPR: DPR, SHUTTER: SHUTTER, clamp: clamp, mix: mix, seg: seg, ease: ease, rng: rng,
    ACCENT: ACCENT, HOT: HOT, INK: INK, INK2: INK2, INK3: INK3, CONTROL: CONTROL, BG: BG, rgba: rgba, lerpC: lerpC, R_S: R_S, R_M: R_M, R_L: R_L,
    FAM: FAM, text: text, riseText: riseText, wordsText: wordsText, numText: numText, measure: measure, setFont: setFont,
    rr: rr, panel: panel, terminal: terminal, glow: glow, stamp: stamp, background: background, layer: layer, hair: hair,
    markDraw: markDraw, mark: mark, fmtTokens: fmtTokens, fmtTurns: fmtTurns, film: film,
    track: track, probe: function () { return TR && !quiet ? TR : null; }
  };
})(window);
