/* Home: the summary text, the tiles, the leaderboard and the findings.
   The four answers under the title are plain HTML, written into the page by the site builder. */
(function () {
  'use strict';
  var S = window.Site;
  if (!S || !S.ready) return;
  var B = window.BENCH, text = B.text || {}, el = S.el;

  S.setText('lede', text.lede);
  var tiles = S.byId('tiles');
  tiles.replaceChildren();
  (text.tiles || []).forEach(function (t) {
    var tile = el('div', { class: 'tile' });
    tile.appendChild(el('div', { class: 'value' }, t.value));
    tile.appendChild(el('div', { class: 'label' }, t.label));
    tiles.appendChild(tile);
  });
  S.fillList(S.byId('findings'), text.findings);

  var setupId = S.defaultSetup(), tierId = S.cheapestTier();

  function methodCell(a) {
    var cell = el('span');
    cell.appendChild(el('span', { class: 'code' }, a.id));
    cell.appendChild(document.createTextNode('  ' + a.name));
    if (a.control) cell.appendChild(el('span', { class: 'tag' }, 'control'));
    return cell;
  }
  function runsCell(n) {
    var cell = el('span', null, String(n));
    if (n < 3) cell.appendChild(el('span', { class: 'tag' }, 'fewer than 3 runs'));
    return cell;
  }
  function barCell(share) {
    var bar = el('span', { class: 'minibar', 'aria-hidden': 'true' });
    bar.style.width = Math.max(1, Math.round(share * 100)) + '%';
    return bar;
  }

  function render() {
    var tiers = S.tiersIn(S.pointsFor(setupId));
    if (!tiers.some(function (t) { return t.id === tierId; })) tierId = tiers.length ? tiers[0].id : null;
    S.segmented(S.byId('board-setup'), S.list('setups'), setupId, function (id) { setupId = id; render(); });
    S.segmented(S.byId('board-tier'), tiers.map(function (t) { return { id: t.id, name: t.name }; }), tierId, function (id) { tierId = id; render(); });

    var rows = S.leaderboard(setupId, tierId);
    var t = S.tier(tierId);
    S.setText('board-note', S.setup(setupId).detail + ' ' + (tierId ? t.name + ': ' + t.model + '. ' : '') +
      S.plural(rows.length, 'method', 'methods') + ' measured here. The rank is by fewest tokens; any column sorts.');
    S.table(S.byId('board'),
      [['Rank', 1], ['Method'], ['Group'], ['Runs', 1], ['Turns', 1], ['Total tokens', 1], ['Tokens as a bar', 0, 'barcell', 1], ['Cost, cache warm', 1], ['Answered correctly', 1]],
      rows.map(function (row) {
        var p = row.point;
        return [row.rank, methodCell(row.arm), row.arm.group ? S.group(row.arm.group).name : '', runsCell(p.n), S.turns(p), S.int(p.tokens),
          barCell(row.share), S.usd(p.cost_warm), S.pct(p.quality)];
      }), 'Leaderboard: methods by fewest tokens',
      { limit: 10, noun: 'method', primary: 5, rowHeader: 1, key: [0, 1, 5, 8] });
  }
  render();
})();
