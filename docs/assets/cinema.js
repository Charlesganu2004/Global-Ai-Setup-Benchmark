/* The films on the Overview. Each is a native player in the page, with captions, that never starts with sound by itself.
   This script gives every film the same skin: a bar under the picture with labelled controls (play or pause, a seek bar,
   mute, captions, full screen), all reachable by keyboard and always visible, so nothing is hidden while it can take focus.
   The ad may show one silent pass in the page while it is in view; a Pause control is always there, a click on the picture
   pauses it, and there is no preview on a phone, with reduced motion, with a data saver, or after motion has been paused on
   the stage.
   A control under the heading chooses which film is shown. If anything here fails, the native controls stay. */
(function () {
  'use strict';
  var App = window.App, data = window.SHOWCASE;
  if (!App) return;
  var each = function (list, fn) { Array.prototype.forEach.call(list, fn); };
  var ICON = {
    play: '<path class="solid" d="M8 5v14l11-7z"/>',
    pause: '<path class="solid" d="M7 5h3.6v14H7zm6.4 0H17v14h-3.6z"/>',
    sound: '<path class="solid" d="M4 9.6v4.8h3.4l4.6 3.8V5.8L7.4 9.6z"/><path d="M15.4 9.2a4 4 0 0 1 0 5.6m2.6-8.2a7.6 7.6 0 0 1 0 10.8"/>',
    muted: '<path class="solid" d="M4 9.6v4.8h3.4l4.6 3.8V5.8L7.4 9.6z"/><path d="m15.6 9.6 4.8 4.8m0-4.8-4.8 4.8"/>',
    captions: '<rect x="3.5" y="6" width="17" height="12" rx="2.5"/><path d="M7.4 11h3m2.6 0h3.6m-9.2 3.4h5.4"/>',
    full: '<path d="M4 9.2V4.6h4.6m11.4 4.6V4.6h-4.6M4 14.8v4.6h4.6m11.4-4.6v4.6h-4.6"/>'
  };
  var saver = !!(navigator.connection && navigator.connection.saveData);
  function clock(t) { t = Math.max(0, Math.floor(t || 0)); return Math.floor(t / 60) + ':' + ('0' + t % 60).slice(-2); }
  function button(cls, label, icon, said) {
    var b = document.createElement('button'), t = document.createElement('span');
    b.type = 'button';
    b.className = cls;
    b.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">' + icon + '</svg>';
    t.className = 't';
    t.textContent = label;
    b.appendChild(t);
    if (said) b.setAttribute('aria-label', said);
    return b;
  }
  function text(b, label) { b.lastChild.textContent = label; }

  // The posters are decoded while the page is idle, so the first sight of a film does not cost a frame.
  (window.requestIdleCallback || function (f) { setTimeout(f, 1200); })(function () {
    each(document.querySelectorAll('.cine video[poster]'), function (v) {
      var im = new Image();
      im.src = v.getAttribute('poster');
      if (im.decode) im.decode().catch(function () { /* decoded when seen */ });
    });
  });

  var players = [];
  each(document.querySelectorAll('.cine'), function (fig, index) {
    var frame = fig.querySelector('.cine-frame'), video = frame ? frame.querySelector('video') : null;
    if (!video || typeof video.play !== 'function') return;
    var name = video.getAttribute('aria-label') || 'the film', track = video.textTracks ? video.textTracks[0] : null;
    var listed = data && data.videos && data.videos[index] ? +data.videos[index].seconds : 0;
    var started = false, preview = false, failed = false, captions = true;
    function length() { return isFinite(video.duration) && video.duration > 0 ? video.duration : listed; }

    var big = button('cine-big', 'Play with sound', ICON.play, 'Play ' + name + ' with sound');
    var ui = document.createElement('div'), time = document.createElement('span'), seek = document.createElement('input');
    var play = button('cine-btn', 'Play', ICON.play), mute = button('cine-btn', 'Mute', ICON.sound);
    var cc = button('cine-btn', 'Captions', ICON.captions), full = button('cine-btn', 'Full screen', ICON.full);
    var chip = document.createElement('p'), status = document.createElement('p'), cap = fig.querySelector('.cine-cap');
    ui.className = 'cine-bar';
    ui.setAttribute('role', 'group');
    ui.setAttribute('aria-label', name + ', controls');
    time.className = 'cine-time';
    seek.className = 'cine-seek';
    seek.type = 'range';
    seek.min = '0';
    seek.max = '1000';
    seek.step = '1';
    seek.value = '0';
    seek.setAttribute('aria-label', 'Seek in ' + name);
    mute.setAttribute('aria-pressed', 'false');
    cc.setAttribute('aria-pressed', 'true');
    if (!track) cc.disabled = true;
    chip.className = 'cine-chip';
    chip.textContent = 'This is a silent preview. Press Play with sound to watch it from the start.';
    chip.hidden = true;
    status.className = 'sr-only';
    status.setAttribute('role', 'status');
    [seek, play, time, mute, cc, full].forEach(function (n) { ui.appendChild(n); });
    frame.appendChild(big);
    frame.parentNode.insertBefore(ui, cap || frame.nextSibling);
    if (cap) cap.appendChild(chip);
    fig.appendChild(status);
    video.controls = false;
    video.removeAttribute('controls');

    function icon(b, which) { b.firstChild.innerHTML = ICON[which]; }
    function paint() {
      var total = length(), at = video.currentTime || 0, share = total > 0 ? Math.min(1, at / total) : 0;
      seek.value = String(Math.round(share * 1000));
      seek.setAttribute('aria-valuetext', clock(at) + ' of ' + clock(total));
      time.textContent = clock(at) + ' / ' + clock(total);
    }
    function showCaptions() { if (track) track.mode = captions && !preview ? 'showing' : 'hidden'; }
    function fail() {
      if (failed) return;
      failed = true;
      preview = false;
      fig.classList.remove('preview', 'playing');
      var note = document.createElement('p');
      note.className = 'cine-note';
      note.setAttribute('role', 'alert');
      note.textContent = 'This film could not be loaded. Reload the page to try again.';
      frame.appendChild(note);
      [big, play, seek, mute, cc, full].forEach(function (n) { n.disabled = true; });
      big.hidden = true;
    }
    // The state of the play control: it says what pressing it does.
    function mark(on, said) {
      fig.classList.toggle('playing', on && !preview);
      icon(play, on ? 'pause' : 'play');
      text(play, on ? 'Pause' : 'Play');
      chip.hidden = !preview;
      if (said) status.textContent = said;
    }
    function endPreview() {
      preview = false;
      fig.classList.remove('preview');
      video.loop = false;
      showCaptions();
    }
    function begin() {
      if (failed) return;
      if (preview) {
        endPreview();
        try { video.currentTime = 0; } catch (err) { /* not seekable yet */ }
      }
      started = true;
      video.muted = mute.getAttribute('aria-pressed') === 'true';
      showCaptions();
      var run = video.play();
      if (run && run.catch) run.catch(function () { /* the error event reports a film that cannot load */ });
      mark(true, 'Playing with sound.');
    }
    function toggle() {
      if (preview && !video.paused) { endPreview(); video.pause(); mark(false, 'Preview paused.'); return; }
      if (!started || preview || video.paused || video.ended) begin(); else video.pause();
    }
    function jump(seconds) {
      var total = length();
      if (!started || !(total > 0)) return;
      video.currentTime = Math.max(0, Math.min(total - 0.05, video.currentTime + seconds));
    }
    function fullscreen() {
      var el = document.fullscreenElement || document.webkitFullscreenElement;
      if (el === fig) { (document.exitFullscreen || document.webkitExitFullscreen).call(document); return; }
      if (fig.requestFullscreen) fig.requestFullscreen().catch(function () {});
      else if (fig.webkitRequestFullscreen) fig.webkitRequestFullscreen();
      else if (video.webkitEnterFullscreen) video.webkitEnterFullscreen();
    }

    big.addEventListener('click', function () { begin(); play.focus(); });
    play.addEventListener('click', toggle);
    video.addEventListener('click', toggle);
    mute.addEventListener('click', function () {
      var on = mute.getAttribute('aria-pressed') !== 'true';
      mute.setAttribute('aria-pressed', String(on));
      icon(mute, on ? 'muted' : 'sound');
      if (!preview) video.muted = on;
    });
    cc.addEventListener('click', function () {
      captions = !captions;
      cc.setAttribute('aria-pressed', String(captions));
      showCaptions();
      status.textContent = captions ? 'Captions on.' : 'Captions off.';
    });
    full.addEventListener('click', fullscreen);
    // Dragging the bar starts the film; the keys on it never do. The arrows step by five seconds, by one with Shift.
    seek.addEventListener('input', function () {
      var total = length();
      if (!(total > 0)) return;
      if (!started) { if (seek._key) { paint(); return; } begin(); }
      try { video.currentTime = seek.value / 1000 * total; } catch (err) { /* not seekable yet */ }
    });
    seek.addEventListener('pointerdown', function () { seek._key = false; });
    seek.addEventListener('keyup', function () { seek._key = false; });
    seek.addEventListener('keydown', function (e) {
      seek._key = true;
      if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
      e.preventDefault();
      jump((e.key === 'ArrowRight' ? 1 : -1) * (e.shiftKey ? 1 : 5));
    });
    fig.addEventListener('keydown', function (e) {
      var tag = e.target.tagName, key = e.key.toLowerCase();
      if (e.altKey || e.ctrlKey || e.metaKey || tag === 'A') return;
      if ((key === ' ' && tag !== 'BUTTON' && tag !== 'INPUT') || key === 'k') toggle();
      else if (key === 'm') mute.click();
      else if (key === 'c' && track) cc.click();
      else if (key === 'f') fullscreen();
      else if (key === 'arrowleft' && tag !== 'INPUT') jump(-5);
      else if (key === 'arrowright' && tag !== 'INPUT') jump(5);
      else return;
      e.preventDefault();
    });
    document.addEventListener('fullscreenchange', function () { full.setAttribute('aria-pressed', String(document.fullscreenElement === fig)); });

    video.addEventListener('play', function () { mark(true, preview ? 'Silent preview playing.' : ''); });
    video.addEventListener('pause', function () { if (!video.ended) mark(false, preview ? '' : 'Paused.'); });
    video.addEventListener('ended', function () {
      if (preview) {
        // The preview plays once and returns to the picture it began with.
        endPreview();
        try { video.currentTime = 0; } catch (err) { /* not seekable */ }
        mark(false, '');
      } else mark(false, 'Ended.');
    });
    ['timeupdate', 'durationchange', 'seeked'].forEach(function (type) { video.addEventListener(type, paint); });
    video.addEventListener('error', fail);
    each(video.querySelectorAll('source'), function (s) { s.addEventListener('error', fail); });

    // The silent preview of the ad: once, only in view, only before anyone has pressed play.
    function startPreview() {
      if (started || preview || failed || document.hidden || App.paused || fig.hidden) return;
      preview = true;
      fig.classList.add('preview');
      video.muted = true;
      video.loop = false;
      showCaptions();
      var run = video.play();
      if (run && run.catch) run.catch(function () { endPreview(); mark(false, ''); });
    }
    var tried = false, waiting = 0;
    var roomy = window.matchMedia && matchMedia('(min-width: 700px)').matches;
    if (fig.getAttribute('data-kind') === 'ad' && roomy && !App.reduced && !saver && 'IntersectionObserver' in window) {
      new IntersectionObserver(function (entries) {
        var inView = entries[entries.length - 1].intersectionRatio >= 0.6;
        // The preview waits until the picture has been in view for most of a second, so scrolling past does not start it.
        if (inView && !tried) { clearTimeout(waiting); waiting = setTimeout(function () { tried = true; startPreview(); }, 900); }
        else if (!inView) { clearTimeout(waiting); }
        if (!inView && preview) { endPreview(); video.pause(); try { video.currentTime = 0; } catch (err) { /* not seekable */ } mark(false, ''); }
        else if (!inView && started && !video.paused) video.pause();
      }, { threshold: [0, 0.6] }).observe(frame);
    }
    document.addEventListener('pgr-motion', function (e) { if (e.detail && e.detail.paused && preview) { endPreview(); video.pause(); mark(false, 'Preview paused.'); } });
    fig.classList.add('custom');
    players.push({ fig: fig, stop: function () { if (preview) { endPreview(); } if (!video.paused) video.pause(); } });
    paint();
  });

  // With two films, one is shown at a time, both at the same size, and a control under the heading chooses.
  var pick = document.querySelector('.reel-pick');
  if (pick && players.length > 1 && App.segmented) {
    pick.hidden = false;
    var show = function (kind) {
      players.forEach(function (pl) {
        var on = pl.fig.getAttribute('data-kind') === kind;
        if (!on) pl.stop();
        pl.fig.hidden = !on;
      });
    };
    App.segmented(pick, function (kind) { show(kind); });
    show(pick.querySelector('[aria-checked="true"]').getAttribute('data-value'));
  }
})();
