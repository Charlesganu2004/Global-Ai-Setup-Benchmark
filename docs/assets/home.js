/* The first screen of the Overview: two terminal windows replay one session, an agent with no index and an agent with pgr. The
   turns and the tokens are the measured means of the headline cell; the page is written finished, and this script hides the turns
   and plays them in order, once, then rests on the finished frame (Replay plays it again). The replay is a function of one clock
   that counts elapsed milliseconds, so it is the same at 60, 120 and 144 Hz and can be paused and replayed without drifting. A
   layer of small elements, each moved by one Web Animation on the compositor, carries the tokens from a command to the context meter
   (a canvas redrawn every frame cost the page a quarter of its frames). The pointer lights the
   windows and moves them a little against each other, in whole device pixels so the text stays sharp. The frame loop runs only
   while something moves: when the page rests, nothing runs. Under reduced motion none of this runs and the page keeps its end. */
(function () {
  'use strict';
  var App = window.App, duel = document.getElementById('duel');
  if (!App || !duel || App.reduced) return;
  var scene = duel.closest('.scene');
  if (!scene) return;
  var each = function (list, fn) { Array.prototype.forEach.call(list, fn); };

  var TURN = 1800, START = 250, TYPE = 22, LINE = 85, GROW = 1000, ENTER = 1500;

  function Windowed(el) {
    this.el = el;
    this.ref = el.classList.contains('term-ref');
    this.turns = Array.prototype.map.call(el.querySelectorAll('.turn'), function (li) {
      var cmd = li.querySelector('.turn-cmd');
      return { li: li, cmd: cmd, full: cmd.textContent, outs: Array.prototype.slice.call(li.querySelectorAll('.ol')), on: false, typed: -1, lines: -1, k: -1, emitted: -1 };
    });
    this.n = this.turns.length;
    this.segs = Array.prototype.slice.call(el.querySelectorAll('.blk'));
    this.tok = el.querySelector('.tok');
    this.total = parseFloat(this.tok.getAttribute('data-to'));
    this.per = this.total / this.n;
    this.state = el.querySelector('.term-state');
    this.turnsLabel = el.querySelector('.meter-turns');
    this.done = this.state.getAttribute('data-done');
    this.finalTok = this.tok.textContent;
    this.finalTurns = this.turnsLabel.textContent;
    this.shown = { tok: '', turns: '', state: '' };
    this.begin = function (i) { return START + i * TURN; };
    this.out = function (i) { return this.turns[i].full.length * TYPE + 140; };
    this.end = this.begin(this.n - 1) + this.out(this.n - 1) + GROW;
  }
  Windowed.prototype.render = function (t) {
    var started = 0, sum = 0, last = this.turns[this.n - 1];
    for (var i = 0; i < this.n; i += 1) {
      var turn = this.turns[i], lt = t - this.begin(i), out = this.out(i);
      var on = lt >= 0, typed = on ? Math.min(turn.full.length, Math.floor(lt / TYPE) + 1) : 0;
      var k = App.ease(Math.max(0, Math.min(1, (lt - out) / GROW)));
      var lines = on ? Math.max(0, Math.floor((lt - out) / LINE) + 1) : 0;
      if (on) started += 1;
      sum += this.per * k;
      if (on !== turn.on) { turn.on = on; turn.li.classList.toggle('on', on); }
      if (typed !== turn.typed) { turn.typed = typed; turn.cmd.textContent = on ? turn.full.slice(0, typed) : turn.full; }
      if (lines !== turn.lines) { turn.lines = lines; turn.outs.forEach(function (ol, j) { ol.classList.toggle('on', j < lines); }); }
      if (k !== turn.k) { turn.k = k; this.segs[i].style.transform = 'scaleX(' + k.toFixed(4) + ')'; }
    }
    var finished = t >= this.end - 40 && last.k >= 1;
    var tok = finished ? this.finalTok : App.tokens(sum);
    var turns = finished ? this.finalTurns : started + (started === 1 ? ' turn' : ' turns');
    var state = finished ? this.done : (t < START ? 'ready' : 'working');
    if (tok !== this.shown.tok) { this.shown.tok = tok; this.tok.textContent = tok; }
    if (turns !== this.shown.turns) { this.shown.turns = turns; this.turnsLabel.textContent = turns; }
    if (state !== this.shown.state) {
      this.shown.state = state;
      this.state.textContent = state;
      this.state.classList.toggle('busy', state === 'working');
      this.state.classList.toggle('fin', finished);
    }
    return finished;
  };

  var wins = [];
  try { each(duel.querySelectorAll('.term'), function (el) { wins.push(new Windowed(el)); }); } catch (err) { return; }
  if (wins.length !== 2) return;
  var delta = duel.querySelector('.duel-delta');
  var refEnd = Math.max.apply(null, wins.map(function (w) { return w.end; })), settle = refEnd + 350;

  /* ---------- the controls: Pause and Replay, in the foot of the duel ---------- */
  var ctl = duel.querySelector('.duel-ctl');
  if (!ctl) { ctl = document.createElement('div'); ctl.className = 'duel-ctl'; duel.appendChild(ctl); }
  ctl.hidden = false;
  ctl.innerHTML = '<button type="button" data-act="pause" aria-pressed="false">Pause</button><button type="button" data-act="replay">Replay</button>';
  var pauseButton = ctl.querySelector('[data-act="pause"]');

  /* ---------- state ---------- */
  var t = -ENTER, playing = true, visible = true, finished = false, lastNow = 0, raf = 0;
  var fx = document.createElement('div'), root = document.documentElement, alive = 0, pending = 0;
  fx.className = 'scene-fx'; fx.setAttribute('aria-hidden', 'true');
  scene.appendChild(fx);
  var accentRgb = '255,122,61';
  function readAccent() { accentRgb = getComputedStyle(root).getPropertyValue('--accent-rgb').trim().replace(/\s+/g, '') || '255,122,61'; }
  readAccent();
  document.addEventListener('themechange', readAccent);

  // A token leaves a command and arrives at the context meter. Each is one small element with one Web Animation: the browser runs
  // it on the compositor from the clock, so it costs the page no frames and is the same at any refresh rate. Where a token starts and
  // ends is read with the other reads of the frame, before anything is written (locate), and the element is made after the writes
  // (launch), so a token never forces a layout. The number of tokens in the air follows the pace.
  function locate(win, i) {
    if (alive + pending >= Math.max(1, Math.round(4 * App.pace.scale)) || App.pace.level >= 3) return null;
    pending += 1;
    var turn = win.turns[i], box = turn.cmd.getBoundingClientRect(), at = scene.getBoundingClientRect(), seg = win.segs[i].getBoundingClientRect();
    return { win: win, x0: box.left + Math.min(box.width, 40) - at.left, y0: box.top + box.height / 2 - at.top, x1: seg.right - at.left, y1: seg.top + seg.height / 2 - at.top };
  }
  function launch(g) {
    var win = g.win, x0 = g.x0, y0 = g.y0, x1 = g.x1, y1 = g.y1;
    pending -= 1;
    var bend = (Math.random() - 0.5) * 60, r = 2.6 + Math.random() * 2.2, d = 520 + Math.random() * 280, keys = [];
    for (var s = 0; s <= 6; s += 1) {
      var u = s / 6, e = 1 - Math.pow(1 - u, 3), arc = Math.sin(e * Math.PI);
      keys.push({ offset: u, opacity: Math.min(1, u * 6) * (1 - u * 0.35), transform: 'translate3d(' + (x0 + (x1 - x0) * e + arc * bend - r).toFixed(1) + 'px,' + (y0 + (y1 - y0) * e - arc * 18 - r).toFixed(1) + 'px,0)' });
    }
    var el = document.createElement('i'), core = win.ref ? '213,218,226' : accentRgb;
    el.style.cssText = 'position:absolute;left:0;top:0;width:' + (r * 5).toFixed(1) + 'px;height:' + (r * 5).toFixed(1) + 'px;margin:' + (-r * 1.5).toFixed(1) + 'px 0 0 ' + (-r * 1.5).toFixed(1) + 'px;border-radius:50%;opacity:0;will-change:transform,opacity;' +
      'background:radial-gradient(circle closest-side,rgba(' + core + ',1) 0,rgba(' + core + ',1) 26%,rgba(' + core + ',.32) 44%,rgba(' + core + ',0) 100%)';
    fx.appendChild(el);
    alive += 1;
    var run = el.animate(keys, { duration: d, easing: 'linear', fill: 'both' });
    run.onfinish = function () { alive -= 1; if (el.parentNode) el.parentNode.removeChild(el); };
  }

  /* ---------- the pointer: a light that follows it, the two windows shifted against each other ---------- */
  var aim = { x: 0, y: 0, nx: 0, ny: 0 }, cur = { x: 0, y: 0, nx: 0, ny: 0 }, pointerSeen = false;
  var glow = scene.querySelector('.glow'), terms = wins.map(function (w) { return w.el; });
  if (App.fine) {
    scene.addEventListener('pointermove', function (e) {
      var box = scene.getBoundingClientRect();
      aim.x = e.clientX - box.left; aim.y = e.clientY - box.top;
      aim.nx = (aim.x / box.width - 0.5) * 2; aim.ny = (aim.y / box.height - 0.5) * 2;
      if (!pointerSeen) { cur.x = aim.x; cur.y = aim.y; pointerSeen = true; }
      kick();
    }, { passive: true });
    scene.addEventListener('pointerleave', function () { aim.nx = 0; aim.ny = 0; kick(); });
    terms.forEach(function (el) { App.light(el); });
  }
  var snap = function (v) { var d = window.devicePixelRatio || 1; return Math.round(v * d) / d; };
  function follow(dt) {
    var f = 1 - Math.pow(0.0015, dt / 1000), moved = false;
    var dx = aim.x - cur.x, dy = aim.y - cur.y, dnx = aim.nx - cur.nx, dny = aim.ny - cur.ny;
    if (Math.abs(dx) + Math.abs(dy) > 0.4 && pointerSeen) { cur.x += dx * f; cur.y += dy * f; moved = true; if (glow) glow.style.translate = cur.x.toFixed(1) + 'px ' + cur.y.toFixed(1) + 'px'; }
    if (App.pace.level >= 2) { dnx = -cur.nx; dny = -cur.ny; }   // frames are missing their slot: the windows hold still
    if (Math.abs(dnx) + Math.abs(dny) > 0.002) {
      cur.nx += dnx * f; cur.ny += dny * f; moved = true;
      terms[0].style.transform = 'translate3d(' + snap(cur.nx * -7) + 'px,' + snap(cur.ny * -4) + 'px,0)';
      terms[1].style.transform = 'translate3d(' + snap(cur.nx * 7) + 'px,' + snap(cur.ny * 4) + 'px,0)';
    }
    return moved;
  }

  /* ---------- the clock ---------- */
  function reset() {
    t = -300; finished = false;
    while (fx.firstChild) fx.removeChild(fx.firstChild);
    alive = 0;
    wins.forEach(function (w) { w.turns.forEach(function (turn) { turn.emitted = -1; }); w.render(0); });
    if (delta) delta.classList.remove('on');
    scene.classList.remove('settled');
  }
  function step(now, dt) {
    if (playing && visible && !finished) {
      var before = t, launches = [];
      t += dt;
      var at = Math.max(0, t);
      // The reads of the frame first, then every write: while a turn's output arrives, tokens leave its command for the meter, one every 260 ms.
      wins.forEach(function (w) {
        for (var i = 0; i < w.n; i += 1) {
          var lt = at - w.begin(i) - w.out(i);
          if (lt >= 0 && lt < GROW && t > before) {
            var due = Math.floor(lt / 260), turn = w.turns[i];
            while (turn.emitted < due) { turn.emitted += 1; var g = locate(w, i); if (g) launches.push(g); }
          }
        }
      });
      wins.forEach(function (w) { w.render(at); });
      launches.forEach(launch);
      if (t >= settle) {
        finished = true;
        wins.forEach(function (w) { w.render(refEnd + 10000); });
        if (delta) delta.classList.add('on');
        scene.classList.add('settled');
      }
    }
  }
  function frame(now) {
    raf = 0;
    App.pace.tick(now);
    // The clock is elapsed time: a slow frame moves the replay on by what it took. Only a gap this long, a page that was in the
    // background for more than a second, is not counted.
    var dt = lastNow ? Math.min(now - lastNow, 1000) : 16;
    lastNow = now;
    step(now, dt);
    var moving = follow(dt);
    if ((playing && visible && !finished) || moving) raf = requestAnimationFrame(frame);
    else lastNow = 0;
  }
  function kick() { if (!raf) raf = requestAnimationFrame(frame); }

  ctl.addEventListener('click', function (e) {
    var b = e.target.closest('button');
    if (!b) return;
    if (b.getAttribute('data-act') === 'pause') {
      playing = !playing;
      b.setAttribute('aria-pressed', playing ? 'false' : 'true');
      b.textContent = playing ? 'Pause' : 'Play';
      scene.classList.toggle('paused', !playing);
    } else {
      reset(); playing = true;
      pauseButton.setAttribute('aria-pressed', 'false'); pauseButton.textContent = 'Pause'; scene.classList.remove('paused');
    }
    kick();
  });
  if ('IntersectionObserver' in window) {
    new IntersectionObserver(function (entries) {
      visible = entries.some(function (e) { return e.isIntersecting; });
      scene.classList.toggle('idle', !visible);
      if (visible) kick();
    }).observe(scene);
  }
  // Leaving the first screen: the pair eases up and fades a little, as one write to one element.
  var lastHp = -1;
  App.onFrame(function (y) {
    var hp = Math.min(1, Math.max(0, y / (scene.offsetHeight * 0.8)));
    if (Math.abs(hp - lastHp) < 0.002 && hp !== 0 && hp !== 1) return;
    lastHp = hp;
    duel.style.opacity = hp > 0 ? (1 - hp * 0.5).toFixed(3) : '';
    duel.style.translate = hp > 0 ? '0 ' + snap(-hp * 40) + 'px' : '';
  });
  App.duel = {
    replay: function () { ctl.querySelector('[data-act="replay"]').click(); },
    pause: function () { if (playing) pauseButton.click(); }
  };

  // The breathing glow of the first capability card runs only while the card is in view.
  var breath = document.querySelector('.cap-main');
  if (breath && 'IntersectionObserver' in window) {
    new IntersectionObserver(function (entries) { breath.classList.toggle('in-view', entries.some(function (e) { return e.isIntersecting; })); }).observe(breath);
  }

  scene.classList.add('live');
  duel.setAttribute('role', 'group');
  each(duel.querySelectorAll('.term'), function (el) { el.setAttribute('aria-hidden', 'true'); });
  reset();
  t = -ENTER;
  kick();
})();
