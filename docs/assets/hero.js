/* The Overview stage. Two streams of particles drawn with WebGL2: the tokens one task takes with no index, and the
   tokens it takes with the best configuration. One particle stands for a fixed number of tokens, so the count of
   particles and the thickness of each stream are the measured numbers from window.SHOWCASE. Scrolling plays the
   film: the small stream rises into the large one, the tokens the best configuration does not need are let go
   while the count falls with them, and what is left draws the mark.
   Without WebGL2, or where the stage has no room, the page keeps its lanes (home.js). With reduced motion the
   stage is one still frame. The numbers are also in the page as text. */
(function () {
  'use strict';
  var App = window.App, data = window.SHOWCASE, root = document.documentElement;
  if (!App) return;
  var each = function (list, fn) { Array.prototype.forEach.call(list, fn); };
  var clamp = App.clamp, mix = function (a, b, t) { return a + (b - a) * t; };
  var smooth = function (t) { t = clamp(t); return t * t * (3 - 2 * t); };

  /* ---------- the stacks of the first chapter: the total above a stack counts the turns that have landed ---------- */
  // The same starts and spans as the stylesheet gives the blocks (.stack, .stack.best and the fade of .slab).
  (function () {
    var chapter = document.querySelector('.ch-problem');
    if (!chapter || !App.film) return;
    var FADE = 0.07, stacks = [];
    each(chapter.querySelectorAll('.stack'), function (el) {
      var total = el.querySelector('[data-od]'), to = total ? parseFloat(total.getAttribute('data-to')) : NaN, best = el.classList.contains('best');
      var od = to > 0 ? App.od(total, 'tokens', to) : null;
      if (!od) return;
      var parts = Array.prototype.map.call(el.querySelectorAll('.slab'), function (s) { return parseFloat(s.style.getPropertyValue('--h')) || 0; });
      var sum = parts.reduce(function (a, b) { return a + b; }, 0) || 1;
      stacks.push({ od: od, to: to, parts: parts, sum: sum, start: best ? 0.66 : 0.06, gap: (best ? 0.2 : 0.5) / parts.length });
    });
    App.pin(chapter, function (p) {
      stacks.forEach(function (s) {
        var landed = 0;
        s.parts.forEach(function (h, i) { landed += clamp((p - s.start - i * s.gap) / FADE) * h; });
        var share = landed / s.sum;
        s.od.set(s.to * share, share >= 1);
      });
    });
  })();

  /* ---------- the particle stage ---------- */
  var hero = document.getElementById('hero'), stage = hero ? hero.querySelector('.hero-stage') : null;
  var read = hero ? hero.querySelector('.hero-read') : null, found = data ? App.headline(data) : null;
  function plain() { root.classList.remove('gl-on', 'gl-live'); App.gl = false; }
  if (!stage || !read || !found || !found.c || !window.WebGL2RenderingContext) { plain(); return; }
  var canvas = document.createElement('canvas'), gl = null;
  try { gl = canvas.getContext('webgl2', { alpha: true, antialias: false, premultipliedAlpha: true, powerPreference: 'high-performance' }); } catch (err) { gl = null; }
  if (!gl) { plain(); return; }
  root.classList.add('gl-on');

  var REF = found.c.tokens, BEST = Math.min(found.b.tokens, REF), KEEP = BEST / REF;
  // One particle is `per` tokens. The finest grain keeps the large stream under sixty thousand particles; a
  // slower machine moves to a coarser grain, and the line under the streams always says which one is drawn.
  var FINEST = [10, 20, 50, 100, 200, 500, 1000, 2000, 5000].filter(function (n) { return REF / n <= 60000; })[0] || 10000;
  var GRAINS = [1, 2, 4, 8, 16], grain = App.fine ? 0 : 1, locked = false;
  var MOST = Math.max(1, Math.round(REF / FINEST)), MOST_BEST = Math.max(1, Math.round(BEST / FINEST));
  var perEl = read.querySelector('[data-per]'), numEl = read.querySelector('.hero-num'), countEl = read.querySelector('.hero-count');
  var tags = [read.querySelector('.tag-ref'), read.querySelector('.tag-best')], word = read.querySelector('.hero-word');
  var copy = stage.querySelector('.hero-copy'), tabbar = document.querySelector('.tabbar'), markEl = read.querySelector('.hero-mark');
  var od = numEl && App.film ? App.od(numEl, 'tokens', REF) : null;

  var VERT = [
    '#version 300 es',
    'precision highp float;',
    'layout(location = 0) in vec4 seed;',       // along the stream, across it, depth, rank
    'uniform vec2 uRes;',                        // the stage in CSS pixels
    'uniform float uTime, uDpr, uBody, uKeep, uSize;',
    'uniform vec4 uBand;',                       // this stream: centre, half thickness, churn, speed
    'uniform vec4 uPhase;',                      // intro, merge, condense, mark
    'uniform vec3 uPtr;',                        // the pointer and how hard it pushes
    'uniform vec2 uCam;',                        // the drift of the camera
    'uniform vec3 uGlyph;',                      // the mark: centre and the pixels of one unit of its grid
    'out float vAlpha;',
    'out float vHeat;',
    'out float vRound;',
    'float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }',
    'float noise(vec2 p) {',
    '  vec2 i = floor(p), f = fract(p);',
    '  f = f * f * (3.0 - 2.0 * f);',
    '  return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), f.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), f.x), f.y) * 2.0 - 1.0;',
    '}',
    // The mark on its 24 unit grid: a stroke 42.35 units long (stem, top, bowl, return), then the dot.
    'vec2 glyph(float t, float v, float w) {',
    '  if (t > 0.9) { float r = 1.9 * sqrt(abs(v)); float g = w * 3.14159; return vec2(18.5, 18.6) + r * vec2(cos(g), sin(g)); }',
    '  float d = t / 0.9 * 42.35;',
    '  vec2 p; vec2 n;',
    '  if (d < 16.0) { p = vec2(7.0, 4.0 + d); n = vec2(1.0, 0.0); }',
    '  else if (d < 22.5) { p = vec2(7.0 + d - 16.0, 4.0); n = vec2(0.0, 1.0); }',
    '  else if (d < 35.85) { float g = (d - 22.5) / 4.25; n = vec2(sin(g), -cos(g)); p = vec2(13.5, 8.25) + 4.25 * n; }',
    '  else { p = vec2(13.5 - (d - 35.85), 12.5); n = vec2(0.0, 1.0); }',
    '  return p + n * v * 1.6;',
    '}',
    'void main() {',
    '  float u = seed.x, v = seed.y, w = seed.z, s = seed.w;',
    '  float intro = uPhase.x, merge = uPhase.y, cond = uPhase.z, mark = uPhase.w;',
    '  float r1 = fract(s * 91.7 + u * 13.3), r2 = fract(s * 37.1 + v * 7.7);',
    '  float lane = fract(u + uTime * uBand.w * (0.65 + 0.7 * r1) * (1.0 - 0.3 * w));',
    '  float x = (lane * 1.24 - 0.12) * uRes.x;',
    // The churn: slow billows that carry a whole strand together, and a little of each particle's own. With a
    // large churn the strands cross and tangle; with a small one they run side by side.
    '  vec2 q = vec2(lane * 4.2 - uTime * 0.1, v * 9.0 + w * 5.0);',
    '  float billow = noise(q + vec2(0.0, uTime * 0.17)) + 0.5 * noise(q * vec2(2.3, 1.0) - vec2(uTime * 0.29, 3.0));',
    '  float own = sin(uTime * (0.7 + r2) + s * 40.0);',
    '  float y = uBand.x + v * uBand.y + (billow * 0.94 + own * 0.06) * uBand.z;',
    '  x += noise(q.yx * 1.7 + 3.0) * uBand.z * 0.6;',
    '  float alpha = 1.0, heat = uBody, k = 0.0;',
    '  if (uBody < 0.5) {',
    // In the large stream, the share the best configuration also needs is handed over to the small stream as
    // it arrives; the rest is let go from the outside in, so the stream thins exactly as its count falls.
    '    if (s < uKeep) alpha *= 1.0 - merge;',
    '    else {',
    '      float shed = clamp((cond * 1.08 - (1.0 - abs(v))) / 0.07, 0.0, 1.0);',
    '      y += (v < 0.0 ? -1.0 : 1.0) * shed * shed * uRes.y * 0.2;',
    '      x += shed * (40.0 + 80.0 * r1);',
    '      alpha *= 1.0 - shed;',
    '      heat = shed * 0.9;',
    '    }',
    '  } else if (mark > 0.0) {',
    '    k = clamp(mark * 1.5 - r1 * 0.5, 0.0, 1.0);',
    '    k = k * k * (3.0 - 2.0 * k);',
    '    vec2 g = uGlyph.xy + (glyph(u, v, w) - vec2(12.0)) * uGlyph.z;',
    '    g += vec2(noise(vec2(s * 50.0, uTime * 0.6)), noise(vec2(uTime * 0.6, s * 50.0))) * uGlyph.z * 0.1;',
    '    x = mix(x, g.x, k);',
    '    y = mix(y, g.y, k);',
    '  }',
    // The pointer parts the stream.
    '  vec2 d = vec2(x, y) - uPtr.xy;',
    '  float push = uPtr.z * exp(-dot(d, d) / 15000.0) * (1.0 - 0.8 * k) * 84.0;',
    '  d /= length(d) + 1.0;',
    '  x += d.x * push;',
    '  y += d.y * push;',
    '  float z = w * (1.0 - k);',
    '  float persp = 1.0 / (1.0 + z * 0.3);',
    '  x = uRes.x * 0.5 + (x - uRes.x * 0.5) * persp + uCam.x * z * 60.0;',
    '  y += uCam.y * z * 34.0;',
    '  alpha *= clamp((intro * 1.4 - lane) * 5.0, 0.0, 1.0);',
    '  gl_Position = vec4(x / uRes.x * 2.0 - 1.0, 1.0 - y / uRes.y * 2.0, 0.0, 1.0);',
    // A few wide, faint particles are the glow of the mass. The rest are sharp: a short dash along the stream
    // while they flow, a point once they sit in the mark.
    '  float wide = step(0.965, r2);',
    '  vRound = max(k, wide);',
    '  gl_PointSize = max(1.0, mix(1.3, 2.7, r1) * persp * mix(mix(2.8, 1.25, k), 8.0, wide) * uSize * uDpr);',
    '  vAlpha = alpha * mix(1.0, 0.05, wide) * mix(1.0, 0.42, clamp(z, 0.0, 1.0)) * mix(0.62, 1.0, max(heat, k));',
    '  vHeat = heat;',
    '}'].join('\n');
  var FRAG = [
    '#version 300 es',
    'precision mediump float;',
    'in float vAlpha;',
    'in float vHeat;',
    'in float vRound;',
    'uniform vec3 uCold, uHot;',
    'out vec4 color;',
    'void main() {',
    '  vec2 c = gl_PointCoord * 2.0 - 1.0;',
    '  c.y *= mix(3.4, 1.0, vRound);',
    '  float a = 1.0 - dot(c, c);',
    '  if (a <= 0.0) discard;',
    '  a = a * a * vAlpha;',
    '  color = vec4(mix(uCold, uHot, vHeat) * a, a);',
    '}'].join('\n');

  var prog = null, uni = {}, vaos = [];
  function shader(type, source) {
    var s = gl.createShader(type);
    gl.shaderSource(s, source);
    gl.compileShader(s);
    return gl.getShaderParameter(s, gl.COMPILE_STATUS) ? s : null;
  }
  // A fixed sequence, so the picture is the same on every visit and a coarser grain is a prefix of a finer one.
  var state = 20240131;
  function rnd() { state = (state * 1664525 + 1013904223) >>> 0; return state / 4294967296; }
  // Two particles in three ride a strand: a fixed place across the stream and a depth of its own. The rest are
  // loose between the strands. Both streams have the same number of strands for the same thickness.
  function cloud(count, strands) {
    var a = new Float32Array(count * 4), depth = [], i, k;
    for (i = 0; i < strands; i += 1) depth.push(rnd() * 2 - 1);
    for (i = 0; i < count; i += 1) {
      a[i * 4] = rnd();
      if (rnd() < 0.66) {
        k = Math.floor(rnd() * strands);
        a[i * 4 + 1] = (k + 0.5) / strands * 2 - 1 + (rnd() - 0.5) * 0.012;
        a[i * 4 + 2] = depth[k] + (rnd() - 0.5) * 0.04;
      } else {
        a[i * 4 + 1] = rnd() * 2 - 1;
        a[i * 4 + 2] = rnd() * 2 - 1;
      }
      a[i * 4 + 3] = rnd();
    }
    return a;
  }
  var STRANDS = 38, clouds = [cloud(MOST, STRANDS), cloud(MOST_BEST, Math.max(3, Math.round(STRANDS * KEEP)))];
  function build() {
    var vs = shader(gl.VERTEX_SHADER, VERT), fs = shader(gl.FRAGMENT_SHADER, FRAG);
    if (!vs || !fs) return false;
    prog = gl.createProgram();
    gl.attachShader(prog, vs);
    gl.attachShader(prog, fs);
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) return false;
    gl.useProgram(prog);
    ['uRes', 'uTime', 'uDpr', 'uBody', 'uKeep', 'uSize', 'uBand', 'uPhase', 'uPtr', 'uCam', 'uGlyph', 'uCold', 'uHot'].forEach(function (name) {
      uni[name] = gl.getUniformLocation(prog, name);
    });
    vaos = clouds.map(function (points) {
      var vao = gl.createVertexArray(), buffer = gl.createBuffer();
      gl.bindVertexArray(vao);
      gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
      gl.bufferData(gl.ARRAY_BUFFER, points, gl.STATIC_DRAW);
      gl.enableVertexAttribArray(0);
      gl.vertexAttribPointer(0, 4, gl.FLOAT, false, 0, 0);
      return vao;
    });
    gl.disable(gl.DEPTH_TEST);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE);
    var rgb = (getComputedStyle(root).getPropertyValue('--accent-rgb') || '255,122,61').split(',').map(function (v) { return parseFloat(v) / 255; });
    gl.uniform3f(uni.uCold, 0.66, 0.67, 0.76);
    gl.uniform3f(uni.uHot, rgb[0], rgb[1], rgb[2]);
    gl.uniform1f(uni.uKeep, KEEP);
    return true;
  }
  if (!build()) { plain(); return; }

  /* ---------- where things are: under the title at first, then the large stream alone under the count ---------- */
  var W = 0, H = 0, dpr = 1, first = null, second = null, glyph = null, coarsest = 0;
  function layout() {
    W = stage.clientWidth; H = stage.clientHeight;
    if (!W || !H) return false;
    // Device pixels up to two for one, within a budget of pixels to fill: a large stage on a dense screen is
    // drawn a little under two for one, which integrated graphics can hold while the page scrolls.
    dpr = Math.min(window.devicePixelRatio || 1, 2, Math.max(1, Math.sqrt(3400000 / (W * H))));
    canvas.width = Math.round(W * dpr);
    canvas.height = Math.round(H * dpr);
    gl.viewport(0, 0, canvas.width, canvas.height);
    var floor = H - (tabbar && tabbar.offsetHeight ? tabbar.offsetHeight : 0);
    // A name sits clear of its stream: the large one spills past its nominal edge by about a sixth of its thickness.
    var name = tags[0] ? tags[0].offsetHeight : 20, below = tags[1] ? tags[1].offsetHeight : 20, key = read.querySelector('.hero-key');
    var top = copy.offsetTop + copy.offsetHeight + name + 26, room = floor - (key ? key.offsetHeight : 18) - 34 - top;
    var thick = Math.min(H * 0.25, (room - below - 30) / (1.32 + KEEP));
    if (thick < 48) return false;
    var spill = thick * 0.16, gap = spill + below + 26;
    // No more particles than the stream has room for: a narrow screen draws a coarser grain, and says so.
    coarsest = 0;
    while (coarsest < GRAINS.length - 1 && MOST / GRAINS[coarsest] > W * thick * 0.5) coarsest += 1;
    if (grain < coarsest) { grain = coarsest; label(); }
    top += spill + (room - (thick * (1 + KEEP) + spill + gap)) * 0.45;
    first = { y: top + thick / 2, t: thick, small: top + thick + gap + thick * KEEP / 2, spill: spill };
    // Under the count the large stream fills what is left of the stage; both streams keep one scale.
    var under = countEl && countEl.offsetHeight ? countEl.offsetTop + countEl.offsetHeight + 28 : H * 0.42;
    var deep = Math.max(thick, Math.min(H * 0.34, (floor - 56 - under) * 0.86));
    second = { y: (under + floor - 40) / 2, t: deep };
    var unit = Math.min((floor - 120) * 0.62, W * 0.7) / 20;
    glyph = { x: W / 2, y: (floor - 60) * 0.46 + 16, unit: unit };
    tags.forEach(function (tag, i) {
      if (!tag) return;
      var t = i ? thick * KEEP : thick;
      tag.style.setProperty('--y', ((i ? first.small : first.y - first.spill) - t / 2).toFixed(1) + 'px');
      tag.style.setProperty('--t', t.toFixed(1) + 'px');
      tag.style.setProperty('--s', (i ? 0 : first.spill).toFixed(1) + 'px');
    });
    if (word) word.style.setProperty('--y', (glyph.y + unit * 10.4 + 18).toFixed(1) + 'px');
    if (markEl) {
      markEl.style.setProperty('--x', glyph.x.toFixed(1) + 'px');
      markEl.style.setProperty('--y', glyph.y.toFixed(1) + 'px');
      markEl.style.setProperty('--u', unit.toFixed(3) + 'px');
    }
    return true;
  }

  /* ---------- one frame ---------- */
  var p = 0, shown = 0, time = 0, intro = 0, ptr = { x: -999, y: -999, tx: -999, ty: -999, f: 0, on: false, nx: 0, ny: 0 };
  function counts() { var per = GRAINS[grain]; return [Math.max(1, Math.round(MOST / per)), Math.max(1, Math.round(MOST_BEST / per))]; }
  function draw() {
    // The same marks on the scroll as the stylesheet uses for the words on the stage.
    var cam = smooth(shown / 0.2), merge = smooth((shown - 0.1) / 0.1), cond = clamp((shown - 0.22) / 0.42), mark = clamp((shown - 0.72) / 0.2);
    var n = counts(), y = mix(first.y, second.y, cam), half = mix(first.t, second.t, cam) / 2;
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.uniform2f(uni.uRes, W, H);
    gl.uniform1f(uni.uTime, time);
    gl.uniform1f(uni.uDpr, dpr);
    // Where a slower machine has fewer particles than the stream has room for, each is drawn a little larger.
    gl.uniform1f(uni.uSize, Math.max(1, Math.min(2.2, Math.sqrt(0.12 * W * first.t / n[0]))));
    gl.uniform4f(uni.uPhase, intro, merge, cond, mark);
    gl.uniform3f(uni.uPtr, ptr.x, ptr.y, ptr.f);
    gl.uniform2f(uni.uCam, Math.sin(time * 0.13) * 0.5 + ptr.nx * 0.7, Math.cos(time * 0.09) * 0.4 + ptr.ny * 0.6);
    gl.uniform3f(uni.uGlyph, glyph.x, glyph.y, glyph.unit);
    if (cond < 1) {
      gl.uniform1f(uni.uBody, 0);
      gl.uniform4f(uni.uBand, y, half, half * (0.38 - 0.2 * cond), 0.034);
      gl.bindVertexArray(vaos[0]);
      gl.drawArrays(gl.POINTS, 0, n[0]);
    }
    gl.uniform1f(uni.uBody, 1);
    gl.uniform4f(uni.uBand, mix(first.small, y, merge), half * KEEP, Math.max(1.5, half * KEEP * 0.12), 0.07);
    gl.bindVertexArray(vaos[1]);
    gl.drawArrays(gl.POINTS, 0, n[1]);
    if (od) od.set(REF - (REF - BEST) * cond, cond <= 0 || cond >= 1);
  }

  /* ---------- the loop: it stops when the stage is off screen or the tab is hidden, and it sizes itself at the start ---------- */
  var running = false, visible = true, last = 0, seen = 0, spent = 0, mean = 0;
  function label() {
    var per = FINEST * GRAINS[grain], n = counts();
    if (perEl) perEl.textContent = per >= 1000 ? per / 1000 + 'k' : String(per);
    canvas.setAttribute('data-per', per);
    canvas.setAttribute('data-particles', n[0] + n[1]);
  }
  // Thirty frames are timed after the first few. Slower than about 45 a second, the grain is doubled and timed again.
  function sample(ms) {
    if (locked) return;
    seen += 1;
    if (seen <= 10) return;
    spent += ms;
    if (seen < 40) return;
    mean = spent / 30;
    canvas.setAttribute('data-ms', mean.toFixed(1));
    seen = 0; spent = 0;
    if (mean > 22 && grain < GRAINS.length - 1) { grain += 1; label(); } else locked = true;
  }
  function tick(now) {
    if (!running) return;
    if (!visible || document.hidden) { running = false; return; }
    var dt = last ? Math.min(0.05, (now - last) / 1000) : 0.016;
    if (last) sample(now - last);
    last = now;
    time += dt;
    intro = Math.min(1, intro + dt / 1.8);
    // The film follows the scroll with a short lag, so a wheel step is a glide and not a jump.
    shown += (p - shown) * Math.min(1, dt * 9);
    if (Math.abs(p - shown) < 0.0004) shown = p;
    // Taken fast, released slowly: the stream closes behind the pointer like something thick.
    ptr.x += (ptr.tx - ptr.x) * Math.min(1, dt * 14);
    ptr.y += (ptr.ty - ptr.y) * Math.min(1, dt * 14);
    ptr.f += ((ptr.on ? 1 : 0) - ptr.f) * Math.min(1, dt * (ptr.on ? 8 : 1.6));
    draw();
    requestAnimationFrame(tick);
  }
  function start() { if (running || App.reduced) return; running = true; last = 0; requestAnimationFrame(tick); }
  function still() { time = 4.2; intro = 1; shown = p = 0; draw(); }

  canvas.className = 'hero-gl';
  canvas.setAttribute('aria-hidden', 'true');
  stage.insertBefore(canvas, stage.firstChild);
  root.classList.add('gl-live');
  if (!layout()) { stage.removeChild(canvas); plain(); return; }
  App.gl = true;
  label();
  if (App.reduced) still(); else draw();

  if (App.film) {
    App.barOwned = true;
    App.pin(hero, function (v) { p = v; App.bar((v - 0.1) / 0.04); if (!running) start(); });
  }
  if ('ResizeObserver' in window) {
    new ResizeObserver(function () { if (layout()) { if (App.reduced || !running) { if (App.reduced) still(); else draw(); } } }).observe(stage);
  }
  if ('IntersectionObserver' in window) {
    new IntersectionObserver(function (entries) {
      visible = entries[entries.length - 1].isIntersecting;
      if (visible) start();
    }).observe(stage);
  }
  document.addEventListener('visibilitychange', function () { if (!document.hidden && visible) start(); });
  stage.addEventListener('pointermove', function (e) {
    var box = stage.getBoundingClientRect();
    ptr.tx = e.clientX - box.left;
    ptr.ty = e.clientY - box.top;
    if (!ptr.on) { ptr.x = ptr.tx; ptr.y = ptr.ty; }
    ptr.on = true;
    ptr.nx = ptr.tx / box.width - 0.5;
    ptr.ny = ptr.ty / box.height - 0.5;
  }, { passive: true });
  ['pointerleave', 'pointerup', 'pointercancel'].forEach(function (type) {
    stage.addEventListener(type, function (e) { if (type === 'pointerleave' || e.pointerType !== 'mouse') ptr.on = false; });
  });
  canvas.addEventListener('webglcontextlost', function (e) { e.preventDefault(); running = false; });
  canvas.addEventListener('webglcontextrestored', function () { if (build() && layout()) { if (App.reduced) still(); else start(); } });

  // The grain can be set by hand, which also stops the stage from choosing it: the frame captures use this.
  App.hero = {
    grain: function (n) { grain = Math.max(coarsest, Math.min(GRAINS.length - 1, n)); locked = true; label(); if (!running) { if (App.reduced) still(); else draw(); } },
    state: function () { var n = counts(); return { tokensPerParticle: FINEST * GRAINS[grain], particles: n[0] + n[1], frameMs: mean, locked: locked }; }
  };
})();
