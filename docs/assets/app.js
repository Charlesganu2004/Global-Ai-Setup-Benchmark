/* Shared behaviour for the four pages: number formats, the motion helpers, numerals that roll, the scroll film
   (pinned chapters), reveals, the compact bar, the reading progress line, the segmented control and the figure of what
   the product does. A thing that is read is always drawn whole: scrolling changes a state and the change takes under
   250 ms, so a stop anywhere shows a finished picture.
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
  var NB = ' ';
  App.tokens = function (v) {
    if (!(v > 0)) return '0';
    return v >= 1e6 ? (v / 1e6).toFixed(2) + 'M' : Math.round(v / 1e3) + 'k';
  };
  // A share is never rounded up to 100%: 99.6% reads as 99%.
  App.share = function (v) { return v >= 0.9995 ? '100%' : Math.min(99, Math.floor(v * 100 + 0.5)) + '%'; };
  // A count and a range are one value: a narrow column breaks a line between values, never inside one.
  App.of = function (count, total) { return count + NB + 'of' + NB + total; };
  App.seconds = function (v) { return Math.round(v) + NB + 's'; };
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
  // set(v) shows the value mid-roll; set(v, true) shows the whole number the page's own format would write: a roll
  // that has stopped is always settled, so no digit is ever left between two positions.
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
      var x = settle ? parseFloat(spec.fmt(v)) : v / spec.unit, below = 0;
      if (!wide) widths();
      for (var n = cols.length - 1; n >= 0; n -= 1) {
        var c = cols[n], q = x / c.w, pos = c.w === low ? q % 10 : Math.floor(q + 1e-9) % 10 + Math.max(0, below - 9);
        if (settle) pos = Math.round(pos) % 10;
        var at = Math.floor(pos) % 10, part = pos - Math.floor(pos);
        below = pos;
        c.d.style.display = c.w > 1 && !(x > c.w - low + 1e-6) ? 'none' : '';
        if (wide) c.d.style.width = (wide[at] + (wide[(at + 1) % 10] - wide[at]) * part).toFixed(4) + 'em';
        c.s.style.transform = 'translateY(' + (-pos * 1.1).toFixed(3) + 'em)';
      }
      if (settle) spoken.textContent = spec.fmt(v);
    } };
  };

  /* ---------- reveals: a thing comes in once, in under 250 ms, when it is first seen ---------- */
  var revealIO = null;
  App.reveal = function (el) {
    if (App.reduced || el.classList.contains('reveal')) return;
    el.classList.add('reveal');
    if (!('IntersectionObserver' in window)) { el.classList.add('in'); return; }
    if (!revealIO) {
      revealIO = new IntersectionObserver(function (entries) {
        entries.forEach(function (e) { if (e.isIntersecting) { e.target.classList.add('in'); revealIO.unobserve(e.target); } });
      }, { rootMargin: '0px 0px -6% 0px' });
    }
    revealIO.observe(el);
  };
  each(document.querySelectorAll('.section > .section-title, .section > .card, .section > .finding-list, .section > .ledger, .rl-row, .flag, .also, .key, .cine, .stream, .proof-text'), App.reveal);
  each(document.querySelectorAll('.kin'), function (el) { if (!App.reduced) App.once(el, function () { el.classList.add('in'); }, 0.15); });

  /* ---------- the scroll film: a pinned chapter publishes its progress as --p ---------- */
  // A script that needs the number itself (the particle stage, the stacks) asks for it with App.pin.
  var pins = [];
  App.film = !App.reduced && window.innerHeight >= 560 && !!document.querySelector('.pin');
  if (App.film) root.classList.add('film-on');
  each(document.querySelectorAll('.pin'), function (el) {
    pins.push({ el: el, stage: el.querySelector('.pin-stage'), top: 0, span: 0, v: -1, fns: [] });
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
  var bar = document.getElementById('bar'), title = document.getElementById('title');
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
    queue();
  }
  App.bar = function (c) { if (bar) bar.style.setProperty('--c', clamp(c).toFixed(3)); };

  /* ---------- the reading progress line, the section the reader is in, the bar that steps aside (one pass per frame) ---------- */
  var line = document.createElement('div'), wipe = document.createElement('div'), spy = [], queued = false, lastY = window.scrollY || 0;
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
      pin.el.style.setProperty('--p', v.toFixed(4));
      pin.fns.forEach(function (fn) { fn(v); });
    });
    if (bar && title && !App.barOwned) App.bar((bar.offsetHeight + 28 - title.getBoundingClientRect().bottom) / 44);
    // On a phone the top bar leaves on the way down and comes back on the way up, so the controls under it get the room.
    if (window.innerWidth < 1000) {
      if (Math.abs(y - lastY) > 6) { root.classList.toggle('away', y > lastY && y > 240); lastY = y; }
    } else root.classList.remove('away');
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
  // Sticky controls cover the top of the screen: what takes focus is scrolled clear of them.
  var sticky = document.querySelector('.seg-bar');
  if (sticky && 'ResizeObserver' in window) {
    new ResizeObserver(function () { root.style.setProperty('--seg', (sticky.offsetParent ? sticky.offsetHeight : 0) + 'px'); }).observe(sticky);
  }
  measure();
  paint();

  /* ---------- numbers that count from the no-index value to the best one, once, when they come into view ---------- */
  each(document.querySelectorAll('[data-count]'), function (el, i) {
    var kind = el.getAttribute('data-count'), final = el.textContent;
    var from = parseFloat(el.getAttribute('data-from')), to = parseFloat(el.getAttribute('data-to'));
    var fmt = kind === 'tokens' ? App.tokens : kind === 'turns' ? App.turns : kind === 'share' ? App.share : null;
    el.classList.add('in');
    if (App.reduced || !fmt) return;
    el.textContent = fmt(from);
    App.once(el, function () {
      setTimeout(function () { App.count(el, from, to, fmt, 340, final); }, 60 + i * 60);
    }, 0.5);
  });
  each(document.querySelectorAll('.close-mark'), function (el) { App.once(el, function () { el.classList.add('draw'); }, 0.9); });

  // The figure of the one capability the study measures plays when first seen and again when pointed at; at rest it shows
  // the outcome. Each part: selector, move, delay, duration, and either the gap between siblings or how far a token travels.
  var MOVES = { on: [{ opacity: 0.16 }, {}], pop: [{ opacity: 0, transform: 'scale(.3)' }, {}], draw: [{ strokeDashoffset: 1 }, { strokeDashoffset: 0 }] };
  var PARTS = [['.fig-one .tok', 'go', 150, 620, 50], ['.fig-one .hit', 'on', 700, 240, 0], ['.fig-one .fan path', 'draw', 780, 520, 0],
    ['.fig-one .lit circle', 'pop', 1180, 420, 0]];
  each(document.querySelectorAll('.flag'), function (item) {
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

  // Last: the pieces that hide something until a script shows it only apply once this line has run.
  if (!App.reduced) root.classList.add('motion-ready');
})();
