/* The films. Each film is a native video element that loads nothing until it is played; with a script the page's own control bar
   replaces the native one (play, a scrub bar, captions, the quality, full screen) and a large play button sits on the picture. A
   film can list the files it is played from, best first, in data-sources (the 2160p file at 120 frames a second first). The page asks
   the browser which of them it can decode and decode smoothly (navigator.mediaCapabilities.decodingInfo) and plays the first one that
   is smooth, so the 120 fps file is the one played wherever the browser says it plays smoothly. Where none is smooth, it plays the
   least demanding one the browser can decode. All of them are offered as the quality choice, and a file that fails to load hands over
   to the next in that order. A film without a list plays the one file it names. One film plays at a time. */
(function () {
  'use strict';
  var App = window.App;
  var films = document.querySelectorAll('.cine');
  if (!films.length) return;
  var each = function (list, fn) { Array.prototype.forEach.call(list, fn); };
  var players = [];
  var pad = function (n) { return (n < 10 ? '0' : '') + n; };
  var clock = function (s) { s = Math.max(0, Math.floor(s || 0)); return Math.floor(s / 60) + ':' + pad(s % 60); };
  var ICON_PLAY = '<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M7 4.5v15l13-7.5z"/></svg>';

  each(films, function (fig) {
    var video = fig.querySelector('video'), frame = fig.querySelector('.cine-frame');
    if (!video || !frame) return;
    var offered = [];
    try { offered = JSON.parse(video.getAttribute('data-sources') || '[]'); } catch (err) { offered = []; }
    var fallback = video.querySelector('source') ? video.querySelector('source').getAttribute('src') : video.getAttribute('src');
    if (!offered.length && fallback) offered = [{ src: fallback, w: parseInt(video.getAttribute('width'), 10) || 1920, h: parseInt(video.getAttribute('height'), 10) || 1080, fps: 0, type: 'video/mp4', label: '', mb: null }];
    var large = video.getAttribute('data-poster-large'), posterSet = false, current = null, started = false;

    video.removeAttribute('controls');
    video.setAttribute('preload', 'none');
    frame.setAttribute('role', 'group');
    frame.setAttribute('aria-label', video.getAttribute('aria-label') || 'Film');

    /* ---- the control bar ---- */
    var big = document.createElement('button');
    big.type = 'button'; big.className = 'cine-big'; big.setAttribute('aria-label', 'Play the film');
    big.innerHTML = '<span>' + ICON_PLAY + '</span>';
    var bar = document.createElement('div');
    bar.className = 'cine-bar';
    var id = 'q-' + (fig.getAttribute('data-kind') || 'film');
    bar.innerHTML = '<button type="button" data-a="play" aria-label="Play">Play</button>' +
      '<input class="cine-scrub" type="range" min="0" max="1000" value="0" step="1" aria-label="Seek">' +
      '<span class="cine-time" aria-hidden="true">0:00 / 0:00</span>' +
      '<button type="button" data-a="cc" aria-pressed="true" aria-label="Captions">CC</button>' +
      (offered.length > 1 ? '<select class="cine-q" id="' + id + '" aria-label="Quality"></select>' : '') +
      '<button type="button" data-a="full" aria-label="Full screen">Full screen</button>';
    frame.appendChild(big); frame.appendChild(bar);
    var playButton = bar.querySelector('[data-a="play"]'), scrub = bar.querySelector('.cine-scrub'), time = bar.querySelector('.cine-time');
    var cc = bar.querySelector('[data-a="cc"]'), full = bar.querySelector('[data-a="full"]'), select = bar.querySelector('.cine-q');
    var note = fig.querySelector('.cine-meta');
    fig.classList.add('paused');
    // The length is in the page already: the bar shows it before the file has loaded, never a start value.
    var total = parseFloat(video.getAttribute('data-seconds')) || 0;
    if (time && total) time.textContent = '0:00 / ' + clock(total);

    function qualityText(item) {
      return (item.label || (item.h + 'p')) + (item.mb ? ', ' + item.mb + ' MB' : '');
    }
    if (select) offered.forEach(function (item, i) {
      var o = document.createElement('option');
      o.value = String(i); o.textContent = qualityText(item);
      select.appendChild(o);
    });

    function setSource(i, resume) {
      var item = offered[i];
      if (!item || (current && current.src === item.src)) return;
      var at = video.currentTime || 0, keep = resume !== undefined ? resume : (!video.paused && !video.ended);
      current = item;
      stalls = 0;
      each(video.querySelectorAll('source'), function (s) { video.removeChild(s); });
      video.src = item.src;
      if (select) select.value = String(i);
      if (started) {
        video.preload = 'auto';
        video.addEventListener('loadedmetadata', function onMeta() {
          video.removeEventListener('loadedmetadata', onMeta);
          video.currentTime = at;
          if (keep) { var p = video.play(); if (p && p.catch) p.catch(function () { /* the viewer presses play */ }); }
        });
        video.load();
      }
    }
    function sharper() {
      if (posterSet || !large || !current || current.w < 2560) return;
      posterSet = true;
      video.setAttribute('poster', large);
    }

    /* ---- the default file: the best one this browser decodes smoothly ---- */
    function fallbackPick() {
      var want = (frame.clientHeight || window.innerHeight) * (window.devicePixelRatio || 1) * 0.9;
      for (var i = offered.length - 1; i >= 0; i -= 1) if (offered[i].h >= want) return i;   // the smallest file that still fills the frame
      return 0;
    }
    var fallbackIndex = offered.length ? Math.max(0, offered.findIndex(function (item) { return item.src === fallback; })) : 0;
    setSource(fallbackIndex, false);
    // The order the files are tried in: those the browser says play smoothly, in the order offered (the best first), then those it can
    // decode but not smoothly, the least demanding first, then those it said nothing about; the file the page names is always last.
    var verdict = [], ranked = null;
    // The first of them is the file that suits this screen: a screen that shows 100 frames a second or more gets the highest frame rate offered,
    // any other gets a file of 60 or fewer, and of those the smallest file that still fills the frame (the largest when none does). The files are
    // listed best first, so a tie goes to the first. The 120 fps file is therefore the one played wherever the screen can show it and the browser
    // says it decodes it smoothly, and a laptop at 60 Hz does not fetch 316 MB for a frame it shows at a third of the size.
    function rank() {
      var all = offered.map(function (item, i) { return i; });
      var fast = !!(App && App.pace && App.pace.probed && App.pace.period < 10.5);
      var smooth = all.filter(function (i) { return verdict[i] === 'smooth'; });
      var rate = smooth.filter(function (i) { return fast ? offered[i].fps >= 100 : offered[i].fps <= 60; });
      var pool = rate.length ? rate : smooth;
      var want = (frame.clientHeight || window.innerHeight) * (window.devicePixelRatio || 1) * 0.9;
      var fill = pool.filter(function (i) { return offered[i].h >= want; });
      var chosen = fill.length ? fill.reduce(function (a, b) { return offered[b].h < offered[a].h ? b : a; }) : pool[0];
      ranked = (chosen === undefined ? [] : [chosen]).concat(smooth.filter(function (i) { return i !== chosen; }),
        all.filter(function (i) { return verdict[i] === 'supported'; }).reverse(), all.filter(function (i) { return !verdict[i]; }));
      if (ranked.indexOf(fallbackIndex) < 0) ranked.push(fallbackIndex);
      return ranked;
    }
    var caps = navigator.mediaCapabilities;
    if (offered.length > 1 && caps && caps.decodingInfo) {
      var asks = offered.map(function (item) {
        var cfg = { type: 'file', video: { contentType: item.type || 'video/mp4', width: item.w, height: item.h, bitrate: Math.round(((item.mb || 30) * 8e6) / Math.max(1, parseFloat(video.getAttribute('data-seconds')) || 30)), framerate: item.fps || 30 } };
        return caps.decodingInfo(cfg).then(function (r) { return r; }, function () { return null; });
      });
      Promise.all(asks).then(function (results) {
        results.forEach(function (r, i) { verdict[i] = r && r.supported ? (r.smooth ? 'smooth' : 'supported') : (r ? 'no' : ''); });
        if (!started) setSource(rank()[0], false);
        sharper();
      });
    } else if (offered.length > 1) { setSource(fallbackPick(), false); }
    if (select) {
      select.addEventListener('change', function () {
        var item = offered[parseInt(select.value, 10)];
        setSource(parseInt(select.value, 10));
        sharper();
        if (note && item) note.setAttribute('data-q', item.label);
      });
    }

    // A file that fails to load (a release file that is not attached yet, a network failure) hands over to the next file in the order
    // above, and the file the page names comes last; the quality choice follows.
    var tried = {}, stalls = 0;
    // Two stalls while a film plays hand it over to the next lighter file, at the place it had reached.
    video.addEventListener('waiting', function () {
      if (!started || video.seeking || !current) return;
      stalls += 1;
      if (stalls < 2) return;
      var lighter = offered.map(function (item, i) { return i; }).filter(function (i) { return (offered[i].mb || 0) < (current.mb || Infinity) && verdict[i] !== 'no'; })
        .sort(function (a, b) { return (offered[b].mb || 0) - (offered[a].mb || 0); })[0];
      if (lighter === undefined) return;
      stalls = 0;
      setSource(lighter, true);
      if (note) note.setAttribute('data-q', offered[lighter].label);
    });
    video.addEventListener('error', function () {
      if (!current) return;
      tried[current.src] = true;
      var order = (ranked || offered.map(function (item, i) { return i; })).concat([fallbackIndex]);
      var next = order.find(function (i) { return !tried[offered[i].src]; });
      if (next === undefined) return;
      setSource(next, started);
    });

    /* ---- the controls ---- */
    function toggle() {
      if (video.paused || video.ended) {
        if (!started && verdict.length) setSource(rank()[0], false);   // the screen's rate is known by now
        started = true;
        var p = video.play();
        if (p && p.catch) p.catch(function () { /* the viewer presses play again */ });
      } else video.pause();
    }
    big.addEventListener('click', toggle);
    video.addEventListener('click', toggle);
    playButton.addEventListener('click', toggle);
    video.addEventListener('play', function () {
      started = true;
      fig.classList.add('playing'); fig.classList.remove('paused');
      playButton.textContent = 'Pause'; playButton.setAttribute('aria-label', 'Pause'); big.setAttribute('aria-label', 'Pause the film');
      players.forEach(function (other) { if (other.video !== video && !other.video.paused) other.video.pause(); });
    });
    function paused() {
      fig.classList.add('paused');
      playButton.textContent = 'Play'; playButton.setAttribute('aria-label', 'Play'); big.setAttribute('aria-label', 'Play the film');
    }
    video.addEventListener('pause', paused);
    video.addEventListener('ended', function () { fig.classList.remove('playing'); paused(); });
    function tick() {
      var d = video.duration || 0, c = video.currentTime || 0;
      time.textContent = clock(c) + ' / ' + clock(d);
      if (d) { var v = Math.round(c / d * 1000); scrub.value = String(v); scrub.style.setProperty('--played', (v / 10) + '%'); }
    }
    video.addEventListener('timeupdate', tick);
    video.addEventListener('loadedmetadata', tick);
    scrub.addEventListener('input', function () {
      var d = video.duration || 0;
      if (!d) { started = true; video.preload = 'auto'; video.load(); return; }
      video.currentTime = d * parseInt(scrub.value, 10) / 1000;
    });
    cc.addEventListener('click', function () {
      var on = cc.getAttribute('aria-pressed') !== 'true';
      cc.setAttribute('aria-pressed', on ? 'true' : 'false');
      each(video.textTracks, function (track) { track.mode = on ? 'showing' : 'hidden'; });
    });
    full.addEventListener('click', function () {
      if (document.fullscreenElement) document.exitFullscreen();
      else if (frame.requestFullscreen) frame.requestFullscreen();
    });
    frame.addEventListener('keydown', function (e) {
      if (e.target !== frame && e.target !== video) return;
      if (e.key === ' ' || e.key === 'k') { e.preventDefault(); toggle(); }
      else if (e.key === 'ArrowRight') { video.currentTime = Math.min(video.duration || 0, video.currentTime + 5); }
      else if (e.key === 'ArrowLeft') { video.currentTime = Math.max(0, video.currentTime - 5); }
    });
    frame.tabIndex = 0;

    if ('IntersectionObserver' in window && large) {
      new IntersectionObserver(function (entries, io) {
        if (entries.some(function (e) { return e.isIntersecting; })) { sharper(); io.disconnect(); }
      }, { rootMargin: '700px 0px' }).observe(fig);
    } else sharper();
    players.push({ video: video });
  });

  // With a script one film is shown at a time; a control chooses it.
  var chooser = document.querySelector('.reel-pick');
  if (chooser && films.length > 1 && App) {
    chooser.hidden = false;
    var show = function (kind) {
      each(films, function (fig) {
        var on = fig.getAttribute('data-kind') === kind;
        fig.hidden = !on;
        if (!on) { var v = fig.querySelector('video'); if (v && !v.paused) v.pause(); }
      });
    };
    var first = chooser.querySelector('[aria-checked="true"]') || chooser.querySelector('[role="radio"]');
    show(first.getAttribute('data-value'));
    App.segmented(chooser, function (value) { show(value); });
  }
})();
