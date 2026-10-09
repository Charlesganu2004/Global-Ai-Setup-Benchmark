/* Setup: the measured cost of each index option. The install text is static HTML. */
(function () {
  'use strict';
  var S = window.Site, C = window.Charts;
  if (!S || !S.ready || !C) return;
  var B = window.BENCH, index = B.index || {}, el = S.el;

  S.setText('index-text', (B.text || {}).index || index.dataset || '');

  var partsLine = function (name, parts) {
    var keys = Object.keys(parts || {});
    return keys.length ? name + ' on disk, in megabytes: ' + keys.map(function (k) { return k + ' ' + parts[k]; }).join(', ') + '.' : '';
  };
  S.setText('index-parts', [partsLine('graphify', index.graphify_parts), partsLine('semble', index.semble_parts)].filter(Boolean).join(' '));

  var quality = index.search_quality || {};
  S.setText('search-sub', typeof quality.queries === 'number'
    ? S.plural(quality.queries, 'query', 'queries') + ' per row. Each query describes one function, and the engine has to return that function.'
    : 'Each query describes one function, and the engine has to return that function.');
  S.table(S.byId('search-table'),
    [['Engine'], ['Query used', 0, 'wide'], ['Right function ranked first', 1], ['Right function in the top five', 1]],
    (quality.rows || []).map(function (r) { return [r.engine, r.query, S.pct(r.top1), S.pct(r.top5)]; }), 'Search quality by engine');
  S.setText('search-note', quality.note || '');

  // One lookup by engine: the columns are whatever the data file lists.
  var LOOKUP_NAMES = { what: 'Question', graphify: 'graphify', rust_raw: 'Rust index, raw output', lx: 'lx' };
  var lookup = Array.isArray(index.lookup) ? index.lookup : [];
  if (lookup.length) {
    var keys = [];
    lookup.forEach(function (row) { Object.keys(row).forEach(function (k) { if (keys.indexOf(k) < 0) keys.push(k); }); });
    S.table(S.byId('lookup-table'), keys.map(function (k) { return [LOOKUP_NAMES[k] || k, 0, 'wide']; }),
      lookup.map(function (row) { return keys.map(function (k) { return row[k]; }); }), 'One lookup by engine');
  } else {
    S.byId('lookup-card').hidden = true;
  }

  var models = Array.isArray(index.models) ? index.models : [];
  var list = S.byId('models');
  list.replaceChildren();
  models.forEach(function (m) {
    var item = el('li');
    item.appendChild(el('strong', null, m.name + ': '));
    item.appendChild(document.createTextNode((typeof m.mb === 'number' ? m.mb + ' MB' : 'size not measured') + (m.note ? ', ' + m.note : '') + '.'));
    list.appendChild(item);
  });
  if (!models.length) S.byId('models-card').hidden = true;

  function draw() { C.drawIndex({ host: S.byId('index'), table: S.byId('index-table') }, index.stacks); }
  draw();
  S.onRedraw(draw);
})();
