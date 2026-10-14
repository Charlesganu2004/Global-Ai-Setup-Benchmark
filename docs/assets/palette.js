/* The command palette: Ctrl+K, Cmd+K or / opens it. It jumps to any page or section of the site, copies the install command for
   either system, switches the theme, and opens the release and the repository. The list is read from the site map in the foot
   of the page, so a section that is renamed or added is found without an edit here. */
(function () {
  'use strict';
  var App = window.App || {};
  var nav = document.getElementById('nav');
  if (!nav || !window.HTMLDialogElement) return;
  var dlg = null, input = null, list = null, entries = [], shown = [], at = 0;

  function entriesFrom() {
    var out = [];
    var here = location.pathname.split('/').pop() || 'index.html';
    var by = function (name) { return nav.getAttribute(name) || ''; };
    if (by('data-cmd-windows')) out.push({ g: 'Actions', label: 'Copy the install command for Windows', hint: 'PowerShell', copy: by('data-cmd-windows') });
    if (by('data-cmd-unix')) out.push({ g: 'Actions', label: 'Copy the install command for macOS and Linux', hint: 'Terminal', copy: by('data-cmd-unix') });
    out.push({ g: 'Actions', label: 'Switch between the light and the dark theme', hint: 'Theme', run: function () { document.querySelector('[data-theme-toggle]').click(); } });
    if (App.duel) out.push({ g: 'Actions', label: 'Replay the first screen', hint: 'Overview', run: function () { location.hash = ''; window.scrollTo(0, 0); App.duel.replay(); } });
    Array.prototype.forEach.call(document.querySelectorAll('.foot-links a'), function (a) {
      out.push({ g: 'Actions', label: a.textContent === 'Repository' ? 'Open the repository on GitHub' : 'Open the latest release on GitHub', hint: 'github.com', href: a.getAttribute('href') });
    });
    Array.prototype.forEach.call(document.querySelectorAll('.sm-col'), function (col) {
      var page = col.querySelector('.sm-page');
      if (!page) return;
      var file = page.getAttribute('href'), title = page.textContent;
      out.push({ g: 'Pages', label: title, hint: file === here ? 'This page' : 'Page', href: file });
      Array.prototype.forEach.call(col.querySelectorAll('li a'), function (a) {
        out.push({ g: 'Sections', label: a.textContent, hint: title, href: a.getAttribute('href'), here: file === here });
      });
    });
    return out;
  }

  function build() {
    dlg = document.createElement('dialog');
    dlg.className = 'pal';
    dlg.setAttribute('aria-label', 'Command palette');
    dlg.innerHTML = '<div class="pal-top"><svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><circle cx="11" cy="11" r="6.5"/><path d="m16 16 4.5 4.5"/></svg>' +
      '<input class="pal-input" type="text" role="combobox" aria-expanded="true" aria-controls="pal-list" aria-autocomplete="list" autocomplete="off" spellcheck="false" ' +
      'placeholder="Jump to a page or a section, or copy the install command" aria-label="Search the site"></div>' +
      '<ul class="pal-list" id="pal-list" role="listbox" aria-label="Results" tabindex="-1"></ul>' +
      '<div class="pal-foot"><span><kbd>↑</kbd> <kbd>↓</kbd> move</span><span><kbd>Enter</kbd> choose</span><span><kbd>Esc</kbd> close</span></div>';
    document.body.appendChild(dlg);
    input = dlg.querySelector('.pal-input');
    list = dlg.querySelector('.pal-list');
    entries = entriesFrom();
    input.addEventListener('input', function () { at = 0; render(); });
    input.addEventListener('keydown', function (e) {
      if (e.key === 'ArrowDown') { e.preventDefault(); move(1); }
      else if (e.key === 'ArrowUp') { e.preventDefault(); move(-1); }
      else if (e.key === 'Enter') { e.preventDefault(); choose(shown[at]); }
      else if (e.key === 'Home' && input.value === '') { e.preventDefault(); at = 0; paint(); }
      else if (e.key === 'End' && input.value === '') { e.preventDefault(); at = shown.length - 1; paint(); }
    });
    list.addEventListener('click', function (e) {
      var li = e.target.closest('li[data-n]');
      if (li) choose(shown[+li.getAttribute('data-n')]);
    });
    list.addEventListener('pointermove', function (e) {
      var li = e.target.closest('li[data-n]');
      if (li && +li.getAttribute('data-n') !== at) { at = +li.getAttribute('data-n'); paint(); }
    });
    dlg.addEventListener('click', function (e) { if (e.target === dlg) dlg.close(); });
  }

  function words(text) { return text.toLowerCase().split(/\s+/).filter(Boolean); }
  function score(entry, q) {
    var hay = (entry.label + ' ' + entry.hint + ' ' + entry.g).toLowerCase(), total = 0;
    for (var i = 0; i < q.length; i += 1) {
      var found = hay.indexOf(q[i]);
      if (found < 0) return -1;
      total += found === 0 || hay.charAt(found - 1) === ' ' ? 2 : 1;
    }
    return total;
  }
  function mark(text, q) {
    var low = text.toLowerCase(), word = q[0] || '', k = word ? low.indexOf(word) : -1;
    if (k < 0) return escapeText(text);
    return escapeText(text.slice(0, k)) + '<mark>' + escapeText(text.slice(k, k + word.length)) + '</mark>' + escapeText(text.slice(k + word.length));
  }
  function escapeText(text) { return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;'); }

  function render() {
    var q = words(input.value);
    if (!q.length) {
      // With nothing typed: the actions, the pages, and the sections of this page.
      shown = entries.filter(function (e) { return e.g !== 'Sections' || e.here; });
    } else {
      shown = entries.map(function (e) { return { e: e, s: score(e, q) }; }).filter(function (x) { return x.s >= 0; })
        .sort(function (a, b) { return b.s - a.s; }).map(function (x) { return x.e; });
    }
    var html = '', last = '';
    shown.forEach(function (e, n) {
      if (e.g !== last) { html += '<li class="pal-group" role="presentation">' + escapeText(e.g) + '</li>'; last = e.g; }
      html += '<li class="pal-item" role="option" id="pal-' + n + '" data-n="' + n + '"><span>' + mark(e.label, q) + '</span><span class="pal-hint">' + escapeText(e.hint) + '</span></li>';
    });
    list.innerHTML = html || '<li class="pal-empty" role="presentation">Nothing matches. Try a page name, such as Install.</li>';
    paint();
  }
  function paint() {
    var items = list.querySelectorAll('.pal-item');
    Array.prototype.forEach.call(items, function (li, n) { li.setAttribute('aria-selected', n === at ? 'true' : 'false'); });
    if (items[at]) {
      input.setAttribute('aria-activedescendant', items[at].id);
      items[at].scrollIntoView({ block: 'nearest' });
    } else input.removeAttribute('aria-activedescendant');
  }
  function move(step) {
    if (!shown.length) return;
    at = (at + step + shown.length) % shown.length;
    paint();
  }
  function choose(entry) {
    if (!entry) return;
    dlg.close();
    if (entry.copy) {
      App.copy(entry.copy, function (ok) { App.toast(ok ? 'Install command copied' : 'Copy failed. Open the Install page to select it.'); });
    } else if (entry.run) entry.run();
    else if (entry.href) {
      if (/^https?:/.test(entry.href)) window.open(entry.href, '_blank', 'noopener');
      else location.href = entry.href;
    }
  }
  function open() {
    if (!dlg) build();
    input.value = '';
    at = 0;
    render();
    if (!dlg.open) dlg.showModal();
    input.focus();
  }
  App.palette = { open: open };

  document.addEventListener('keydown', function (e) {
    var tag = (e.target && e.target.tagName) || '', typing = /^(INPUT|TEXTAREA|SELECT)$/.test(tag) || (e.target && e.target.isContentEditable);
    if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && (e.key === 'k' || e.key === 'K')) { e.preventDefault(); open(); }
    else if (e.key === '/' && !typing && !e.ctrlKey && !e.metaKey && !e.altKey) { e.preventDefault(); open(); }
  });
  Array.prototype.forEach.call(document.querySelectorAll('[data-palette]'), function (button) { button.addEventListener('click', open); });
})();
