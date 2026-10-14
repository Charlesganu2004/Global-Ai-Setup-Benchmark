/* Shared behaviour of the four pages: number formats, the motion helpers, the top bar, the scroll film (pinned chapters publish
   their progress as --p), reveals, counters, magnetic buttons, the pointer light on cards and windows, the install command and
   its copy buttons, the theme. Every page reads without any of it; this only adds what needs a script. A thing that is read is
   always drawn whole: under reduced motion, and with no script, every scene shows its end state. */
(function () {
  'use strict';
  var App = window.App = {};
  var root = document.documentElement;
  var each = function (list, fn) { Array.prototype.forEach.call(list, fn); };
  var all = function (sel, scope) { return (scope || document).querySelectorAll(sel); };
  var clamp = App.clamp = function (v) { return v < 0 ? 0 : v > 1 ? 1 : v; };
  var store = {
    get: function (key) { try { return window.localStorage.getItem(key); } catch (err) { return null; } },
    set: function (key, value) { try { window.localStorage.setItem(key, value); } catch (err) { /* private window: the choice lasts for this page only */ } }
  };

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
      App.pace.tick(now);
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
    // A frame writes the number only when what it reads has changed: a write of the same text still sends the page back to layout.
    el._stop = App.run(ms, function (p) {
      var text = p >= 1 && final != null ? final : fmt(from + (to - from) * App.ease(p));
      if (el.textContent !== text) el.textContent = text;
    });
  };
  App.once = function (el, fn, threshold) {
    if (!('IntersectionObserver' in window)) { fn(); return; }
    var want = threshold == null ? 0.3 : threshold, steps = [];
    for (var i = 0; i <= 50; i += 1) steps.push(i / 50);
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        // A share of the element, or the share of the window it can fill, whichever is less: an element taller than the window is
        // seen long before a fifth of it is, and on a small phone a fifth of the leaderboard is more than the window shows.
        var need = Math.min(want * e.boundingClientRect.height, 0.6 * (e.rootBounds ? e.rootBounds.height : window.innerHeight));
        if (e.isIntersecting && e.intersectionRect.height >= need - 1) { io.disconnect(); fn(); }
      });
    }, { threshold: steps });
    io.observe(el);
  };

  /* ---------- pace: the frame budget, and the effects that give way when frames miss it ---------- */
  // The budget is 3.5 ms a frame, the period of a 280 Hz screen. Every animation reads the clock and never counts frames, so it is the
  // same at 60, 120, 144 and 280 Hz and on a screen of any size. The page draws no canvas: what costs a frame here is its glows and
  // its pointer lights, and those are drawn at a scale. The scale is 1 until frames are late, and then it goes down a step. A frame is
  // late when the work it made took more than four fifths of the screen's slot (measured from the first animation callback to the task
  // after the frame was rendered), or when it came more than one and a half slots after the frame before it. The slot is the screen's own
  // period, learnt from the gaps of the frames that were drawn one after the other and, whenever the page has been idle for a moment,
  // from a short run of empty frames, which is the only time the screen's real rate shows. Frames of a page in the background do not
  // count. A step back up needs a full window of frames made on time, or two seconds of rest: effects that are lowered stay lowered only
  // while frames miss their deadline.
  App.pace = (function () {
    var STEPS = [1, 0.75, 0.5, 0.35], WINDOW = 32, SLOT_MIN = 3.4, SLOT_MAX = 34, IDLE = 1500;
    var pace = { budget: 3.5, period: 16.7, level: 0, scale: 1, probed: false };
    var ring = [], gaps = [], level = 0, since = 0, lastLate = 0, lastNow = 0, lastWork = 0, started = 0, gapNow = 0, listeners = [], timer = 0, probing = false, probedAt = -1e9, warm = 4000;
    var channel = window.MessageChannel ? new MessageChannel() : null;
    var fxEls = null;
    function clampSlot(v) { return v < SLOT_MIN ? SLOT_MIN : v > SLOT_MAX ? SLOT_MAX : v; }
    function set(next) {
      level = pace.level = next; pace.scale = STEPS[next]; since = performance.now(); ring.length = 0;
      // Only the glows are written, each on its own element: a property on the root restyles every element of the page.
      if (!fxEls) fxEls = Array.prototype.slice.call(document.querySelectorAll('.amb-glow, .scene-bg .glow'));
      fxEls.forEach(function (el) { el.style.scale = next ? String(STEPS[next]) : ''; });
      root.setAttribute('data-fx', String(next));
      listeners.forEach(function (fn) { fn(pace); });
    }
    function learn(gap) {
      if (!(gap > 0 && gap < 40) || pace.probed) return;
      gaps.push(gap);
      if (gaps.length > 120) gaps.shift();
      if (gaps.length >= 30) pace.period = clampSlot(gaps.slice().sort(function (a, b) { return a - b; })[Math.floor(gaps.length * 0.25)]);
    }
    // One finished frame: how much work it made, and how long after the one before it. Judged over the frames of the last 600 ms, at
    // least eight of them: a slow screen gets its answer from fewer frames, a fast one from more. The gap counts once the slot is known.
    function judge(cost, gap) {
      var t = performance.now();
      if (t < warm) return;
      learn(gap);
      var slot = pace.period, known = pace.probed || gaps.length >= 30;
      var isLate = cost > slot * 0.8 || (known && gap > slot * 1.5 && gap < slot * 4);
      if (isLate) lastLate = t;
      ring.push({ t: t, late: isLate });
      if (ring.length > 64) ring.shift();
      var n = 0, late = 0;
      for (var i = ring.length - 1; i >= 0 && (n < 8 || (ring[i].t > t - 600 && n < 48)); i -= 1) { n += 1; if (ring[i].late) late += 1; }
      if (n < 8) return;
      if (late >= 2 && late >= n * 0.25 && level < STEPS.length - 1 && t - since > 500) set(level + 1);
      else if (level > 0 && t - lastLate > 3000 && t - since > 3000) set(level - 1);
    }
    if (channel) channel.port1.onmessage = function () {
      if (!started) return;
      var cost = performance.now() - started;
      started = 0;
      judge(cost, gapNow);
    };
    // Called at the start of each frame that does work. Many callbacks may ask in one frame: the first one starts the measure.
    pace.tick = function (now) {
      if (now === lastNow || !channel) return;
      gapNow = lastNow && now - lastNow < SLOT_MAX * 4 ? now - lastNow : 0;
      lastNow = now;
      lastWork = performance.now();
      started = lastWork;
      channel.port2.postMessage(0);
      arm();
    };
    // Empty frames, for a moment, when nothing else is drawing: their gap is the slot of the screen. Resolves with the median gap.
    function probe(done) {
      var n = 0, prev = 0, list = [];
      probing = true;
      function frame(t) {
        if (prev) list.push(t - prev);
        prev = t;
        n += 1;
        if (n < 30 && !document.hidden) { requestAnimationFrame(frame); return; }
        probing = false;
        done(list);
      }
      requestAnimationFrame(frame);
    }
    function arm() { if (!timer) timer = setTimeout(rest, IDLE); }
    // The page has been still for a moment: lower effects come back a step, and the screen's slot is measured, at most every half minute.
    function rest() {
      timer = 0;
      var idle = performance.now() - lastWork;
      if (document.hidden || probing || idle < IDLE - 50) { if (!probing) arm(); return; }
      if (level > 0) set(level - 1);
      measureSlot();
      if (level > 0 || !pace.probed) arm();
    }
    function measureSlot() {
      if (probing || performance.now() - probedAt < 30000) return;
      probedAt = performance.now();
      probe(function (list) {
        if (list.length < 10) return;
        pace.period = clampSlot(list.slice().sort(function (a, b) { return a - b; })[Math.floor(list.length / 2)]);
        pace.probed = true;
        ring.length = 0;
      });
    }
    document.addEventListener('visibilitychange', function () { lastNow = 0; started = 0; ring.length = 0; });
    // A page that is not drawing when it has loaded learns its screen's slot at once, before anything on it moves.
    window.addEventListener('load', function () {
      warm = performance.now() + 800;
      setTimeout(function () { if (!document.hidden && performance.now() - lastWork > 300) measureSlot(); }, 300);
    });
    arm();
    pace.on = function (fn) { listeners.push(fn); };
    return pace;
  })();

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
  App.fmt = { tokens: App.tokens, turns: App.turns, share: App.share };

  // The best configuration and the control on the smallest model and the original questions, the same pair the pages are written from.
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

  /* ---------- the system: the install command and the key hint follow it ---------- */
  App.os = (function () {
    var hint = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || navigator.userAgent || '';
    return /win/i.test(hint) ? 'windows' : /mac|iphone|ipad/i.test(hint) ? 'mac' : /linux|x11|cros|android/i.test(hint) ? 'linux' : '';
  })();
  if (App.os === 'mac') each(all('[data-mod]'), function (el) { el.textContent = '⌘'; });

  /* ---------- a toast, and the copy control ---------- */
  var toast = document.createElement('div');
  toast.className = 'toast';
  toast.setAttribute('role', 'status');
  toast.setAttribute('aria-live', 'polite');
  document.body.appendChild(toast);
  var toastTimer = 0;
  App.toast = function (text) {
    toast.textContent = text;
    toast.classList.add('on');
    clearTimeout(toastTimer);
    toastTimer = setTimeout(function () { toast.classList.remove('on'); }, 1800);
  };
  App.copy = function (text, done) {
    function fallback() {
      var area = document.createElement('textarea');
      area.value = text;
      area.setAttribute('readonly', '');
      area.style.cssText = 'position:fixed;top:0;left:-9999px;opacity:0';
      document.body.appendChild(area);
      area.select();
      var ok = false;
      try { ok = document.execCommand('copy'); } catch (err) { ok = false; }
      document.body.removeChild(area);
      done(ok);
    }
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(function () { done(true); }, fallback);
    } else fallback();
  };
  each(all('[data-copy]'), function (button) {
    var label = button.querySelector('.copy-label'), timer = 0;
    button.addEventListener('click', function () {
      App.copy(button.getAttribute('data-copy'), function (ok) {
        if (!ok) { App.toast('Copy failed. Select the text and copy it.'); return; }
        button.classList.add('done');
        if (label) label.textContent = 'Copied';
        App.toast('Copied to the clipboard');
        clearTimeout(timer);
        timer = setTimeout(function () { button.classList.remove('done'); if (label) label.textContent = 'Copy'; }, 1800);
      });
    });
  });

  /* ---------- a segmented control: one radio group, arrow keys, selection follows focus ---------- */
  App.segmented = function (rootEl, onChange) {
    var buttons = Array.prototype.slice.call(rootEl.querySelectorAll('[role="radio"]'));
    var current = Math.max(0, buttons.findIndex(function (b) { return b.getAttribute('aria-checked') === 'true'; }));
    function select(i, focus, quiet) {
      current = i;
      buttons.forEach(function (b, j) {
        b.setAttribute('aria-checked', j === i ? 'true' : 'false');
        b.tabIndex = j === i ? 0 : -1;
      });
      rootEl.style.setProperty('--i', i);
      if (focus) buttons[i].focus();
      if (!quiet) onChange(buttons[i].getAttribute('data-value') || buttons[i].getAttribute('data-class'), buttons[i].textContent, i);
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
    return { select: select, index: function () { return current; } };
  };

  /* ---------- the install command: both commands are in the page, the visitor's system is shown ---------- */
  var boxes = Array.prototype.slice.call(all('[data-cmdbox]')), choice = App.os === 'windows' ? 'windows' : 'unix';
  var boxCtl = [];
  function showSystem(sys, from) {
    choice = sys;
    each(all('[data-cmdbox] [data-sys]'), function (el) {
      if (el.tagName === 'BUTTON') return;
      el.classList.toggle('on', el.getAttribute('data-sys') === sys);
    });
    boxCtl.forEach(function (ctl) { if (ctl !== from) ctl.select(sys === 'windows' ? 0 : 1, false, true); });
    each(all('[data-install-term] .log'), function (el) { el.hidden = el.getAttribute('data-sys') !== sys; });
  }
  boxes.forEach(function (box) {
    var group = box.querySelector('[role="radiogroup"]');
    var ctl = App.segmented(group, function (value, label, i) { showSystem(i === 0 ? 'windows' : 'unix', ctl); });
    boxCtl.push(ctl);
  });
  if (boxes.length) showSystem(choice, null);
  boxCtl.forEach(function (ctl) { ctl.select(choice === 'windows' ? 0 : 1, false, true); });
  App.system = function () { return choice; };

  /* ---------- the theme ---------- */
  var themeButton = document.querySelector('[data-theme-toggle]');
  function themeNow() {
    var set = root.getAttribute('data-theme');
    return set || (window.matchMedia && matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark');
  }
  function themeLabel() {
    if (themeButton) themeButton.setAttribute('aria-label', themeNow() === 'light' ? 'Switch to the dark theme' : 'Switch to the light theme');
  }
  App.theme = function (name) {
    root.setAttribute('data-theme', name);
    store.set('pgr-theme', name);
    var meta = document.querySelector('meta[name="theme-color"]');
    if (meta) meta.setAttribute('content', name === 'light' ? '#f5f3ef' : '#08090b');
    themeLabel();
    document.dispatchEvent(new CustomEvent('themechange', { detail: name }));
  };
  if (themeButton) {
    themeLabel();
    themeButton.addEventListener('click', function () { App.theme(themeNow() === 'light' ? 'dark' : 'light'); });
  }

  /* ---------- the top bar: a glass bar once the page moves, a gliding mark under the page you point at ---------- */
  var nav = document.getElementById('nav'), links = document.getElementById('nav-links'), ind = links && links.querySelector('.nav-ind');
  function glide(a) {
    if (!ind) return;
    if (!a) { ind.classList.remove('on'); return; }
    ind.style.width = a.offsetWidth + 'px';
    ind.style.setProperty('--x', a.offsetLeft + 'px');
    ind.classList.add('on');
  }
  if (links && ind && App.fine) {
    links.addEventListener('pointerover', function (e) { var a = e.target.closest ? e.target.closest('a') : null; if (a) glide(a); });
    links.addEventListener('focusin', function (e) { var a = e.target.closest ? e.target.closest('a') : null; if (a) glide(a); });
    links.addEventListener('pointerleave', function () { glide(null); });
    links.addEventListener('focusout', function () { glide(null); });
  }

  /* ---------- the scroll film: a pinned chapter is a screen that plays while the page scrolls ---------- */
  // A chapter with room to play is pinned and calls its functions with its progress, 0 to 1. A chapter that does not fit the
  // window, and every chapter under reduced motion or in a small window, is not pinned: it is a plain section and its functions
  // are called once with 1, so every figure in it is the finished one and nothing is cut.
  var pins = [];
  var filmWanted = function () { return !App.reduced && window.innerHeight >= 560 && window.innerWidth >= 1101 && !!document.querySelector('.pin'); };
  App.film = filmWanted();
  if (App.film) root.classList.add('film-on');
  each(all('.pin'), function (el) { pins.push({ el: el, stage: el.querySelector('.pin-stage'), top: 0, span: 0, v: -1, fns: [], loose: false }); });
  App.pin = function (el, fn) {
    pins.forEach(function (pin) {
      if (pin.el !== el) return;
      pin.fns.push(fn);
      fn(!App.film || pin.loose ? 1 : pin.v >= 0 ? pin.v : 0);
    });
  };
  // A scene taller than the window is scaled down to fit it (CSS zoom on its children: the layout is that of a wider window, drawn
  // smaller, and the text is laid out at its final size, so it stays sharp). Below 62% the scene is let go instead.
  var MIN_FIT = 0.62;
  function fitTo(box, kids, room) {
    kids.forEach(function (c) { c.style.zoom = ''; });
    var k = 1;
    for (var n = 0; n < 4; n += 1) {
      var need = box.scrollHeight, have = room();
      if (need <= have + 1) break;
      k *= have / need * 0.995;
      if (k < MIN_FIT) { kids.forEach(function (c) { c.style.zoom = ''; }); return 0; }
      kids.forEach(function (c) { c.style.zoom = k.toFixed(4); });
    }
    return k || 1;
  }
  var sceneBox = document.querySelector('.scene'), sceneIn = sceneBox && sceneBox.querySelector('.scene-in');
  function fitScene() {
    if (!sceneIn) return;
    sceneIn.style.zoom = ''; sceneIn.style.width = '';
    if (window.innerWidth < 1101) return;
    var cs = getComputedStyle(sceneBox), k = 1;
    var avail = window.innerHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
    var natural = sceneIn.getBoundingClientRect().height;
    if (natural > avail) { k = Math.max(MIN_FIT, avail / natural * 0.995); sceneIn.style.zoom = k.toFixed(4); var column = nav ? nav.querySelector('.nav-in') : null; if (column) sceneIn.style.width = (column.getBoundingClientRect().width / k).toFixed(2) + 'px'; }
  }
  var lastBox = '', room = 0, scrolled = 0;
  function measure(force) {
    var box = window.innerWidth + 'x' + window.innerHeight + 'x' + root.scrollHeight;
    if (!force && box === lastBox) return;
    lastBox = box;
    // The window decides, at any time, whether the chapters are pinned: a window made taller or wider pins them, one made smaller lets them go.
    var film = filmWanted();
    if (film !== App.film) { App.film = film; root.classList.toggle('film-on', film); pins.forEach(function (pin) { pin.v = -1; }); }
    var y = scrolled = window.scrollY || root.scrollTop || 0;
    // What a frame of scrolling needs from the layout is read here, when the window or the page changes size, and never in the frame:
    // a read in the frame forces a layout on every frame in which a counter or a bar was written, which is a second layout of that frame.
    room = root.scrollHeight - root.clientHeight;
    spy.forEach(function (s) { s.top = s.h.getBoundingClientRect().top + y; });
    fitScene();
    pins.forEach(function (pin) {
      var was = pin.loose;
      pin.loose = false;
      pin.el.classList.remove('loose');
      pin.top = pin.el.getBoundingClientRect().top + y;
      // A stage whose content does not fit the window is scaled to fit; one that would need less than 62% is let go: no pin, the
      // chapter reads as a plain section, finished.
      if (App.film && pin.stage) {
        var fit = fitTo(pin.stage, Array.prototype.slice.call(pin.stage.children), function () { return pin.stage.clientHeight; });
        if (!fit || pin.stage.scrollHeight > pin.stage.clientHeight + 2) pin.loose = true;
      } else if (pin.stage) Array.prototype.forEach.call(pin.stage.children, function (c) { c.style.zoom = ''; });
      pin.span = App.film && pin.stage && !pin.loose ? pin.el.offsetHeight - pin.stage.offsetHeight : 0;
      if (App.film && pin.span <= 0) pin.loose = true;
      if (pin.loose) {
        pin.el.classList.add('loose');
        if (pin.stage) Array.prototype.forEach.call(pin.stage.children, function (c) { c.style.zoom = ''; });
      }
      if (pin.loose || !App.film) {
        pin.span = 0;
        if (pin.v !== 1) { pin.v = 1; pin.fns.forEach(function (fn) { fn(1); }); }
      } else if (was || pin.v === 1) pin.v = -1;
    });
    queue();
  }

  /* ---------- one pass per frame: the progress line, the bar, the pinned chapters, the section the reader is in ---------- */
  var progress = document.getElementById('progress'), ambient = document.querySelector('.amb-glow'), queued = false, lastY = window.scrollY || 0;
  // The progress line and the glow follow the scroll in the stylesheet where the browser can tie an animation to it (see app.css).
  var linked = !!(window.CSS && CSS.supports && CSS.supports('animation-timeline: scroll()')) && !App.reduced;
  var frameHooks = [];
  App.onFrame = function (fn) { frameHooks.push(fn); };
  var spy = [];
  each(all('.toc a'), function (a) {
    var h = document.getElementById(a.getAttribute('href').slice(1));
    if (h) spy.push({ a: a, h: h, top: 0 });
  });
  function paint(now) {
    queued = false;
    if (now) App.pace.tick(now);
    var y = scrolled, vh = window.innerHeight;   // the position was read by the scroll event, before this frame's writes
    if (progress && !linked) progress.style.setProperty('--sp', (room > 0 ? Math.min(1, y / room) : 0).toFixed(4));
    if (nav) {
      nav.classList.toggle('solid', y > 8);
      var down = y > lastY, moved = Math.abs(y - lastY) > 6;
      if (moved) {
        var hold = nav.contains(document.activeElement) || document.querySelector('dialog[open]');
        var away = down && y > 260 && !hold;
        nav.classList.toggle('away', away);
        root.classList.toggle('nav-gone', away);
        lastY = y;
      }
    }
    if (ambient && !linked) ambient.style.translate = (Math.sin(y / (room || 1) * 9.4) * 34).toFixed(2) + 'vw ' + (Math.cos(y / (room || 1) * 6.3) * 22).toFixed(2) + 'vh';
    frameHooks.forEach(function (fn) { fn(y, vh); });
    pins.forEach(function (pin) {
      if (!App.film || pin.loose || pin.span <= 0) return;
      var v = clamp((y - pin.top) / pin.span);
      if (v === pin.v) return;
      pin.v = v;
      pin.fns.forEach(function (fn) { fn(v); });
    });
    var at = null, limit = vh * 0.4;
    spy.forEach(function (s) { if (s.top - y <= limit) at = s; });
    spy.forEach(function (s) { if (s === at) s.a.setAttribute('aria-current', 'location'); else s.a.removeAttribute('aria-current'); });
  }
  function queue() { if (!queued) { queued = true; requestAnimationFrame(paint); } }
  // Reading the scroll position flushes the layout when it is dirty. A scroll event runs before the frame's callbacks, while the layout is
  // still clean, so the position is read there once and the frame uses it: the counters and bars written in the frame cost one layout, not two.
  window.addEventListener('scroll', function () { scrolled = window.scrollY || root.scrollTop || 0; queue(); }, { passive: true });
  window.addEventListener('resize', function () { measure(); });
  if ('ResizeObserver' in window) new ResizeObserver(function () { measure(); }).observe(document.body);
  document.addEventListener('focusin', function (e) { if (nav && nav.contains(e.target)) { nav.classList.remove('away'); root.classList.remove('nav-gone'); } });
  measure(true);
  paint();
  // A link into a pinned chapter, from the site map or the palette, opens it finished: the end of its run, never a start value.
  function arrive() {
    var id = ''; try { id = decodeURIComponent((location.hash || '').slice(1)); } catch (err) { id = ''; }
    var target = id && document.getElementById(id);
    if (!target || !App.film) return;
    pins.forEach(function (pin) { if (!pin.loose && pin.span > 0 && pin.el.contains(target)) window.scrollTo(0, Math.round(pin.top + pin.span - 2)); });
  }
  arrive();
  window.addEventListener('hashchange', function () { requestAnimationFrame(arrive); });

  /* ---------- reveals: a thing comes in once, with a little travel, when it is first seen ---------- */
  var revealIO = null;
  App.reveal = function (el, delay) {
    if (App.reduced || el.classList.contains('reveal')) return;
    el.classList.add('reveal');
    if (delay) el.style.setProperty('--rd', delay + 'ms');
    if (!('IntersectionObserver' in window)) { el.classList.add('in'); return; }
    if (!revealIO) {
      revealIO = new IntersectionObserver(function (entries) {
        entries.forEach(function (e) { if (e.isIntersecting) { e.target.classList.add('in'); revealIO.unobserve(e.target); } });
      }, { rootMargin: '0px 0px -8% 0px' });
    }
    revealIO.observe(el);
  };
  each(all('.section > .section-title, .section > .card, .section > .finding-list, .section > .stats, .section > .table-wrap, .section > .cx-headline, .chapter > .section-title, .best-card, .readkey, .cine, .ch-close .close-copy, .ch-close .close-term, .bench-lead, .block, .scope > .section'), function (el, i) {
    if (el.closest('.pin-stage')) return;
    App.reveal(el, (i % 3) * 60);
  });
  each(all('.kin'), function (el) { if (!App.reduced) App.once(el, function () { el.classList.add('in'); }, 0.2); });

  App.counters = [];

  /* ---------- a magnetic button: near it, the button leans a third of the way toward the pointer ---------- */
  each(all('.magnet'), function (button) {
    var zone = button.parentNode;
    if (!App.fine || App.reduced || !zone) return;
    zone.addEventListener('pointermove', function (e) {
      var box = button.getBoundingClientRect(), dx = e.clientX - (box.left + box.width / 2 - (parseFloat(button.getAttribute('data-tx')) || 0));
      var dy = e.clientY - (box.top + box.height / 2 - (parseFloat(button.getAttribute('data-ty')) || 0));
      var near = Math.abs(dx) < box.width / 2 + 70 && Math.abs(dy) < box.height / 2 + 56;
      var tx = near ? dx * 0.28 : 0, ty = near ? dy * 0.28 : 0;
      button.setAttribute('data-tx', tx.toFixed(1));
      button.setAttribute('data-ty', ty.toFixed(1));
      button.style.translate = near ? tx.toFixed(1) + 'px ' + ty.toFixed(1) + 'px' : '';
    });
    zone.addEventListener('pointerleave', function () { button.style.translate = ''; button.setAttribute('data-tx', 0); button.setAttribute('data-ty', 0); });
  });

  /* ---------- light: a window or a card catches the pointer. Two values that do not inherit, so only the element restyles ---------- */
  App.light = function (el) {
    if (!App.fine || App.reduced) return;
    var raf = 0, ev = null;
    function apply() {
      raf = 0;
      var box = el.getBoundingClientRect();
      el.style.setProperty('--mx', (ev.clientX - box.left).toFixed(1) + 'px');
      el.style.setProperty('--my', (ev.clientY - box.top).toFixed(1) + 'px');
      el.style.setProperty('--lit', 1);
    }
    el.addEventListener('pointermove', function (e) { ev = e; if (!raf) raf = requestAnimationFrame(apply); }, { passive: true });
    el.addEventListener('pointerleave', function () { el.style.setProperty('--lit', 0); });
  };
  each(all('.best-card'), function (el) { App.light(el); });

  /* ---------- blocks that draw: a bar grows once, when its block first comes into view ---------- */
  each(all('.draw'), function (el) {
    if (App.reduced) return;
    if (!('IntersectionObserver' in window)) { el.classList.add('in'); return; }
    App.once(el, function () { el.classList.add('in'); }, 0.25);
  });

  // Last: the pieces that hide something until a script shows it only apply once this line has run.
  if (!App.reduced) root.classList.add('motion-ready');
})();
