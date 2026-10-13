/* The films on the Overview. Each one is a native player in the page, with captions, that never starts by itself.
   This script gives it the page's own controls: play, a seek bar, mute, captions and full screen, all reachable by
   keyboard. The ad may show a silent, looping preview while it is in view; it has no sound until someone presses
   play, and there is no preview with reduced motion or a data saver. If anything here fails, the native controls stay. */
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
  function button(cls, name, icon) {
    var b = document.createElement('button');
    b.type = 'button';
    b.className = cls;
    b.setAttribute('aria-label', name);
    b.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">' + icon + '</svg>';
    return b;
  }

  each(document.querySelectorAll('.cine'), function (fig, index) {
    var frame = fig.querySelector('.cine-frame'), video = frame ? frame.querySelector('video') : null;
    if (!video || typeof video.play !== 'function') return;
    var name = video.getAttribute('aria-label') || 'the film', track = video.textTracks ? video.textTracks[0] : null;
    var listed = data && data.videos && data.videos[index] ? +data.videos[index].seconds : 0;
    var started = false, preview = false, failed = false, captions = true, idle = 0;
    function length() { return isFinite(video.duration) && video.duration > 0 ? video.duration : listed; }

    var big = button('cine-big', 'Play ' + name, ICON.play);
    var ui = document.createElement('div'), time = document.createElement('span'), seek = document.createElement('input');
    var play = button('cine-btn', 'Play', ICON.play), mute = button('cine-btn', 'Mute', ICON.sound);
    var cc = button('cine-btn', 'Captions', ICON.captions), full = button('cine-btn', 'Full screen', ICON.full);
    var chip = document.createElement('span'), status = document.createElement('p');
    ui.className = 'cine-ui';
    time.className = 'cine-time';
    seek.className = 'cine-seek';
    seek.type = 'range';
    seek.min = '0';
    seek.max = '1000';
    seek.step = '1';
    seek.value = '0';
    seek.setAttribute('aria-label', 'Seek');
    mute.setAttribute('aria-pressed', 'false');
    cc.setAttribute('aria-pressed', 'true');
    full.setAttribute('aria-pressed', 'false');
    if (!track) cc.disabled = true;
    chip.className = 'cine-chip';
    chip.textContent = 'Silent preview';
    status.className = 'sr-only';
    status.setAttribute('role', 'status');
    [play, time, seek, mute, cc, full].forEach(function (n) { ui.appendChild(n); });
    frame.appendChild(chip);
    frame.appendChild(big);
    frame.appendChild(ui);
    frame.appendChild(status);
    video.controls = false;
    video.removeAttribute('controls');

    function icon(b, which) { b.firstChild.innerHTML = ICON[which]; }
    function paint() {
      var total = length(), at = preview ? 0 : video.currentTime || 0, share = total > 0 ? Math.min(1, at / total) : 0;
      seek.value = String(Math.round(share * 1000));
      seek.setAttribute('aria-valuetext', clock(at) + ' of ' + clock(total));
      time.textContent = clock(at) + ' / ' + clock(total);
    }
    function wake() {
      fig.classList.remove('idle');
      clearTimeout(idle);
      idle = setTimeout(function () { fig.classList.add('idle'); }, 2600);
    }
    // Captions sit above the control bar, not under it.
    function lift() { if (track && track.cues) each(track.cues, function (cue) { cue.snapToLines = true; cue.line = -4; }); }
    function showCaptions() { if (track) { track.mode = captions && !preview ? 'showing' : 'hidden'; lift(); } }
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
    function mark(on, said) {
      fig.classList.toggle('playing', on);
      icon(play, on ? 'pause' : 'play');
      play.setAttribute('aria-label', on ? 'Pause' : 'Play');
      status.textContent = said;
    }
    function begin() {
      if (failed) return;
      var live = !video.paused;
      if (preview) {
        preview = false;
        fig.classList.remove('preview');
        video.loop = false;
        try { video.currentTime = 0; } catch (err) { /* not seekable yet */ }
      }
      started = true;
      video.muted = mute.getAttribute('aria-pressed') === 'true';
      showCaptions();
      var run = video.play();
      if (run && run.catch) run.catch(function () { /* the error event reports a film that cannot load */ });
      if (live) mark(true, 'Playing.');   // the preview was already running, so no play event follows
      wake();
    }
    function toggle() { if (!started || preview || video.paused || video.ended) begin(); else video.pause(); }
    function jump(seconds) {
      var total = length();
      if (!started || !(total > 0)) return;
      video.currentTime = Math.max(0, Math.min(total - 0.05, video.currentTime + seconds));
    }
    function fullscreen() {
      var el = document.fullscreenElement || document.webkitFullscreenElement;
      if (el === frame) { (document.exitFullscreen || document.webkitExitFullscreen).call(document); return; }
      if (frame.requestFullscreen) frame.requestFullscreen().catch(function () {});
      else if (frame.webkitRequestFullscreen) frame.webkitRequestFullscreen();
      else if (video.webkitEnterFullscreen) video.webkitEnterFullscreen();
    }

    big.addEventListener('click', begin);
    play.addEventListener('click', toggle);
    video.addEventListener('click', function () { if (fig.classList.contains('idle') && !video.paused) wake(); else toggle(); });
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
    seek.addEventListener('input', function () {
      var total = length();
      if (!(total > 0)) return;
      if (!started) begin();
      try { video.currentTime = seek.value / 1000 * total; } catch (err) { /* not seekable yet */ }
      wake();
    });
    frame.addEventListener('keydown', function (e) {
      var tag = e.target.tagName, key = e.key.toLowerCase();
      if (e.altKey || e.ctrlKey || e.metaKey) return;
      if ((key === ' ' || key === 'k') && tag !== 'BUTTON' && tag !== 'INPUT') toggle();
      else if (key === 'k') toggle();
      else if (key === 'm') mute.click();
      else if (key === 'c' && track) cc.click();
      else if (key === 'f') fullscreen();
      else if (key === 'arrowleft' && tag !== 'INPUT') jump(-5);
      else if (key === 'arrowright' && tag !== 'INPUT') jump(5);
      else return;
      e.preventDefault();
      wake();
    });
    ['pointermove', 'pointerdown', 'focusin'].forEach(function (type) { frame.addEventListener(type, wake); });
    document.addEventListener('fullscreenchange', function () { full.setAttribute('aria-pressed', String(document.fullscreenElement === frame)); });

    video.addEventListener('play', function () { if (!preview) mark(true, 'Playing.'); });
    ['pause', 'ended'].forEach(function (type) {
      video.addEventListener(type, function () {
        if (!preview) mark(false, type === 'ended' ? 'Ended.' : 'Paused.');
      });
    });
    ['timeupdate', 'durationchange', 'seeked'].forEach(function (type) { video.addEventListener(type, paint); });
    video.addEventListener('loadeddata', lift);
    each(video.querySelectorAll('track'), function (t) { t.addEventListener('load', lift); });
    video.addEventListener('error', fail);
    each(video.querySelectorAll('source'), function (s) { s.addEventListener('error', fail); });

    // The silent preview of the ad: only in view, only before anyone has pressed play.
    if (fig.getAttribute('data-kind') === 'ad' && !App.reduced && !saver && 'IntersectionObserver' in window) {
      new IntersectionObserver(function (entries) {
        var inView = entries[entries.length - 1].intersectionRatio >= 0.6;
        if (inView && !started && !preview && !failed && !document.hidden) {
          preview = true;
          fig.classList.add('preview');
          video.muted = true;
          video.loop = true;
          showCaptions();
          var run = video.play();
          if (run && run.catch) run.catch(function () { preview = false; fig.classList.remove('preview'); });
        } else if (!inView && preview) {
          preview = false;
          fig.classList.remove('preview');
          video.loop = false;
          video.pause();
        } else if (!inView && started && !video.paused) {
          video.pause();
        }
      }, { threshold: [0, 0.6] }).observe(frame);
    }
    fig.classList.add('custom');
    paint();
  });
})();
