/* The three pinned chapters of the Overview. Each is a function of its progress p, 0 to 1, which the pin engine in app.js gives it
   from the scroll position: the same scroll always shows the same picture, so it is correct at any refresh rate. A chapter that
   does not pin (reduced motion, a small window, a window too short for the stage) is called once with p = 1, and then every
   inline value is cleared and the page shows its written, finished state. No figure is counted up: every number on a chapter is
   its measured value from the first frame. Nothing starts empty either: every block, row and card is on the stage from the first
   frame in a resting state, and the scroll lights it, lands it and wires it. A stretch of the chapter that has finished, or not
   begun, is not written again. */
(function () {
  'use strict';
  var App = window.App;
  if (!App || !App.pin) return;
  var each = function (list, fn) { Array.prototype.forEach.call(list, fn); };
  var clamp = App.clamp, PI = Math.PI;
  var seg = function (p, a, b) { return clamp((p - a) / (b - a)); };               // progress inside a stretch of the chapter
  var out = function (x) { return 1 - Math.pow(1 - x, 3); };                         // ease out
  var back = function (x) { var c = 1.7; return 1 + (c + 1) * Math.pow(x - 1, 3) + c * Math.pow(x - 1, 2); };   // ease out with a small overshoot
  var bump = function (x) { return Math.sin(PI * x); };                              // 0, up to 1, back to 0
  var rest = function (floor, x) { return (floor + (1 - floor) * x).toFixed(3); };   // from a resting level up to 1
  function clear(el) { el.style.transform = ''; el.style.opacity = ''; }
  // Whether an item needs writing at p: always inside its stretch [a, b], and once on each side of it.
  function gate(item, a, b, p) {
    var side = p < a ? -1 : p > b ? 1 : 0;
    if (side !== 0 && item.side === side) return false;
    item.side = side;
    return true;
  }

  /* ---------- 1. every turn re-reads everything: blocks land, and each landing lights the blocks it reads again ---------- */
  var problem = document.querySelector('.ch-problem');
  if (problem) {
    var stacks = Array.prototype.map.call(problem.querySelectorAll('.stack'), function (stack) {
      return { slabs: Array.prototype.slice.call(stack.querySelectorAll('.slab')), echoes: Array.prototype.slice.call(stack.querySelectorAll('.ec')), col: stack.querySelector('.stack-col') };
    });
    var landing = function (i) { return 0.06 + i * 0.2; };                          // when block i starts to land
    App.pin(problem, function (p) {
      stacks.forEach(function (st) {
        st.slabs.forEach(function (slab, i) {
          if (p >= 1) { clear(slab); st.echoes[i].style.opacity = ''; return; }
          var l = out(seg(p, landing(i), landing(i) + 0.17)), settle = bump(seg(p, landing(i) + 0.09, landing(i) + 0.24));
          slab.style.opacity = rest(0.2, l);
          slab.style.transform = 'translate3d(0,' + (-(1 - l) * 2.2).toFixed(3) + 'rem,0) scale(' + (1 + 0.025 * settle).toFixed(4) + ',' + (1 - 0.07 * settle).toFixed(4) + ')';
          var pulse = 0;
          for (var j = i + 1; j < st.slabs.length; j += 1) pulse = Math.max(pulse, bump(seg(p, landing(j) + 0.02, landing(j) + 0.2)));
          st.echoes[i].style.opacity = pulse.toFixed(3);
        });
        // the stack that finishes first glows once while the other one is still reading
        if (st.col) st.col.style.boxShadow = p < 1 && st.slabs.length < 4 ? '0 14px 30px -18px rgba(var(--accent-rgb), ' + (0.7 * bump(seg(p, 0.3, 0.7))).toFixed(3) + ')' : '';
      });
    });
  }

  /* ---------- 2. what it saves: the ring draws the share, the gauge drains, four rows arrive with their lanes ---------- */
  var saving = document.querySelector('.ch-saving');
  if (saving) {
    var ring = saving.querySelector('.ring-fill'), share = ring ? parseFloat(ring.style.getPropertyValue('--v')) : 0;
    var num = saving.querySelector('.lead-num b'), fill = saving.querySelector('.g-fill'), caps = saving.querySelectorAll('.lead-cap, .lead-range, .lead-kicker, .gauge');
    var gaugeEnd = fill ? parseFloat(fill.style.getPropertyValue('--v')) : 1;
    var rows = Array.prototype.map.call(saving.querySelectorAll('.rl-row'), function (row) {
      return { row: row, bars: Array.prototype.slice.call(row.querySelectorAll('.rl-bar i')), v: Array.prototype.map.call(row.querySelectorAll('.rl-bar i'), function (b) { return parseFloat(b.style.getPropertyValue('--v')); }) };
    });
    rows.forEach(function (r) { r.bars.forEach(function (b) { b.style.transition = 'none'; }); });
    App.pin(saving, function (p) {
      if (p >= 1) {
        if (ring) ring.style.strokeDashoffset = '';
        if (num) clear(num);
        if (fill) fill.style.transform = '';
        each(caps, clear);
        rows.forEach(function (r) { r.side = 1; clear(r.row); r.bars.forEach(function (b, k) { b.style.transform = 'scaleX(' + r.v[k] + ')'; }); });
        return;
      }
      if (ring) ring.style.strokeDashoffset = (1 - share * (0.08 + 0.92 * out(seg(p, 0.04, 0.4)))).toFixed(4);
      if (num) { var n = back(seg(p, 0.3, 0.5)); num.style.opacity = rest(0.35, seg(p, 0.28, 0.4)); num.style.transform = 'scale(' + (0.86 + 0.14 * n).toFixed(4) + ')'; }
      if (fill) fill.style.transform = 'scaleX(' + (1 - (1 - gaugeEnd) * out(seg(p, 0.12, 0.5))).toFixed(4) + ')';
      each(caps, function (el, i) { el.style.opacity = rest(0.4, out(seg(p, 0.02 + i * 0.03, 0.2 + i * 0.03))); });
      rows.forEach(function (r, i) {
        var a = 0.3 + i * 0.12;
        if (!gate(r, a, a + 0.3, p)) return;
        var l = out(seg(p, a, a + 0.14));
        r.row.style.opacity = rest(0.3, l);
        r.row.style.transform = 'translate3d(0,' + ((1 - l) * 1).toFixed(3) + 'rem,0)';
        var grow = out(seg(p, a + 0.05, a + 0.3));
        r.bars.forEach(function (b, k) { b.style.transform = 'scaleX(' + (r.v[k] * (0.1 + 0.9 * grow)).toFixed(4) + ')'; });
      });
    });
  }

  /* ---------- 3. what it does: four capabilities rest on the stage; each is lit in turn and wires a request to its answer ---------- */
  var does = document.querySelector('.ch-does');
  if (does) {
    var cards = Array.prototype.map.call(does.querySelectorAll('.cap'), function (card) {
      return { card: card, wires: Array.prototype.slice.call(card.querySelectorAll('.wire, .fan path')), lit: card.querySelector('.lit'), tok: card.querySelector('.tok'),
        nodes: Array.prototype.slice.call(card.querySelectorAll('.node, .hit')) };
    });
    cards.forEach(function (c) { if (App.light) App.light(c.card); });
    App.pin(does, function (p) {
      cards.forEach(function (c, i) {
        if (p >= 1) {
          c.side = 1;
          clear(c.card);
          c.wires.forEach(function (w) { w.style.strokeDashoffset = ''; });
          if (c.lit) c.lit.style.opacity = '';
          if (c.tok) { c.tok.style.opacity = '0'; c.tok.style.transform = ''; }
          c.nodes.forEach(function (n) { n.style.opacity = ''; });
          return;
        }
        var a = 0.04 + i * 0.2;
        if (!gate(c, a, a + 0.42, p)) return;
        var l = out(seg(p, a, a + 0.2));
        c.card.style.opacity = rest(0.4, l);
        c.card.style.transform = 'translate3d(0,' + ((1 - l) * 1.4).toFixed(3) + 'rem,0)';
        var draw = out(seg(p, a + 0.06, a + 0.34));
        c.wires.forEach(function (w) { w.style.strokeDashoffset = (1 - (0.3 + 0.7 * draw)).toFixed(4); });
        c.nodes.forEach(function (n) { n.style.opacity = (0.3 + 0.7 * l).toFixed(3); });
        if (c.lit) c.lit.style.opacity = rest(0.15, out(seg(p, a + 0.2, a + 0.4)));
        if (c.tok) {
          var run = seg(p, a + 0.1, a + 0.34);
          c.tok.style.transform = 'translateX(' + (50 * out(run)).toFixed(2) + 'px)';   // user units of the figure; the circle starts at its cx
          c.tok.style.opacity = bump(run).toFixed(3);
        }
      });
    });
  }
})();
