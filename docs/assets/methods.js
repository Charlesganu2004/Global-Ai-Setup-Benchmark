/* Methods: the questions, an index of every method, then one card per method with its commands. */
(function () {
  'use strict';
  var S = window.Site;
  if (!S || !S.ready) return;
  var el = S.el;

  /* ---------- the questions ---------- */
  var questions = S.byId('questions-list');
  questions.replaceChildren();
  S.list('questions').forEach(function (q, i) {
    var card = el('div', { class: 'question' });
    card.appendChild(el('h3', null, 'Question ' + (i + 1)));
    card.appendChild(el('p', { class: 'ask' }, q.text));
    [['Right answer', q.truth], ['Graded by', q.graded]].forEach(function (pair) {
      if (!pair[1]) return;
      var line = el('p');
      line.appendChild(el('span', { class: 'k' }, pair[0] + ': '));
      line.appendChild(document.createTextNode(pair[1]));
      card.appendChild(line);
    });
    questions.appendChild(card);
  });

  var groupsText = S.byId('groups-text');
  groupsText.replaceChildren();
  S.groupOptions().slice(1).forEach(function (g) {
    var item = el('li');
    item.appendChild(el('strong', null, g.name + '. '));
    item.appendChild(document.createTextNode(S.group(g.id).detail || ''));
    groupsText.appendChild(item);
  });

  /* ---------- the catalogue ---------- */
  var groupId = 'all', query = '';
  var setupId = S.defaultSetup(), tierId = S.cheapestTier();
  var input = S.byId('q');

  // The result on the default setup and the cheapest tier. A method that was not
  // measured there shows its first result in setup order, then tier order.
  function firstPoint(a) {
    var found = null;
    S.list('setups').some(function (s) {
      return S.list('tiers').some(function (t) { found = S.point(a.id, s.id, t.id); return !!found; });
    });
    return found;
  }
  function pointOf(a) { return (setupId && tierId ? S.point(a.id, setupId, tierId) : null) || firstPoint(a); }
  function resultLine(a) {
    var p = pointOf(a);
    if (!p) return el('p', { class: 'result none' }, 'Not measured yet.');
    return el('p', { class: 'result' }, S.setup(p.setup).name + ', ' + S.tier(p.tier).name + ': ' + S.tok(p.tokens) + ' tokens, ' +
      S.turnsText(p) + ', ' + S.pct(p.quality) + ' answered correctly, ' + S.plural(p.n, 'run', 'runs') + '.');
  }
  function card(a) {
    var box = el('article', { class: 'method' });
    // The heading is the code and the name, nothing else: tags sit on their own line under it.
    var head = el('h3', { id: 'm-' + a.id });
    head.appendChild(el('span', { class: 'code' }, a.id));
    head.appendChild(document.createTextNode(' '));
    head.appendChild(el('span', null, a.name));
    box.appendChild(head);
    S.anchor(head);
    var meta = [a.group ? S.group(a.group).name : '', a.control ? 'the control' : '', a.index || ''].filter(Boolean);
    if (meta.length) box.appendChild(el('p', { class: 'meta' }, meta.join(', ')));
    if (a.detail) box.appendChild(el('p', null, a.detail));
    box.appendChild(el('p', { class: 'label' }, 'Commands the agents were given'));
    box.appendChild(el('pre', { class: 'wrap-lines' }, (a.commands || []).join('\n') || 'None listed.'));
    box.appendChild(el('p', { class: 'label' }, 'How code was read'));
    box.appendChild(el('p', null, a.reads || 'Not recorded.'));
    box.appendChild(el('p', { class: 'label' }, 'Result'));
    box.appendChild(resultLine(a));
    return box;
  }
  function render() {
    S.segmented(S.byId('m-group'), S.groupOptions(), groupId, function (id) { groupId = id; render(); });
    drawCards();
  }
  function drawCards() {
    var found = S.searchArms(query, groupId), host = S.byId('methods');
    S.setText('m-note', found.length + ' of ' + S.plural(S.list('arms').length, 'method', 'methods') + ' shown.');
    // The index: one line a method, each code a link to its card below.
    S.table(S.byId('m-index'),
      [['ID'], ['Method'], ['Group'], ['Measured as'], ['Runs', 1], ['Turns', 1], ['Tokens', 1], ['Answered correctly', 1]],
      found.map(function (a) {
        var p = pointOf(a), link = el('a', { href: '#m-' + a.id, class: 'code' }, a.id);
        return [link, a.name, a.group ? S.group(a.group).name : '', p ? S.setup(p.setup).name + ', Tier ' + p.tier : 'not measured',
          p ? p.n : '', p ? S.turns(p) : '', p ? S.int(p.tokens) : '', p ? S.pct(p.quality) : ''];
      }), 'Index of the methods shown', { noun: 'method', primary: 6, rowHeader: 1 });
    host.replaceChildren();
    if (!found.length) { host.appendChild(el('p', { class: 'note' }, 'No method matches. Clear the search or pick another group.')); return; }
    found.forEach(function (a) { host.appendChild(card(a)); });
  }
  input.addEventListener('input', function () { query = input.value; drawCards(); });
  render();
})();
