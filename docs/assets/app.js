/* Shared behaviour for the four pages: the compact bar, the segmented control, number formats and
   the tooltip. Every page reads without any of it; this only adds the parts that need a script. */
(function () {
  'use strict';
  var App = window.App = {};

  /* ---------- number formats, the same as the ones written into the pages ---------- */
  App.tokens = function (v) {
    if (!(v > 0)) return '0';
    return v >= 1e6 ? (v / 1e6).toFixed(2) + 'M' : Math.round(v / 1e3) + 'k';
  };
  // A share is never rounded up to 100%: 99.6% reads as 99%.
  App.share = function (v) { return v >= 0.9995 ? '100%' : Math.min(99, Math.floor(v * 100 + 0.5)) + '%'; };
  App.of = function (count, total) { return count + ' of ' + total; };
  App.seconds = function (v) { return Math.round(v) + ' s'; };

  /* ---------- the compact bar shows the page title once the large title has scrolled away ---------- */
  var bar = document.getElementById('bar'), title = document.getElementById('title');
  if (bar && title && 'IntersectionObserver' in window) {
    new IntersectionObserver(function (entries) {
      var entry = entries[entries.length - 1];
      bar.classList.toggle('shown', !entry.isIntersecting && entry.boundingClientRect.bottom < 72);
    }, { rootMargin: '-56px 0px 0px 0px' }).observe(title);
  }

  /* ---------- a segmented control: one radio group, arrow keys, selection follows focus ---------- */
  App.segmented = function (root, onChange) {
    var buttons = Array.prototype.slice.call(root.querySelectorAll('[role="radio"]'));
    var current = Math.max(0, buttons.findIndex(function (b) { return b.getAttribute('aria-checked') === 'true'; }));
    function select(i, focus) {
      current = i;
      buttons.forEach(function (b, j) {
        b.setAttribute('aria-checked', j === i ? 'true' : 'false');
        b.tabIndex = j === i ? 0 : -1;
      });
      root.style.setProperty('--i', i);
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
    root.style.setProperty('--i', current);
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
        var line = document.createElement('div'), a = document.createElement('span'), b = document.createElement('span');
        a.textContent = row[0];
        b.textContent = row[1];
        line.appendChild(a);
        line.appendChild(b);
        tip.appendChild(line);
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
})();
