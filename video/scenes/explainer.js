/* The explainer. Every frame is drawn from the time t alone: window.PGR.seek(t). The picture follows the voice, so
   every reveal is keyed to a cue of build/explainer.timeline.js, and every number is read from window.FACTS (built from
   docs/data/showcase.js by tools/facts.mjs). Anything that qualifies a claim is 60 px or more on the 1920 x 1080 stage
   and sits in the upper two thirds of the frame, clear of the captions and the player's controls. One dot is 100 tokens.
   The models are named as the runs used them (FACTS.models), never by a class.

   The film is one continuous camera move. The cold open and the race are drawn at the origin; from the end of the claim the
   camera travels along a row of stations in the same world (what it is, the four things it gives, the study, the limits, the
   install card): each station is a 1920 x 1080 frame at its own place, the camera dollies from one to the next with a dip in
   zoom, and nothing cuts or fades to black between them. */
(function () {
  'use strict';
  var L = window.PGRLIB, K = window.PGRKIT, G = window.PGRGFX, F = window.FACTS, TL = window.TIMELINE;
  var clamp = L.clamp, mix = L.mix, seg = L.seg, ease = L.ease, rgba = L.rgba;
  var ACC = L.ACCENT, HOT = L.HOT, INK = L.INK, INK2 = L.INK2, INK3 = L.INK3, CONTROL = L.CONTROL;
  var DUR = TL.duration, HALO = 0.9;

  function sc(id) { return TL.scenes.filter(function (s) { return s.id === id; })[0]; }
  function cues(id) { return sc(id).cues; }
  function cueAt(c, phrase) {
    var i = c.spoken.toLowerCase().indexOf(phrase.toLowerCase());
    return c.start + (c.end - c.start) * (i < 0 ? 0 : i / c.spoken.length);
  }
  function win(t, a, b, c, d) { return seg(t, a, b) * (1 - seg(t, c, d)); }
  var C = cues('cold'), H = cues('change'), WH = cues('what'), GV = cues('gives'), ST = cues('study'), CL = cues('close');
  var MODEL = F.labels.classLabel;                                // Claude Haiku 5.5, low effort
  var D = { size: 60, weight: 560, font: 'd', ls: -0.01, color: INK2 };     // the qualifier type
  function o(size, weight, extra) { return Object.assign({ size: size, weight: weight || 700, font: 'd', ls: size > 100 ? -0.045 : -0.02, color: INK }, extra || {}); }
  // text cut to a width with an ellipsis (used for the configuration names only, which are short enough to fit)
  function fit(ctx, s, maxW, opt) {
    if (L.measure(ctx, s, opt) <= maxW) return s;
    while (s.length > 4 && L.measure(ctx, s + '…', opt) > maxW) s = s.slice(0, -1);
    return s.replace(/\s+$/, '') + '…';
  }
  // a text broken into lines of at most maxW, never cut
  function wrap(ctx, s, maxW, opt) {
    var words = s.split(' '), line = '', lines = [];
    words.forEach(function (w) { var tt = line ? line + ' ' + w : w; if (line && L.measure(ctx, tt, opt) > maxW) { lines.push(line); line = w; } else line = tt; });
    lines.push(line);
    return lines;
  }
  function pct(x) { return Math.round(x) + '%'; }

  // ================================================================ world: the cold open and the change
  var N = Math.round(F.control.tokens / 100), M = Math.round(F.best.tokens / 100);
  var TX = -50, TY = 335, TW = 940, TH = 610, Y0 = TY + 66, Y1 = TY + TH - 26;
  // one read head sweep for each turn of the agent: turn 4 begins as "Four turns." ends
  var BEAMS = [0, 1, 2, 3].map(function (i) { return { a: C[1].start - 0.1 + i * 0.4, b: C[1].start - 0.1 + i * 0.4 + 0.4 }; });
  function headY(tb) {
    var i = 0;
    for (; i < BEAMS.length; i++) if (tb <= BEAMS[i].b) break;
    var b = BEAMS[Math.min(i, BEAMS.length - 1)];
    return Y0 + (Y1 - Y0) * ease.ioQ(seg(tb, b.a, b.b));
  }
  var wts = [1, 2, 3, 4], used = 0;
  var passes = BEAMS.map(function (b, i) { var n = i === 3 ? N - used : Math.round(N * wts[i] / 10); used += n; return { n: n, a: b.a, b: b.b }; });
  var CX = 1500;
  var cloud = new K.Cloud({ N: N, M: M, seed: 23, center: [CX, 540], R: 560, squash: 0.62, emit: { x: TX + TW, head: headY }, passes: passes });
  var born = cloud.d.map(function (e) { return e.born; }).sort(function (a, b) { return a - b; });
  function bornCount(t) { var lo = 0, hi = born.length; while (lo < hi) { var m = (lo + hi) >> 1; if (born[m] <= t) lo = m + 1; else hi = m; } return lo; }
  var cam0 = new K.Cam([
    [0, -250, 20, 1.08, 0], [C[1].start, -245, 14, 1.11, 0, 'lin'], [C[2].start - 0.1, -240, 8, 1.14, 0, 'lin'],
    [C[2].start + 1.0, -70, -10, 0.8, 0, 'ioE'], [H[0].start - 0.2, -50, -10, 0.82, 0, 'lin'],
    [H[0].start + 1.2, 540, -15, 1.0, -0.015, 'ioC'], [H[0].end + 0.3, 560, -20, 1.4, 0.02, 'lin'],
    [H[1].start - 0.45, 300, -10, 1.2, 0.01, 'ioC'], [H[1].start + 0.3, 0, 0, 1.0, 0, 'ioC'], [H[1].start - 0.05 + 4.8 + 0.15, 0, 0, 1.0, 0, 'lin'],
    [H[4].start - 0.05, 0, -250, 0.78, 0, 'ioC'], [H[5].end + 0.05, 0, -250, 0.78, 0, 'lin']
  ]);
  var NAMES = ['read config.py', 'read loader.py', 'search parse_config', 'read settings.py', 'read cli.py', 'read server.py', 'read tests', 'read config.py'];
  var LOOKS = ['who calls parse_config?', 'where is load_settings defined?', 'which function builds the report?'];
  var LINES = [
    { at: C[0].start + 0.0, text: LOOKS[0], kind: 'cmd', cps: 38 }, { at: C[0].start + 0.62, text: LOOKS[1], kind: 'cmd', cps: 38 }, { at: C[0].start + 1.24, text: LOOKS[2], kind: 'cmd', cps: 38 }
  ];
  (function () { var k = 0, at = C[1].start; while (at < BEAMS[3].b + 0.4) { LINES.push({ at: at, text: NAMES[k % NAMES.length], kind: 'out' }); k++; at += 0.16; } })();
  // the race of the two agents, from the question to the end of the claim
  var RACE = { x: 110, y: 310, w: 810, h: 640, gap: 60, size: 30, cs: 118, meterH: 150, task: 'parse_config',
    tShow: H[1].start - 0.5, tAsk: H[1].start - 0.05, tStart: H[1].start - 0.05, tEnd: H[1].start - 0.05 + 4.8 };
  RACE.lines = K.raceLines(Object.assign({ ask: LOOKS[0], answer: 'load_settings, start_server, run_cli', reads: NAMES }, RACE));
  var tDone = K.raceTimes(RACE).tDone;

  // ================================================================ the stations and the camera between them
  // Each station is a 1920 x 1080 frame at [ox, oy] in the world. The four windows of "gives" are frames of their own.
  var SO = { what: [2400, 0], agents: [8800, 0], questions: [11200, 0], models: [13600, 0], board: [16000, 0], range: [18400, 0], reliable: [20800, 0], limits: [23200, 0], close: [25600, 0] };
  var GFR = [[4800, 0], [6300, 0], [4800, 850], [6300, 850]];                     // frames of the four windows of "gives"
  var GWIN = { w: 1240, h: 700, dx: 340, dy: 190 };
  var OVERVIEW = { x: 5550, y: 425, z: 0.52 };
  var WZ = 1.15;                                                                 // the camera's zoom on one window
  var stops = [];
  function go(depart, arrive, x, y, z, roll) { stops.push({ t0: depart, t1: Math.max(arrive, depart + 1.0), x: x, y: y, z: z, roll: roll || 0 }); }
  go(H[5].end + 0.05, WH[0].start + 0.45, SO.what[0], SO.what[1], 1.0, 0.012);
  go(WH[2].end - 0.1, GV[0].start + 0.5, OVERVIEW.x, OVERVIEW.y, OVERVIEW.z, -0.01);
  [0, 1, 2, 3].forEach(function (i) {
    var prev = i === 0 ? GV[0] : GV[i];
    go(prev.end - 0.1, GV[i + 1].start + 0.45, GFR[i][0], GFR[i][1], WZ, i % 2 ? -0.008 : 0.01);
  });
  var order = [['agents', ST[0], GV[4]], ['questions', ST[1], ST[0]], ['models', ST[2], ST[1]], ['board', ST[3], ST[2]], ['range', ST[4], ST[3]], ['reliable', ST[5], ST[4]], ['limits', CL[0], ST[5]], ['close', CL[2], CL[1]]];
  order.forEach(function (s, i) { go(s[2].end - 0.1, s[1].start + 0.45, SO[s[0]][0], SO[s[0]][1], 1.0, i % 2 ? -0.01 : 0.012); });
  // each stop knows when the camera arrives (t1) and when it leaves for the next (leave)
  stops.forEach(function (s, i) { s.leave = i + 1 < stops.length ? stops[i + 1].t0 : DUR + 1; });
  // At rest the camera keeps pushing in, 3 percent over the hold, so that no hold is a still picture; the next move starts from where the push got to.
  var PUSH = 0.03;
  var cam1 = (function () {
    var start = cam0.at(stops[0].t0);
    return {
      at: function (t) {
        var from = start, i, s, p, e;
        for (i = 0; i < stops.length; i++) {
          s = stops[i];
          if (t < s.t0) break;
          if (t < s.t1) {
            p = seg(t, s.t0, s.t1); e = ease.ioC(p);
            var dip = 1 - 0.1 * Math.sin(Math.PI * p);
            return { x: mix(from.x, s.x, e), y: mix(from.y, s.y, e), zoom: Math.exp(mix(Math.log(from.zoom), Math.log(s.z), e)) * dip, rot: s.roll * Math.sin(Math.PI * p) };
          }
          from = { x: s.x, y: s.y, zoom: s.z * (1 + PUSH) };
          if (t < s.leave) return { x: s.x, y: s.y, zoom: s.z * (1 + PUSH * seg(t, s.t1, s.leave)), rot: 0 };
        }
        return { x: from.x, y: from.y, zoom: from.zoom, rot: 0 };
      }
    };
  })();
  var tMove0 = stops[0].t0, tPanEnd = stops[0].t1;
  function camAt(t) { return t < tMove0 ? cam0.at(t) : cam1.at(t); }
  // is a 1920 x 1080 frame at (ox, oy) near enough to the view to be drawn
  function near(c, ox, oy, pad) {
    pad = pad || 0;
    return Math.abs(c.x - ox) < 960 + 960 / c.zoom + pad && Math.abs(c.y - oy) < 540 + 540 / c.zoom + pad;
  }
  // how much the camera is at rest at place i: 1 from just before it arrives to when it leaves
  function atStop(t, i) { var s = stops[i]; return win(t, s.t1 - 0.6, s.t1, s.leave, s.leave + 0.4); }

  // ================================================================ cold open and change (screen space overlays)
  function drawWorld(ctx, t, tc, c) {
    var dim = 1 - 0.5 * win(t, H[0].start - 0.15, H[0].start + 0.5, H[1].start - 0.5, H[1].start - 0.1);
    var cout = 1 - seg(t, H[1].start - 0.5, H[1].start + 0.05);
    var ta = 1 - seg(t, C[2].start, C[2].start + 0.7);
    if (ta > 0.003) {
      L.layer(ctx, c, 1.0);
      L.terminal(ctx, TX, TY, TW, TH, t, { title: 'agent  -  three lookups', lines: LINES, size: 30, a: ta, glow: ACC, glowK: 0.5 });
      K.head(ctx, BEAMS, headY, t, TX, TW);
      ctx.setTransform(L.DPR, 0, 0, L.DPR, 0, 0);
    }
    if (cout > 0.003) cloud.draw(ctx, t, { cam: c, camAt: camAt, a: dim * cout, heat: 1 });
    if (t > RACE.tShow) {
      L.layer(ctx, c, 1.0);
      K.race(ctx, t, Object.assign({}, RACE, { tc: tc, a: 1 }));
    }
    ctx.setTransform(L.DPR, 0, 0, L.DPR, 0, 0);
  }

  function overlayCold(ctx, t, tc) {
    var cg = 1 - seg(t, H[0].start - 0.3, H[0].start + 0.05);
    // three lookups
    var g1 = win(t, 0, 0, C[2].start - 0.05, C[2].start + 0.3);
    if (g1 > 0.003) {
      L.wordsText(ctx, 'Three simple', 146, 112, t, C[0].start, o(72, 600, { color: INK2, a: g1 }));
      L.wordsText(ctx, 'code lookups.', 140, 250, t, C[0].start + 0.25, o(132, 700, { ls: -0.038, a: g1 }));
    }
    // the counters: the turn bottom left (it holds while "Four turns." is said), tokens read bottom right
    var turn = 0, i;
    for (i = 0; i < BEAMS.length; i++) if (t >= BEAMS[i].a) turn = i + 1;
    var read = F.control.tokens * bornCount(tc) / N, cp = ease.ioE(seg(t, C[2].start + 0.3, C[2].start + 1.3));
    var cs = L.fmtTokens(t >= BEAMS[3].b + 0.9 ? F.control.tokens : read), csize = mix(96, 360, cp);
    var cw = L.measure(ctx, cs, o(csize, 700, { ls: -0.055 }));
    var cvis = seg(t, C[1].start - 0.2, C[1].start + 0.2) * cg;
    if (cvis > 0.003) {
      L.numText(ctx, cs, mix(1770 - cw, 150, cp), mix(1000, 520, cp), o(csize, 700, { ls: -0.055, a: cvis, counting: L.fmtTokens(F.control.tokens) }));
      L.text(ctx, 'tokens read', 1770, 1042, { size: 30, weight: 500, font: 't', color: INK3, a: cvis * (1 - cp), align: 'r' });
    }
    var tvis = cg * seg(t, C[1].start - 0.2, C[1].start + 0.1) * (1 - seg(t, C[2].start + 0.2, C[2].start + 0.5));
    if (turn > 0 && tvis > 0.003) {
      L.text(ctx, String(turn), 150, 1000, o(96, 700, { a: tvis }));
      L.text(ctx, 'turn', 150, 1042, { size: 30, weight: 500, font: 't', color: INK3, a: tvis });
    }
    var mg = win(t, C[2].start, C[2].start, H[0].start - 0.3, H[0].start + 0.05);
    if (mg > 0.003) {
      L.riseText(ctx, 'tokens for three lookups.', 154, 592, ease.outQ(seg(t, C[2].start + 1.2, C[2].start + 1.6)), { size: 60, weight: 520, font: 'd', ls: -0.012, color: INK2, a: mg, shadow: HALO });
      L.riseText(ctx, 'No index. ' + MODEL + ',', 154, 660, ease.outQ(seg(t, C[3].start - 0.1, C[3].start + 0.3)), Object.assign({}, D, { size: 56, a: mg, shadow: HALO }));
      L.riseText(ctx, F.labels.firstSet + '.', 154, 724, ease.outQ(seg(t, C[3].start, C[3].start + 0.4)), Object.assign({}, D, { size: 56, a: mg, shadow: HALO }));
      L.riseText(ctx, 'Every turn, an AI coding agent', 146, 96, ease.outQ(seg(t, C[3].start, C[3].start + 0.4)), { size: 56, weight: 560, font: 'd', ls: -0.02, color: INK2, a: mg, shadow: HALO });
      L.riseText(ctx, 'reads its whole conversation again.', 142, 196, ease.outQ(seg(t, C[3].start + 0.3, C[3].start + 0.8)), o(84, 700, { ls: -0.035, a: mg, shadow: HALO }));
    }
    if (t > C[2].start + 0.5 && t < H[4].start) L.text(ctx, 'one dot = 100 tokens', 960, 1042, { size: 30, weight: 500, font: 't', color: INK3, align: 'c', a: seg(t, C[2].start + 0.5, C[2].start + 1.0) * (1 - seg(t, H[4].start - 0.4, H[4].start - 0.1)) });
  }

  function overlayChange(ctx, t, tc) {
    var tPGR = cueAt(H[0], 'P G R');
    var ng = win(t, H[0].start - 0.05, H[0].start, H[1].start - 0.7, H[1].start - 0.4);
    if (ng > 0.003) {
      L.text(ctx, 'Now the same lookups, with', 146, 120, o(56, 560, { color: INK2, a: ng * seg(t, H[0].start, H[0].start + 0.3) }));
      L.markDraw(ctx, 146, 170, 100, seg(t, H[0].start + 0.9, H[0].start + 1.7), ACC, ng);
      L.riseText(ctx, 'PeterGanuRusty', 270, 262, ease.outQ(seg(t, H[0].start + 0.9, H[0].start + 1.4)), o(150, 700, { ls: -0.04, a: ng, shadow: HALO }));
      var pp = ease.spring(seg(t, tPGR, tPGR + 0.6));
      if (pp > 0.001) {
        var pw = L.measure(ctx, 'PGR', { size: 76, weight: 700, font: 'd', ls: 0.02 }), px = 150, py = 322, bw = pw + 64;
        ctx.save(); ctx.translate(px + bw / 2, py + 48); ctx.scale(pp, pp); ctx.translate(-(px + bw / 2), -(py + 48));
        L.rr(ctx, px, py, bw, 96, L.R_M); ctx.fillStyle = rgba(ACC, 0.14 * ng); ctx.fill(); ctx.strokeStyle = rgba(ACC, ng); ctx.lineWidth = 2.5; ctx.stroke();
        L.text(ctx, 'PGR', px + 32, py + 70, { size: 76, weight: 700, font: 'd', ls: 0.02, color: ACC, a: ng });
        L.text(ctx, 'for short', px + bw + 30, py + 68, { size: 56, weight: 500, font: 'd', ls: -0.01, color: INK2, a: ng * seg(t, tPGR + 0.2, tPGR + 0.5) });
        ctx.restore();
      }
    }
    // one question, one answer: above the two terminals
    var qg = win(t, H[1].start - 0.1, H[1].start, RACE.tEnd, RACE.tEnd + 0.3);
    if (qg > 0.003) {
      L.riseText(ctx, 'One question.', 116, 172, ease.outQ(seg(t, H[1].start, H[1].start + 0.4)), o(112, 700, { ls: -0.045, a: qg }));
      L.riseText(ctx, 'One answer.', 986, 172, ease.outQ(seg(t, H[2].start, H[2].start + 0.4)), o(112, 700, { ls: -0.045, color: ACC, a: qg }));
    }
    // the claim, over the two terminals that have moved down; it stays until the camera leaves
    var kg = win(t, H[4].start - 0.05, H[4].start, H[5].end + 0.1, H[5].end + 0.5);
    if (kg > 0.003) {
      var pv = F.pct * ease.outQ(seg(tc, H[4].start, H[4].start + 0.7));
      L.numText(ctx, Math.round(pv) + '%', 134, 340, o(420, 700, { ls: -0.055, a: kg, counting: F.pct + '%' }));
      L.riseText(ctx, 'fewer tokens', 146, 444, ease.outQ(seg(t, H[4].start + 0.25, H[4].start + 0.6)), o(100, 700, { ls: -0.04, a: kg }));
      var q1 = [F.control.tokText + ' to ' + F.best.tokText + ' tokens per task,', 'against no index.'], q2 = ['That is one comparison:', MODEL + ',', F.labels.firstSet + '.'];
      q1.forEach(function (s1, n) { L.riseText(ctx, s1, 980, 100 + n * 66, ease.outQ(seg(t, H[4].start + 0.3 + n * 0.07, H[4].start + 0.7 + n * 0.07)), Object.assign({}, D, { a: kg, shadow: HALO })); });
      q2.forEach(function (s1, n) { L.riseText(ctx, s1, 980, 264 + n * 66, ease.outQ(seg(t, H[5].start + n * 0.07, H[5].start + 0.4 + n * 0.07)), Object.assign({}, D, { a: kg, shadow: HALO })); });
    }
  }

  // ================================================================ what it is: a program on the desktop that agents call
  // The question runs from the agent's window to PGR's as a pulse of light, and the answer runs back.
  var WQ = WH[1].start;
  var WA_LINES = [{ at: WQ + 0.15, text: LOOKS[0], kind: 'cmd', cps: 34 }, { at: WQ + 2.95, text: 'callers:', kind: 'dim' }, { at: WQ + 3.1, text: 'load_settings, start_server, run_cli', kind: 'acc', cps: 90 }];
  var WB_LINES = [{ at: WQ + 1.7, text: 'looking up parse_config', kind: 'dim' }, { at: WQ + 2.1, text: 'defined in config.py', kind: 'out' }, { at: WQ + 2.3, text: 'called by', kind: 'dim' }, { at: WQ + 2.4, text: 'load_settings, start_server, run_cli', kind: 'acc' }];
  function pulse(ctx, x0, x1, y, p, col, a) {
    if (p <= 0 || p >= 1) return;
    var e = ease.ioC(p), k;
    ctx.save(); ctx.globalCompositeOperation = 'lighter';
    for (k = 0; k < 7; k++) { var pk = Math.max(0, e - k * 0.035); L.stamp(ctx, k ? col : HOT, mix(x0, x1, pk), y, 30 - k * 3, a * (1 - k / 8) * Math.sin(Math.PI * Math.min(1, p * 1.05)), 0.5); }
    ctx.restore();
  }
  function drawWhat(ctx, t, c) {
    var ox = SO.what[0], oy = SO.what[1];
    if (!near(c, ox, oy)) return;
    ctx.save(); ctx.translate(ox, oy);
    var w0 = WH[0].start + 0.15, g = 1;
    L.riseText(ctx, 'A command-line program', 146, 160, ease.outQ(seg(t, w0, w0 + 0.5)), o(118, 700, { ls: -0.04, a: g }));
    L.riseText(ctx, 'for the desktop.', 146, 280, ease.outQ(seg(t, w0 + 0.3, w0 + 0.8)), o(118, 700, { ls: -0.04, color: ACC, a: g }));
    var bx = 150, by = 340, bw = 1620, bh = 560, ba = ease.outC(seg(t, WH[1].start - 0.4, WH[1].start + 0.2)) * g;
    if (ba > 0.003) {
      ctx.save(); ctx.globalAlpha = ba; ctx.setLineDash([10, 12]); ctx.lineWidth = 1.5; ctx.strokeStyle = 'rgba(255,255,255,0.28)';
      L.rr(ctx, bx, by, bw, bh, L.R_L); ctx.stroke(); ctx.setLineDash([]); ctx.restore();
      L.text(ctx, 'Your machine', bx + 28, by + 48, { size: 34, weight: 560, font: 't', color: INK3, a: ba });
    }
    var pa = ease.outC(seg(t, WH[1].start - 0.4, WH[1].start + 0.2)) * g, dy = (1 - ease.outC(seg(t, WH[1].start - 0.4, WH[1].start + 0.3))) * 40;
    var ty = by + 100 + dy, th = 420;
    L.terminal(ctx, 180, ty, 640, th, t, { title: 'AI coding agent', lines: WA_LINES, size: 26, a: pa, sheen: 0.8 });
    L.terminal(ctx, 1100, ty, 640, th, t, { title: 'PGR', lines: WB_LINES, size: 26, a: pa, sheen: 0.8, glow: ACC, glowK: 0.8 + 0.8 * seg(t, WQ + 1.55, WQ + 1.8), edge: ACC });
    if (pa > 0.003) {
      var ym1 = ty + 230, ym2 = ty + 300;
      ctx.save(); ctx.globalAlpha = pa; ctx.strokeStyle = 'rgba(255,255,255,0.2)'; ctx.lineWidth = 1;
      ctx.beginPath(); ctx.moveTo(826, ym1); ctx.lineTo(1094, ym1); ctx.moveTo(1094, ym2); ctx.lineTo(826, ym2); ctx.stroke(); ctx.restore();
      pulse(ctx, 830, 1090, ym1, seg(t, WQ + 0.95, WQ + 1.55), ACC, pa);
      pulse(ctx, 1090, 830, ym2, seg(t, WQ + 2.45, WQ + 3.05), HOT, pa);
    }
    L.riseText(ctx, 'Local, no account.', 1770, 280, ease.outQ(seg(t, WH[2].start + 0.15, WH[2].start + 0.55)), o(64, 700, { color: ACC, align: 'r', a: g, ls: -0.03 }));
    ctx.restore();
  }

  // ================================================================ gives: four things, each a live drawing in a window of its own
  var GTITLE = ['Definition and callers', 'Search by meaning', 'What a change reaches', 'Findings that last'];
  var GASK = ['where is load_settings defined, and who calls it?', 'which function saves a report to disk?', 'what does a change to load_settings reach?', 'what did the last session find?'];
  var GANS = ['defined in settings.py, called by main, run_job, reload and others', 'write_report, in report.py', 'every function within two calls; the rest stay dark', 'session two starts with what session one found'];
  function drawGives(ctx, t, c) {
    var i, fr, gx, gy, cu, act, p, al, x, y, w = GWIN.w, h = GWIN.h;
    // the headline sits above the four windows, in the world
    var hg = win(t, stops[1].t1 - 0.5, stops[1].t1, stops[1].leave - 0.1, stops[1].leave + 0.3);
    if (hg > 0.003 && near(c, OVERVIEW.x - 960, OVERVIEW.y - 540, 400)) L.wordsText(ctx, 'It gives an agent four things.', 5140, 125, t, GV[0].start + 0.2, o(124, 700, { ls: -0.04, a: hg }));
    for (i = 0; i < 4; i++) {
      fr = GFR[i];
      if (!near(c, fr[0], fr[1], 1500)) continue;
      cu = GV[i + 1]; act = atStop(t, 2 + i);
      x = fr[0] + GWIN.dx; y = fr[1] + GWIN.dy;
      var seen = seg(t, stops[2 + i].t1 - 0.6, stops[2 + i].t1);
      al = ease.outC(seg(t, GV[0].start - 0.2, GV[0].start + 0.5 + i * 0.12)) * (0.5 + 0.5 * Math.max(act, seen));
      L.terminal(ctx, x, y, w, h, t, { title: (i + 1) + '   ' + GTITLE[i], titleSize: 28, lines: [], a: al, sheen: act > 0.5 ? 0.9 : 0.25, glow: i === 2 ? null : ACC, glowK: 0.5 * act, cursor: false });
      ctx.save(); ctx.globalAlpha = al; L.rr(ctx, x, y + 47, w, h - 47, 0); ctx.clip();
      p = seg(t, cu.start + 0.15, cu.start + 0.15 + Math.max(2.6, cu.end - cu.start + 0.7));
      // in the overview each window shows its finished drawing, dimmed; when the camera arrives the drawing starts over from nothing
      var pv = 0.5 * (1 - seen) * (1 - seg(p, 0, 0.02));
      if (pv > 0.01) { ctx.globalAlpha = al * pv; G.list[i](ctx, 0.95, { cx: x + w / 2, cy: y + 47 + 282, s: 0.86, labels: false }); ctx.globalAlpha = al; }
      G.list[i](ctx, p, { cx: x + w / 2, cy: y + 47 + 282, s: 0.86, labels: true });
      ctx.restore();
      // what the agent asks, and the answer: typed under the drawing
      var q0 = cu.start + 0.1, a0 = cu.start + 1.9, qa = al, qn = Math.max(0, Math.floor((t - q0) * 46)), an = Math.max(0, Math.floor((t - a0) * 70));
      L.text(ctx, '>', x + 34, y + h - 66, { size: 28, font: 'm', weight: 600, color: ACC, a: qa });
      L.text(ctx, GASK[i].slice(0, qn), x + 70, y + h - 66, { size: 28, font: 'm', weight: 500, color: INK, a: qa });
      L.text(ctx, GANS[i].slice(0, an), x + 70, y + h - 24, { size: 28, font: 'm', weight: 500, color: ACC, a: qa });
    }
  }

  // ================================================================ study
  var ROWS = F.rows.slice().sort(function (a, b) { return a.tokens - b.tokens; });
  var MAXTOK = ROWS.reduce(function (m, r) { return Math.max(m, r.tokens); }, 1);
  function rowColor(r) { return r.best ? ACC : r.family === 'control' ? CONTROL : r.family === 'tool' ? [211, 211, 219] : [176, 100, 70]; }
  var TREE = (function () {
    var R = L.rng(5), nodes = [], edges = [], d, i;
    nodes.push({ x: 0.5, y: 0 });
    var prev = [0];
    for (d = 1; d <= 3; d++) {
      var cur = [], n = Math.pow(2, d);
      for (i = 0; i < n; i++) { var idx = nodes.length; nodes.push({ x: (i + 0.5) / n + (R() - 0.5) * 0.04, y: d }); cur.push(idx); edges.push([prev[Math.floor(i / 2)], idx]); }
      prev = cur;
    }
    return { nodes: nodes, edges: edges };
  })();
  var MD = [F.models.light, F.models.mid, F.models.top];

  // one: the agents and the files
  function sAgents(ctx, t, tc, c) {
    var ox = SO.agents[0], oy = SO.agents[1], a0 = ST[0].start + 0.3, g = 1, i;
    if (!near(c, ox, oy)) return;
    ctx.save(); ctx.translate(ox, oy);
    L.numText(ctx, String(Math.round(F.study.agents * ease.outQ(seg(tc, a0, a0 + 1.6)))), 150, 330, o(250, 700, { ls: -0.055, a: g, counting: String(F.study.agents) }));
    L.riseText(ctx, 'agents', 154, 410, ease.outQ(seg(t, a0 + 0.2, a0 + 0.6)), o(80, 600, { color: INK2, ls: -0.02, a: g }));
    L.numText(ctx, String(Math.round(F.study.files * ease.outQ(seg(tc, a0 + 1.4, a0 + 2.6)))), 1000, 330, o(250, 700, { ls: -0.055, a: g, counting: String(F.study.files) }));
    L.riseText(ctx, 'files, one ' + F.study.language + ' repository', 1004, 410, ease.outQ(seg(t, a0 + 1.6, a0 + 2.1)), o(60, 560, { color: INK2, ls: -0.015, a: g, shadow: HALO }));
    // 884 dots, one for each agent, and 86 squares, one for each file, lit in turn under a sweep of light
    var na = F.study.agents, cols = 52, lk = ease.outC(seg(t, a0 + 0.5, a0 + 3.4)), lit = Math.round(na * lk);
    for (i = 0; i < na; i++) {
      var on = i < lit, x = 156 + (i % cols) * 14.4, y = 480 + Math.floor(i / cols) * 17.5;
      ctx.fillStyle = on ? rgba(i % 7 === 0 ? ACC : INK2, i % 7 === 0 ? 1 : 0.8) : 'rgba(255,255,255,0.08)';
      ctx.beginPath(); ctx.arc(x, y, 3.6, 0, 6.2832); ctx.fill();
    }
    var sx = 156 + 748 * lk;
    L.track(ctx, sx, 480);
    ctx.save(); ctx.globalCompositeOperation = 'lighter'; L.stamp(ctx, ACC, sx, 640, 150, 0.5 * (lk > 0 && lk < 1 ? 1 : 0), 0.2); ctx.restore();
    var nf = F.study.files, kf = ease.outC(seg(t, a0 + 1.8, a0 + 4.0)), litf = Math.round(nf * kf);
    for (i = 0; i < nf; i++) {
      var fx = 1006 + (i % 14) * 54, fy = 480 + Math.floor(i / 14) * 54;
      L.rr(ctx, fx, fy, 38, 38, 5); ctx.fillStyle = i < litf ? rgba(INK2, 0.82) : 'rgba(255,255,255,0.08)'; ctx.fill();
    }
    ctx.restore();
  }

  // two: three questions, graded against the syntax tree
  function sQuestions(ctx, t, c) {
    var ox = SO.questions[0], oy = SO.questions[1], a0 = ST[1].start + 0.3, qs = F.study.questions, i;
    if (!near(c, ox, oy)) return;
    ctx.save(); ctx.translate(ox, oy);
    L.riseText(ctx, 'Three questions', 146, 130, ease.outQ(seg(t, a0, a0 + 0.4)), o(100, 700, { ls: -0.04 }));
    var qopt = { size: 34, weight: 500, font: 't' };
    for (i = 0; i < 3; i++) {
      var qa = ease.outC(seg(t, a0 + 0.1 + i * 0.2, a0 + 0.7 + i * 0.2)), qy = 200 + i * 236 + (1 - qa) * 30;
      L.panel(ctx, 150, qy, 1090, 206, { a: qa, sheen: 0.6, t: t });
      L.text(ctx, qs[i].title, 190, qy + 60, { size: 44, weight: 650, font: 'd', ls: -0.02, color: INK, a: qa });
      wrap(ctx, qs[i].text, 1010, qopt).forEach(function (ln, n) { L.text(ctx, ln, 190, qy + 112 + n * 42, Object.assign({ color: INK2, a: qa }, qopt)); });
    }
    // the syntax tree, drawn on, and the tick that grades against it
    var tk = seg(t, a0 + 0.8, a0 + 2.4), tx0 = 1330, ty0 = 230, tw = 420, th = 450, nodes = TREE.nodes, edges = TREE.edges;
    ctx.lineWidth = 2;
    edges.forEach(function (e, n) {
      var a = nodes[e[0]], b = nodes[e[1]], p = ease.outC(seg(tk, n / edges.length * 0.7, n / edges.length * 0.7 + 0.3));
      ctx.strokeStyle = rgba(INK2, 0.6 * p); ctx.beginPath(); ctx.moveTo(tx0 + a.x * tw, ty0 + a.y * th / 3); ctx.lineTo(mix(tx0 + a.x * tw, tx0 + b.x * tw, p), mix(ty0 + a.y * th / 3, ty0 + b.y * th / 3, p)); ctx.stroke();
    });
    nodes.forEach(function (nd, n) {
      var p = ease.outC(seg(tk, n / nodes.length * 0.7, n / nodes.length * 0.7 + 0.3));
      ctx.fillStyle = rgba(n % 3 === 0 ? ACC : INK2, p); ctx.beginPath(); ctx.arc(tx0 + nd.x * tw, ty0 + nd.y * th / 3, 10 * p, 0, 6.2832); ctx.fill();
      if (n % 3 === 0 && p > 0.3) L.stamp(ctx, ACC, tx0 + nd.x * tw, ty0 + nd.y * th / 3, 34, 0.5 * p, 0.3);
    });
    var gt = cueAt(ST[1], 'graded');
    L.riseText(ctx, 'graded against', 1290, 790, ease.outQ(seg(t, gt, gt + 0.4)), o(56, 560, { color: INK2, ls: -0.012, shadow: HALO }));
    L.riseText(ctx, 'the syntax tree.', 1290, 856, ease.outQ(seg(t, gt + 0.1, gt + 0.5)), o(56, 560, { color: INK2, ls: -0.012, shadow: HALO }));
    ctx.restore();
  }

  // three: the models of the runs
  function sModels(ctx, t, c) {
    var ox = SO.models[0], oy = SO.models[1], a0 = ST[2].start + 0.3, i;
    if (!near(c, ox, oy)) return;
    ctx.save(); ctx.translate(ox, oy);
    L.riseText(ctx, 'Three models', 146, 140, ease.outQ(seg(t, a0, a0 + 0.4)), o(100, 700, { ls: -0.04 }));
    var keys = ['haiku', 'sonnet', 'opus'];
    for (i = 0; i < 3; i++) {
      var m = MD[i], tt0 = cueAt(ST[2], keys[i]) - 0.2, ka = ease.outC(seg(t, tt0, tt0 + 0.6)), bh = 300 + i * 70, bx = 150 + i * 550, by = 700 - bh * ka;
      L.panel(ctx, bx, by, 510, bh * ka + 0.01, { a: ka, sheen: 0.5, t: t, glow: i === 2 ? ACC : null, glowK: 0.6 });
      var ta = ka * seg(ka, 0.5, 1);
      L.text(ctx, m.name, bx + 36, by + 92, { size: 52, weight: 700, font: 'd', ls: -0.03, color: i === 2 ? ACC : INK, a: ta });
      L.text(ctx, m.effort + ' effort', bx + 36, by + 144, { size: 36, weight: 560, font: 't', color: INK2, a: ta });
    }
    var sa = ease.outC(seg(t, ST[2].end - 1.2, ST[2].end - 0.6)), np = { size: 30, weight: 500, font: 't', color: INK3, a: sa };
    wrap(ctx, F.modelsNote, 1620, np).forEach(function (ln, n) { L.text(ctx, ln, 150, 790 + n * 40, np); });
    ctx.restore();
  }

  // four: the leaderboard, fewest tokens first
  function sBoard(ctx, t, tc, c) {
    var ox = SO.board[0], oy = SO.board[1], a0 = ST[3].start + 0.3, i;
    if (!near(c, ox, oy)) return;
    ctx.save(); ctx.translate(ox, oy);
    var ha = seg(t, a0, a0 + 0.3), title = F.cap.configs + ' ways to find code, fewest tokens first';
    L.text(ctx, title, 146, 92, o(60, 700, { a: ha, ls: -0.03 }));
    L.text(ctx, MODEL + ', ' + F.labels.firstSet, 146, 160, o(60, 560, { color: INK2, a: ha, ls: -0.015 }));
    L.text(ctx, 'tokens per task', 940, 222, { size: 24, weight: 560, font: 't', color: INK3, a: ha });
    L.text(ctx, 'turns', 1770, 222, { size: 24, weight: 560, font: 't', color: INK3, a: ha, align: 'r' });
    var bx0 = 940, bmax = 520;
    for (i = 0; i < ROWS.length; i++) {
      var r = ROWS[i], rt0 = a0 + 0.2 + i * 0.2, rp = ease.outC(seg(t, rt0, rt0 + 0.55)), rpc = ease.outC(seg(tc, rt0, rt0 + 0.55)), ry = 262 + i * 50 + (1 - rp) * 24, ra = rp, col = rowColor(r);
      var bw = Math.max(3, bmax * r.tokens / MAXTOK * rp);
      L.track(ctx, bx0 + bw, ry);
      if (r.best) { L.rr(ctx, 134, ry - 36, 1652, 46, L.R_M); ctx.fillStyle = 'rgba(255,122,61,' + (0.1 * ra).toFixed(3) + ')'; ctx.fill(); ctx.strokeStyle = rgba(ACC, 0.6 * ra); ctx.lineWidth = 1; ctx.stroke(); }
      var nm = { size: 26, weight: r.best ? 700 : 520, font: 't', color: r.best ? INK : INK2, a: ra };
      L.text(ctx, fit(ctx, r.name, 770, nm), 156, ry, nm);
      ctx.fillStyle = rgba(col, ra * 0.9); L.rr(ctx, bx0, ry - 24, bw, 20, 4); ctx.fill();
      if (ra > 0.2 && bw > 8) L.stamp(ctx, r.best ? ACC : INK2, bx0 + bw, ry - 14, 26, 0.5 * ra * (rpc < 1 ? 1 : 0.35), 0.4);
      var lx = bx0 + Math.max(3, bmax * r.tokens / MAXTOK * rpc) + 16;                // the label rides the frame's own time, so it never doubles
      L.numText(ctx, L.fmtTokens(r.tokens * rpc), lx, ry, { size: 28, weight: 650, font: 't', color: r.best ? INK : INK2, a: ra, counting: L.fmtTokens(r.tokens) });
      L.numText(ctx, r.turnsText, 1770, ry, { size: 28, weight: 560, font: 't', color: INK2, a: ra, align: 'r' });
      if (r.best) { var tg = F.bestLabel || 'Most reliable', tm = cueAt(ST[3], 'most reliable'); L.text(ctx, tg, 1290, ry - 2, { size: 26, weight: 700, font: 't', ls: 0.02, color: ACC, a: ra * seg(t, tm - 0.1, tm + 0.3) }); }
    }
    ctx.restore();
  }

  // five: the range over four comparisons, grouped by model
  function sRange(ctx, t, tc, c) {
    var ox = SO.range[0], oy = SO.range[1], a0 = ST[4].start + 0.3, i, j;
    if (!near(c, ox, oy)) return;
    ctx.save(); ctx.translate(ox, oy);
    L.numText(ctx, F.cap.range_min + ' to ' + F.cap.range_max, 134, 215, o(190, 700, { ls: -0.05, a: seg(t, a0, a0 + 0.3), counting: F.cap.range_min + ' to ' + F.cap.range_max }));
    L.riseText(ctx, 'fewer tokens than no index, across ' + F.cap.cells + ' comparisons.', 150, 290, ease.outQ(seg(t, a0 + 0.3, a0 + 0.8)), Object.assign({}, D, { size: 54, shadow: HALO }));
    var groups = [F.models.light, F.models.mid], sets = [{ id: 'original', name: F.labels.firstSet }, { id: 'held-out', name: F.labels.secondSet.replace(' question', '') }], n = 0;
    for (i = 0; i < 2; i++) {
      var m = groups[i], gy = 370 + i * 215, ga = ease.outC(seg(t, a0 + 0.7 + i * 0.9, a0 + 1.2 + i * 0.9));
      L.text(ctx, m.label, 150, gy, { size: 56, weight: 700, font: 'd', ls: -0.02, color: INK, a: ga });
      for (j = 0; j < 2; j++) {
        var cell = F.cells.filter(function (q) { return q.class === m.id && q.set === sets[j].id; })[0];
        if (!cell) continue;
        var t0 = a0 + 0.9 + n * 0.45, ca = ease.outC(seg(t, t0, t0 + 0.6)), cac = ease.outC(seg(tc, t0, t0 + 0.6)), cy = gy + 66 + j * 70, bwf = 800 * cell.pct / F.range.max;
        var isMax = cell.pct === F.range.max, isMin = cell.pct === F.range.min, bwx = Math.max(4, bwf * ca);
        L.track(ctx, 760 + bwx, cy);
        L.text(ctx, sets[j].name, 150, cy + 6, { size: 50, weight: 560, font: 'd', ls: -0.015, color: INK2, a: ca });
        ctx.fillStyle = rgba(isMax ? ACC : isMin ? [211, 211, 219] : [176, 100, 70], ca * 0.95); L.rr(ctx, 760, cy - 36, bwx, 46, 6); ctx.fill();
        L.numText(ctx, pct(cell.pct * cac), 760 + Math.max(4, bwf * cac) + 20, cy + 6, { size: 50, weight: 700, font: 'd', color: INK, a: ca, counting: pct(cell.pct) });
        n++;
      }
    }
    ctx.restore();
  }

  // six: fully correct, every run
  function sReliable(ctx, t, c) {
    var ox = SO.reliable[0], oy = SO.reliable[1], a0 = ST[5].start + 0.3, i;
    if (!near(c, ox, oy)) return;
    ctx.save(); ctx.translate(ox, oy);
    var best = F.best, tg = (F.bestLabel || 'Most reliable') + ': ' + best.name;
    L.riseText(ctx, tg, 150, 118, ease.outQ(seg(t, a0 - 0.1, a0 + 0.4)), o(60, 700, { color: ACC, ls: -0.02, shadow: HALO }));
    L.numText(ctx, F.reliable.perfect + ' of ' + F.reliable.runs, 134, 340, o(250, 700, { ls: -0.05, a: seg(t, a0, a0 + 0.3) }));
    L.riseText(ctx, 'runs fully correct, on both question sets.', 150, 424, ease.outQ(seg(t, a0 + 0.3, a0 + 0.8)), Object.assign({}, D, { size: 62, shadow: HALO }));
    L.riseText(ctx, F.models.light.name + ' and ' + F.models.mid.name + '.', 150, 496, ease.outQ(seg(t, a0 + 0.6, a0 + 1.1)), Object.assign({}, D, { size: 56, shadow: HALO }));
    var n12 = F.reliable.runs;
    for (i = 0; i < n12; i++) {
      var ta = ease.spring(seg(t, a0 + 0.9 + i * 0.16, a0 + 1.4 + i * 0.16)), tx = 170 + (i % 6) * 170 + (i % 6 >= 3 ? 40 : 0), ty = 560 + Math.floor(i / 6) * 140;
      ctx.save(); ctx.translate(tx + 50, ty + 50); ctx.scale(Math.max(0.001, ta), Math.max(0.001, ta));
      ctx.beginPath(); ctx.arc(0, 0, 50, 0, 6.2832); ctx.fillStyle = rgba(ACC, 0.14); ctx.fill(); ctx.strokeStyle = rgba(ACC, 1); ctx.lineWidth = 2.5; ctx.stroke();
      ctx.beginPath(); ctx.moveTo(-18, 2); ctx.lineTo(-5, 15); ctx.lineTo(20, -14); ctx.lineWidth = 6; ctx.lineCap = 'round'; ctx.lineJoin = 'round'; ctx.strokeStyle = rgba(INK, 1); ctx.stroke();
      ctx.restore();
      if (ta > 0.2 && ta < 1.05) { ctx.save(); ctx.globalCompositeOperation = 'lighter'; L.stamp(ctx, ACC, tx + 50, ty + 50, 90, 0.4 * (1.05 - ta), 0.3); ctx.restore(); }
    }
    ctx.restore();
  }

  // the limits: one repository, and two of the four things measured
  function sLimits(ctx, t, c) {
    var ox = SO.limits[0], oy = SO.limits[1], a0 = CL[0].start + 0.3, i;
    if (!near(c, ox, oy)) return;
    ctx.save(); ctx.translate(ox, oy);
    var qs = ['The limits are real:', 'one repository,', F.study.runsMin + ' to ' + F.study.runsMax + ' runs for each setup.'];
    qs.forEach(function (s, n) { L.riseText(ctx, s, 146, 150 + n * 112, ease.outQ(seg(t, a0 + n * 0.5, a0 + 0.5 + n * 0.5)), o(n === 0 ? 100 : 104, 700, { ls: -0.04, color: n === 0 ? ACC : INK })); });
    // the repository: one box, the files of the study as squares
    var ra = ease.outC(seg(t, a0 + 0.8, a0 + 1.4));
    if (ra > 0.003) {
      ctx.save(); ctx.globalAlpha = ra; ctx.setLineDash([10, 12]); ctx.lineWidth = 1.5; ctx.strokeStyle = 'rgba(255,255,255,0.3)';
      L.rr(ctx, 150, 440, 1620, 150, L.R_L); ctx.stroke(); ctx.setLineDash([]);
      L.text(ctx, 'one ' + F.study.language + ' repository', 180, 486, { size: 30, weight: 560, font: 't', color: INK3, a: 1 });
      for (i = 0; i < F.study.files; i++) { L.rr(ctx, 180 + (i % 43) * 36, 508 + Math.floor(i / 43) * 36, 28, 28, 4); ctx.fillStyle = 'rgba(184,184,193,0.5)'; ctx.fill(); }
      ctx.restore();
    }
    var s2 = CL[1].start + 0.1;
    L.riseText(ctx, 'The study measures the first two things only.', 150, 700, ease.outQ(seg(t, s2, s2 + 0.5)), Object.assign({}, D, { size: 62, shadow: HALO }));
    var names = ['Definition and callers', 'Search by meaning', 'What a change reaches', 'Findings that last'];
    names.forEach(function (s, n) {
      var ia = ease.outC(seg(t, s2 + 0.4 + n * 0.12, s2 + 0.9 + n * 0.12)), on = n < 2, x = 150 + n * 410;
      L.rr(ctx, x, 740, 390, 76, L.R_M); ctx.strokeStyle = on ? rgba(ACC, ia) : 'rgba(255,255,255,' + (0.3 * ia).toFixed(3) + ')'; ctx.lineWidth = on ? 2 : 1.5; ctx.stroke();
      if (on) { ctx.fillStyle = rgba(ACC, 0.14 * ia); ctx.fill(); }
      L.text(ctx, s, x + 26, 790, { size: 32, weight: 620, font: 't', color: on ? INK : INK2, a: ia });
    });
    ctx.restore();
  }

  // close: the name and the install command
  var CLOSE_LINES = K.installLines(F.installers.items, cueAt(CL[2], 'One command') - 0.05, 420);
  function sClose(ctx, t, c) {
    var ox = SO.close[0], oy = SO.close[1], s0 = CL[2].start + 0.1;
    if (!near(c, ox, oy)) return;
    ctx.save(); ctx.translate(ox, oy);
    L.markDraw(ctx, 146, 54, 88, seg(t, s0, s0 + 0.7), ACC, 1);
    L.riseText(ctx, 'PeterGanuRusty', 262, 124, ease.outQ(seg(t, s0, s0 + 0.5)), o(80, 700, { ls: -0.04 }));
    L.wordsText(ctx, 'One command installs it.', 140, 330, t, cueAt(CL[2], 'One command'), o(150, 700, {}));
    L.terminal(ctx, 150, 392, 1620, 500, t, { title: 'Install', lines: CLOSE_LINES, size: 34, a: ease.outC(seg(t, s0, s0 + 0.35)), sheen: 0.7, glow: ACC, glowK: 0.6 });
    ctx.restore();
  }

  // ================================================================ the film
  function draw(ctx, t, sub) {
    var c = camAt(t), tc = sub && sub.t !== undefined ? sub.t : t;
    L.background(ctx, t, -140 * Math.sin(c.x * 0.0007), -90 * Math.sin(c.y * 0.0009), 1);
    K.floor(ctx, c, t, 1 - 0.55 * seg(t, CL[2].start - 0.2, CL[2].start + 0.5));
    if (t < tPanEnd + 0.1) drawWorld(ctx, t, tc, c);
    ctx.setTransform(L.DPR, 0, 0, L.DPR, 0, 0);
    if (t > tMove0 - 0.2) {
      L.layer(ctx, c, 1.0);
      drawWhat(ctx, t, c); drawGives(ctx, t, c);
      sAgents(ctx, t, tc, c); sQuestions(ctx, t, c); sModels(ctx, t, c); sBoard(ctx, t, tc, c); sRange(ctx, t, tc, c); sReliable(ctx, t, c); sLimits(ctx, t, c); sClose(ctx, t, c);
      ctx.setTransform(L.DPR, 0, 0, L.DPR, 0, 0);
    }
    K.bokeh(ctx, c, t, 1 - 0.5 * seg(t, CL[2].start - 0.2, CL[2].start + 0.5));
    ctx.setTransform(L.DPR, 0, 0, L.DPR, 0, 0);
    if (t < tPanEnd + 0.1) { overlayCold(ctx, t, tc); overlayChange(ctx, t, tc); }
    // the names in the pictures up to here are invented examples: say so, small, for as long as they are on screen
    var ill = win(t, C[0].start, C[0].start + 0.5, stops[6].t0, stops[6].t0 + 0.5);
    if (ill > 0.003) L.text(ctx, 'Illustration: example names', 1770, 44, { size: 28, weight: 560, font: 't', color: INK3, align: 'r', a: ill * 0.9 });
  }
  function post(t) {
    var fi = 1 - seg(t, 0, 0.5), fo = seg(t, DUR - 0.3, DUR);
    return { bloom: 0.22 + 0.12 * win(t, tDone - 0.2, tDone + 0.1, tDone + 0.6, tDone + 1.4), fade: Math.max(fi, fo), vig: 0.3, thr: 0.58 };
  }
  L.film({ duration: DUR, fps: TL.fps, draw: draw, post: post, sceneTimes: TL.scenes.map(function (s) { return { id: s.id, start: s.start, dur: s.dur }; }) });
})();
