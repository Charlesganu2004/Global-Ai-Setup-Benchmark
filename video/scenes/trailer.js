/* The trailer. One idea: every dot is 100 tokens. The agent re-reads its conversation and the dots pile up; then the same
   task is run twice, side by side, and the dots of each run are counted: no index against PGR. The camera travels through
   the world between the beats instead of cutting. Every number is read from window.FACTS (tools/facts.mjs, from
   docs/data/showcase.js), every reveal is keyed to a cue of build/trailer.timeline.js, and every frame is a function of t
   alone: window.PGR.seek(t). Anything that qualifies a claim is 60 px or more on the 1920 x 1080 stage and sits in the
   upper two thirds, clear of the player's play button and its control bar. */
(function () {
  'use strict';
  var L = window.PGRLIB, K = window.PGRKIT, F = window.FACTS, TL = window.TIMELINE;
  var W = L.W, H = L.H, clamp = L.clamp, mix = L.mix, seg = L.seg, ease = L.ease, rgba = L.rgba;
  var ACC = L.ACCENT, HOT = L.HOT, INK = L.INK, INK2 = L.INK2, INK3 = L.INK3;
  var DUR = TL.duration;

  function sc(id) { return TL.scenes.filter(function (s) { return s.id === id; })[0]; }
  function cue(id, n) { return sc(id).cues[n - 1]; }
  function cueAt(c, phrase) {
    var i = c.spoken.toLowerCase().indexOf(phrase.toLowerCase());
    return c.start + (c.end - c.start) * (i < 0 ? 0 : i / c.spoken.length);
  }
  function win(t, a, b, c, d) { return seg(t, a, b) * (1 - seg(t, c, d)); }
  var HK = [cue('hook', 1), cue('hook', 2)], MS = [cue('mass', 1), cue('mass', 2)], NM = [cue('name', 1), cue('name', 2), cue('name', 3)];
  var CO = [cue('collapse', 1), cue('collapse', 2)], NB = cue('number', 1), CM = [cue('claim', 1), cue('claim', 2)], CS = cue('close', 1);
  var tAgain = cueAt(HK[1], 'And again'), tPGR = cueAt(NM[0], 'P G R');
  var MODEL = F.labels.classLabel;                           // Claude Haiku 5.5, low effort
  var HALO = 0.9;

  // ---------------------------------------------------------------- the world: the agent's session and the dots it reads
  var N = Math.round(F.control.tokens / 100);
  var TX = -50, TY = 335, TW = 940, TH = 610, Y0 = TY + 66, Y1 = TY + TH - 26;
  var BEAMS = [{ a: HK[0].start + 0.15, b: HK[0].end - 0.05 }, { a: HK[1].start, b: HK[1].start + 0.5 }, { a: tAgain, b: tAgain + 0.42 }];
  function headY(tb) {
    var i = 0;
    for (; i < BEAMS.length; i++) if (tb <= BEAMS[i].b) break;
    var b = BEAMS[Math.min(i, BEAMS.length - 1)];
    return Y0 + (Y1 - Y0) * ease.ioQ(seg(tb, b.a, b.b));
  }
  var n1 = Math.round(N * 0.28), n2 = Math.round(N * 0.33);
  var CX = 1500;
  var cloud = new K.Cloud({
    N: N, M: Math.round(F.best.tokens / 100), seed: 11, center: [CX, 540], R: 560, squash: 0.62, emit: { x: TX + TW, head: headY },
    passes: [{ n: n1, a: BEAMS[0].a, b: BEAMS[0].b }, { n: n2, a: BEAMS[1].a, b: BEAMS[1].b }, { n: N - n1 - n2, a: BEAMS[2].a, b: BEAMS[2].b }]
  });
  var born = cloud.d.map(function (e) { return e.born; }).sort(function (a, b) { return a - b; });
  function bornCount(t) { var lo = 0, hi = born.length; while (lo < hi) { var m = (lo + hi) >> 1; if (born[m] <= t) lo = m + 1; else hi = m; } return lo; }

  // the race of the two agents, from the question to the end of the claim
  var RACE = { x: 126, y: 310, w: 800, h: 640, gap: 60, size: 30, cs: 118, meterH: 150, task: 'parse_config',
    tShow: CO[0].start - 0.5, tAsk: CO[0].start - 0.05, tStart: CO[0].start - 0.05, tEnd: NB.start + 1.15 };
  RACE.lines = K.raceLines(Object.assign({ ask: 'who calls parse_config?', answer: 'load_settings, start_server, run_cli',
    reads: ['read config.py', 'read loader.py', 'search parse_config', 'read settings.py', 'read cli.py', 'read server.py', 'read tests', 'read config.py'] }, RACE));
  var tDone = K.raceTimes(RACE).tDone;

  var cam = new K.Cam([
    [0, -250, 20, 1.08, 0], [HK[1].start, -245, 14, 1.11, 0, 'lin'], [MS[0].start - 0.1, -240, 8, 1.14, 0, 'lin'],
    [MS[0].start + 1.0, -70, -10, 0.8, 0, 'ioE'], [NM[0].start - 0.2, -50, -10, 0.82, 0, 'lin'],
    [NM[0].start + 1.2, 540, -15, 1.0, -0.015, 'ioC'], [NM[2].end + 0.3, 560, -20, 1.45, 0.025, 'lin'],
    [CO[0].start - 0.45, 300, -10, 1.2, 0.01, 'ioC'], [CO[0].start + 0.3, 0, 0, 1.0, 0, 'ioC'], [RACE.tEnd + 0.15, 0, 0, 1.03, 0.004, 'lin'],
    [CM[0].start - 0.1, 0, -250, 0.78, 0, 'ioC'], [CS.start - 0.3, 0, -250, 0.78, 0, 'lin'], [DUR, 0, -230, 0.8, 0, 'lin']
  ]);
  var NAMES = ['read config.py', 'read loader.py', 'search parse_config', 'read settings.py', 'read cli.py', 'read server.py', 'read tests', 'read config.py'];
  // the conversation is already there when the film opens: earlier questions and what the agent read for them
  var LINES = [
    { at: -9, text: 'where is parse_config defined?', kind: 'cmd' }, { at: -9, text: 'searching the code', kind: 'dim' },
    { at: -9, text: 'read config.py', kind: 'out' }, { at: -9, text: 'read loader.py', kind: 'out' },
    { at: -9, text: 'who else uses it?', kind: 'cmd' }, { at: -9, text: 'read settings.py', kind: 'out' }, { at: -9, text: 'read cli.py', kind: 'out' },
    { at: HK[0].start + 0.2, text: 'find every caller of parse_config', kind: 'cmd', cps: 36 }
  ];
  (function () { var k = 0, at = HK[0].start + 1.25; while (at < tAgain + 0.8) { LINES.push({ at: at, text: NAMES[k % NAMES.length], kind: 'out' }); k++; at += 0.19; } })();
  var SESSION_ALPHA = function (t) { return 1 - seg(t, MS[0].start, MS[0].start + 0.7); };

  // the install terminal (closing card)
  var CLOSE_LINES = K.installLines(F.installers.items, CS.start + 0.1, 420);

  // ---------------------------------------------------------------- the picture
  function draw(ctx, t, sub) {
    var c = cam.at(t), tc = sub && sub.t !== undefined ? sub.t : t;
    L.mark(ctx, 'start');
    L.background(ctx, t, -c.x * 0.03, -c.y * 0.03, 1);
    L.mark(ctx, 'bg');
    K.floor(ctx, c, t, 1 - 0.55 * seg(t, CS.start - 0.2, CS.start + 0.5));
    L.mark(ctx, 'floor');
    var camAt = function (tb) { return cam.at(tb); };
    var dim = 1 - 0.72 * win(t, NM[0].start - 0.15, NM[0].start + 0.5, CO[0].start - 0.5, CO[0].start - 0.1);
    var cout = 1 - seg(t, CO[0].start - 0.5, CO[0].start + 0.05);

    // the agent's session and its read head (world)
    var ta = SESSION_ALPHA(t);
    if (ta > 0.003 && t < MS[0].start + 1.2) {
      L.layer(ctx, c, 1.0);
      L.terminal(ctx, TX, TY, TW, TH, t, { title: 'agent  -  parse_config', lines: LINES, size: 30, a: ta, glow: ACC, glowK: 0.5 });
      K.head(ctx, BEAMS, headY, t, TX, TW);
      ctx.setTransform(L.DPR, 0, 0, L.DPR, 0, 0);
    }
    L.mark(ctx, 'terminal');
    if (cout > 0.003) cloud.draw(ctx, t, { cam: c, camAt: camAt, a: dim * cout, heat: 1 });
    L.mark(ctx, 'cloud');

    // the race (world, on the glass at zoom 1 and carried down when the claim arrives)
    if (t > RACE.tShow) {
      L.layer(ctx, c, 1.0);
      K.race(ctx, t, Object.assign({}, RACE, { tc: tc, a: 1 - seg(t, CS.start - 0.3, CS.start + 0.1) }));
      ctx.setTransform(L.DPR, 0, 0, L.DPR, 0, 0);
    }
    L.mark(ctx, 'race');
    K.bokeh(ctx, c, t, 1 - 0.5 * seg(t, CS.start - 0.2, CS.start + 0.5));
    L.mark(ctx, 'bokeh');

    // ---- type, in screen space
    ctx.setTransform(L.DPR, 0, 0, L.DPR, 0, 0);
    // hook
    var hg = 1 - seg(t, MS[0].start - 0.05, MS[0].start + 0.3);
    if (hg > 0.003) {
      L.wordsText(ctx, 'Your agent reads', 146, 112, t, HK[0].start, { size: 72, weight: 600, font: 'd', ls: -0.02, color: INK2, a: hg });
      L.wordsText(ctx, 'the whole conversation.', 140, 250, t, HK[0].start + 0.25, { size: 132, weight: 700, font: 'd', ls: -0.038, color: INK, a: hg });
      L.riseText(ctx, 'Again.', 1060, 520, ease.outQ(seg(t, HK[1].start, HK[1].start + 0.35)), { size: 190, weight: 800, font: 'd', ls: -0.05, color: INK, a: hg });
      L.riseText(ctx, 'And again.', 1060, 705, ease.outQ(seg(t, tAgain, tAgain + 0.3)), { size: 170, weight: 800, font: 'd', ls: -0.05, color: ACC, a: hg });
    }
    // the counter: bottom right while the dots pile up, then it is carried to the left and grows into the number
    var read = F.control.tokens * bornCount(tc) / N, cp = ease.ioE(seg(t, MS[0].start - 0.05, MS[0].start + 0.95));
    var cs = L.fmtTokens(t >= BEAMS[2].b + 0.9 ? F.control.tokens : read), csize = mix(96, 400, cp);
    var cw = L.measure(ctx, cs, { size: csize, weight: 700, font: 'd', ls: -0.055 });
    var cvis = win(t, HK[0].start + 0.6, HK[0].start + 0.9, NM[0].start - 0.3, NM[0].start + 0.05);
    if (cvis > 0.003) {
      var cx = mix(1770 - cw, 150, cp), cy = mix(1000, 480, cp);
      L.numText(ctx, cs, cx, cy, { size: csize, weight: 700, font: 'd', ls: -0.055, color: INK, a: cvis, counting: L.fmtTokens(F.control.tokens) });
      L.text(ctx, 'tokens read', 1770, 1042, { size: 30, weight: 500, font: 't', color: INK3, a: cvis * (1 - cp), align: 'r' });
    }
    // mass
    var mg = win(t, MS[0].start, MS[0].start, NM[0].start - 0.3, NM[0].start + 0.05);
    if (mg > 0.003) {
      L.riseText(ctx, 'tokens per task, for three lookups.', 154, 632, ease.outQ(seg(t, MS[0].start + 0.9, MS[0].start + 1.3)), { size: 56, weight: 500, font: 'd', ls: -0.012, color: INK2, a: mg, shadow: HALO });
      L.riseText(ctx, 'No index. ' + MODEL + ',', 154, 696, ease.outQ(seg(t, MS[1].start, MS[1].start + 0.4)), { size: 56, weight: 560, font: 'd', ls: -0.01, color: INK2, a: mg, shadow: HALO });
      L.riseText(ctx, F.labels.firstSet + '.', 154, 760, ease.outQ(seg(t, MS[1].start + 0.1, MS[1].start + 0.5)), { size: 56, weight: 560, font: 'd', ls: -0.01, color: INK2, a: mg, shadow: HALO });
    }
    if (t > MS[0].start + 0.5 && t < CM[0].start) {
      L.text(ctx, 'one dot = 100 tokens', 150, 1040, { size: 30, weight: 500, font: 't', color: INK3, a: seg(t, MS[0].start + 0.5, MS[0].start + 1.0) * (1 - seg(t, CM[0].start - 0.3, CM[0].start)) });
    }
    L.mark(ctx, 'hook+counter+mass');
    // the name
    var ng = win(t, NM[0].start - 0.05, NM[0].start, CO[0].start - 0.7, CO[0].start - 0.4);
    if (ng > 0.003) {
      L.markDraw(ctx, 146, 56, 112, seg(t, NM[0].start, NM[0].start + 0.8), ACC, ng);
      L.riseText(ctx, 'PeterGanuRusty', 140, 340, ease.outQ(seg(t, NM[0].start + 0.1, NM[0].start + 0.65)), { size: 200, weight: 700, font: 'd', ls: -0.045, color: INK, a: ng, shadow: HALO });
      var pp = ease.spring(seg(t, tPGR, tPGR + 0.6));
      if (pp > 0.001) {
        var pw = L.measure(ctx, 'PGR', { size: 76, weight: 700, font: 'd', ls: 0.02 }), px = 150, py = 372, bw = pw + 64;
        ctx.save(); ctx.translate(px + bw / 2, py + 48); ctx.scale(pp, pp); ctx.translate(-(px + bw / 2), -(py + 48));
        L.rr(ctx, px, py, bw, 96, L.R_M); ctx.fillStyle = rgba(ACC, 0.14 * ng); ctx.fill(); ctx.strokeStyle = rgba(ACC, ng); ctx.lineWidth = 2.5; ctx.stroke();
        L.text(ctx, 'PGR', px + 32, py + 70, { size: 76, weight: 700, font: 'd', ls: 0.02, color: ACC, a: ng });
        L.text(ctx, 'for short', px + bw + 30, py + 68, { size: 56, weight: 500, font: 'd', ls: -0.01, color: INK2, a: ng * seg(t, tPGR + 0.2, tPGR + 0.5) });
        ctx.restore();
      }
      L.riseText(ctx, 'A command-line program for the desktop', 146, 576, ease.outQ(seg(t, NM[1].start, NM[1].start + 0.4)), { size: 56, weight: 520, font: 'd', ls: -0.012, color: INK, a: ng, shadow: HALO });
      L.riseText(ctx, 'that AI agents call to look up code.', 146, 644, ease.outQ(seg(t, NM[1].start + 0.9, NM[1].start + 1.3)), { size: 56, weight: 520, font: 'd', ls: -0.012, color: INK2, a: ng, shadow: HALO });
      K.link(ctx, t, { x: 150, y: 846, w: 1620, h: 120, bw: 600, a: ng * ease.outC(seg(t, NM[1].start + 0.2, NM[1].start + 0.8)), t0: NM[1].start + 0.9 });
      L.riseText(ctx, 'Local, no account.', 146, 740, ease.outQ(seg(t, NM[2].start, NM[2].start + 0.35)), { size: 72, weight: 700, font: 'd', ls: -0.03, color: ACC, a: ng, shadow: HALO });
    }
    L.mark(ctx, 'name');
    // one question, one answer: above the two terminals
    var qg = win(t, CO[0].start - 0.1, CO[0].start, NB.start + 0.2, NB.start + 0.7);
    if (qg > 0.003) {
      L.riseText(ctx, 'One question.', 126, 172, ease.outQ(seg(t, CO[0].start, CO[0].start + 0.4)), { size: 112, weight: 700, font: 'd', ls: -0.045, color: INK, a: qg });
      L.riseText(ctx, 'One answer.', 986, 172, ease.outQ(seg(t, CO[1].start, CO[1].start + 0.4)), { size: 112, weight: 700, font: 'd', ls: -0.045, color: ACC, a: qg });
    }
    // the claim, over the two terminals that have moved down
    var kg = win(t, CM[0].start - 0.05, CM[0].start, CS.start - 0.35, CS.start - 0.05);
    if (kg > 0.003) {
      var pv = F.pct * ease.outQ(seg(tc, CM[0].start, CM[0].start + 0.7));
      L.numText(ctx, Math.round(pv) + '%', 134, 340, { size: 420, weight: 700, font: 'd', ls: -0.055, color: INK, a: kg, counting: F.pct + '%' });
      L.riseText(ctx, 'fewer tokens', 146, 444, ease.outQ(seg(t, CM[0].start + 0.25, CM[0].start + 0.6)), { size: 100, weight: 700, font: 'd', ls: -0.04, color: INK, a: kg });
      var q1 = [F.control.tokText + ' to ' + F.best.tokText + ' tokens per task,', 'against no index.'], q2 = ['One comparison:', MODEL + ',', F.labels.firstSet + '.'];
      q1.forEach(function (s1, n) { L.riseText(ctx, s1, 980, 100 + n * 66, ease.outQ(seg(t, CM[0].start + 0.3 + n * 0.07, CM[0].start + 0.7 + n * 0.07)), { size: 60, weight: 560, font: 'd', ls: -0.01, color: INK2, a: kg, shadow: HALO }); });
      q2.forEach(function (s1, n) { L.riseText(ctx, s1, 980, 264 + n * 66, ease.outQ(seg(t, CM[1].start + n * 0.07, CM[1].start + 0.4 + n * 0.07)), { size: 60, weight: 560, font: 'd', ls: -0.01, color: INK2, a: kg, shadow: HALO }); });
    }
    L.mark(ctx, 'claim');
    var ill = win(t, HK[0].start, HK[0].start + 0.5, CM[1].end + 0.3, CM[1].end + 0.7);
    if (ill > 0.003) L.text(ctx, 'Illustration: example names', 1770, 44, { size: 28, weight: 560, font: 't', color: INK3, align: 'r', a: ill * 0.9 });
    // the close
    var sg = seg(t, CS.start - 0.25, CS.start + 0.05);
    if (sg > 0.003) {
      L.markDraw(ctx, 146, 54, 88, seg(t, CS.start, CS.start + 0.7), ACC, sg);
      L.riseText(ctx, 'PeterGanuRusty', 262, 124, ease.outQ(seg(t, CS.start, CS.start + 0.5)), { size: 80, weight: 700, font: 'd', ls: -0.04, color: INK, a: sg });
      L.wordsText(ctx, 'One command installs it.', 140, 330, t, CS.start, { size: 150, weight: 700, font: 'd', ls: -0.045, color: INK, a: sg });
      L.terminal(ctx, 150, 392, 1620, 500, t, { title: 'Install', lines: CLOSE_LINES, size: 34, a: sg * ease.outC(seg(t, CS.start + 0.1, CS.start + 0.5)), sheen: 0.7, glow: ACC, glowK: 0.6 });
    }
  }

  function post(t) {
    var fi = 1 - seg(t, 0, 0.5), fo = seg(t, DUR - 0.3, DUR);
    return { bloom: 0.22 + 0.12 * win(t, tDone - 0.2, tDone + 0.1, tDone + 0.6, tDone + 1.4), fade: Math.max(fi, fo), vig: 0.3, thr: 0.58 };
  }

  L.film({ duration: DUR, fps: TL.fps, draw: draw, post: post, sceneTimes: TL.scenes.map(function (s) { return { id: s.id, start: s.start, dur: s.dur }; }) });
})();
