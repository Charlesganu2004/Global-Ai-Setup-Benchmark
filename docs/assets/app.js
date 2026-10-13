/* Shared behaviour for the four pages: number formats, the motion helpers, the compact bar, the reading
   progress line, counting numbers, pointer light and tilt, the segmented control and the tooltip.
   Every page reads without any of it; this only adds the parts that need a script. */
(function () {
  'use strict';
  var App = window.App = {};
  var root = document.documentElement;
  var each = function (list, fn) { Array.prototype.forEach.call(list, fn); };

  /* ---------- motion: reduced motion is read live, and every helper ends in the final state ---------- */
  var mq = window.matchMedia ? window.matchMedia('(prefers-reduced-motion: reduce)') : null;
  App.reduced = !!(mq && mq.matches);
  if (mq && mq.addEventListener) mq.addEventListener('change', function (e) { App.reduced = e.matches; });
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

  // The best configuration and the control on the light class, the same pair the pages are written from.
  App.headline = function (data) {
    var best = null, control = null;
    ((data && data.configs) || []).forEach(function (c) {
      if (c.best) best = c;
      if (c.family === 'control' && !control) control = c;
    });
    function light(c) { return c ? (c.points.filter(function (p) { return p.class === 'light'; })[0] || null) : null; }
    return best && light(best) ? { best: best, control: control, b: light(best), c: light(control) } : null;
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
  each(document.querySelectorAll('.section > .section-title, .section > .card:not(.tilt), .section > .figures, .section > .finding-list, .section > .video-frame, .stream, .foot'), App.reveal);

  /* ---------- the compact bar shows the page title once the large title has scrolled away ---------- */
  var bar = document.getElementById('bar'), title = document.getElementById('title');
  if (bar && title && 'IntersectionObserver' in window) {
    new IntersectionObserver(function (entries) {
      var entry = entries[entries.length - 1];
      bar.classList.toggle('shown', !entry.isIntersecting && entry.boundingClientRect.bottom < 72);
    }, { rootMargin: '-56px 0px 0px 0px' }).observe(title);
  }

  /* ---------- the reading progress line, and the section the reader is in (one pass per frame) ---------- */
  var line = document.createElement('div'), spy = [], queued = false;
  line.className = 'progress';
  line.setAttribute('aria-hidden', 'true');
  document.body.appendChild(line);
  each(document.querySelectorAll('.side-sections a'), function (a) {
    var h = document.getElementById(a.getAttribute('href').slice(1));
    if (h) spy.push({ a: a, h: h });
  });
  function paint() {
    queued = false;
    var room = root.scrollHeight - root.clientHeight;
    line.style.transform = 'scaleX(' + (room > 0 ? Math.min(1, root.scrollTop / room) : 0).toFixed(4) + ')';
    var at = null, limit = window.innerHeight * 0.35;
    spy.forEach(function (s) { if (s.h.getBoundingClientRect().top <= limit) at = s; });
    spy.forEach(function (s) {
      if (s === at) s.a.setAttribute('aria-current', 'location'); else s.a.removeAttribute('aria-current');
    });
  }
  function queue() { if (!queued) { queued = true; requestAnimationFrame(paint); } }
  window.addEventListener('scroll', queue, { passive: true });
  window.addEventListener('resize', queue);
  paint();

  /* ---------- numbers that count from the no-index value to the best one when they come into view ---------- */
  // On first load the counts start with the lane of the best configuration and take the same time, so both settle together.
  var lead = window.performance && performance.now() < 2500 ? 450 : 0;
  each(document.querySelectorAll('[data-count]'), function (el, i) {
    var kind = el.getAttribute('data-count'), final = el.textContent;
    var from = parseFloat(el.getAttribute('data-from')), to = parseFloat(el.getAttribute('data-to'));
    var fromText = el.getAttribute('data-from-text');
    var fmt = kind === 'tokens' ? App.tokens : kind === 'turns' ? App.turns : kind === 'share' ? App.share : null;
    if (App.reduced || (!fmt && fromText == null)) { el.classList.add('in'); return; }
    el.textContent = fmt ? fmt(from) : fromText;
    el.classList.add('in');
    App.once(el, function () {
      setTimeout(function () {
        if (fmt) { App.count(el, from, to, fmt, 1500, final); return; }
        // A count such as "4 of 5" to "3 of 3" cannot pass through states that were never measured: it swaps once.
        App.run(700, function (p) { el.textContent = p >= 0.55 ? final : fromText; });
      }, lead + i * 60);
    }, 0.5);
  });

  /* ---------- pointer light on cards, and a slight tilt on the best one (fine pointers only) ---------- */
  var fine = !!(window.matchMedia && matchMedia('(hover: hover) and (pointer: fine)').matches);
  each(document.querySelectorAll('.spot'), function (card) {
    if (App.reduced) { card.classList.add('lit'); return; }
    if (!fine) return;
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

  /* ---------- the indicator glides on its own where the browser has no page transitions ---------- */
  (function () {
    var tab = document.querySelector('.tab-ind'), side = document.querySelector('.side-ind');
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
      onChange(buttons[i].getAttribute('data-class'), buttons[i].textContent, i);
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
