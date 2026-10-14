/* Parts both films build from: a camera that travels between keyframes, the floor and the bokeh that give it depth, the
   token cloud and the race of the two agents. One dot is 100 tokens,
   so the no index total is 2,937 dots and the best configuration is 1,340 of them. Every position is a function of
   time; nothing here keeps state between frames. */
(function (global) {
  'use strict';
  var L = global.PGRLIB, clamp = L.clamp, mix = L.mix, seg = L.seg, ease = L.ease, rgba = L.rgba;
  var W = L.W, H = L.H;

  // ---------------------------------------------------------------- camera
  // keys: [t, x, y, zoom, rot, easeName]; the ease shapes the move that arrives at that key.
  function Cam(keys) {
    var self = this;
    this.keys = keys;
    this.at = function (t) {
      var k = keys, i = 0;
      if (t <= k[0][0]) return { x: k[0][1], y: k[0][2], zoom: k[0][3], rot: k[0][4] || 0 };
      while (i < k.length - 1 && t > k[i + 1][0]) i++;
      if (i >= k.length - 1) { var e = k[k.length - 1]; return { x: e[1], y: e[2], zoom: e[3], rot: e[4] || 0 }; }
      var a = k[i], b = k[i + 1], p = (ease[b[5] || 'ioC'] || ease.ioC)(seg(t, a[0], b[0]));
      return { x: mix(a[1], b[1], p), y: mix(a[2], b[2], p), zoom: Math.exp(mix(Math.log(a[3]), Math.log(b[3]), p)), rot: mix(a[4] || 0, b[4] || 0, p) };
    };
    // how fast the picture moves at its edges, in stage px per second: sets the number of sub-frames
    this.speed = function (t) {
      var d = 0.01, a = self.at(t - d), b = self.at(t + d), c = self.at(t);
      var dx = (b.x - a.x) / (2 * d) * c.zoom, dy = (b.y - a.y) / (2 * d) * c.zoom;
      var dz = Math.abs(Math.log(b.zoom / a.zoom)) / (2 * d) * 900, dr = Math.abs(b.rot - a.rot) / (2 * d) * 900;
      return Math.sqrt(dx * dx + dy * dy) + dz + dr;
    };
  }
  function project(cam, p, wx, wy) {
    var s = 1 + (cam.zoom - 1) * p, c = Math.cos((cam.rot || 0) * p), n = Math.sin((cam.rot || 0) * p);
    var x = (wx - (W / 2 + cam.x * p)) * s, y = (wy - (H / 2 + cam.y * p)) * s;
    return [W / 2 + x * c - y * n, H / 2 + x * n + y * c, s];
  }
  function unproject(cam, p, sx, sy) {
    var s = 1 + (cam.zoom - 1) * p, c = Math.cos((cam.rot || 0) * p), n = Math.sin((cam.rot || 0) * p);
    var x = sx - W / 2, y = sy - H / 2;
    var ux = (x * c + y * n) / s, uy = (-x * n + y * c) / s;
    return [ux + W / 2 + cam.x * p, uy + H / 2 + cam.y * p];
  }

  // ---------------------------------------------------------------- the floor: a perspective grid that runs toward the camera
  // The lines to the horizon and the cross lines are hairlines; the cross lines move toward the viewer at a steady pace, so
  // the scene always has a ground to travel over. a: 0..1.
  function floor(ctx, cam, t, a) {
    if (a <= 0.003) return;
    ctx.setTransform(L.DPR, 0, 0, L.DPR, 0, 0);
    // the lines to the horizon repeat every 118 px, so the shift by the camera wraps: the floor stays under the camera however far it travels
    var vx = W / 2 - (((cam.x * 0.16) % 118) + 118) % 118 + Math.sin(t * 0.21) * 36, hy = H * 0.3 - cam.y * 0.05, bot = H + 60, span = bot - hy, run = (t * 0.2) % 1, i, z, y;
    ctx.save();
    var gl = ctx.createRadialGradient(vx, hy, 0, vx, hy, 1100);
    ctx.translate(0, hy); ctx.scale(1, 0.34); ctx.translate(0, -hy);
    gl.addColorStop(0, rgba(L.ACCENT, 0.12 * a)); gl.addColorStop(0.45, rgba(L.ACCENT, 0.035 * a)); gl.addColorStop(1, rgba(L.ACCENT, 0));
    ctx.fillStyle = gl; ctx.fillRect(-300, hy - 1300, W + 600, 2600);      // wider than the gradient, so no edge of the rectangle shows
    ctx.restore();
    var g = ctx.createLinearGradient(0, hy, 0, bot);
    g.addColorStop(0, 'rgba(255,255,255,0)'); g.addColorStop(0.4, 'rgba(255,255,255,' + (0.06 * a).toFixed(4) + ')'); g.addColorStop(1, 'rgba(255,255,255,' + (0.15 * a).toFixed(4) + ')');
    ctx.strokeStyle = g; ctx.lineWidth = 1; ctx.beginPath();
    for (i = -26; i <= 26; i++) { ctx.moveTo(vx, hy); ctx.lineTo(vx + i * 118, bot); }
    ctx.stroke();
    ctx.lineWidth = 1;
    for (i = 1; i <= 22; i++) {
      z = i - run; y = hy + span / (z * 0.82 + 0.18);
      if (y > bot || y < hy + 2) continue;
      ctx.strokeStyle = 'rgba(255,255,255,' + (Math.min(0.16, 0.3 / (z + 1.2)) * a * seg(y, hy, hy + 40)).toFixed(4) + ')';
      ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(W, y); ctx.stroke();
    }
  }

  // ---------------------------------------------------------------- bokeh: large soft lights in front of the scene, for depth
  // They sit on a nearer layer than the world (it moves more than the world does when the camera travels), drift on their own and
  // are drawn from fixed seeds. a: 0..1.
  var BOK = (function () {
    var R = L.rng(91), out = [], i;
    for (i = 0; i < 40; i++) out.push({ x: R() * W * 1.5 - W * 0.25, y: R() * H * 1.2 - H * 0.1, r: 22 + R() * R() * 130, dx: (R() - 0.5) * 16, dy: -(5 + R() * 14), ph: R() * 6.2832, hot: R() < 0.3, al: 0.035 + 0.06 * R() });
    return out;
  })();
  function bokeh(ctx, cam, t, a) {
    if (a <= 0.003) return;
    var i, e, x, y, wx = W * 1.5, wy = H * 1.2;
    // the lights wrap over wx by wy, so the shift by the camera wraps too: they stay round the camera however far it travels
    var bx = cam.x * 1.6, by = cam.y * 1.6;
    L.layer(ctx, { x: (bx - Math.floor(bx / wx) * wx) / 1.6, y: (by - Math.floor(by / wy) * wy) / 1.6, zoom: cam.zoom, rot: cam.rot }, 1.6);
    for (i = 0; i < BOK.length; i++) {
      e = BOK[i];
      x = (((e.x + e.dx * t) % wx) + wx) % wx - W * 0.25; y = (((e.y + e.dy * t) % wy) + wy) % wy - H * 0.1 + Math.sin(t * 0.35 + e.ph) * 14;
      L.stamp(ctx, e.hot ? L.ACCENT : L.INK2, x, y, e.r, e.al * a, 0.55);
    }
    ctx.setTransform(L.DPR, 0, 0, L.DPR, 0, 0);
  }

  // ---------------------------------------------------------------- the token cloud
  // o: N (all dots), M (the dots that stay), seed, center [x, y], R (radius), emit {x, head(t)} where the dots are born,
  //    passes [{n, a, b}] (how many are born between a and b), collapse {t0, dur} when the cloud draws in to the filament,
  //    fil {x0, y, len}, cam (the Cam, for the depth-corrected birth point)
  var PARALLAX = [0.8, 1.0, 1.28];
  function Cloud(o) {
    var N = o.N, M = o.M, R = L.rng(o.seed || 7), i, j, self = this;
    this.o = o;
    var d = this.d = [];
    var order = [];
    for (i = 0; i < N; i++) order.push(i);
    for (i = N - 1; i > 0; i--) { j = Math.floor(R() * (i + 1)); var tmp = order[i]; order[i] = order[j]; order[j] = tmp; }
    // who is born in which pass, and when
    var born = new Array(N), ptr = 0, pass;
    for (j = 0; j < o.passes.length; j++) {
      pass = o.passes[j];
      for (i = 0; i < pass.n && ptr < N; i++, ptr++) born[order[ptr]] = pass.a + (pass.b - pass.a) * Math.pow((i + R() * 0.9) / pass.n, 0.9);
    }
    for (; ptr < N; ptr++) born[order[ptr]] = o.passes[o.passes.length - 1].b;
    var staySlot = {};
    for (i = 0; i < N; i++) staySlot[i] = -1;
    // the dots that stay are a random choice of M, laid on the filament in the order of their angle round the cloud
    var stay = order.slice(0, M).sort(function (a, b) { return a - b; });
    for (i = 0; i < stay.length; i++) staySlot[stay[i]] = i;
    for (i = 0; i < N; i++) {
      var q = R(), cls = q < 0.2 ? 2 : q < 0.58 ? 1 : 0, rr = Math.sqrt(R()), th = R() * Math.PI * 2;
      d.push({
        r: rr * o.R, th: th, om: (0.2 + 0.5 * (1 - rr)) * (R() < 0.5 ? 1 : 1) * 0.55, cls: cls, size: 2.1 + R() * 1.1 + cls * 0.8,
        born: born[i], fl: 0.75 + R() * 0.55, arc: (R() - 0.5) * 220, jy: (R() - 0.5) * 26, slot: staySlot[i],
        cdel: R() * 0.55, dir: R() * Math.PI * 2, sp: 380 + R() * 760, tw: R() * 6.28, bri: 0.55 + 0.45 * R()
      });
    }
    // where a dot rests at time t: a flat spiral that turns slowly, the inner dots faster
    this.rest = function (e, t) {
      var th = e.th + e.om * t;
      return [o.center[0] + e.r * Math.cos(th), o.center[1] + e.r * Math.sin(th) * (o.squash || 0.62)];
    };
    // draw every dot at time t. s: {cam, a (overall), heat (0..1 all accent), ghost (0..1: show the dots that left), hold (stop the turning at this t)}
    this.draw = function (ctx, t, s) {
      var cam = s.cam, c, e, k, buckets, pos, key, bk, cb = o.collapse, tt = s.hold === undefined ? t : Math.min(t, s.hold);
      var ca = cb ? seg(t, cb.t0, cb.t0 + cb.dur) : 0;
      var P = L.probe(), mm = null;                  // the motion probe: where every dot is on the output, by its own number
      if (P && !P.cloud) { P.cloud = new Float32Array(N * 2); P.cloud.fill(NaN); }
      for (c = 0; c < 3; c++) {
        L.layer(ctx, cam, PARALLAX[c]);
        if (P) mm = ctx.getTransform();
        buckets = {};
        for (k = 0; k < N; k++) {
          e = d[k];
          if (e.cls !== c || t < e.born) continue;
          var u = (t - e.born) / e.fl, x, y, a = e.bri, heat = 0;
          pos = this.rest(e, tt);
          if (u < 1) {
            // born at the read head: the point on the glass where the head is, put back into this layer's own world
            var cb0 = s.camAt(e.born), ey = o.emit.head(e.born) + e.jy, sp = project(cb0, 1, o.emit.x, ey);
            var st = unproject(cb0, PARALLAX[c], sp[0], sp[1]);
            var eu = ease.outQ(clamp(u));
            x = mix(st[0], pos[0], eu); y = mix(st[1], pos[1], eu) + Math.sin(Math.PI * clamp(u)) * e.arc;
            heat = 1 - clamp(u * 1.4);
            a *= clamp(u * 5);
          } else { x = pos[0]; y = pos[1]; heat = clamp(1 - (u - 1) * 1.4); }
          if (cb && ca > 0) {
            var kc = seg(t, cb.t0 + e.cdel, cb.t0 + e.cdel + cb.dur * 0.75);
            if (e.slot >= 0) {
              var fx = o.fil.x0 + (e.slot + 0.5) / M * o.fil.len, fy = o.fil.y + e.jy * 0.1, p2 = ease.ioC(kc);
              // the filament is one line on the glass: each dot's own layer is shifted so that it lands on it
              var tp = project(cam, 1, fx, fy), tg = unproject(cam, PARALLAX[c], tp[0], tp[1]);
              x = mix(x, tg[0], p2); y = mix(y, tg[1], p2); heat = Math.max(heat, p2 * (s.heat === undefined ? 1 : s.heat));
            } else {
              var kk = ease.outC(kc); x += Math.cos(e.dir) * e.sp * kk; y += Math.sin(e.dir) * e.sp * kk * 0.7;
              a *= 1 - ease.inC(kc);
            }
          }
          if (P) { P.cloud[k * 2] = mm.a * x + mm.c * y + mm.e; P.cloud[k * 2 + 1] = mm.b * x + mm.d * y + mm.f; }
          a *= s.a === undefined ? 1 : s.a;
          if (a <= 0.01) continue;
          var ab = Math.min(4, Math.floor(a * 5)), hb = heat > 0.66 ? 3 : heat > 0.33 ? 2 : heat > 0.08 ? 1 : 0;
          if (s.heat !== undefined && s.heat >= 1 && e.slot >= 0 && ca >= 1) hb = 3;
          key = hb * 5 + ab;
          (buckets[key] || (buckets[key] = [])).push(x, y, e.size);
        }
        for (key in buckets) {
          bk = buckets[key];
          var hb2 = Math.floor(key / 5), ab2 = key % 5, col = hb2 === 3 ? L.ACCENT : hb2 === 2 ? L.lerpC(L.ACCENT, L.HOT, 0.5) : hb2 === 1 ? L.lerpC(L.INK2, L.HOT, 0.55) : L.INK2;
          ctx.fillStyle = rgba(col, (ab2 + 0.5) / 5 * (hb2 >= 2 ? 1 : 0.9));
          ctx.beginPath();
          for (k = 0; k < bk.length; k += 3) { ctx.moveTo(bk[k] + bk[k + 2], bk[k + 1]); ctx.arc(bk[k], bk[k + 1], bk[k + 2], 0, 6.2832); }
          ctx.fill();
        }
      }
      ctx.setTransform(L.DPR, 0, 0, L.DPR, 0, 0);
    };
    // the dots that left, as a faint line where the full length would have been (the length of the no index total)
    this.ghost = function (ctx, cam, a) {
      if (a <= 0.01) return;
      L.layer(ctx, cam, 1.0);
      var k, n = N;
      ctx.fillStyle = 'rgba(184,184,193,' + (0.5 * a).toFixed(3) + ')';
      ctx.beginPath();
      for (k = M; k < n; k++) { var x = o.fil.x0 + (k + 0.5) / M * o.fil.len; ctx.moveTo(x + 1.5, o.fil.y); ctx.arc(x, o.fil.y, 1.5, 0, 6.2832); }
      ctx.fill();
      ctx.setTransform(L.DPR, 0, 0, L.DPR, 0, 0);
    };
  }


  // ---------------------------------------------------------------- the read head: a bright line that sweeps down a terminal
  // beams: [{a, b}], y(t): where the head is at t, x/w: the terminal's span, env: 0..1 overall
  function head(ctx, beams, y, t, x, w, k) {
    var i, b, env, yy;
    ctx.globalCompositeOperation = 'lighter';
    for (i = 0; i < beams.length; i++) {
      b = beams[i];
      env = seg(t, b.a - 0.05, b.a + 0.05) * (1 - seg(t, b.b, b.b + 0.25)) * (k === undefined ? 1 : k);
      if (env <= 0) continue;
      yy = y(t);
      var g = ctx.createLinearGradient(0, yy - 90, 0, yy);
      g.addColorStop(0, 'rgba(255,122,61,0)'); g.addColorStop(1, 'rgba(255,122,61,' + (0.2 * env).toFixed(3) + ')');
      ctx.fillStyle = g; ctx.fillRect(x + 8, yy - 90, w - 16, 90);
      ctx.fillStyle = rgba(L.ACCENT, env); ctx.fillRect(x + 8, yy - 1.2, w - 16, 2.4);
      L.stamp(ctx, L.ACCENT, x + w, yy, 90, 0.9 * env);
    }
    ctx.globalCompositeOperation = 'source-over';
  }

  // ---------------------------------------------------------------- the filament: the dots that stay, as one lit line
  // fil {x0, y, len}; k 0..1 how far it has drawn; pulseT: seconds since the pulse started running (or null)
  function filament(ctx, cam, fil, k, pulseT, pulseA) {
    if (k <= 0) return;
    L.layer(ctx, cam, 1.0);
    ctx.globalCompositeOperation = 'lighter';
    var x0 = fil.x0, x1 = fil.x0 + fil.len * ease.outC(k);
    var g = ctx.createLinearGradient(x0, 0, x1, 0);
    g.addColorStop(0, rgba(L.ACCENT, 0.0)); g.addColorStop(0.08, rgba(L.ACCENT, 0.5)); g.addColorStop(0.92, rgba(L.ACCENT, 0.5)); g.addColorStop(1, rgba(L.ACCENT, 0));
    ctx.strokeStyle = g; ctx.lineWidth = 2.4; ctx.beginPath(); ctx.moveTo(x0, fil.y); ctx.lineTo(x1, fil.y); ctx.stroke();
    if (pulseT !== null && pulseT !== undefined && pulseT > -0.1) {
      var pp = (pulseT / 1.4) % 1, px = fil.x0 + fil.len * (pp < 0 ? pp + 1 : pp);
      L.stamp(ctx, L.HOT, px, fil.y, 64, 0.85 * (pulseA === undefined ? 1 : pulseA), 0.3);
    }
    L.stamp(ctx, L.ACCENT, x0, fil.y, 46, 0.7 * k); L.stamp(ctx, L.ACCENT, fil.x0 + fil.len, fil.y, 46, 0.7 * k);
    ctx.globalCompositeOperation = 'source-over';
    ctx.setTransform(L.DPR, 0, 0, L.DPR, 0, 0);
  }

  // ---------------------------------------------------------------- the closing card's install lines, typed from at0
  // items: [{system, shell, command}] (build/facts.js installers); a command that is too wide for the panel is wrapped at a slash
  function installLines(items, at0, cps) {
    var out = [], at = at0;
    items.forEach(function (it) {
      out.push({ at: at, text: it.system + '  (' + it.shell + ')', kind: 'dim' }); at += 0.04;
      var cut = it.command.lastIndexOf('/', 64), parts = cut > 20 ? [it.command.slice(0, cut + 1), it.command.slice(cut + 1)] : [it.command];
      parts.forEach(function (p, i) { out.push({ at: at, text: p, kind: i === 0 ? 'cmd' : 'cont', cps: cps }); at += p.length / cps + 0.01; });
      at += 0.02;
    });
    return out;
  }


  // ---------------------------------------------------------------- the race: the same task with no index and with PGR
  // Both agents read at the same pace and each has a meter of dots, one dot for every 100 tokens, filled column by column.
  // PGR is finished when its own total is reached; the other agent keeps reading to its longer total. The pace is the
  // same for both, so the picture says only what the numbers say: how many tokens each one read.
  var COLS = 98, ROWS = 30;
  function pace(u) { return u <= 0 ? 0 : u >= 1 ? 1 : (u < 0.1 ? 5 * u * u : u - 0.05) / 0.95; }
  function unpace(v) { return v <= 0 ? 0 : v >= 1 ? 1 : v < 0.05 / 0.95 ? Math.sqrt(0.95 * v / 5) : v * 0.95 + 0.05; }
  // times of the race: tDone is when PGR has read its own total; at(share) is when the no index agent has read that share of its total
  function raceTimes(s) {
    var F = global.FACTS;
    return { tDone: s.tStart + unpace(F.best.tokens / F.control.tokens) * (s.tEnd - s.tStart), at: function (share) { return s.tStart + unpace(share) * (s.tEnd - s.tStart); } };
  }
  // what each terminal prints, with absolute times (the lines are in time order)
  function raceLines(s) {
    var F = global.FACTS, rt = raceTimes(s), ctrl = [], pgr = [], k, at, n = 0;
    var nC = Math.max(1, Math.round(F.control.turns)), nP = Math.max(1, Math.round(F.best.turns));
    var byTime = function (x, y) { return x.at - y.at; };
    ctrl.push({ at: s.tAsk, text: s.ask, kind: 'cmd', cps: 40 });
    for (k = 0; k < nC; k++) ctrl.push({ at: Math.max(s.tAsk + 0.7, rt.at(k / nC)), text: 'turn ' + (k + 1), kind: 'dim' });
    for (at = s.tAsk + 0.9; at < s.tEnd - 0.25; at += 0.125) { ctrl.push({ at: at, text: s.reads[n % s.reads.length], kind: 'out' }); n++; }
    ctrl.push({ at: s.tEnd - 0.05, text: s.answer, kind: 'acc' });
    ctrl.push({ at: s.tEnd + 0.15, text: 'done in ' + F.control.turnsText + ' turns', kind: 'dim' });
    pgr.push({ at: s.tAsk, text: s.ask, kind: 'cmd', cps: 40 });
    pgr.push({ at: s.tAsk + 0.55, text: 'turn 1', kind: 'dim' });
    pgr.push({ at: s.tAsk + 0.8, text: 'looking up parse_config', kind: 'dim' });
    pgr.push({ at: s.tAsk + 1.15, text: 'defined in config.py', kind: 'out' });
    for (k = 1; k < nP; k++) pgr.push({ at: Math.max(s.tAsk + 1.4, rt.at(k / nP * F.best.tokens / F.control.tokens)), text: 'turn ' + (k + 1), kind: 'dim' });
    pgr.push({ at: rt.tDone - 0.3, text: s.answer, kind: 'acc' });
    pgr.push({ at: rt.tDone + 0.1, text: 'done in ' + F.best.turnsText + ' turns', kind: 'hot' });
    return { ctrl: ctrl.sort(byTime), pgr: pgr.sort(byTime) };
  }
  // the dots of one meter: lit ones in col, unlit ones as a faint ghost, the newest few hot
  function meter(ctx, x, y, w, h, lit, total, col, a) {
    var dx = w / COLS, dy = h / ROWS, i, c, r, cx, cy, hot = Math.max(0, lit - 60), rad = Math.min(dx, dy) * 0.34;
    ctx.fillStyle = 'rgba(255,255,255,' + (0.075 * a).toFixed(3) + ')'; ctx.beginPath();
    for (i = lit; i < total; i++) { c = Math.floor(i / ROWS); r = i % ROWS; cx = x + (c + 0.5) * dx; cy = y + (r + 0.5) * dy; ctx.moveTo(cx + rad, cy); ctx.arc(cx, cy, rad, 0, 6.2832); }
    ctx.fill();
    ctx.fillStyle = rgba(col, 0.95 * a); ctx.beginPath();
    for (i = 0; i < Math.min(lit, hot); i++) { c = Math.floor(i / ROWS); r = i % ROWS; cx = x + (c + 0.5) * dx; cy = y + (r + 0.5) * dy; ctx.moveTo(cx + rad, cy); ctx.arc(cx, cy, rad, 0, 6.2832); }
    ctx.fill();
    ctx.fillStyle = rgba(L.HOT, a); ctx.beginPath();
    for (i = hot; i < lit; i++) { c = Math.floor(i / ROWS); r = i % ROWS; cx = x + (c + 0.5) * dx; cy = y + (r + 0.5) * dy; ctx.moveTo(cx + rad * 1.25, cy); ctx.arc(cx, cy, rad * 1.25, 0, 6.2832); }
    ctx.fill();
  }
  // s: {x, y, w, h, gap, a, tShow, tAsk, tStart, tEnd, size, cs (counter size), meterH, task, lines (from raceLines), tc (the time of
  // the frame itself, for the counters; default t)}
  function race(ctx, t, s) {
    var F = global.FACTS, a = s.a === undefined ? 1 : s.a;
    if (a <= 0.003) return;
    var rt = raceTimes(s), tc = s.tc === undefined ? t : s.tc, u = seg(tc, s.tStart, s.tEnd), tokC = F.control.tokens * pace(u), tokP = Math.min(F.best.tokens, tokC);
    var nC = Math.round(F.control.tokens / 100), side, ACC = L.ACCENT, INK = L.INK, INK2 = L.INK2, INK3 = L.INK3;
    var showK = ease.outC(seg(t, s.tShow, s.tShow + 0.6)), dy = (1 - showK) * 60;
    for (side = 0; side < 2; side++) {
      var pgr = side === 1, x = s.x + side * (s.w + s.gap), y = s.y + dy, k = a * showK;
      var tDone = pgr ? rt.tDone : s.tEnd, done = seg(tc, tDone, tDone + 0.5), tok = pgr ? tokP : tokC;
      var total = pgr ? F.best.tokens : F.control.tokens, turns = Math.max(1, Math.round(pgr ? F.best.turns : F.control.turns));
      var turn = Math.min(turns, 1 + Math.floor(tok / total * turns - 1e-9));
      if (k <= 0.003) continue;
      L.text(ctx, pgr ? 'PGR' : 'No index', x + 6, y - 26, { size: 52, weight: 700, font: 'd', ls: -0.03, color: pgr ? ACC : INK2, a: k });
      L.terminal(ctx, x, y, s.w, s.h, t, { title: (pgr ? 'PGR' : 'agent') + '  -  ' + s.task, lines: pgr ? s.lines.pgr : s.lines.ctrl, size: s.size, a: k, sheen: 0.9,
        glow: pgr ? ACC : null, glowK: pgr ? 0.45 + 0.7 * done : 0, edge: pgr && done > 0 ? ACC : undefined, padBottom: s.meterH + 44 });
      var bump = 1 + 0.06 * Math.sin(Math.PI * done), cx = x + s.w - 36, cy = y + 56 + s.cs * 0.86;
      ctx.save(); ctx.translate(cx, cy); ctx.scale(bump, bump); ctx.translate(-cx, -cy);
      L.numText(ctx, L.fmtTokens(tok), cx, cy, { size: s.cs, weight: 700, font: 'd', ls: -0.055, color: pgr && done > 0.2 ? ACC : INK, align: 'r', a: k, counting: L.fmtTokens(total) });
      ctx.restore();
      // a soft pulse of light round the counter when a run is done: a short ring that stays inside the content of its window
      var rk = seg(tc, tDone, tDone + 0.6);
      if (rk > 0 && rk < 1) {
        ctx.save(); ctx.globalCompositeOperation = 'lighter';
        var pr = 40 + 110 * ease.outC(rk), pa = Math.sin(Math.PI * rk);
        L.stamp(ctx, pgr ? L.HOT : INK2, cx - 130, cy - s.cs * 0.3, pr, (pgr ? 0.5 : 0.18) * pa * k, 0.1);
        ctx.restore();
      }
      var lab = done > 0.4 ? 'done in ' + (pgr ? F.best.turnsText : F.control.turnsText) + ' turns' : 'tokens read, turn ' + turn;
      L.text(ctx, lab, cx, cy + 40, { size: 28, weight: 600, font: 't', color: pgr && done > 0.4 ? ACC : INK3, align: 'r', a: k });
      var mx = x + 30, my = y + s.h - 30 - s.meterH, mw = s.w - 60;
      meter(ctx, mx, my, mw, s.meterH, Math.round(tok / 100), nC, pgr ? ACC : INK2, k);
      if (pgr && done > 0.01) L.text(ctx, F.fewerText + ' fewer', mx + mw * 0.73, my + s.meterH * 0.62, { size: 40, weight: 700, font: 'd', ls: -0.02, color: INK, align: 'c', a: k * seg(done, 0.3, 1), shadow: 0.9 });
    }
  }

  // ---------------------------------------------------------------- the link: the agent and PGR on one machine, a question and an answer between them
  // o: {x, y, w, h, bw (width of each box), a, t0 (when the dots start to run)}
  function link(ctx, t, o) {
    var a = o.a, x = o.x, y = o.y, w = o.w, h = o.h, bw = o.bw, i, ph, xa = x + bw + 16, xb = x + w - bw - 16, my1 = y + h * 0.36, my2 = y + h * 0.66;
    if (a <= 0.003) return;
    ctx.save(); ctx.globalAlpha = a; ctx.setLineDash([10, 12]); ctx.lineWidth = 1.5; ctx.strokeStyle = 'rgba(255,255,255,0.28)';
    L.rr(ctx, x - 26, y - 62, w + 52, h + 90, L.R_L); ctx.stroke(); ctx.setLineDash([]); ctx.restore();
    L.text(ctx, 'Your machine', x, y - 20, { size: 30, weight: 560, font: 't', color: L.INK3, a: a });
    [[x, 'AI coding agent', false], [x + w - bw, 'PGR', true]].forEach(function (b) {
      L.panel(ctx, b[0], y, bw, h, { a: a, sheen: 0.6, t: t, glow: b[2] ? L.ACCENT : null, glowK: 0.7, edge: b[2] ? L.ACCENT : undefined });
      L.text(ctx, b[1], b[0] + 36, y + h / 2 + 17, { size: 50, weight: 700, font: 'd', ls: -0.025, color: b[2] ? L.ACCENT : L.INK, a: a });
    });
    ctx.save(); ctx.globalAlpha = a; ctx.strokeStyle = 'rgba(255,255,255,0.2)'; ctx.lineWidth = 1;
    ctx.beginPath(); ctx.moveTo(xa, my1); ctx.lineTo(xb, my1); ctx.moveTo(xb, my2); ctx.lineTo(xa, my2); ctx.stroke(); ctx.restore();
    if (t > o.t0) {
      ctx.globalCompositeOperation = 'lighter';
      for (i = 0; i < 3; i++) {
        ph = ((t - o.t0) / 1.8 - i * 0.11) % 1; if (ph < 0) ph += 1;
        L.stamp(ctx, L.ACCENT, mix(xa, xb, ph), my1, 24, a * Math.sin(Math.PI * ph), 0.5);
        L.stamp(ctx, L.HOT, mix(xb, xa, (ph + 0.5) % 1), my2, 20, a * Math.sin(Math.PI * ((ph + 0.5) % 1)), 0.4);
      }
      ctx.globalCompositeOperation = 'source-over';
    }
  }

  global.PGRKIT = { head: head, filament: filament, installLines: installLines, Cam: Cam, Cloud: Cloud, floor: floor, bokeh: bokeh, link: link, race: race, raceLines: raceLines, raceTimes: raceTimes, project: project, unproject: unproject, PARALLAX: PARALLAX };
})(window);
