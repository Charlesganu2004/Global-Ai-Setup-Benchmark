/* Orchestration: who used what inside an orchestrated run, what the review
   changed, the three setups side by side, and the reviewer variants. */
(function () {
  'use strict';
  var S = window.Site, C = window.Charts;
  if (!S || !S.ready || !C) return;
  var B = window.BENCH, O = B.orchestration || {}, text = B.text || {};
  var rolesAll = Array.isArray(O.roles) ? O.roles : [], setupsAll = Array.isArray(O.setups) ? O.setups : [];
  var reviewers = Array.isArray(O.reviewers) ? O.reviewers : [], questions = Array.isArray(O.questions) ? O.questions : [];

  var ROLES = [['plan', 'Orchestrator: the plan', '--s1'], ['workers', 'The three subagents', '--s2'], ['verify', 'Orchestrator: the review', '--s3']];
  var WAYS = [['single', 'One agent', '--s1'], ['parallel', 'Three subagents, no review', '--s2'], ['orch', 'Orchestrated', '--s3']];
  var MEASURES = [{ id: 'tokens', name: 'Tokens' }, { id: 'cost_warm', name: 'Cost' }];

  var tiers = S.list('tiers').filter(function (t) { return rolesAll.some(function (r) { return r.tier === t.id; }); });
  var tierId = tiers.length ? tiers[0].id : null, measure = 'tokens', groupId = 'all';
  var groups = S.groupOptions().filter(function (g) {
    return g.id === 'all' || rolesAll.some(function (r) { return S.arm(r.arm).group === g.id; });
  });

  var label = function (r) { return S.armLabel(S.arm(r.arm)); };
  var show = function (v) { return measure === 'tokens' ? S.tok(v) : S.usd(v); };
  var exact = function (v) { return measure === 'tokens' ? S.int(v) : S.usd(v); };
  var times = function (v, digits) { return typeof v === 'number' && isFinite(v) ? S.num(v, digits) + ' times' : ''; };
  var inView = function (r) { return r.tier === tierId && S.inGroup(r.arm, groupId); };
  var hosts = function (name) { return { host: S.byId(name), legend: S.byId(name + '-legend') }; };
  // A reviewer is named by what it is; a setup the data file does not describe keeps its own name.
  var reviewer = function (setupId) { var s = S.setup(setupId); return s.reviewer || s.name; };

  S.fillList(S.byId('o-findings'), text.orchestration);

  function drawFilters() {
    S.segmented(S.byId('o-tier'), tiers, tierId, function (id) { tierId = id; render(); });
    S.segmented(S.byId('o-measure'), MEASURES, measure, function (id) { measure = id; render(); });
    S.segmented(S.byId('o-group'), groups, groupId, function (id) { groupId = id; render(); });
    var rows = rolesAll.filter(inView), runs = rows.reduce(function (sum, r) { return sum + r.n; }, 0);
    if (!tierId) { S.setText('o-note', 'No orchestrated runs were measured.'); return; }
    var t = S.tier(tierId);
    S.setText('o-note', S.plural(rows.length, 'method', 'methods') + ' and ' + S.plural(runs, 'orchestrated run', 'orchestrated runs') +
      ' with ' + t.name + ' subagents (' + t.model + ') in this selection.');
  }

  /* ---------- who used what ---------- */
  function drawRoles() {
    var rows = rolesAll.filter(inView).slice().sort(function (a, b) { return a[measure] - b[measure] || S.armOrder(a.arm) - S.armOrder(b.arm); });
    C.drawStack(hosts('roles'), rows.map(function (r) {
      return {
        label: label(r), total: r[measure],
        parts: ROLES.map(function (role) { return r.roles[role[0]][measure]; }),
        tip: function () {
          return [{ value: exact(r[measure]), label: 'the whole run' },
            { value: S.pct(measure === 'tokens' ? r.lead_token_share : r.lead_cost_share), label: 'of it is the orchestrator' },
            { value: S.pct(r.quality), label: 'answered correctly, ' + S.plural(r.n, 'run', 'runs') }];
        }
      };
    }), {
      parts: ROLES.map(function (role) { return [role[1], role[2]]; }), format: show,
      label: (measure === 'tokens' ? 'Tokens' : 'Cost') + ' of one orchestrated run by role, ' + S.tier(tierId).name + ' subagents'
    });
    S.table(S.byId('roles-table'),
      [['Method'], ['Runs', 1], ['Plan tokens', 1], ['Subagent tokens', 1], ['Review tokens', 1], ['Total tokens', 1], ['Orchestrator share of tokens', 1],
        ['Plan cost', 1], ['Subagent cost', 1], ['Review cost', 1], ['Total cost', 1], ['Orchestrator share of cost', 1],
        ['Subagent turns', 1], ['Subagent tool calls', 1], ['Review turns', 1], ['Review tool calls', 1], ['Seconds', 1]],
      rows.map(function (r) {
        var p = r.roles.plan, w = r.roles.workers, v = r.roles.verify;
        return [label(r), r.n, S.int(p.tokens), S.int(w.tokens), S.int(v.tokens), S.int(r.tokens), S.pct(r.lead_token_share),
          S.usd(p.cost_warm), S.usd(w.cost_warm), S.usd(v.cost_warm), S.usd(r.cost_warm), S.pct(r.lead_cost_share),
          S.num(w.requests, 1), S.num(w.tool_calls, 1), S.num(v.requests, 1), S.num(v.tool_calls, 1), S.int(r.wall_seconds)];
      }), 'Tokens and cost of an orchestrated run by role');
  }

  /* ---------- what the review changed ---------- */
  function drawReview() {
    var rows = rolesAll.filter(inView).slice().sort(function (a, b) { return S.armOrder(a.arm) - S.armOrder(b.arm); });
    S.table(S.byId('review-table'),
      [['Method'], ['Runs', 1], ['Answers', 1], ['Subagents, mean score', 1], ['After review, mean score', 1], ['Wrong before review', 1], ['Corrected', 1],
        ['Still wrong', 1], ['Right answers broken', 1], ['Runs fully correct', 1]],
      rows.map(function (r) {
        return [label(r), r.n, r.questions, S.pct(r.worker_quality), S.pct(r.quality), r.fixed + r.still_wrong, r.fixed, r.still_wrong, r.broken,
          r.perfect_runs + ' of ' + r.n];
      }), 'What the review changed, by method');
  }
  function drawQuestions() {
    var asked = S.list('questions');
    S.table(S.byId('questions-table'),
      [['Subagent tier'], ['Question', 0, 'wide'], ['Answers', 1], ['Subagents fully right', 1], ['After review fully right', 1],
        ['Subagents, mean score', 1], ['After review, mean score', 1]],
      questions.map(function (q) {
        var at = Number(String(q.question).replace(/\D/g, '')) - 1;
        return [S.tier(q.tier).name, 'Q' + (at + 1) + (asked[at] ? ': ' + asked[at].text : ''), q.answers,
          q.subagents_right + ' of ' + q.answers, q.reviewed_right + ' of ' + q.answers, S.pct(q.subagents), S.pct(q.reviewed)];
      }), 'Each question before and after the review');
  }

  /* ---------- the three setups ---------- */
  function drawSetups() {
    var rows = setupsAll.filter(inView).slice().sort(function (a, b) {
      return (a.single ? a.single[measure] : Infinity) - (b.single ? b.single[measure] : Infinity) || S.armOrder(a.arm) - S.armOrder(b.arm);
    });
    C.drawDots(hosts('setups'), rows.map(function (r) {
      return {
        label: label(r),
        dots: WAYS.map(function (way, i) {
          var b = r[way[0]];
          return {
            series: i, value: b ? b[measure] : null,
            tip: function () {
              return [{ value: S.int(b.tokens), label: 'tokens' }, { value: S.usd(b.cost_warm), label: 'per run, cache warm' },
                { value: S.num(b.requests, 1), label: 'turns' }, { value: S.pct(b.quality), label: 'answered correctly, ' + S.plural(b.n, 'run', 'runs') }];
            }
          };
        })
      };
    }), {
      series: WAYS.map(function (way) { return [way[1], way[2]]; }), format: show, log: true,
      unit: measure === 'tokens' ? 'tokens per run' : 'per run, cache warm',
      label: (measure === 'tokens' ? 'Tokens' : 'Cost') + ' per run for one agent, three subagents and an orchestrated run, ' + S.tier(tierId).name
    });
    var body = [];
    rows.forEach(function (r) {
      WAYS.forEach(function (way) {
        var b = r[way[0]];
        if (!b) return;
        var base = r.single && way[0] !== 'single' ? r.single : null;
        body.push([label(r), way[1], b.n, S.int(b.tokens), S.num(b.requests, 1), S.num(b.tool_calls, 1), S.usd(b.cost_warm), S.usd(b.cost_cold),
          S.pct(b.quality), b.perfect_runs + ' of ' + b.n, S.int(b.wall_seconds),
          base ? times(b.tokens / base.tokens, 1) : '',
          base && base.cost_warm > 0 ? times(b.cost_warm / base.cost_warm, b.cost_warm / base.cost_warm < 10 ? 1 : 0) : '']);
      });
    });
    S.table(S.byId('setups-table'),
      [['Method'], ['Setup'], ['Runs', 1], ['Tokens', 1], ['Turns', 1], ['Tool calls', 1], ['Cache warm', 1], ['Cache cold', 1], ['Correct', 1],
        ['Runs fully correct', 1], ['Seconds', 1], ['Tokens against one agent', 1], ['Cost against one agent', 1]],
      body, 'One agent, three subagents and an orchestrated run, by method');

    var perAnswer = [];
    rows.forEach(function (r) {
      WAYS.forEach(function (way) {
        var b = r[way[0]];
        if (b && b.tokens_per_correct) perAnswer.push([label(r), way[1], b.n, S.pct(b.quality), S.int(b.tokens_per_correct), S.usd(b.cost_per_correct), S.int(b.wall_seconds)]);
      });
    });
    S.table(S.byId('correct-table'),
      [['Method'], ['Setup'], ['Runs', 1], ['Mean score', 1], ['Tokens per correct answer', 1], ['Cost per correct answer', 1], ['Seconds a run', 1]],
      perAnswer, 'Tokens and cost per correct answer, by method and setup');
  }

  /* ---------- reviewer variants: not filtered ---------- */
  // The variants ran on a few methods on one tier. The standard reviewer is listed twice: on everything
  // it reviewed, and on those same methods, which is the row to compare a variant with.
  function tallies() {
    var review = O.review || {}, out = [];
    S.list('setups').forEach(function (s) {
      if (!review[s.id]) return;
      out.push([reviewer(s.id) + (s.id === 'orch' ? ', every method and tier' : ''), review[s.id]]);
      if (s.id === 'orch' && O.review_matched) out.push([reviewer(s.id) + ', on the methods and tier the variants ran on', O.review_matched]);
    });
    return out;
  }
  function drawReviewers() {
    var review = O.review || {};
    S.table(S.byId('tally-table'),
      [['Reviewer', 0, 'wide'], ['Runs', 1], ['Answers', 1], ['Wrong before review', 1], ['Corrected', 1], ['Still wrong', 1], ['Right answers broken', 1], ['Runs fully correct', 1]],
      tallies().map(function (pair) {
        var t = pair[1];
        return [pair[0], t.runs, t.questions, t.wrong_before, t.fixed, t.still_wrong, t.broken, t.perfect_runs + ' of ' + t.runs];
      }), 'Reviews added up by reviewer');
    S.table(S.byId('reviewers-table'),
      [['Method'], ['Subagent tier'], ['Reviewer', 0, 'wide'], ['Runs', 1], ['Review tokens', 1], ['Review output tokens', 1], ['Review turns', 1], ['Review tool calls', 1], ['Review cost', 1],
        ['Total tokens', 1], ['Total cost', 1], ['Subagents, mean score', 1], ['After review, mean score', 1], ['Corrected', 1], ['Still wrong', 1], ['Broken', 1], ['Runs fully correct', 1]],
      reviewers.map(function (r) {
        var v = r.roles.verify;
        return [label(r), S.tier(r.tier).name, reviewer(r.setup), r.n, S.int(v.tokens), S.int(v.output), S.num(v.requests, 1), S.num(v.tool_calls, 1), S.usd(v.cost_warm),
          S.int(r.tokens), S.usd(r.cost_warm), S.pct(r.worker_quality), S.pct(r.quality), r.fixed, r.still_wrong, r.broken, r.perfect_runs + ' of ' + r.n];
      }), 'Reviewer variants by method');
  }

  /* ---------- every orchestrated run ---------- */
  function drawRuns() {
    // A method with a twin is the copy measured with token-goat's hooks on: those runs are on the Hooks page.
    var runs = S.list('runs').filter(function (c) { return c.roles && c.tier === tierId && S.inGroup(c.arm, groupId) && !S.arm(c.arm).twin; });
    S.setText('o-runs-note', S.plural(runs.length, 'run', 'runs') + ' in this selection, every reviewer included. Runs made with token-goat\'s hooks on are averaged on the Hooks page and listed one by one on the Results page.');
    S.table(S.byId('o-runs-table'),
      [['Method'], ['Reviewer', 0, 'wide'], ['Run'], ['Finished'], ['Plan'], ['Plan tokens', 1], ['Subagent tokens', 1], ['Review tokens', 1], ['Total tokens', 1],
        ['Review output tokens', 1], ['Review turns', 1], ['Review tool calls', 1], ['Cache warm', 1], ['Of which orchestrator', 1], ['Subagents, mean score', 1], ['After review, mean score', 1],
        ['Corrected', 1], ['Still wrong', 1], ['Broken', 1], ['Seconds', 1]],
      runs.map(function (c) {
        var r = c.roles;
        return [S.armLabel(S.arm(c.arm)), reviewer(c.setup), c.rep, c.complete === false ? 'no' : 'yes', c.plan_imputed ? 'shared' : 'measured',
          S.int(r.plan.tokens), S.int(r.workers.tokens), S.int(r.verify.tokens), S.int(c.tokens), S.int(r.verify.output), S.int(r.verify.requests), S.int(r.verify.tool_calls),
          S.usd(c.cost_warm), S.usd(c.orchestrator_cost_warm), S.pct(c.worker_quality), S.pct(c.quality), c.fixed, c.still_wrong, c.broken, S.int(c.wall_seconds)];
      }), 'Every orchestrated run in this selection');
  }

  function drawCharts() { drawRoles(); drawSetups(); }
  function render() { drawFilters(); drawCharts(); drawReview(); drawRuns(); }
  drawQuestions();
  drawReviewers();
  render();
  S.onRedraw(drawCharts);
})();
