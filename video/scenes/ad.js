/* The ad. One idea: the token mass collapses into a filament. Hard cuts land on the first frame of each spoken
   cue (build/ad.timeline.js), type is set at the scale of the stage, and the only numbers are the three the site
   headlines: tokens with no index, tokens with the best configuration, and the saving between them. Every frame is
   a function of t alone: window.PGR.seek(t). */
(function () {
  'use strict';
  var L = window.PGRLIB, F = window.FACTS, TL = window.TIMELINE;
  var W = L.W, H = L.H, clamp = L.clamp, mix = L.mix, seg = L.seg, ease = L.ease, rgba = L.rgba;
  var ACC = L.ACCENT, HOT = L.HOT, INK = L.INK, INK2 = L.INK2, CONTROL = L.CONTROL;
  var ui = document.getElementById('ui'), world = L.makeCanvas('world'), ctx = world.ctx;
  var grainEl = document.getElementById('grain'), cam = document.getElementById('cam');
  var FPS = TL.fps, DUR = TL.duration;

  function sc(id) { return TL.scenes.filter(function (s) { return s.id === id; })[0]; }
  function cue(id, n) { return sc(id).cues[n - 1]; }
  function cueAt(c, phrase) {
    var i = c.spoken.toLowerCase().indexOf(phrase.toLowerCase());
    return c.start + (c.end - c.start) * (i < 0 ? 0 : i / c.spoken.length);
  }
  var HK = [cue('hook', 1), cue('hook', 2), cue('hook', 3)], MS = [cue('mass', 1), cue('mass', 2)];
  var CO = [cue('collapse', 1), cue('collapse', 2)], NB = cue('number', 1), CM = [cue('claim', 1), cue('claim', 2)], CS = [cue('close', 1), cue('close', 2)];

  // ---------- type: every line is its own element, cut in on a frame, never faded
  function T(cls, html, x, y, size) {
    var e = L.el('div', 't ' + cls, html, ui);
    e._x = x; e._y = y; e.style.transform = 'translate3d(' + x + 'px,' + y + 'px,0)';
    if (size) e.style.fontSize = size + 'px';
    return e;
  }
  function win(e, t, a, b) { L.vis(e, t >= a && t < b); }
  function setText(e, s) { if (e._s !== s) { e._s = s; e.textContent = s; } }

  var a_reads = T('display', 'reads', 140, 112, 470), a_every = T('display', 'everything.', 146, 560, 300);
  var a_again = T('display', 'again.', 140, 250, 580), a_and = T('display', 'and again.', 146, 330, 340);
  var a_num1 = T('num', '0', 134, 190, 700), a_num1sub = T('sub', 'tokens per task. No index.', 150, 842, 56);
  var a_lookups = T('display', 'for three lookups.', 146, 826, 118);
  var a_q1 = T('display', 'one question.', 146, 84, 290), a_q2 = T('display', 'one answer.', 146, 84, 290);
  var a_num2 = T('num', F.best.tokText, 134, 190, 700), a_num2sub = T('sub', 'tokens per task.', 150, 842, 56);
  var a_pct = T('num', F.pct + '%', 134, 70, 700), a_fewer = T('display', 'fewer tokens', 146, 700, 150);
  var a_against = T('sub', 'Against no index, on our study.', 150, 884, 58);
  var a_mark = L.markSvg(ui, 300, 't'); a_mark.root._x = 150; a_mark.root._y = 70; a_mark.root.style.transform = 'translate3d(150px,70px,0)';
  var a_name = T('display', 'PeterGanuRusty', 142, 420, 214), a_find = T('display', 'Find code in one step.', 148, 726, 92), a_pp = T('sub', 'Private preview', 150, 858, 50);
  a_pct.style.color = 'var(--ink)'; a_num2.style.color = 'var(--ink)'; a_pp.style.color = 'var(--ink3)';
  var domAll = Array.prototype.slice.call(ui.children);

  // ---------- the token mass: one particle for each 1k tokens of the no index total
  var N = Math.round(F.control.tokens / 1000), M = Math.round(F.best.tokens / 1000);
  var X0 = 150, SPAN = 1620, FIL_Y = 540, FIL_LEN = SPAN * M / N, FIL_X0 = 960 - FIL_LEN / 2;
  var P = [], R = L.rng(3), i;
  for (i = 0; i < N; i++) {
    var r1 = R(), r2 = R(), r3 = R(), r4 = R(), r5 = R(), q = R();
    var z = q < 0.22 ? 1 : q < 0.58 ? 0.62 : 0.3;
    P.push({
      bx: r1 * SPAN, by: 120 + r2 * 840, z: z, v: 34 + 230 * Math.pow(z, 1.4) * (0.6 + 0.4 * r3), r4: r4, r5: r5,
      slot: FIL_X0 + (i + 0.5) / M * FIL_LEN, x: 0, y: 0, a: 0, ca: 0, boost: 0, passes: 0
    });
  }

  // beams: the read head. Three passes in the hook, each faster, and one that counts the tokens.
  var BEAMS = [
    { a: HK[0].start + 0.05, b: HK[0].start + 1.35 }, { a: HK[1].start, b: HK[1].start + 0.55 },
    { a: HK[2].start, b: HK[2].start + 0.46 }, { a: MS[0].start, b: MS[0].start + 0.9 }
  ];
  var KICKS = [[HK[0].start, 5], [HK[1].start, 12], [HK[2].start, 16], [MS[0].start, 10], [CO[0].start, 12], [CO[1].start, 14],
    [NB.start, 18], [CM[0].start, 16], [CS[0].start, 11]];
  function beamX(b, t) { return X0 - 40 + (SPAN + 80) * ease.ioQ(seg(t, b.a, b.b)); }

  function drawWorld(t) {
    var tColl = CO[0].start, tFil = CO[1].start;
    var kz = 1 + 0.62 * ease.outQ(seg(t, NB.start - 0.05, NB.start + 1.7));
    var K = mix(1, kz, 1 - ease.ioC(seg(t, CM[0].start, CM[0].start + 0.3)));
    ctx.save();
    ctx.translate(W / 2, H / 2); ctx.scale(K, K); ctx.translate(-W / 2, -H / 2);

    var intro = ease.outQ(seg(t, 0, 0.7));
    var c1 = ease.ioC(seg(t, CO[0].start, CO[0].start + 0.85));
    var gone = 1 - ease.inC(seg(t, CM[0].start, CM[0].start + 0.2));     // the claim and the close are on a clean ground
    var counted = 0, bm = BEAMS[3], bx3 = beamX(bm, t);
    var tf = Math.min(t, tColl), j;
    for (i = 0; i < N; i++) {
      var p = P[i];
      var xf = X0 + (((p.bx + p.v * tf) % SPAN) + SPAN) % SPAN;
      var edge = clamp((xf - X0) / 90) * clamp((X0 + SPAN - xf) / 90);
      var m = i < M ? ease.ioC(seg(t, tFil + 0.22 * p.r5, tFil + 0.22 * p.r5 + 0.72)) : 0;
      var x = mix(xf, p.slot, m), y = mix(p.by, FIL_Y + (p.r4 - 0.5) * 5, c1);
      var a = (0.3 + 0.7 * p.z) * intro * mix(edge, 1, c1), ca = 0, boost = 0;
      if (i < M) {
        ca = ease.ioC(seg(t, tFil + 0.12 + 0.5 * ((p.slot - FIL_X0) / FIL_LEN), tFil + 0.12 + 0.5 * ((p.slot - FIL_X0) / FIL_LEN) + 0.5));
        // a bright pulse runs along the filament
        var u = ((t * 0.72) % 1) * FIL_LEN, du = (p.slot - FIL_X0) - u;
        boost += ca * Math.exp(-(du * du) / (2 * 38 * 38)) * 1.4 * seg(t, tFil + 1.0, tFil + 1.4);
      } else {
        var d = ease.inC(seg(t, tFil + 0.08 + 0.4 * p.r5, tFil + 0.08 + 0.4 * p.r5 + 0.45));
        a *= 1 - d; y += d * 46; boost += Math.sin(d * Math.PI) * 1.4;
      }
      // read passes: every beam that has crossed the particle lifts it
      var passes = 0;
      for (j = 0; j < BEAMS.length; j++) {
        var bX = beamX(BEAMS[j], t), r = seg(t, BEAMS[j].a, BEAMS[j].b);
        if (r > 0 && r < 1) boost += Math.exp(-Math.pow((x - bX) / 100, 2)) * 1.8;
        if (t >= BEAMS[j].a && (x < bX || t >= BEAMS[j].b)) passes++;
      }
      if (t >= bm.a && x < bx3) counted++;
      a *= (0.55 + 0.17 * Math.min(passes, 4)) * gone;
      p.x = x; p.y = y; p.a = a; p.ca = ca; p.boost = boost;
    }
    // beams
    BEAMS.forEach(function (b, n) {
      var r = seg(t, b.a, b.b);
      if (r <= 0 || r >= 1) return;
      var x = beamX(b, t), g = ctx.createLinearGradient(x - 360, 0, x, 0);
      g.addColorStop(0, 'rgba(244,244,246,0)'); g.addColorStop(1, 'rgba(244,244,246,' + (0.07 + 0.02 * n).toFixed(3) + ')');
      ctx.fillStyle = g; ctx.fillRect(x - 360, 0, 360, H);
      ctx.fillStyle = 'rgba(244,244,246,0.9)'; ctx.fillRect(x - 0.5, 0, 1.5, H);
    });
    // the filament: a hairline of light under the particles once the stream has shortened
    var fl = ease.outQ(seg(t, tFil + 0.5, tFil + 1.3)) * gone;
    if (fl > 0) {
      var gl = ctx.createLinearGradient(0, FIL_Y - 46, 0, FIL_Y + 46);
      gl.addColorStop(0, 'rgba(255,122,61,0)'); gl.addColorStop(0.5, 'rgba(255,122,61,' + (0.2 * fl).toFixed(3) + ')'); gl.addColorStop(1, 'rgba(255,122,61,0)');
      ctx.fillStyle = gl; ctx.fillRect(FIL_X0 - 20, FIL_Y - 46, FIL_LEN + 40, 92);
      ctx.fillStyle = rgba(HOT, 0.95 * fl); ctx.fillRect(FIL_X0, FIL_Y - 1, FIL_LEN, 2);
      L.stamp(ctx, ACC, FIL_X0, FIL_Y, 50, 0.5 * fl); L.stamp(ctx, ACC, FIL_X0 + FIL_LEN, FIL_Y, 50, 0.5 * fl);
    }
    // particles: glows, then cores
    ctx.globalCompositeOperation = 'lighter';
    for (i = 0; i < N; i++) {
      p = P[i]; if (p.a <= 0.004) continue;
      var rr = 5.2 + 6.4 * p.z, gs = rr * (4 + p.boost * 2.2);
      if (p.ca < 1) L.stamp(ctx, CONTROL, p.x, p.y, rr * 11, p.a * 0.05 * (1 - p.ca));   // haze: the mass reads as one body
      if (p.ca < 1) L.stamp(ctx, p.boost > 0.5 ? INK : CONTROL, p.x, p.y, gs, p.a * (0.2 + p.boost * 0.35) * (1 - p.ca));
      if (p.ca > 0) L.stamp(ctx, ACC, p.x, p.y, gs * 1.3, p.a * (0.35 + p.boost * 0.4) * p.ca);
    }
    ctx.globalCompositeOperation = 'source-over';
    for (i = 0; i < N; i++) {
      p = P[i]; if (p.a <= 0.004) continue;
      var rc = 5.2 + 6.4 * p.z, base = L.lerpC(CONTROL, INK2, p.z), hot = L.lerpC(base, INK, clamp(p.boost / 1.6));
      var col = L.lerpC(hot, L.lerpC(ACC, HOT, clamp(p.boost / 1.6) * 0.7), p.ca);
      ctx.fillStyle = rgba(col, clamp(p.a + p.boost * 0.12));
      ctx.beginPath(); ctx.arc(p.x, p.y, rc * (1 + p.boost * 0.1) * (i < M && p.ca > 0 ? mix(1, 0.62, p.ca) : 1), 0, Math.PI * 2); ctx.fill();
    }
    ctx.restore();

    // the mark of the saving: the no index length as a dashed hairline against the filament's length
    var bar = ease.outQ(seg(t, CM[1].start - 0.1, CM[1].start + 0.5));
    if (t >= CM[0].start && t < CS[0].start - 0.02) {
      var y = 1016;
      L.hair(ctx, 0.3); ctx.setLineDash([8, 10]); ctx.beginPath(); ctx.moveTo(X0, y); ctx.lineTo(X0 + SPAN * bar, y); ctx.stroke(); ctx.setLineDash([]);
      ctx.fillStyle = rgba(ACC, 1); ctx.fillRect(X0, y - 2, FIL_LEN * seg(t, CM[0].start, CM[0].start + 0.3), 4);
      L.stamp(ctx, ACC, X0 + FIL_LEN * seg(t, CM[0].start, CM[0].start + 0.3), y, 40, 0.8);
    }
    // the closing filament under the name
    if (t >= CS[0].start) {
      var cs = ease.outQ(seg(t, CS[0].start + 0.35, CS[0].start + 0.95)), fy = 680, fx = 150;
      ctx.fillStyle = rgba(HOT, 0.95 * cs); ctx.fillRect(fx, fy - 1, FIL_LEN * cs, 2);
      var gg = ctx.createLinearGradient(0, fy - 30, 0, fy + 30); gg.addColorStop(0, 'rgba(255,122,61,0)'); gg.addColorStop(0.5, 'rgba(255,122,61,' + (0.3 * cs).toFixed(3) + ')'); gg.addColorStop(1, 'rgba(255,122,61,0)');
      ctx.fillStyle = gg; ctx.fillRect(fx - 10, fy - 30, FIL_LEN * cs + 20, 60);
      L.stamp(ctx, ACC, fx + FIL_LEN * cs, fy, 50, 0.9 * cs);
    }
    return counted;
  }

  function seek(t) {
    t = clamp(t, 0, DUR);
    var frame = Math.round(t * FPS);
    world.begin();
    L.background(ctx, world.dpr, Math.sin(t * 0.37) * 40, Math.cos(t * 0.29) * 26);
    ctx.lineCap = 'butt';
    var counted = drawWorld(t);

    // ---- type, cut in on the first frame of the cue that names it
    win(a_reads, t, HK[0].start, HK[1].start);
    win(a_every, t, cueAt(HK[0], 'everything'), HK[1].start);
    win(a_again, t, HK[1].start, HK[2].start);
    win(a_and, t, HK[2].start, MS[0].start);
    var tokText = t < MS[0].start ? '0' : seg(t, BEAMS[3].a, BEAMS[3].b) >= 1 ? F.control.tokText : L.fmtTokens(counted * 1000);
    setText(a_num1, tokText);
    win(a_num1, t, MS[0].start, CO[0].start);
    win(a_num1sub, t, MS[0].start + 0.9, MS[1].start);
    win(a_lookups, t, MS[1].start, CO[0].start);
    win(a_q1, t, CO[0].start, CO[1].start);
    win(a_q2, t, CO[1].start, NB.start);
    win(a_num2, t, NB.start, CM[0].start);
    win(a_num2sub, t, NB.start + 0.35, CM[0].start);
    win(a_pct, t, CM[0].start, CS[0].start);
    win(a_fewer, t, cueAt(CM[0], 'fewer'), CS[0].start);
    win(a_against, t, CM[1].start, CS[0].start);
    L.markDraw(a_mark, ease.ioC(seg(t, CS[0].start, CS[0].start + 0.5)));
    win(a_name, t, CS[0].start + 0.12, DUR + 1);
    win(a_find, t, CS[1].start, DUR + 1);
    win(a_pp, t, CS[1].start + 0.3, DUR + 1);

    // ---- the cut lands: a few frames of shake that die away
    var dx = 0, dy = 0;
    KICKS.forEach(function (k) {
      var age = t - k[0];
      if (age >= 0 && age < 0.5) { var e = Math.exp(-age * 17) * k[1]; dx += Math.sin(frame * 2.399963 + k[0] * 3.1) * e; dy += Math.cos(frame * 3.7 + k[0] * 1.7) * e * 0.7; }
    });
    cam.style.transform = 'translate3d(' + dx.toFixed(2) + 'px,' + dy.toFixed(2) + 'px,0)';
    var fade = seg(t, DUR - 0.3, DUR);
    if (fade > 0) { ctx.globalAlpha = fade; ctx.fillStyle = '#0a0a0c'; ctx.fillRect(0, 0, W, H); ctx.globalAlpha = 1; }
    var gs = L.grainShift(frame, 4);
    grainEl.style.transform = 'translate3d(' + (-gs[0]) + 'px,' + (-gs[1]) + 'px,0)';
  }

  window.PGR = { fps: FPS, duration: DUR, ready: false, seek: seek, sceneTimes: TL.scenes.map(function (s) { return { id: s.id, start: s.start, dur: s.dur }; }) };
  L.ready().then(function () { seek(0); window.PGR.ready = true; });
})();
