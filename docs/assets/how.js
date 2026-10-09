/* How it was measured: the method notes, setups, tiers, prices and the start-up context. */
(function () {
  'use strict';
  var S = window.Site, C = window.Charts;
  if (!S || !S.ready || !C) return;
  var B = window.BENCH, text = B.text || {}, price = B.price || {};

  S.fillList(S.byId('method'), text.method);
  S.setText('cache-text', text.cache);
  S.setText('context-sub', text.context);

  S.table(S.byId('setups-table'), [['Setup'], ['What it is', 0, 'wide']],
    S.list('setups').map(function (s) { return [s.name, s.detail]; }), 'The agent setups');

  // A price per million tokens, without padding zeros.
  var rate = function (id, key) {
    var v = price[id] && price[id][key];
    if (typeof v !== 'number' || !isFinite(v)) return '';
    return '$' + (v >= 1 ? (Number.isInteger(v) ? String(v) : v.toFixed(2)) : String(+v.toPrecision(3)));
  };
  S.table(S.byId('tiers-table'),
    [['Tier'], ['Model class', 0, 'wide'], ['Input', 1], ['Output', 1], ['Cache read', 1], ['Cache write', 1]],
    S.list('tiers').map(function (t) { return [t.name, t.model, rate(t.id, 'input'), rate(t.id, 'output'), rate(t.id, 'cache_read'), rate(t.id, 'cache_write')]; }),
    'Model tiers and prices per million tokens');

  S.table(S.byId('cache-table'),
    [['Model tier'], ['Start-up tokens', 1], ['Cold start', 1], ['Warm start', 1], ['Cold against warm', 1], ['Same call measured both ways', 0, 'wide']],
    (Array.isArray(B.cache) ? B.cache : []).map(function (r) {
      return [r.tier, S.int(r.tokens), S.usd(r.cold), S.usd(r.warm), r.warm > 0 ? S.num(r.cold / r.warm, 0) + ' times' : '', r.measured || ''];
    }), 'Cold start against warm start');

  function draw() {
    C.drawContext({ host: S.byId('context'), legend: S.byId('context-legend'), table: S.byId('context-table') }, (B.context || {}).parts);
  }
  draw();
  S.onRedraw(draw);
})();
