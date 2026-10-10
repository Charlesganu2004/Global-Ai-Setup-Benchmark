/* Hooks: each method measured without token-goat's hooks beside its twin measured with them. */
(function () {
  'use strict';
  var S = window.Site, C = window.Charts;
  if (!S || !S.ready || !C) return;
  var B = window.BENCH, H = B.hooks || {}, text = B.text || {};
  var pairs = Array.isArray(H.pairs) ? H.pairs : [], facts = H.facts || {};

  var STATES = [['without', 'Hooks off', '--s1'], ['with', 'Hooks on', '--s2']];
  var MEASURES = [{ id: 'tokens', name: 'Tokens' }, { id: 'requests', name: 'Turns' }, { id: 'cost_warm', name: 'Cost' }];
  var UNIT = { tokens: 'tokens per run', requests: 'turns per run', cost_warm: 'per run, cache warm' };

  var setups = S.list('setups').filter(function (s) { return pairs.some(function (p) { return p.setup === s.id; }); });
  var setupId = setups.length ? setups[0].id : null, measure = 'tokens';

  // A pair is named after its method, without the state one of its two sides carries in its name.
  var stateless = function (name) { return String(name).replace(/, hooks (off|on)$/, ''); };
  var label = function (p) { return 'Tier ' + p.tier + ': ' + p.off + '  ' + stateless(S.arm(p.off).name); };
  var show = function (v) { return measure === 'tokens' ? S.tok(v) : measure === 'requests' ? S.num(v, 1) : S.usd(v); };
  var signed = function (v) {
    if (typeof v !== 'number' || !isFinite(v)) return '';
    var p = Math.round(v * 100);
    return (p > 0 ? '+' : '') + p + '%';
  };
  var inView = function (p) { return p.setup === setupId; };
  var order = function (a, b) { return S.armOrder(a.off) - S.armOrder(b.off) || S.tierOrder(a.tier) - S.tierOrder(b.tier); };

  // The first line is the verdict, already in the page as plain HTML. The rest go in the block under it.
  S.fillList(S.byId('h-findings'), (text.hooks || []).slice(1));

  function drawFilters() {
    S.segmented(S.byId('h-setup'), setups, setupId, function (id) { setupId = id; render(); });
    S.segmented(S.byId('h-measure'), MEASURES, measure, function (id) { measure = id; render(); });
    var rows = pairs.filter(inView);
    var off = rows.reduce(function (sum, p) { return sum + p.without.n; }, 0), on = rows.reduce(function (sum, p) { return sum + p.with.n; }, 0);
    S.setText('h-note', S.setup(setupId).detail + ' ' + S.plural(rows.length, 'pair', 'pairs') + ': ' + S.plural(off, 'run', 'runs') +
      ' without the hooks, ' + on + ' with them.');
  }

  function drawPairs() {
    var rows = pairs.filter(inView).slice().sort(order);
    C.drawDots({ host: S.byId('pairs'), legend: S.byId('pairs-legend') }, rows.map(function (p) {
      return {
        label: label(p),
        dots: STATES.map(function (state, i) {
          var b = p[state[0]];
          return {
            series: i, value: b[measure], label: 'Tier ' + p.tier + ': ' + S.armLabel(S.arm(state[0] === 'with' ? p.on : p.off)),
            tip: function () {
              var lines = [{ value: S.int(b.tokens), label: 'tokens' }, { value: S.num(b.requests, 1), label: 'turns, ' + S.num(b.tool_calls, 1) + ' tool calls' },
                { value: S.usd(b.cost_warm), label: 'per run, cache warm' }, { value: S.pct(b.quality), label: 'answered correctly, ' + S.plural(b.n, 'run', 'runs') }];
              if (state[0] === 'with') lines.push({ value: S.num(b.hook_calls, 1), label: 'hook runs a run, in ' + b.runs_with_hooks + ' of ' + S.plural(b.n, 'run', 'runs') });
              return lines;
            }
          };
        })
      };
    }), {
      // Cost is a hundred times apart between tiers: on a straight axis the cheapest tier's dots would all sit on zero.
      series: STATES.map(function (state) { return [state[1], state[2]]; }), format: show, unit: UNIT[measure], log: measure === 'cost_warm',
      label: MEASURES.find(function (m) { return m.id === measure; }).name + ' per run with the hooks off and on, ' + S.setup(setupId).name
    });
    S.table(S.byId('pairs-table'),
      [['Method'], ['Tier'], ['Runs off', 1], ['Runs on', 1], ['Tokens off', 1], ['Tokens on', 1], ['Change', 1], ['Turns off', 1], ['Turns on', 1],
        ['Tool calls off', 1], ['Tool calls on', 1], ['Cost off', 1], ['Cost on', 1], ['Mean score off', 1], ['Mean score on', 1],
        ['Runs fully correct off', 1], ['Runs fully correct on', 1], ['Start-up tokens off', 1], ['Start-up tokens on', 1], ['Runs the hooks ran in', 1]],
      rows.map(function (p) {
        var a = p.without, b = p.with;
        return [S.armLabel(S.arm(p.off)), S.tier(p.tier).name, a.n, b.n, S.int(a.tokens), S.int(b.tokens), signed(p.token_change), S.num(a.requests, 1), S.num(b.requests, 1),
          S.num(a.tool_calls, 1), S.num(b.tool_calls, 1), S.usd(a.cost_warm), S.usd(b.cost_warm), S.pct(a.quality), S.pct(b.quality),
          a.perfect_runs + ' of ' + a.n, b.perfect_runs + ' of ' + b.n, S.int(a.start), S.int(b.start), b.runs_with_hooks + ' of ' + b.n];
      }), 'Hooks off against hooks on, by method and tier', { noun: 'pair', primary: 6, key: [0, 1, 4, 5, 6, 15, 16, 19] });
  }

  // In how many runs the agent that looked up the callers left one out of its own answer.
  function lost(side) {
    var q = side.q1_incomplete;
    return q ? q.runs + ' of ' + side.n + ' runs' : '';
  }
  function drawActivity() {
    var rows = pairs.filter(inView).slice().sort(order);
    S.table(S.byId('activity-table'),
      [['Method, hooks on'], ['Tier'], ['How its commands were sent', 0, 'wide'], ['Runs', 1], ['Runs the hooks ran in', 1], ['Hook runs a run', 1],
        ['Calls rewritten', 1], ['Redaction markers', 1], ['Notes added', 1], ['Calls refused', 1], ['Callers question 1 lost, off', 1], ['Callers question 1 lost, on', 1],
        ['Characters of tool results off', 1], ['Characters of tool results on', 1]],
      rows.map(function (p) {
        var a = p.without, b = p.with, twin = S.arm(p.on);
        var sent = (twin.uses || []).join() === 'grep' ? 'No commands: the Grep, Glob and Read tools only'
          : twin.shell === 'Bash' ? 'The Bash tool' : 'The PowerShell tool';
        return [S.armLabel(twin), S.tier(p.tier).name, sent, b.n, b.runs_with_hooks + ' of ' + b.n, S.num(b.hook_calls, 1), S.num(b.hook_rewrites, 1),
          S.num(b.hook_redactions, 1), S.num(b.hook_notes, 1), S.num(b.hook_denials, 2), lost(a), lost(b), S.int(a.tool_output_chars), S.int(b.tool_output_chars)];
      }), 'What the hooks did, by method and tier', { noun: 'pair', key: [0, 1, 4, 5, 6, 7, 9, 11] });
  }

  function drawFacts() {
    var rows = [['Version', facts.version], ['Licence', facts.licence], ['Installed size', typeof facts.installed_size_mb === 'number' ? facts.installed_size_mb + ' MB' : ''],
      ['Events hooked in the client', (facts.claude_events || []).join(', ')], ['Tools matched before a call', facts.pre_tool_matcher],
      ['Tools matched after a call', facts.post_tool_matcher], ['Not matched', facts.not_matched],
      ['What a rewrite took out', facts.redaction_note], ['Also installed', facts.skill_note],
      ['What else differs between the two sides', facts.context_note],
      ['How this was measured', facts.how_measured]]
      .filter(function (row) { return row[1]; });
    S.table(S.byId('facts-table'), [['Fact'], ['Value', 0, 'wide']], rows, 'What the hooks are', { sort: false });
    S.setText('block-note', facts.block_note);
    var sizes = facts.block_bytes || {};
    S.table(S.byId('block-table'), [['Rules file'], ['Bytes added', 1], ['About, in tokens', 1]],
      Object.keys(sizes).map(function (path) { return [path, S.int(sizes[path]), S.int(sizes[path] / 4)]; }), 'Size of the block added to each rules file');
  }

  function render() { drawFilters(); drawPairs(); drawActivity(); }
  drawFacts();
  render();
  S.onRedraw(drawPairs);
})();
