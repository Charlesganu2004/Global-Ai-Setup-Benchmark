/* The explainer. Every frame is drawn from the time t alone: window.PGR.seek(t). The picture follows the voice,
   so every reveal is keyed to a cue of build/explainer.timeline.js, and every number is read from window.FACTS
   (built from docs/data/showcase.js by tools/facts.mjs). Anything that qualifies a claim is set in the .q size
   (60 px or more on the 1920 x 1080 stage) and kept in the upper two thirds of the frame, clear of the captions. */
(function () {
  'use strict';
  var L = window.PGRLIB, G = window.PGRGFX, F = window.FACTS, TL = window.TIMELINE;
  var W = L.W, H = L.H, clamp = L.clamp, mix = L.mix, seg = L.seg, ease = L.ease, rgba = L.rgba;
  var ACC = L.ACCENT, HOT = L.HOT, INK = L.INK, INK2 = L.INK2, CONTROL = L.CONTROL;
  var ui = document.getElementById('ui'), world = L.makeCanvas('world'), ctx = world.ctx;
  var grainEl = document.getElementById('grain'), curtain = document.getElementById('curtain');
  var FPS = TL.fps, SANS = '600 34px -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif';
  var SANS36 = '500 36px -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif';
  var SANS36B = '650 36px -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif';

  // ---------- the timeline
  function sc(id) { return TL.scenes.filter(function (s) { return s.id === id; })[0]; }
  function cue(id, n) { return sc(id).cues[n - 1]; }
  // When a phrase of a cue is spoken, estimated from its place in the spoken text.
  function cueAt(c, phrase) {
    var i = c.spoken.toLowerCase().indexOf(phrase.toLowerCase());
    return c.start + (c.end - c.start) * (i < 0 ? 0 : i / c.spoken.length);
  }
  var C1 = cue('cold', 1), C2 = cue('cold', 2), C3 = cue('cold', 3), C4 = cue('cold', 4);
  var H1 = cue('change', 1), H2 = cue('change', 2), H3 = cue('change', 3), H4 = cue('change', 4), H5 = cue('change', 5), H6 = cue('change', 6);
  var GV = sc('gives'), SD = sc('study'), CL = sc('close');
  var DUR = TL.duration;

  // ---------- DOM
  function T(cls, html, x, y, size) {
    var e = L.el('div', 't ' + cls, html, ui);
    e._x = x; e._y = y; e.style.transform = 'translate3d(' + x + 'px,' + y + 'px,0)';
    if (size) e.style.fontSize = size + 'px';
    return e;
  }
  function ML(text, cls, x, y, size) {
    var l = L.maskLine(ui, text, 't ' + cls);
    l.root._x = x; l.root._y = y; l.root.style.transform = 'translate3d(' + x + 'px,' + y + 'px,0)';
    if (size) l.root.style.fontSize = size + 'px';
    return l;
  }
  function setText(e, s) { if (e._s !== s) { e._s = s; e.textContent = s; } }
  function at(e, o) { L.put(e, { x: e._x + (o.x || 0), y: e._y + (o.y || 0), s: o.s, o: o.o }); }
  function pop(line, p) { L.rise(line, p); }

  // ---------- the token mass: one particle for each 1k tokens of the no index lane
  var N = Math.round(F.control.tokens / 1000), M = Math.round(F.best.tokens / 1000);
  var TURNS = Math.max(1, Math.round(F.control.turns)), BT = Math.max(1, Math.round(F.best.turns));
  var SX = 150, SW = 1620, LANE_Y = 640, LANE_H = 150;
  var BAND_Y0 = 472, BAND_PITCH = 102, BAND_H = 84;
  var sumK = TURNS * (TURNS + 1) / 2, cum = [], bandOf = [], k, i;
  for (k = 0; k < TURNS; k++) cum.push(Math.round(N * (k + 1) * (k + 2) / 2 / sumK));
  cum[TURNS - 1] = N;
  var P = [], R = L.rng(7);
  for (i = 0, k = 0; i < N; i++) {
    while (i >= cum[k]) k++;
    var lo = k ? cum[k - 1] : 0, cnt = cum[k] - lo, j = i - lo, r1 = R(), r2 = R(), r3 = R();
    var w = SW * (k + 1) / TURNS;
    P.push({
      band: k, r1: r1, r2: r2, z: r3 < 0.2 ? 1 : r3 < 0.55 ? 0.62 : 0.3,
      su: (j + 0.5) / cnt, sw: w, sx: SX + (j + 0.5) / cnt * w + (r1 - 0.5) * 7, sy: BAND_Y0 + k * BAND_PITCH + 12 + r2 * (BAND_H - 24),
      lx: SX + (i + 0.5) / N * SW + (r1 - 0.5) * 7, ly: LANE_Y + (((i * 5) % 6) + 0.5) / 6 * LANE_H + (r2 - 0.5) * 7,
      x: 0, y: 0, a: 0, ca: 0, boost: 0
    });
  }
  var LANE_BEST_END = SX + SW * M / N;

  // ---------- flow: the cold open and the change
  // What the first figure is a figure of: one comparison, the light class on the first question set.
  var CLS = F.labels.classLabel, SETL = F.labels.firstSet, CMP = CLS + ', ' + SETL + '.';
  var f_h1 = ML('Three simple', 'display h1', 150, 250), f_h2 = ML('code lookups.', 'display h1', 150, 400);
  var f_hs = ML('One agent, no index. ' + CMP, 'q', 150, 590);
  var f_num = T('num', '0', 150, 96, 300), f_numl = ML('turns', 'sub ink2', 700, 290);
  var f_numl2 = ML('tokens per task', 'sub ink2', 860, 290);
  var f_subA = ML('No index. ' + CMP, 'q', 150, 388);
  var f_subB = ML(CMP, 'q', 150, 388);
  var f_each = [ML('Each turn, it reads', 'display', 1150, 466, 62), ML('its whole conversation', 'display', 1150, 534, 62), ML('again.', 'display', 1150, 602, 62)];
  var f_lane0 = ML('No index', 'sub ink2', 150, 556, 56);
  var f_mark = L.markSvg(ui, 62, 't'); f_mark.root._x = 150; f_mark.root._y = 550; f_mark.root.style.transform = 'translate3d(150px,550px,0)';
  var f_lane1 = ML('PeterGanuRusty', 'sub', 232, 556, 56);
  f_lane1.root.style.color = 'var(--ink)';
  var f_pct = T('num', F.pct + '%', 1080, 96, 300), f_pctl = ML('fewer tokens', 'q big', 1080, 380);
  var f_pctl2 = ML('than no index', 'q big', 1080, 452);
  var f_gap = ML(F.fewerText + ' fewer tokens', 'sub ink2', 0, 0, 52);
  var f_ask = ML('ask', 'sub s ink2', 0, 0, 48), f_ans = ML('answer', 'sub s ink2', 0, 0, 48);
  // the stream's time, in seconds: the numeral counts and the particles shorten while the voice says "134k tokens"
  var CNT = Math.max(1.2, C3.end - C3.start - 0.45);
  var COUNT0 = H4.start + 0.3, COUNT1 = Math.max(COUNT0 + 1.2, H4.end - 0.4), SHR = Math.max(1, (COUNT1 - COUNT0) / 1.6);

  function drawFlow(t) {
    var inStack = t < H1.start + 0.05;
    // ---- headline
    pop(f_h1, ease.outQ(seg(t, C1.start, C1.start + 0.6)) * (1 - ease.inC(seg(t, C2.start - 0.04, C2.start + 0.22))));
    pop(f_h2, ease.outQ(seg(t, C1.start + 0.09, C1.start + 0.69)) * (1 - ease.inC(seg(t, C2.start - 0.04, C2.start + 0.22))));
    pop(f_hs, ease.outQ(seg(t, C1.start + 0.5, C1.start + 1.0)) * (1 - ease.inC(seg(t, C2.start - 0.04, C2.start + 0.2))));

    // ---- bands: one for each turn, hard-cut cadence
    var bandT = [], b;
    for (b = 0; b < TURNS; b++) bandT.push(C2.start + b * 0.16);
    var bandsOut = 1 - ease.ioC(seg(t, H1.start + 0.05, H1.start + 0.7));

    // ---- hero numeral
    var turnsShown = 0;
    for (b = 0; b < TURNS; b++) if (t >= bandT[b]) turnsShown = b + 1;
    var filled = 0, tf = (t - C3.start) / CNT;
    if (t >= C3.start) filled = clamp(Math.round(Math.pow(clamp(tf), 1.0) * N), 0, N);
    var numText = '', numCol = 'var(--ink)';
    if (t >= C2.start && t < C3.start) numText = turnsShown >= TURNS ? F.control.turnsText : String(turnsShown);
    else if (t >= C3.start && t < COUNT0) numText = filled >= N ? F.control.tokText : (filled < 1 ? '0' : filled + 'k');
    var count = ease.outQ(seg(t, COUNT0, COUNT1));
    if (t >= COUNT0) {
      var v = mix(F.control.tokens, F.best.tokens, count);
      numText = count >= 1 ? F.best.tokText : L.fmtTokens(v);
      numCol = 'var(--accent)';
    }
    if (numText) {
      setText(f_num, numText);
      f_num.style.color = numCol;
      at(f_num, { o: 1 });
    } else L.vis(f_num, false);
    pop(f_numl, ease.outQ(seg(t, C2.start + 0.1, C2.start + 0.5)) * (1 - ease.inC(seg(t, C3.start - 0.02, C3.start + 0.18))));
    pop(f_numl2, ease.outQ(seg(t, C3.start + 0.1, C3.start + 0.5)) * (1 - ease.inC(seg(t, H5.start - 0.3, H5.start - 0.05))));
    pop(f_subA, ease.outQ(seg(t, C2.start + 0.15, C2.start + 0.55)) * (1 - ease.inC(seg(t, H4.start - 0.1, H4.start + 0.2))));
    pop(f_subB, ease.outQ(seg(t, H4.start + 0.3, H4.start + 0.7)));

    // ---- the explanation in the staircase's empty corner
    var eachIn = C4.start + 0.05, eachOut = 1 - ease.inC(seg(t, H1.start - 0.25, H1.start + 0.1));
    f_each.forEach(function (l, n) { pop(l, ease.outQ(seg(t, eachIn + n * 0.12, eachIn + n * 0.12 + 0.55)) * eachOut); });

    // ---- the lane's label: a new state when the name is spoken
    var tName = cueAt(H1, 'Peter');
    pop(f_lane0, ease.outQ(seg(t, H1.start + 0.5, H1.start + 0.9)) * (1 - ease.inC(seg(t, tName - 0.1, tName + 0.2))));
    var mk = ease.ioC(seg(t, tName, tName + 0.9));
    L.markDraw(f_mark, mk);
    pop(f_lane1, ease.outQ(seg(t, tName + 0.5, tName + 1.0)));

    // ---- the percentage
    var t64 = H5.start + 0.02;
    if (t >= t64) { at(f_pct, { y: (1 - ease.outQ(seg(t, t64, t64 + 0.45))) * 40, o: ease.outQ(seg(t, t64, t64 + 0.2)) }); setText(f_pct, F.pct + '%'); } else L.vis(f_pct, false);
    pop(f_pctl, ease.outQ(seg(t, t64 + 0.25, t64 + 0.7)));
    pop(f_pctl2, ease.outQ(seg(t, t64 + 0.33, t64 + 0.78)));

    // ---- positions of the particles
    var tMerge = H1.start + 0.05, tShort = H4.start + 0.3, tAcc = H4.start + 0.3;
    var cam = 1 + 0.012 * seg(t, 0, H1.start);
    ctx.save();
    ctx.translate(W / 2, H / 2); ctx.scale(cam, cam); ctx.translate(-W / 2, -H / 2);
    // band outlines and turn names
    for (b = 0; b < TURNS; b++) {
      var wb = SW * (b + 1) / TURNS, g = ease.outQ(seg(t, bandT[b], bandT[b] + 0.3)) * bandsOut;
      if (g <= 0) continue;
      var by = BAND_Y0 + b * BAND_PITCH;
      L.hair(ctx, 0.22 * g);
      ctx.beginPath(); if (ctx.roundRect) ctx.roundRect(SX - 12, by, wb * g + 24, BAND_H, 14); else ctx.rect(SX - 12, by, wb * g + 24, BAND_H); ctx.stroke();
      ctx.font = SANS; ctx.textBaseline = 'middle'; ctx.textAlign = 'left'; ctx.fillStyle = rgba(L.CONTROL, 0.9 * g * seg(g, 0.6, 1));
      ctx.fillText('turn ' + (b + 1), SX + wb + 36, by + BAND_H / 2);
    }
    // lane track (appears as the bands melt into it)
    var laneIn = ease.ioC(seg(t, tMerge + 0.7, tMerge + 1.4));
    var ghost = ease.outQ(seg(t, tShort + 1.5 * SHR, tShort + 2.0 * SHR));
    if (laneIn > 0) {
      L.hair(ctx, 0.16 * laneIn);
      ctx.beginPath(); if (ctx.roundRect) ctx.roundRect(SX - 16, LANE_Y - 14, SW + 32, LANE_H + 28, 18); else ctx.rect(SX - 16, LANE_Y - 14, SW + 32, LANE_H + 28); ctx.stroke();
    }
    // ghost of the no index length after the stream has shortened
    if (ghost > 0) {
      L.hair(ctx, 0.26 * ghost); ctx.setLineDash([6, 8]);
      var gx = LANE_BEST_END + 24;
      ctx.beginPath(); ctx.moveTo(gx, LANE_Y - 14); ctx.lineTo(SX + SW + 16, LANE_Y - 14); ctx.lineTo(SX + SW + 16, LANE_Y + LANE_H + 14); ctx.lineTo(gx, LANE_Y + LANE_H + 14); ctx.stroke();
      ctx.setLineDash([]);
    }

    var beams = [], mB;
    for (mB = 0; mB < TURNS; mB++) {
      var tb = C4.start + 0.15 + mB * 0.8, q = seg(t, tb, tb + 0.7);
      if (q > 0 && q < 1) beams.push({ x: SX + (SW * (mB + 1) / TURNS + 30) * q - 10, top: BAND_Y0 - 14, bot: BAND_Y0 + mB * BAND_PITCH + BAND_H + 14, q: q });
    }

    for (i = 0; i < N; i++) {
      var p = P[i], age = t - (C3.start + CNT * (i / N));
      if (age <= 0) { p.a = 0; continue; }
      var fly = ease.outQ(seg(age, 0, 0.4)), m = ease.ioC(seg(t, tMerge + 0.55 * (i / N), tMerge + 0.55 * (i / N) + 1.15));
      // the stream moves along its band; near layers faster; it stops where the merge begins
      var du = ((p.su + (14 + 26 * p.z) * Math.min(age, tMerge - (C3.start + CNT * (i / N))) / p.sw) % 1 + 1) % 1;
      var sxx = SX + du * p.sw + (p.r1 - 0.5) * 7;
      var edge = clamp(du * p.sw / 70) * clamp((1 - du) * p.sw / 70);
      var x = mix(sxx, p.lx, m), y = mix(p.sy, p.ly, m) - Math.sin(m * Math.PI) * (p.r1 - 0.5) * 110;
      x -= (1 - fly) * 240;
      x += Math.sin(t * 0.9 + i * 1.3) * 1.5; y += Math.cos(t * 0.7 + i * 1.9) * 1.5;
      var a = seg(age, 0, 0.1) * (0.55 + 0.45 * p.z) * mix(edge, 1, m), boost = 1.6 * (1 - seg(age, 0, 0.35)), ca = 0, flash = 0;
      // the stream's pulse runs at the speed of its turns: five for no index, two for the best
      if (m > 0.4) a *= 0.8 + 0.2 * Math.sin(p.lx * 0.018 - t * 2.3 * mix(TURNS, BT, i < M ? ease.ioC(seg(t, tAcc + 0.9 * SHR * (i / M), tAcc + 0.9 * SHR * (i / M) + 0.5 * SHR)) : 0));
      if (i < M) ca = ease.ioC(seg(t, tAcc + 0.9 * SHR * (i / M), tAcc + 0.9 * SHR * (i / M) + 0.5 * SHR));
      else {
        var d0 = tShort + 1.1 * SHR * ((N - 1 - i) / (N - M)), d = ease.inC(seg(t, d0, d0 + 0.5 * SHR));
        a *= 1 - d; y += d * 34; flash = Math.sin(d * Math.PI);
      }
      if (inStack || m < 1) {
        // the read head lights what it passes
        for (var bi = 0; bi < beams.length; bi++) {
          var bm = beams[bi];
          if (y >= bm.top && y <= bm.bot) { var dx = (x - bm.x) / 90; boost += Math.exp(-dx * dx) * 1.5; }
        }
      }
      p.x = x; p.y = y; p.a = a; p.ca = ca; p.boost = boost + flash * 1.5;
    }
    // beams: a thin line and a soft trail behind it
    for (mB = 0; mB < beams.length; mB++) {
      var bm2 = beams[mB], tr = ctx.createLinearGradient(bm2.x - 220, 0, bm2.x, 0);
      tr.addColorStop(0, 'rgba(244,244,246,0)'); tr.addColorStop(1, 'rgba(244,244,246,0.075)');
      ctx.fillStyle = tr; ctx.fillRect(bm2.x - 220, bm2.top, 220, bm2.bot - bm2.top);
      ctx.fillStyle = 'rgba(244,244,246,0.85)'; ctx.fillRect(bm2.x - 0.5, bm2.top, 1.5, bm2.bot - bm2.top);
    }
    drawParticles();
    ctx.restore();

    // ---- the two turns of the best lane, and the gap that was saved
    if (t >= H2.start) {
      var nodeY = LANE_Y + LANE_H + 64, n1 = ease.outQ(seg(t, H2.start, H2.start + 0.3)), n2 = ease.outQ(seg(t, H3.start, H3.start + 0.3));
      var span = ease.ioC(seg(t, H3.start, H3.start + 0.55)), x1 = SX, x2 = LANE_BEST_END;
      L.hair(ctx, 0.5); ctx.beginPath(); ctx.moveTo(x1, nodeY); ctx.lineTo(mix(x1, x2, span), nodeY); ctx.stroke();
      ctx.beginPath(); ctx.arc(x1, nodeY, 13 * n1, 0, Math.PI * 2); ctx.fillStyle = rgba(ACC, 1); ctx.fill();
      L.stamp(ctx, ACC, x1, nodeY, 50 * n1, 0.4 * n1);
      ctx.beginPath(); ctx.arc(x2, nodeY, 13 * n2, 0, Math.PI * 2); ctx.fillStyle = rgba(ACC, 1); ctx.fill();
      L.stamp(ctx, ACC, x2, nodeY, 50 * n2, 0.4 * n2);
      f_ask._x = x1 - 12; f_ask._y = nodeY + 28; f_ans._x = x2 - 90; f_ans._y = nodeY + 28;
      f_ask.root.style.transform = 'translate3d(' + f_ask._x + 'px,' + f_ask._y + 'px,0)'; f_ans.root.style.transform = 'translate3d(' + f_ans._x + 'px,' + f_ans._y + 'px,0)';
      pop(f_ask, ease.outQ(seg(t, H2.start + 0.1, H2.start + 0.45)));
      pop(f_ans, ease.outQ(seg(t, H3.start + 0.1, H3.start + 0.45)));
    } else { L.vis(f_ask.root, false); L.vis(f_ans.root, false); }
    f_gap.root._x = LANE_BEST_END + 24 + 40; f_gap.root._y = LANE_Y + LANE_H / 2 - 30;
    f_gap.root.style.transform = 'translate3d(' + f_gap.root._x + 'px,' + f_gap.root._y + 'px,0)';
    var tGap = cueAt(H5, 'in two') - 0.1;
    pop(f_gap, ease.outQ(seg(t, tGap, tGap + 0.5)));
    // the label of the figure: each half is underlined as the voice says it ("light class," then "first question set")
    var tLight = cueAt(H6, 'light'), tFirst = cueAt(H6, 'first');
    var u1 = ease.outQ(seg(t, tLight - 0.1, tLight + 0.5)), u2 = ease.outQ(seg(t, tFirst - 0.1, tFirst + 0.6));
    if (u1 > 0 || u2 > 0) {
      var sp = labelSpans();
      ctx.fillStyle = rgba(ACC, 0.9);
      ctx.fillRect(150, 462, sp.a * u1, 3); ctx.fillRect(150 + sp.b0, 462, (sp.b1 - sp.b0) * u2, 3);
    }
  }
  // where the two halves of the label lie, measured on the text itself once the fonts are in
  var spans = null;
  function labelSpans() {
    if (spans) return spans;
    var node = f_subB.inner.firstChild, text = node.nodeValue, rg = document.createRange();
    var w = function (to) { rg.setStart(node, 0); rg.setEnd(node, to); return rg.getBoundingClientRect().width; };
    spans = { a: w(text.indexOf(',') + 1), b0: w(text.indexOf('first')), b1: w(text.length) };
    return spans;
  }

  function drawParticles() {
    var i, p, r;
    ctx.globalCompositeOperation = 'lighter';
    for (i = 0; i < N; i++) {
      p = P[i]; if (p.a <= 0.004) continue;
      r = 3.4 + 1.9 * p.z;
      var gs = r * (4.2 + p.boost * 1.8), ga = p.a * (0.22 + p.boost * 0.4);
      if (p.ca < 1) L.stamp(ctx, p.boost > 0.6 ? INK : CONTROL, p.x, p.y, gs, ga * (1 - p.ca));
      if (p.ca > 0) L.stamp(ctx, ACC, p.x, p.y, gs * 1.2, ga * p.ca * 1.6);
    }
    ctx.globalCompositeOperation = 'source-over';
    for (i = 0; i < N; i++) {
      p = P[i]; if (p.a <= 0.004) continue;
      r = 3.4 + 1.9 * p.z;
      var base = L.lerpC(CONTROL, INK2, p.z), hotC = L.lerpC(base, INK, clamp(p.boost / 1.6));
      var col = L.lerpC(hotC, p.ca > 0 ? L.lerpC(ACC, HOT, clamp(p.boost / 2) * 0.6) : hotC, p.ca);
      ctx.fillStyle = rgba(col, clamp(p.a + p.boost * 0.15));
      ctx.beginPath(); ctx.arc(p.x, p.y, r * (1 + p.boost * 0.12), 0, Math.PI * 2); ctx.fill();
    }
  }

  // ---------- gives: four things, drawn as outcomes
  var STATEMENTS = [
    ['Definitions and callers.', 'Both, in one lookup.'],
    ['Search by meaning.', 'Describe the job. Get the function.'],
    ['What a change reaches.', 'Before anyone edits.'],
    ['Findings that stay.', 'Remembered between sessions.']
  ];
  // the study measured the first two only; the sample names in the pictures are made up
  var MEASURED = [true, true, false, false];
  var GV_NAMES = [['Definitions', 'and callers'], ['Search by', 'meaning'], ['What a change', 'reaches'], ['Findings', 'that stay']];
  var gv_h = STATEMENTS.map(function (s) { return ML(s[0], 'display h2', 150, 92); });
  var gv_s = STATEMENTS.map(function (s) { return ML(s[1], 'sub', 150, 232); });
  var gv_tag = MEASURED.map(function (m) { return T('tag' + (m ? '' : ' off'), m ? 'Measured' : 'Not yet measured', 150, 20, 44); });
  var gv_ill = T('tag off r', 'Illustration: example names', -150, 20, 44);
  var gv_lbl = GV_NAMES.map(function (s) { return ML(s[0] + '<br>' + s[1], 'sub s ink2 ctr', 0, 0, 46); });
  var gv_st = MEASURED.map(function (m) { return ML(m ? 'Measured' : 'Not yet measured', 'sub s ctr ' + (m ? 'acc' : 'ink2'), 0, 0, 48); });
  var GV_CENTERS = [315, 745, 1175, 1605];

  function drawGives(t) {
    var cues = GV.cues, g1 = cues[0], k;
    // g1: the four, small, with what the study measured; each statement then takes the stage
    var rowOut = 1 - ease.inC(seg(t, cues[1].start - 0.3, cues[1].start - 0.05));
    for (k = 0; k < 4; k++) {
      var st = g1.start + 0.15 + k * 0.28, p = ease.outC(seg(t, st, st + 0.9));
      if (p > 0 && rowOut > 0) {
        ctx.globalAlpha = rowOut * (MEASURED[k] ? 1 : 0.6);
        G.list[k](ctx, p, { cx: GV_CENTERS[k], cy: 480, s: 0.34, labels: false });
        ctx.globalAlpha = 1;
      }
      var lb = gv_lbl[k], sb = gv_st[k];
      lb.root._x = GV_CENTERS[k] - lb.root.offsetWidth / 2; lb.root._y = 612; lb.root.style.transform = 'translate3d(' + lb.root._x + 'px,612px,0)';
      sb.root._x = GV_CENTERS[k] - sb.root.offsetWidth / 2; sb.root._y = 332; sb.root.style.transform = 'translate3d(' + sb.root._x + 'px,332px,0)';
      pop(lb, ease.outQ(seg(t, st + 0.3, st + 0.7)) * rowOut);
      pop(sb, ease.outQ(seg(t, st + 0.3, st + 0.7)) * rowOut);
    }
    var illO = 0;
    for (k = 0; k < 4; k++) {
      var c = cues[k + 1], t0 = c.start - 0.02, t1 = (k < 3 ? cues[k + 2].start : GV.start + GV.dur) - 0.02;
      var build = seg(t, t0 + 0.05, t0 + (c.end - c.start) * 0.9);
      var inP = ease.outQ(seg(t, t0, t0 + 0.55)), outP = 1 - ease.inC(seg(t, t1 - 0.2, t1));
      if (k === 3) outP = 1;
      if (t >= t0 - 0.01 && t < t1 + 0.05) {
        ctx.globalAlpha = Math.min(1, inP * 2) * (k === 3 ? 1 : outP);
        G.list[k](ctx, ease.ioC(build), { cx: 1000, cy: 620, s: 1.1, labels: true });
        ctx.globalAlpha = 1;
      }
      pop(gv_h[k], inP * outP); pop(gv_s[k], ease.outQ(seg(t, t0 + 0.14, t0 + 0.69)) * outP);
      at(gv_tag[k], { o: ease.outQ(seg(t, t0 + 0.1, t0 + 0.45)) * outP });
      illO = Math.max(illO, ease.outQ(seg(t, t0 + 0.1, t0 + 0.45)) * outP);
    }
    at(gv_ill, { o: illO });
  }

  // ---------- study
  var Q = F.study.questions || [];
  var QY0 = 110, QPITCH = 158;
  var sd_n = T('num', '0', 150, 96, 300), sd_nl = ML('agents', 'sub ink2', 700, 290);
  var sd_n2 = T('num', String(F.study.files), 1010, 96, 300), sd_n2l = ML(F.study.language + ' files,', 'sub ink2', 1390, 234), sd_n2l2 = ML('one repository', 'sub ink2', 1390, 290);
  var sd_q = Q.map(function (q, n) { return ML(q.title, 'display', 150, QY0 + n * QPITCH, 78); });
  var CLASS_NAMES = (window.SHOWCASE && window.SHOWCASE.classes || []).map(function (c) { return c.name; });
  var sd_c = CLASS_NAMES.map(function (n, ci) { return T('pill', n, 150 + ci * 230, 622, 52); });
  var sd_cap = ML('Graded against the syntax tree.', 'q', 150, 548);
  // the leaderboard: one row for each configuration, fewest tokens first. The bar is the row's background, so the
  // name, the bar and the figures can only be read as one row. Tokens and turns only: no fully-correct counts here.
  var sd_lbT = ML(CMP, 'q', 150, 36);
  var sd_lbN = ML('Tokens per task and turns, fewest tokens first. Marked: ' + F.bestLabel.toLowerCase() + '.', 'sub s ink2', 150, 108, 44);
  var ROWS = F.rows.slice().sort(function (a, b) { return a.tokens - b.tokens || a.turns - b.turns; });
  var MAXTOK = Math.max.apply(null, ROWS.map(function (r) { return r.tokens; }));
  var LB_Y0 = 214, LB_PITCH = Math.min(60, 790 / ROWS.length), LB_CELL = LB_PITCH - 8, LB_X0 = 150, LB_BAR_MAX = 1250;
  var LB_FONT = Math.min(36, Math.floor(LB_CELL * 0.7));
  var sd_rows = ROWS.map(function (r, n) {
    var y = LB_Y0 + n * LB_PITCH, cy = y + LB_CELL / 2;
    var nm = T('lbname', r.name.replace(' (control)', ''), LB_X0 + 18, cy - LB_FONT * 0.56, LB_FONT);
    nm.style.color = r.best ? 'var(--ink)' : 'var(--ink2)';
    if (r.best) nm.style.fontWeight = '650';
    var val = T('lbval r', '', -150, cy - 33 * 0.56, 33);
    var tag = r.best ? T('tag', F.bestLabel, 0, cy - 18, 34) : null;
    return { r: r, y: y, nm: nm, val: val, tag: tag };
  });
  // the saving in four comparisons: two model classes by two question sets
  var CELLS = F.cells;
  var CLASS_IDS = [], SET_IDS = [];
  CELLS.forEach(function (q) { if (CLASS_IDS.indexOf(q.class) < 0) CLASS_IDS.push(q.class); if (SET_IDS.indexOf(q.set) < 0) SET_IDS.push(q.set); });
  function setLabel(q) { return q.set === SET_IDS[0] ? SETL : q.setName.toLowerCase() + ' set'; }
  var rg_t = ML(F.range.min + '% to ' + F.range.max + '%', 'display', 150, 36, 230);
  var rg_s = ML('fewer tokens than no index, across ' + F.range.n + ' comparisons.', 'q big', 150, 262);
  var rg_cells = CELLS.map(function (q) {
    var x = 150 + SET_IDS.indexOf(q.set) * 820, y = 380 + CLASS_IDS.indexOf(q.class) * 165;
    var num = T('num', q.pct + '%', x, y, 130);
    num.style.color = q.headline ? 'var(--accent)' : 'var(--ink)';
    return { q: q, num: num, l1: ML(q.className + ' class,', 'q', x + 340, y + 8), l2: ML(setLabel(q), 'q', x + 340, y + 74) };
  });
  // the highlighted configuration: fully correct in every run on both question sets (its own count, nothing set against it)
  var CLASS_LIST = CLASS_IDS.map(function (id) { return CELLS.filter(function (q) { return q.class === id; })[0].className; });
  var rl_tag = T('tag', F.bestLabel, 150, 70, 56);
  var rl_n = T('num', F.reliable.perfect + ' of ' + F.reliable.runs, 150, 190, 300);
  rl_n.style.color = 'var(--accent)';
  var rl_l1 = ML('runs fully correct,', 'q big', 150, 480), rl_l2 = ML('on both question sets.', 'q big', 150, 552);
  var rl_l3 = ML(CLASS_LIST.join(' and ') + (CLASS_LIST.length > 1 ? ' classes.' : ' class.'), 'q', 150, 632);

  function rowColor(r) { return r.best ? ACC : r.family === 'control' ? CONTROL : r.family === 'tool' ? [211, 211, 219] : [176, 100, 70]; }

  function drawStudy(t) {
    var c1 = SD.cues[0], c2 = SD.cues[1], c3 = SD.cues[2], c4 = SD.cues[3], c5 = SD.cues[4], c6 = SD.cues[5], i;
    var tRepo = cueAt(c1, 'one Python') - 0.2;
    var phase1 = 1 - ease.inC(seg(t, c2.start - 0.25, c2.start));
    // 1. agents and the repository
    var cnt = ease.outQ(seg(t, c1.start + 0.15, c1.start + 1.9)), n = Math.round(F.study.agents * cnt);
    if (t >= c1.start && phase1 > 0) {
      setText(sd_n, String(n)); at(sd_n, { o: phase1, y: (1 - ease.outQ(seg(t, c1.start, c1.start + 0.4))) * 40 }); pop(sd_nl, ease.outQ(seg(t, c1.start + 0.3, c1.start + 0.7)) * phase1);
      var COLS = 39, ROWSM = Math.ceil(F.study.agents / COLS);
      for (i = 0; i < F.study.agents; i++) {
        var lit = i < n, cx = 150 + (i % COLS) * 22 + 11, cy = 500 + Math.floor(i / COLS) * 22 + 11;
        var near = lit ? clamp(1 - (n - i) / 40) : 0;
        ctx.fillStyle = lit ? rgba(INK, (0.35 + 0.55 * near) * phase1) : 'rgba(255,255,255,' + (0.07 * phase1).toFixed(3) + ')';
        ctx.beginPath(); ctx.arc(cx, cy, lit ? 5 : 3.5, 0, Math.PI * 2); ctx.fill();
        if (lit && n - i < 14) L.stamp(ctx, INK, cx, cy, 16, (1 - (n - i) / 14) * 0.5 * phase1);
      }
      if (t >= tRepo) {
        var rp = ease.outQ(seg(t, tRepo, tRepo + 0.5));
        setText(sd_n2, String(F.study.files)); at(sd_n2, { o: rp * phase1, y: (1 - rp) * 40 });
        pop(sd_n2l, ease.outQ(seg(t, tRepo + 0.2, tRepo + 0.6)) * phase1); pop(sd_n2l2, ease.outQ(seg(t, tRepo + 0.28, tRepo + 0.68)) * phase1);
        var FC = 12, ft = F.study.files, tiles = Math.min(ft, 96);
        for (i = 0; i < tiles; i++) {
          var tx = 1100 + (i % FC) * 55, ty = 500 + Math.floor(i / FC) * 58, tp = ease.outC(seg(t, tRepo + 0.15 + i * 0.012, tRepo + 0.45 + i * 0.012)) * phase1;
          if (tp <= 0) continue;
          ctx.fillStyle = 'rgba(244,244,246,' + (0.11 * tp).toFixed(3) + ')'; ctx.strokeStyle = 'rgba(244,244,246,' + (0.5 * tp).toFixed(3) + ')'; ctx.lineWidth = 1;
          ctx.beginPath(); if (ctx.roundRect) ctx.roundRect(tx, ty, 36, 44, 5); else ctx.rect(tx, ty, 36, 44); ctx.fill(); ctx.stroke();
        }
      } else { L.vis(sd_n2, false); L.vis(sd_n2l.root, false); L.vis(sd_n2l2.root, false); }
    } else {
      L.vis(sd_n, false); L.vis(sd_nl.root, false); L.vis(sd_n2, false); L.vis(sd_n2l.root, false); L.vis(sd_n2l2.root, false);
    }
    // 2. three questions, graded against the syntax tree, then the three classes one by one (stay until the leaderboard)
    var qOut = 1 - ease.inC(seg(t, c4.start - 0.3, c4.start - 0.02));
    var ph2 = ease.outQ(seg(t, c2.start, c2.start + 0.2)) * qOut;
    var tGraded = cueAt(c2, 'graded');
    sd_q.forEach(function (l, qn) {
      var s0 = c2.start + 0.05 + qn * 0.34;
      pop(l, ease.outQ(seg(t, s0, s0 + 0.55)) * (ph2 > 0 ? 1 : 0) * qOut);
    });
    if (ph2 > 0) {
      // the syntax tree, and a neutral marker beside each question: the key they were graded against
      var tree = ease.outC(seg(t, tGraded - 0.1, tGraded + 0.9));
      drawTree(1360, 200, tree, ph2, t, tGraded);
      Q.forEach(function (q, qn) {
        var s0 = tGraded + 0.35 + qn * 0.4, mk = ease.outC(seg(t, s0, s0 + 0.3));
        if (mk <= 0) return;
        var my = QY0 + qn * QPITCH + 64, mx = 1190;
        ctx.fillStyle = rgba(INK, 0.7 * ph2); ctx.beginPath(); ctx.arc(mx, my, 13 * mk, 0, Math.PI * 2); ctx.fill();
        L.stamp(ctx, INK, mx, my, 34, 0.3 * mk * ph2);
      });
    }
    pop(sd_cap, ease.outQ(seg(t, tGraded - 0.1, tGraded + 0.4)) * ph2);
    sd_c.forEach(function (e, ci) {
      var s0 = cueAt(c3, CLASS_NAMES[ci].toLowerCase()) - 0.1;
      at(e, { o: ease.outQ(seg(t, s0, s0 + 0.3)) * qOut, y: (1 - ease.outQ(seg(t, s0, s0 + 0.4))) * 24 });
    });
    // 3. the leaderboard forms, fewest tokens first; the most reliable row is marked
    var lbOut = 1 - ease.inC(seg(t, c5.start - 0.1, c5.start + 0.25));
    pop(sd_lbT, ease.outQ(seg(t, c4.start, c4.start + 0.5)) * lbOut);
    pop(sd_lbN, ease.outQ(seg(t, c4.start + 0.3, c4.start + 0.8)) * lbOut);
    var nrows = sd_rows.length, span = (c4.end - c4.start) * 0.62;
    sd_rows.forEach(function (row, rn) {
      var r = row.r, order = nrows - 1 - rn;                   // the last place is drawn first, the first last
      var s0 = c4.start + 0.1 + order * (span / nrows), p = ease.outQ(seg(t, s0, s0 + 0.5));
      var o = p * lbOut;
      at(row.nm, { o: o, x: (1 - p) * -40 });
      var grow = ease.outQ(seg(t, s0 + 0.05, s0 + 0.7)), len = LB_BAR_MAX * r.tokens / MAXTOK * grow;
      var col = rowColor(r), flash = r.best ? seg(t, c4.end - 0.1, c4.end + 0.15) : 0;
      if (o > 0) {
        ctx.fillStyle = 'rgba(255,255,255,0.05)'; ctx.fillRect(LB_X0, row.y, LB_BAR_MAX, LB_CELL);
        ctx.fillStyle = rgba(col, (r.best ? 0.55 : 0.36) * o); ctx.fillRect(LB_X0, row.y, len, LB_CELL);
        ctx.fillStyle = rgba(col, (r.best ? 1 : 0.8) * o); ctx.fillRect(LB_X0 + len - 4, row.y, 4, LB_CELL);
        if (r.best) { L.stamp(ctx, ACC, LB_X0 + len, row.y + LB_CELL / 2, 54, 0.55 * o * (0.6 + 0.4 * ease.outQ(flash))); }
      }
      var shownTok = Math.round(mix(0, r.tokens, grow));
      var vtxt = (len > 8 ? (seg(t, s0 + 0.7, s0 + 0.75) >= 1 ? r.tokText : L.fmtTokens(shownTok)) : '') + ' tokens, ' + r.turnsText + ' turns';
      setText(row.val, vtxt); row.val.style.color = r.best ? 'var(--ink)' : 'var(--ink2)';
      L.put(row.val, { x: row.val._x, y: row.val._y, o: o * seg(len, 6, 60) });
      if (row.tag) {
        row.tag._x = LB_X0 + 18 + row.nm.offsetWidth + 26; row.tag._y = row.y + LB_CELL / 2 - 18;
        at(row.tag, { o: ease.outQ(seg(t, c4.end - 0.5, c4.end - 0.1)) * lbOut });
      }
    });
    // 4. the saving in four comparisons
    var rgOut = 1 - ease.inC(seg(t, c6.start - 0.1, c6.start + 0.2));
    var tNum = cueAt(c5, 'twenty-eight') - 0.15, tGrid = tNum + 1.6;
    pop(rg_t, ease.outQ(seg(t, tNum, tNum + 0.55)) * rgOut);
    pop(rg_s, ease.outQ(seg(t, tNum + 0.4, tNum + 0.9)) * rgOut);
    rg_cells.forEach(function (cell, cn) {
      var s0 = tGrid + cn * 0.55, a = ease.outQ(seg(t, s0, s0 + 0.4)) * rgOut;
      at(cell.num, { o: a, y: (1 - ease.outQ(seg(t, s0, s0 + 0.5))) * 30 });
      pop(cell.l1, ease.outQ(seg(t, s0 + 0.1, s0 + 0.5)) * rgOut); pop(cell.l2, ease.outQ(seg(t, s0 + 0.18, s0 + 0.58)) * rgOut);
    });
    // 5. the highlighted configuration, fully correct in every run on both question sets
    var tFull = cueAt(c6, 'fully'), tOn = cueAt(c6, 'on both');
    at(rl_tag, { o: ease.outQ(seg(t, c6.start + 0.05, c6.start + 0.4)), y: (1 - ease.outQ(seg(t, c6.start + 0.05, c6.start + 0.5))) * 20 });
    at(rl_n, { o: ease.outQ(seg(t, tFull - 0.1, tFull + 0.35)), y: (1 - ease.outQ(seg(t, tFull - 0.1, tFull + 0.45))) * 40 });
    pop(rl_l1, ease.outQ(seg(t, tFull + 0.15, tFull + 0.55)));
    pop(rl_l2, ease.outQ(seg(t, tOn, tOn + 0.4)));
    pop(rl_l3, ease.outQ(seg(t, tOn + 0.25, tOn + 0.65)));
  }

  // a small syntax tree, drawn as hairlines: the key the answers were graded against
  var TREE = (function () {
    var nodes = [{ x: 200, y: 0, d: 0, p: -1 }], levels = [1, 3, 6, 9], idx = [[0]], k, j;
    for (k = 1; k < levels.length; k++) {
      idx[k] = [];
      for (j = 0; j < levels[k]; j++) {
        var parent = idx[k - 1][Math.min(idx[k - 1].length - 1, Math.floor(j * idx[k - 1].length / levels[k]))];
        nodes.push({ x: 200 + (j - (levels[k] - 1) / 2) * (420 / levels[k]), y: k * 118, d: k, p: parent });
        idx[k].push(nodes.length - 1);
      }
    }
    return nodes;
  })();
  function drawTree(ox, oy, p, op, t, tGraded) {
    ctx.save(); ctx.translate(ox, oy);
    TREE.forEach(function (n, i) {
      if (n.p < 0) return;
      var q = ease.outC(seg(p, n.d * 0.22, n.d * 0.22 + 0.3)), a = TREE[n.p];
      L.hair(ctx, 0.35 * op * q); ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(mix(a.x, n.x, q), mix(a.y, n.y, q)); ctx.stroke();
    });
    TREE.forEach(function (n, i) {
      var q = ease.outC(seg(p, n.d * 0.22 + 0.08, n.d * 0.22 + 0.3));
      if (q <= 0) return;
      var hit = n.d === 3 && (i % 3 === 0) ? ease.outQ(seg(t, tGraded + 0.5 + (i % 7) * 0.12, tGraded + 0.9 + (i % 7) * 0.12)) : 0;
      ctx.fillStyle = rgba(hit > 0 ? ACC : INK, (hit > 0 ? 1 : 0.55) * op * q); ctx.beginPath(); ctx.arc(n.x, n.y, 7 + hit * 3, 0, Math.PI * 2); ctx.fill();
      if (hit > 0) L.stamp(ctx, ACC, n.x, n.y, 30, 0.55 * hit * op);
    });
    ctx.restore();
  }

  // ---------- close
  var cl_1 = ML('One repository.', 'display h1', 150, 20, 96);
  var cl_2 = ML(F.study.runsMin + ' to ' + F.study.runsMax + ' runs for each setup.', 'display h1', 150, 120, 96);
  var cl_3 = ML('Measured: the first two.', 'display h1', 150, 220, 96);
  var cl_g = GV_NAMES.map(function (s) { return ML(s[0] + '<br>' + s[1], 'sub s ctr', 0, 0, 44); });
  var cl_st = MEASURED.map(function (m) { return ML(m ? 'Measured' : 'Not measured', 'sub s ctr ' + (m ? 'acc' : 'ink2'), 0, 0, 48); });
  var cl_nm = ML('PeterGanuRusty', 'display', 0, 0, 156);
  var cl_pp = ML('Private preview', 'q big', 0, 0);
  var cl_mk = L.markSvg(ui, 340, 't');

  function drawClose(t) {
    var c1 = CL.cues[0], c2 = CL.cues[1], c3 = CL.cues[2], k;
    var pg = 1 - ease.inC(seg(t, c3.start - 0.5, c3.start - 0.05));
    var tRuns = cueAt(c1, 'a few runs');
    pop(cl_1, ease.outQ(seg(t, c1.start + 0.05, c1.start + 0.55)) * pg);
    pop(cl_2, ease.outQ(seg(t, tRuns - 0.1, tRuns + 0.45)) * pg);
    var o2 = ease.outQ(seg(t, c2.start, c2.start + 0.5));
    pop(cl_3, o2 * pg);
    // the four things from before: the first two measured, the other two not
    for (k = 0; k < 4; k++) {
      var cx = GV_CENTERS[k], s0 = c2.start + 0.25 + k * 0.12, a = ease.outC(seg(t, s0, s0 + 0.6)) * pg;
      if (a > 0) {
        ctx.save(); ctx.globalAlpha = a * (k < 2 ? 1 : 0.4);
        G.list[k](ctx, 1, { cx: cx, cy: 495, s: 0.34, labels: false });
        ctx.restore();
        if (k > 1) { L.hair(ctx, 0.5 * a); ctx.setLineDash([5, 7]); ctx.beginPath(); if (ctx.roundRect) ctx.roundRect(cx - 180, 495 - 118, 360, 236, 18); else ctx.rect(cx - 180, 495 - 118, 360, 236); ctx.stroke(); ctx.setLineDash([]); }
      }
      var l = cl_g[k], sb = cl_st[k];
      l.root._x = cx - l.root.offsetWidth / 2; l.root._y = 608; l.root.style.transform = 'translate3d(' + l.root._x + 'px,608px,0)';
      sb.root._x = cx - sb.root.offsetWidth / 2; sb.root._y = 326; sb.root.style.transform = 'translate3d(' + sb.root._x + 'px,326px,0)';
      pop(l, ease.outQ(seg(t, s0 + 0.2, s0 + 0.6)) * pg);
      pop(sb, ease.outQ(seg(t, s0 + 0.2, s0 + 0.6)) * pg);
    }
    // the name and the mark
    var tn = c3.start - 0.05;
    var mp = ease.ioC(seg(t, tn, tn + 0.9)), np = ease.outQ(seg(t, tn + 0.5, tn + 1.1)), pp = ease.outQ(seg(t, tn + 0.85, tn + 1.3));
    cl_mk.root._x = 150; cl_mk.root._y = 330;
    cl_mk.root.style.transform = 'translate3d(150px,330px,0)';
    L.markDraw(cl_mk, mp);
    cl_nm.root._x = 560; cl_nm.root._y = 400; cl_nm.root.style.transform = 'translate3d(560px,400px,0)';
    cl_pp.root._x = 566; cl_pp.root._y = 590; cl_pp.root.style.transform = 'translate3d(566px,590px,0)';
    pop(cl_nm, np); pop(cl_pp, pp);
    // the filament under the name: the best lane, once
    var fl = ease.outQ(seg(t, tn + 0.9, tn + 1.6));
    if (fl > 0) {
      var fx0 = 566, fx1 = fx0 + (1120 - 0) * M / N * 1.6 * fl, fy = 760;
      ctx.fillStyle = rgba(ACC, 0.9); ctx.fillRect(fx0, fy, Math.max(0, fx1 - fx0), 2);
      var grad = ctx.createLinearGradient(fx0, 0, fx1, 0); grad.addColorStop(0, 'rgba(255,122,61,0)'); grad.addColorStop(1, 'rgba(255,122,61,0.3)');
      ctx.fillStyle = grad; ctx.fillRect(fx0, fy - 10, Math.max(0, fx1 - fx0), 22);
      L.stamp(ctx, ACC, fx1, fy + 1, 46, 0.8 * fl);
    }
  }

  // ---------- the film
  var domAll = Array.prototype.slice.call(ui.children);

  // which scene draws at time t, and the curtain between them
  var BOUNDS = [GV.start, SD.start, CL.start];
  function curtainAt(t) {
    var x = -101;
    BOUNDS.forEach(function (b) {
      var cover = seg(t, b - 0.22, b), uncover = seg(t, b + 0.02, b + 0.34);
      if (cover > 0 && uncover === 0) x = mix(-101, 0, ease.inC(cover));
      else if (uncover > 0 && uncover < 1) x = mix(0, 101, ease.outC(uncover));
    });
    return x;
  }

  var lastScene = null;
  function seek(t) {
    t = clamp(t, 0, DUR);
    var frame = Math.round(t * FPS);
    world.begin();
    L.background(ctx, world.dpr, Math.sin(t * 0.21) * 30, Math.cos(t * 0.17) * 20);
    var which = t < GV.start ? 'flow' : t < SD.start ? 'gives' : t < CL.start ? 'study' : 'close';
    if (which !== lastScene) {
      domAll.forEach(function (e) { L.vis(e, false); });
      lastScene = which;
    }
    ctx.lineCap = 'butt'; ctx.lineJoin = 'miter';
    if (which === 'flow') drawFlow(t);
    else if (which === 'gives') drawGives(t);
    else if (which === 'study') drawStudy(t);
    else drawClose(t);
    // end of the film: a short fade, so the file does not stop on a hard frame
    var fade = seg(t, DUR - 0.35, DUR);
    if (fade > 0) { ctx.globalAlpha = fade; ctx.fillStyle = '#0a0a0c'; ctx.fillRect(0, 0, W, H); ctx.globalAlpha = 1; }
    curtain.style.transform = 'translate3d(' + curtainAt(t).toFixed(2) + '%,0,0)';
    var gs = L.grainShift(frame, 3);
    grainEl.style.transform = 'translate3d(' + (-gs[0]) + 'px,' + (-gs[1]) + 'px,0)';
  }

  window.PGR = {
    fps: FPS, duration: DUR, ready: false, seek: seek,
    sceneTimes: TL.scenes.map(function (s) { return { id: s.id, start: s.start, dur: s.dur }; })
  };
  L.ready().then(function () { seek(0); window.PGR.ready = true; });
})();
