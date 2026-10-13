/* Shared behaviour for the four pages: number formats, the motion helpers, numerals that roll, the scroll film
   (pinned chapters and scrubbed entries), the compact bar, the reading progress line, pointer light, tilt, the
   lattice behind the page, the segmented control and the tooltip.
   Every page reads without any of it; this only adds the parts that need a script. */
(function () {
  'use strict';
  var App = window.App = {};
  var root = document.documentElement;
  var each = function (list, fn) { Array.prototype.forEach.call(list, fn); };
  var clamp = App.clamp = function (v) { return v < 0 ? 0 : v > 1 ? 1 : v; };

  /* ---------- motion: reduced motion is read live, and every helper ends in the final state ---------- */
  var mq = window.matchMedia ? window.matchMedia('(prefers-reduced-motion: reduce)') : null;
  App.reduced = !!(mq && mq.matches);
  if (mq && mq.addEventListener) mq.addEventListener('change', function (e) { App.reduced = e.matches; });
  App.fine = !!(window.matchMedia && matchMedia('(hover: hover) and (pointer: fine)').matches);
  App.spring = (function () {
    try { var v = getComputedStyle(root).getPropertyValue('--spring').trim(); if (v) return v; } catch (err) { /* fall through */ }
    return 'cubic-bezier(.2,.8,.2,1)';
  })();
  App.ease = function (p) { return 1 - Math.pow(1 - p, 4); };
  // Calls step(p) from 0 to 1 over ms milliseconds. Under reduced motion it calls step(1) at once. Returns a stop function.
  App.run = function (ms, step, done) {
    if (App.reduced || !window.requestAnimationFrame) { step(1); if (done) done(); return function () {}; }
    var t0 = null, id = 0, dead = false;
    function frame(now) {
      if (dead) return;
      if (t0 === null) t0 = now;
      var p = Math.min(1, (now - t0) / ms);
      step(p);
      if (p < 1) id = requestAnimationFrame(frame); else if (done) done();
    }
    id = requestAnimationFrame(frame);
    return function () { dead = true; cancelAnimationFrame(id); };
  };
  // Counts the text of an element from one value to another. The last frame writes `final` exactly.
  App.count = function (el, from, to, fmt, ms, final) {
    if (el._stop) el._stop();
    el._stop = App.run(ms, function (p) { el.textContent = p >= 1 && final != null ? final : fmt(from + (to - from) * App.ease(p)); });
  };
  App.once = function (el, fn, threshold) {
    if (!('IntersectionObserver' in window)) { fn(); return; }
    var io = new IntersectionObserver(function (entries) {
      if (entries.some(function (e) { return e.isIntersecting; })) { io.disconnect(); fn(); }
    }, { threshold: threshold == null ? 0.3 : threshold });
    io.observe(el);
  };

  /* ---------- number formats, the same as the ones written into the pages ---------- */
  App.tokens = function (v) {
    if (!(v > 0)) return '0';
    return v >= 1e6 ? (v / 1e6).toFixed(2) + 'M' : Math.round(v / 1e3) + 'k';
  };
  // A share is never rounded up to 100%: 99.6% reads as 99%.
  App.share = function (v) { return v >= 0.9995 ? '100%' : Math.min(99, Math.floor(v * 100 + 0.5)) + '%'; };
  App.of = function (count, total) { return count + ' of ' + total; };
  App.seconds = function (v) { return Math.round(v) + ' s'; };
  App.turns = function (v) { return v.toFixed(1); };

  // The best configuration and the control on the light class and the original questions, the same pair the pages are written from.
  App.headline = function (data) {
    var best = null, control = null;
    ((data && data.configs) || []).forEach(function (c) {
      if (c.best) best = c;
      if (c.family === 'control' && !control) control = c;
    });
    function light(c) {
      return c ? (c.points.filter(function (p) { return p.class === 'light' && (p.set || 'original') === 'original'; })[0] || null) : null;
    }
    return best && light(best) ? { best: best, control: control, b: light(best), c: light(control) } : null;
  };

  /* ---------- numerals that roll: one strip of digits for each place, turned like a counter ---------- */
  // The lowest place turns with the value; a higher place turns only while the place below it passes from 9 to 0.
  // The element keeps its text for a screen reader. Returns null where the value cannot be shown this way.
  var PLACES = { tokens: { unit: 1000, dec: 0, tail: 'k', fmt: App.tokens }, turns: { unit: 1, dec: 1, tail: '', fmt: App.turns },
    share: { unit: 0.01, dec: 0, tail: '%', fmt: App.share } };
  App.od = function (el, kind, most) {
    var spec = PLACES[kind];
    if (!spec || App.reduced || !(most / spec.unit < 999.5)) return null;
    var spoken = document.createElement('span'), box = document.createElement('span'), cols = [], low = Math.pow(10, -spec.dec);
    function char(text) { var c = document.createElement('span'); c.className = 'od-c'; c.textContent = text; box.appendChild(c); }
    spoken.className = 'sr-only';
    spoken.textContent = el.textContent;
    box.className = 'od';
    box.setAttribute('aria-hidden', 'true');
    for (var j = 2; j >= -spec.dec; j -= 1) {
      if (j === -1) char('.');
      var d = document.createElement('span'), s = document.createElement('span');
      d.className = 'od-d';
      s.textContent = '0\n1\n2\n3\n4\n5\n6\n7\n8\n9\n0';
      d.appendChild(s);
      box.appendChild(d);
      cols.push({ d: d, s: s, w: Math.pow(10, j) });
    }
    if (spec.tail) char(spec.tail);
    el.replaceChildren(spoken, box);
    // A place is as wide as the digit it shows, so a 1 does not sit in the room of a 0. The widths are measured
    // once, in ems, the first time the number is on screen.
    var wide = null;
    function widths() {
      var probe = document.createElement('span'), size = parseFloat(getComputedStyle(box).fontSize) || 16, found = [];
      probe.style.cssText = 'position:absolute;visibility:hidden;white-space:pre';
      box.appendChild(probe);
      for (var i = 0; i < 10; i += 1) { probe.textContent = new Array(9).join(i); found.push(probe.offsetWidth / 8 / size); }
      box.removeChild(probe);
      if (found[0] > 0) wide = found;
    }
    return { set: function (v, settle) {
      // Settled, the strips show exactly what the page's own format would write.
      var x = settle ? parseFloat(spec.fmt(v)) : v / spec.unit, below = 0;
      if (!wide) widths();
      for (var n = cols.length - 1; n >= 0; n -= 1) {
        var c = cols[n], q = x / c.w, pos = c.w === low ? q % 10 : Math.floor(q + 1e-9) % 10 + Math.max(0, below - 9);
        var at = Math.floor(pos) % 10, part = pos - Math.floor(pos);
        below = pos;
        c.d.style.display = c.w > 1 && !(x > c.w - low + 1e-6) ? 'none' : '';
        if (wide) c.d.style.width = (wide[at] + (wide[(at + 1) % 10] - wide[at]) * part).toFixed(4) + 'em';
        c.s.style.transform = 'translateY(' + (-pos * 1.1).toFixed(3) + 'em)';
      }
      if (settle) spoken.textContent = spec.fmt(v);
    } };
  };

  /* ---------- scroll reveals: scrubbed by the scroll position where the browser can, otherwise once ---------- */
  var scrubbed = !!(window.CSS && CSS.supports && CSS.supports('animation-timeline: view()'));
  var revealIO = null;
  App.reveal = function (el) {
    if (App.reduced || el.classList.contains('reveal')) return;
    el.classList.add('reveal');
    if (scrubbed) return;
    if (!revealIO && 'IntersectionObserver' in window) {
      revealIO = new IntersectionObserver(function (entries) {
        entries.forEach(function (e) { if (e.isIntersecting) { e.target.classList.add('in'); revealIO.unobserve(e.target); } });
      }, { rootMargin: '0px 0px -8% 0px' });
    }
    if (revealIO) revealIO.observe(el); else el.classList.add('in');
  };
  each(document.querySelectorAll('.section > .section-title, .section > .card:not(.tilt), .section > .finding-list, .cine-frame, .stream, .foot'), App.reveal);

  /* ---------- the scroll film: a pinned chapter publishes its progress as --p, an entry as --k ---------- */
  // Where the browser has scroll-driven animations the stylesheet drives both; here they are set by hand otherwise.
  // A script that needs the number itself (the particle stage, a count) asks for it with App.pin.
  var pins = [], scrubs = [];
  App.film = !App.reduced && window.innerHeight >= 560 && !!document.querySelector('.pin');
  if (App.film) root.classList.add('film-on');
  if (scrubbed && !App.reduced) root.classList.add('css-scrub');
  each(document.querySelectorAll('.pin'), function (el) {
    pins.push({ el: el, stage: el.querySelector('.pin-stage'), top: 0, span: 0, v: -1, fns: [] });
  });
  if (!App.reduced) each(document.querySelectorAll('.kin, .hops'), function (el) {
    el.classList.add('scrub');
    if (!scrubbed) scrubs.push({ el: el, top: 0, h: 0, v: -1 });
  });
  App.pin = function (el, fn) {
    pins.forEach(function (pin) { if (pin.el === el) { pin.fns.push(fn); if (pin.v >= 0) fn(pin.v); } });
  };
  function docTop(el, y) {
    // An element inside a stage that sticks is placed from its track, which does not move.
    var stage = el.closest('.pin-stage');
    if (stage && stage !== el) return docTop(stage.parentNode, y) + el.getBoundingClientRect().top - stage.getBoundingClientRect().top;
    return el.getBoundingClientRect().top + y;
  }
  function measure() {
    var y = window.scrollY || root.scrollTop || 0;
    pins.forEach(function (pin) {
      pin.top = docTop(pin.el, y);
      // A stage whose chapter does not fit the screen is let go: no pin, the chapter reads as a plain section.
      if (App.film && pin.stage && !pin.el.classList.contains('hero')) {
        var lo = Infinity, hi = -Infinity, tabs = document.querySelector('.tabbar');
        each(pin.stage.children, function (kid) { var box = kid.getBoundingClientRect(); lo = Math.min(lo, box.top); hi = Math.max(hi, box.bottom); });
        pin.el.classList.toggle('loose', hi - lo + (bar ? bar.offsetHeight : 0) + (tabs ? tabs.offsetHeight : 0) + 28 > window.innerHeight);
      }
      pin.span = App.film && pin.stage ? pin.el.offsetHeight - pin.stage.offsetHeight : 0;
    });
    scrubs.forEach(function (s) { s.top = docTop(s.el, y); s.h = s.el.offsetHeight; });
    queue();
  }

  /* ---------- the compact bar condenses the large title as it scrolls away ---------- */
  var bar = document.getElementById('bar'), title = document.getElementById('title');
  App.bar = function (c) { if (bar) bar.style.setProperty('--c', clamp(c).toFixed(3)); };

  /* ---------- the reading progress line, and the section the reader is in (one pass per frame) ---------- */
  var line = document.createElement('div'), wipe = document.createElement('div'), spy = [], queued = false;
  line.className = 'progress';
  line.setAttribute('aria-hidden', 'true');
  document.body.appendChild(line);
  // The line that crosses the screen when the page changes: the same element on every page, so it is carried over.
  wipe.style.cssText = 'position:fixed;top:0;left:0;right:0;z-index:49;height:2px;pointer-events:none;opacity:0;background:var(--accent);view-transition-name:wipe';
  wipe.setAttribute('aria-hidden', 'true');
  document.body.appendChild(wipe);
  if (!('PageSwapEvent' in window)) root.classList.add('no-vt');
  each(document.querySelectorAll('.side-sections a'), function (a) {
    var h = document.getElementById(a.getAttribute('href').slice(1));
    if (h) spy.push({ a: a, h: h });
  });
  function paint() {
    queued = false;
    var y = window.scrollY || root.scrollTop || 0, vh = window.innerHeight, room = root.scrollHeight - root.clientHeight;
    line.style.transform = 'scaleX(' + (room > 0 ? Math.min(1, y / room) : 0).toFixed(4) + ')';
    pins.forEach(function (pin) {
      var v = pin.span > 0 ? clamp((y - pin.top) / pin.span) : 0;
      if (v === pin.v) return;
      pin.v = v;
      if (!scrubbed) pin.el.style.setProperty('--p', v.toFixed(4));
      pin.fns.forEach(function (fn) { fn(v); });
    });
    scrubs.forEach(function (s) {
      var k = clamp((y + vh - s.top) / (0.44 * (vh + s.h)));
      if (k !== s.v) { s.v = k; s.el.style.setProperty('--k', k.toFixed(4)); }
    });
    if (bar && title && !App.barOwned) App.bar((bar.offsetHeight + 28 - title.getBoundingClientRect().bottom) / 44);
    var at = null, limit = vh * 0.35;
    spy.forEach(function (s) { if (s.h.getBoundingClientRect().top <= limit) at = s; });
    spy.forEach(function (s) {
      if (s === at) s.a.setAttribute('aria-current', 'location'); else s.a.removeAttribute('aria-current');
    });
  }
  function queue() { if (!queued) { queued = true; requestAnimationFrame(paint); } }
  window.addEventListener('scroll', queue, { passive: true });
  window.addEventListener('resize', measure);
  if ('ResizeObserver' in window) new ResizeObserver(measure).observe(document.body);
  measure();
  paint();

  /* ---------- numbers that roll from the no-index value to the best one when they come into view ---------- */
  each(document.querySelectorAll('[data-count]'), function (el, i) {
    var kind = el.getAttribute('data-count'), final = el.textContent;
    var from = parseFloat(el.getAttribute('data-from')), to = parseFloat(el.getAttribute('data-to'));
    var fromText = el.getAttribute('data-from-text');
    var fmt = kind === 'tokens' ? App.tokens : kind === 'turns' ? App.turns : kind === 'share' ? App.share : null;
    if (App.reduced || (!fmt && fromText == null)) { el.classList.add('in'); return; }
    var od = fmt ? App.od(el, kind, Math.max(from, to)) : null, strip = null;
    if (od) od.set(from);
    else if (fmt) el.textContent = fmt(from);
    else {
      // A count such as "4 of 5" to "3 of 3" cannot pass through states that were never measured: one line
      // leaves and the other arrives, once.
      var spoken = document.createElement('span'), box = document.createElement('span'), cell = document.createElement('span');
      spoken.className = 'sr-only';
      spoken.textContent = final;
      box.className = 'od';
      box.setAttribute('aria-hidden', 'true');
      cell.className = 'od-d';
      strip = cell.appendChild(document.createElement('span'));
      strip.textContent = fromText + '\n' + final;
      box.appendChild(cell);
      el.replaceChildren(spoken, box);
    }
    el.classList.add('in');
    App.once(el, function () {
      setTimeout(function () {
        if (od) { App.run(1700, function (p) { od.set(from + (to - from) * App.ease(p), p >= 1); }); return; }
        if (fmt) { App.count(el, from, to, fmt, 1500, final); return; }
        strip.style.transition = 'transform 760ms ' + App.spring;
        strip.style.transform = 'translateY(-1.1em)';
      }, 160 + i * 110);
    }, 0.5);
  });
  // The pair drawn beside a figure, and the small figures of what the product does, play when they are first seen.
  each(document.querySelectorAll('.close-mark'), function (el) { App.once(el, function () { el.classList.add('draw'); }, 0.9); });
  each(document.querySelectorAll('.figure'), function (el) {
    if (App.reduced) { el.classList.add('in'); return; }
    App.once(el, function () { el.classList.add('in'); }, 0.4);
  });
  // What each part of a figure does and when: selector, move, delay, duration, and either the gap between
  // siblings or how far a token travels. At rest every part is in its final place, so nothing depends on this.
  var MOVES = { on: [{ opacity: 0.16 }, {}], pop: [{ opacity: 0, transform: 'scale(.3)' }, {}], draw: [{ strokeDashoffset: 1 }, { strokeDashoffset: 0 }],
    wave: [{ opacity: 0.7, transform: 'scale(.3)' }, { opacity: 0, transform: 'scale(9)' }] };
  var PARTS = [['.fig-one .tok', 'go', 150, 620, 50], ['.fig-one .hit', 'on', 700, 240, 0], ['.fig-one .fan path', 'draw', 780, 520, 0],
    ['.fig-one .lit circle', 'pop', 1180, 420, 0], ['.fig-mean .words rect', 'on', 120, 260, 140], ['.fig-mean .wire', 'draw', 620, 700, 0],
    ['.fig-mean .hit', 'pop', 1280, 420, 0], ['.fig-mean .halo', 'pop', 1340, 620, 0], ['.fig-reach .wave', 'wave', 200, 1700, 0],
    ['.fig-reach .hop', 'on', 420, 300, 400], ['.fig-mem .t1', 'go', 150, 1500, 112], ['.fig-mem .hit:not(.kept)', 'pop', 1560, 420, 0],
    ['.fig-mem .wire', 'draw', 1820, 420, 0], ['.fig-mem .kept', 'pop', 2180, 420, 0], ['.fig-mem .t2', 'go', 2560, 420, 112]];
  each(document.querySelectorAll('.do'), function (item) {
    if (App.reduced || !item.animate) return;
    var running = [];
    function play() {
      running.forEach(function (a) { a.cancel(); });
      running = [];
      PARTS.forEach(function (part) {
        each(item.querySelectorAll(part[0]), function (el, n) {
          var go = part[1] === 'go', to = 'translateX(' + part[4] + 'px)';
          var frames = go ? [{ opacity: 1, transform: 'none' }, { opacity: 1, transform: to, offset: 0.86 }, { opacity: 0, transform: to }] : MOVES[part[1]];
          running.push(el.animate(frames, { delay: part[2] + (go ? 0 : n * part[4]), duration: part[3], easing: 'cubic-bezier(.23,1,.32,1)', fill: 'backwards' }));
        });
      });
    }
    App.once(item, play, 0.6);
    item.addEventListener('pointerenter', function (e) { if (e.pointerType !== 'touch') play(); });
    item.addEventListener('click', play);
  });

  /* ---------- pointer light on cards, and a slight tilt on the best one (fine pointers only) ---------- */
  each(document.querySelectorAll('.spot'), function (card) {
    if (App.reduced) { card.classList.add('lit'); return; }
    if (!App.fine) return;
    var tilt = card.classList.contains('tilt'), mx = 0, my = 0, rx = 0, ry = 0, tx = 0, ty = 0, over = false, id = 0;
    function frame() {
      id = 0;
      rx += (tx - rx) * 0.16;
      ry += (ty - ry) * 0.16;
      card.style.setProperty('--mx', mx.toFixed(1) + 'px');
      card.style.setProperty('--my', my.toFixed(1) + 'px');
      if (tilt) card.style.transform = over || Math.abs(rx) + Math.abs(ry) > 0.02 ? 'perspective(900px) rotateX(' + rx.toFixed(2) + 'deg) rotateY(' + ry.toFixed(2) + 'deg)' : '';
      if (over || Math.abs(tx - rx) + Math.abs(ty - ry) > 0.02) id = requestAnimationFrame(frame);
    }
    card.addEventListener('pointermove', function (e) {
      var box = card.getBoundingClientRect();
      mx = e.clientX - box.left;
      my = e.clientY - box.top;
      if (tilt) { ty = (mx / box.width - 0.5) * 10; tx = -(my / box.height - 0.5) * 10; }
      over = true;
      if (!id) id = requestAnimationFrame(frame);
    });
    card.addEventListener('pointerleave', function () {
      over = false; tx = 0; ty = 0;
      if (!id) id = requestAnimationFrame(frame);
    });
  });

  /* ---------- a magnetic button: near it, the button leans a third of the way toward the pointer ---------- */
  each(document.querySelectorAll('.magnet'), function (button) {
    var zone = button.closest('.card') || button.parentNode;
    if (!App.fine || App.reduced || !zone) return;
    zone.addEventListener('pointermove', function (e) {
      var box = button.getBoundingClientRect(), dx = e.clientX - (box.left + box.width / 2), dy = e.clientY - (box.top + box.height / 2);
      var near = Math.abs(dx) < box.width / 2 + 56 && Math.abs(dy) < box.height / 2 + 44;
      // Measured against where the button rests, so its own lean never feeds back into the pull.
      button.style.transform = near ? 'translate(' + (dx * 0.3).toFixed(1) + 'px,' + (dy * 0.3).toFixed(1) + 'px)' : '';
    });
    zone.addEventListener('pointerleave', function () { button.style.transform = ''; });
  });

  /* ---------- behind the page: a lattice of points that the pointer parts, and the light the cursor carries ---------- */
  if (App.fine && !App.reduced) (function () {
    var canvas = document.createElement('canvas'), g = canvas.getContext ? canvas.getContext('2d') : null, glint = document.createElement('div');
    if (!g) return;
    canvas.style.cssText = 'position:fixed;inset:0;z-index:0;width:100%;height:100%;pointer-events:none';
    canvas.setAttribute('aria-hidden', 'true');
    glint.style.cssText = 'position:fixed;left:0;top:0;z-index:44;width:440px;height:440px;margin:-220px 0 0 -220px;border-radius:50%;' +
      'pointer-events:none;opacity:0;transition:opacity .5s ease-out;will-change:transform;' +
      'background:radial-gradient(closest-side,rgba(255,255,255,.05),rgba(255,255,255,.018) 45%,transparent)';
    glint.setAttribute('aria-hidden', 'true');
    document.body.insertBefore(canvas, document.body.firstChild);
    document.body.appendChild(glint);
    var STEP = 36, REACH = 150, W = 0, H = 0, dpr = 1, px = -999, py = -999, tx = -999, ty = -999, id = 0, seen = false;
    var rgb = (getComputedStyle(root).getPropertyValue('--accent-rgb') || '255,122,61').replace(/\s/g, '');
    function draw() {
      g.setTransform(dpr, 0, 0, dpr, 0, 0);
      g.clearRect(0, 0, W, H);
      for (var y = STEP / 2; y < H; y += STEP) {
        for (var x = STEP / 2; x < W; x += STEP) {
          var dx = x - px, dy = y - py, d2 = dx * dx + dy * dy;
          if (d2 < REACH * REACH) {
            var d = Math.sqrt(d2) || 1, f = 1 - d / REACH;
            f *= f;
            g.fillStyle = 'rgba(' + rgb + ',' + (0.12 + f * 0.75).toFixed(3) + ')';
            g.fillRect(x + dx / d * f * 26 - 0.5 - f, y + dy / d * f * 26 - 0.5 - f, 1 + 2 * f, 1 + 2 * f);
          } else {
            g.fillStyle = 'rgba(255,255,255,.11)';
            g.fillRect(x - 0.5, y - 0.5, 1, 1);
          }
        }
      }
    }
    function size() {
      W = window.innerWidth; H = window.innerHeight;
      dpr = Math.min(window.devicePixelRatio || 1, 2);
      canvas.width = Math.round(W * dpr);
      canvas.height = Math.round(H * dpr);
      draw();
    }
    function frame() {
      id = 0;
      // Taken fast and released slowly: the light and the parting trail the pointer a little.
      px += (tx - px) * 0.22;
      py += (ty - py) * 0.22;
      glint.style.transform = 'translate(' + px.toFixed(1) + 'px,' + py.toFixed(1) + 'px)';
      draw();
      if (Math.abs(tx - px) + Math.abs(ty - py) > 0.4 && !document.hidden) id = requestAnimationFrame(frame);
    }
    function wake() { if (!id) id = requestAnimationFrame(frame); }
    window.addEventListener('pointermove', function (e) {
      if (e.pointerType === 'touch') return;
      tx = e.clientX; ty = e.clientY;
      if (!seen) { seen = true; px = tx; py = ty; glint.style.opacity = '1'; }
      wake();
    }, { passive: true });
    root.addEventListener('pointerleave', function () { seen = false; glint.style.opacity = '0'; tx = px = -999; ty = py = -999; wake(); });
    window.addEventListener('resize', size);
    size();
  })();

  /* ---------- the sidebar indicator leans to the page under the pointer, and glides where there are no page transitions ---------- */
  (function () {
    var tab = document.querySelector('.tab-ind'), side = document.querySelector('.side-ind');
    if (side && !App.reduced) {
      var wrap = side.parentNode;
      var peek = function (e) {
        var a = e.target.closest ? e.target.closest('.side-pages > li > a') : null;
        if (!a) return;
        side.style.setProperty('--y', a.offsetTop + 'px');
        side.classList.toggle('peek', a.getAttribute('aria-current') !== 'page');
      };
      var back = function () { side.style.removeProperty('--y'); side.classList.remove('peek'); };
      wrap.addEventListener('pointerover', peek);
      wrap.addEventListener('focusin', peek);
      wrap.addEventListener('pointerleave', back);
      wrap.addEventListener('focusout', back);
    }
    var now = parseFloat(getComputedStyle(tab || side || root).getPropertyValue('--i')) || 0, before = null;
    try { before = sessionStorage.getItem('pgr-nav'); sessionStorage.setItem('pgr-nav', String(now)); } catch (err) { before = null; }
    if (before === null || App.reduced || 'PageSwapEvent' in window || !tab && !side) return;
    before = parseFloat(before);
    if (!(before >= 0) || before === now) return;
    var opts = { duration: 480, easing: App.spring };
    try {
      if (tab) {
        tab.animate([{ transform: 'translateX(' + before * 100 + '%) scaleX(1)' },
          { transform: 'translateX(' + (before + now) * 50 + '%) scaleX(1.6)', offset: 0.45 },
          { transform: 'translateX(' + now * 100 + '%) scaleX(1)' }], opts);
      }
      if (side) side.animate([{ transform: 'translateY(' + before * 44 + 'px)' }, { transform: 'translateY(' + now * 44 + 'px)' }], opts);
    } catch (err) { /* the indicator is already in place */ }
  })();

  /* ---------- a segmented control: one radio group, arrow keys, selection follows focus ---------- */
  App.segmented = function (rootEl, onChange) {
    var buttons = Array.prototype.slice.call(rootEl.querySelectorAll('[role="radio"]'));
    var current = Math.max(0, buttons.findIndex(function (b) { return b.getAttribute('aria-checked') === 'true'; }));
    function select(i, focus) {
      current = i;
      buttons.forEach(function (b, j) {
        b.setAttribute('aria-checked', j === i ? 'true' : 'false');
        b.tabIndex = j === i ? 0 : -1;
      });
      rootEl.style.setProperty('--i', i);
      if (focus) buttons[i].focus();
      onChange(buttons[i].getAttribute('data-value') || buttons[i].getAttribute('data-class'), buttons[i].textContent, i);
    }
    buttons.forEach(function (button, i) {
      button.addEventListener('click', function () { if (i !== current) select(i); });
      button.addEventListener('keydown', function (e) {
        var n = buttons.length, next = -1;
        if (e.key === 'ArrowRight' || e.key === 'ArrowDown') next = (i + 1) % n;
        else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') next = (i - 1 + n) % n;
        else if (e.key === 'Home') next = 0;
        else if (e.key === 'End') next = n - 1;
        if (next < 0) return;
        e.preventDefault();
        select(next, true);
      });
    });
    rootEl.style.setProperty('--i', current);
    return { select: select };
  };

  /* ---------- the tooltip: hover, focus and tap ---------- */
  var tipEl = null;
  function tipNode() {
    if (!tipEl) {
      tipEl = document.createElement('div');
      tipEl.className = 'tip';
      tipEl.setAttribute('role', 'tooltip');
      tipEl.hidden = true;
      document.body.appendChild(tipEl);
    }
    return tipEl;
  }
  App.tip = {
    show: function (name, rows, x, y) {
      var tip = tipNode(), head = document.createElement('b');
      head.textContent = name;
      tip.replaceChildren(head);
      rows.forEach(function (row) {
        var lineEl = document.createElement('div'), a = document.createElement('span'), b = document.createElement('span');
        a.textContent = row[0];
        b.textContent = row[1];
        lineEl.appendChild(a);
        lineEl.appendChild(b);
        tip.appendChild(lineEl);
      });
      tip.hidden = false;
      var w = tip.offsetWidth, h = tip.offsetHeight, vw = document.documentElement.clientWidth, vh = window.innerHeight;
      var left = x + 14 + w > vw - 8 ? x - 14 - w : x + 14;
      var top = y + 14 + h > vh - 8 ? y - 14 - h : y + 14;
      tip.style.left = Math.max(8, left) + 'px';
      tip.style.top = Math.max(8, top) + 'px';
    },
    hide: function () { if (tipEl) tipEl.hidden = true; }
  };
  window.addEventListener('scroll', function () { App.tip.hide(); }, { passive: true });
  document.addEventListener('keydown', function (e) { if (e.key === 'Escape') App.tip.hide(); });

  // Last: the pieces that hide something until a script shows it only apply once this line has run.
  if (!App.reduced) root.classList.add('motion-ready');
})();
