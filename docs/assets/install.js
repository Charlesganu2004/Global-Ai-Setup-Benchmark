/* The install page: when the package is public, the command gets a copy button. The command itself is
   already in the page; without this script it can still be selected and copied by hand. */
(function () {
  'use strict';
  var data = window.SHOWCASE, code = document.getElementById('install-command'), row = document.querySelector('.copy-row');
  if (!data || !data.install || data.install.status !== 'public' || !code || !row || !data.install.command) return;

  var button = document.createElement('button'), status = document.createElement('span');
  button.type = 'button';
  button.className = 'btn';
  button.textContent = 'Copy command';
  status.className = 'btn-status';
  status.setAttribute('role', 'status');
  row.appendChild(button);
  row.appendChild(status);

  function fallback(text) {
    var area = document.createElement('textarea');
    area.value = text;
    area.setAttribute('readonly', '');
    area.style.position = 'fixed';
    area.style.opacity = '0';
    document.body.appendChild(area);
    area.select();
    var ok = false;
    try { ok = document.execCommand('copy'); } catch (err) { ok = false; }
    document.body.removeChild(area);
    return ok;
  }
  button.addEventListener('click', function () {
    var text = data.install.command;
    var done = function (ok) { status.textContent = ok ? 'Copied.' : 'Copy failed. Select the command and copy it by hand.'; };
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(function () { done(true); }, function () { done(fallback(text)); });
    } else {
      done(fallback(text));
    }
  });
})();
