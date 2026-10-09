/* Results: one filter row that scopes the three charts and the table of runs. */
(function () {
  'use strict';
  var S = window.Site, C = window.Charts;
  if (!S || !S.ready || !C) return;

  var setupId = S.defaultSetup(), groupId = 'all';
  var hosts = function (name) { return { host: S.byId(name), legend: S.byId(name + '-legend'), table: S.byId(name + '-table') }; };

  function drawFilters() {
    S.segmented(S.byId('f-setup'), S.list('setups'), setupId, function (id) { setupId = id; render(); });
    S.segmented(S.byId('f-group'), S.groupOptions(), groupId, function (id) { groupId = id; render(); });
    var note = S.setup(setupId).detail;
    if (groupId !== 'all') note += ' ' + S.group(groupId).name + ': ' + S.group(groupId).detail;
    S.setText('f-note', note);
  }
  function drawCharts() {
    var points = S.pointsFor(setupId, groupId);
    C.drawBubble(hosts('bubble'), points);
    C.drawTokens(hosts('tokens'), points);
    C.drawCost(hosts('cost'), points);
  }
  function drawRuns() {
    var runs = S.runsFor(setupId, groupId);
    S.setText('runs-note', S.plural(runs.length, 'run', 'runs') + ' in this selection. A run that did not finish is listed but is left out of the averages above.');
    var questions = S.questionColumns();
    S.table(S.byId('runs-table'),
      [['Method'], ['Tier'], ['Run'], ['Agents', 1], ['Finished'], ['Turns', 1], ['Tool calls', 1], ['Start-up', 1], ['Re-read', 1], ['Added', 1], ['Output', 1],
        ['Total tokens', 1], ['Cache warm', 1], ['Cache cold', 1], ['As run', 1]]
        .concat(questions.map(function (q) { return [q.label, 1]; }), [['Seconds', 1], ['Outside the tool policy']]),
      runs.map(function (c) {
        return [S.armLabel(S.arm(c.arm)), S.tier(c.tier).name, c.rep, (c.agents == null ? '' : c.agents) + (c.plan_imputed ? ' + plan' : ''), c.complete === false ? 'no' : 'yes',
          S.int(c.requests), S.int(c.tool_calls), S.int(c.start), S.int(c.reread), S.int(c.added), S.int(c.output), S.int(c.tokens),
          S.usd(c.cost_warm), S.usd(c.cost_cold), S.usd(c.cost)]
          .concat(questions.map(function (q) { return S.score(c.scores, q.key); }), [S.int(c.wall_seconds), (c.off_policy || []).join(', ')]);
      }), 'Every run in this selection');
  }
  function render() { drawFilters(); drawCharts(); drawRuns(); }
  render();
  S.onRedraw(drawCharts);
})();
