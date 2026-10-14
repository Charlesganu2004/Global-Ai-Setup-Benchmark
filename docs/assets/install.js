/* The Install page: the file list marks the files for the visitor's system, the Python route shows the command for the tool and
   the wheel chosen, and the terminal prints what the installer prints, line by line, when it comes into view or the system
   changes. Every command is already in the page; without this script the Windows commands show and both logs are written out. */
(function () {
  'use strict';
  var App = window.App;
  if (!App) return;
  var each = function (list, fn) { Array.prototype.forEach.call(list, fn); };
  var os = App.os;

  // The files that fit this system are marked.
  each(document.querySelectorAll('.file[data-os]'), function (row) { row.classList.toggle('mine', !!os && row.getAttribute('data-os') === os); });

  // The Python route: a tool, and the wheel of a system.
  var controls = document.querySelector('.py-controls'), select = document.querySelector('[data-wheel]');
  if (controls && select) {
    controls.hidden = false;
    var tool = 'uv', pref = { windows: 'windows', mac: 'macos', linux: 'linux' }[os];
    if (pref) select.value = pref;
    var lines = document.querySelectorAll('.py-lines .cmd-line');
    var showLines = function () {
      each(lines, function (line) {
        line.hidden = !(line.getAttribute('data-tool') === tool && line.getAttribute('data-sys') === select.value);
      });
    };
    App.segmented(controls.querySelector('.seg'), function (value) { tool = value; showLines(); });
    select.addEventListener('change', showLines);
    showLines();
  }

  // The installer's own output, a line at a time. A line of a system's log arrives in one frame, as it does in a terminal.
  var term = document.querySelector('[data-install-term]');
  if (term && !App.reduced) {
    var play = function () {
      var log = term.querySelector('.log:not([hidden])');
      if (!log) return;
      each(term.querySelectorAll('.log'), function (el) { el.classList.remove('go'); });
      each(log.querySelectorAll('.lg'), function (line, i) { line.style.setProperty('--n', i); });
      void log.offsetWidth;
      log.classList.add('go');
    };
    term.classList.add('live');
    App.once(term, play, 0.4);
    each(document.querySelectorAll('[data-cmdbox] [role="radio"]'), function (button) { button.addEventListener('click', function () { setTimeout(play, 30); }); });
    var again = document.createElement('button');
    again.type = 'button';
    again.className = 'term-replay';
    again.textContent = 'Replay';
    term.querySelector('.term-bar').appendChild(again);
    again.addEventListener('click', play);
  }
})();
