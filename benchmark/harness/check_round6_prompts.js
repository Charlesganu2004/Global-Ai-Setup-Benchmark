// A dry run of token-benchmark.workflow.js: no model is called. The workflow runs with fake agent(),
// parallel() and log() functions that record every prompt, so the prompts of every round six cell can
// be built from dummy packs and checked.
//
//   node benchmark/harness/check_round6_prompts.js [--dump FILE] [--inside FILE] [--args FILE] [--workflow FILE]
//
// Checks, each counted and each reported by name when it fails:
//   - the workflow is plain JavaScript for the workflow runtime: it parses as the body of an async
//     function and has no import, require, file access, Date.now, Math.random or argless new Date,
//   - its meta block is a pure literal whose phases are the phase names the script uses,
//   - R6 sends byte for byte what RGS sends, in every setup and on both question sets,
//   - a pack arm's prompt holds its own pack and no other, a hub arm's prompt names its own bank and no
//     other, and no two repeats of a hub cell share a bank,
//   - a missing or stale pack, a missing run tag and the other mistakes stop the round before any agent
//     starts,
//   - the packs and loadouts reach the workflow through args, which the orchestrating model has to type
//     out: a text that was cut, changed or moved to another place is found against args.sums (SHA-256 of
//     every text, by position) before any agent starts, and the SHA-256 in the script gives the same
//     digest as node's own.
// With --dump FILE the prompts of the two round six matrices are written there as JSON, for the tests
// that compare them with arms.json. --inside FILE writes the question texts and the arm tables the
// script holds, --args FILE also runs the workflow once with an args file written by round6_args.py.
'use strict'
const fs = require('fs')
const path = require('path')
const crypto = require('crypto')

const HERE = __dirname
// --workflow FILE checks another copy of the workflow (the tests use this to prove that a broken copy fails).
const flag = name => (process.argv.includes(name) ? process.argv[process.argv.indexOf(name) + 1] : null)
const SOURCE = fs.readFileSync(flag('--workflow') || path.join(HERE, 'token-benchmark.workflow.js'), 'utf8').replace(/\r\n/g, '\n')
const failures = []
let passed = 0
function expect(ok, what) {
  if (ok) passed += 1
  else failures.push(what)
}

// ---------------------------------------------------------------- the source as the runtime sees it

// The source with comments, strings and template text blanked out, so that identifiers can be looked for.
function blanked(text) {
  let out = ''
  let i = 0
  const open = []             // for each ${ inside a template literal: the brace depth it started at
  let braces = 0
  let mode = 'code'           // code, line, block, single, double or template
  while (i < text.length) {
    const c = text[i]
    const next = text[i + 1]
    if (mode === 'line') {
      if (c === '\n') { mode = 'code'; out += c } else out += ' '
    } else if (mode === 'block') {
      if (c === '*' && next === '/') { mode = 'code'; out += '  '; i += 1 } else out += c === '\n' ? c : ' '
    } else if (mode === 'single' || mode === 'double') {
      if (c === '\\') { out += '  '; i += 1 } else if ((mode === 'single' && c === "'") || (mode === 'double' && c === '"')) { mode = 'code'; out += c } else out += ' '
    } else if (mode === 'template') {
      if (c === '\\') { out += '  '; i += 1 } else if (c === '`') { mode = 'code'; out += c } else if (c === '$' && next === '{') { open.push(braces); braces += 1; mode = 'code'; out += '  '; i += 1 } else out += c === '\n' ? c : ' '
    } else if (c === '/' && next === '/') { mode = 'line'; out += '  '; i += 1 } else if (c === '/' && next === '*') { mode = 'block'; out += '  '; i += 1 } else if (c === "'") { mode = 'single'; out += c } else if (c === '"') { mode = 'double'; out += c } else if (c === '`') { mode = 'template'; out += c } else if (c === '{') { braces += 1; out += c } else if (c === '}') {
      braces -= 1
      if (open.length && open[open.length - 1] === braces) { open.pop(); mode = 'template'; out += ' ' } else out += c
    } else out += c
    i += 1
  }
  return out
}

const code = blanked(SOURCE)
const FORBIDDEN = [
  [/\bimport\b/, 'import'], [/\brequire\b/, 'require'], [/\bDate\.now\b/, 'Date.now'], [/\bMath\.random\b/, 'Math.random'],
  [/new\s+Date\s*\(\s*\)/, 'an argless new Date'], [/\bprocess\b/, 'process'], [/\bfs\b/, 'fs'], [/\bfetch\b/, 'fetch'],
  [/\bXMLHttpRequest\b/, 'XMLHttpRequest'], [/\beval\b/, 'eval'], [/\bFunction\s*\(/, 'Function('],
  [/\bsetTimeout\b|\bsetInterval\b/, 'a timer'], [/\bperformance\b/, 'performance'], [/\breadFile|\bwriteFile|\bchild_process\b/, 'file or process access'],
]
for (const [pattern, name] of FORBIDDEN) expect(!pattern.test(code), `the workflow uses ${name}`)
const exported = code.match(/\bexport\b[^\n]*/g) || []
expect(exported.length === 1 && /^export\s+const\s+meta\s*=\s*\{/.test(exported[0]), `the workflow exports more than its meta block: ${exported.join(' | ')}`)

// The meta block runs from "export const meta = {" to the first closing brace at the start of a line.
const metaStart = SOURCE.indexOf('export const meta = ')
const metaEnd = SOURCE.indexOf('\n}\n', metaStart)
expect(metaStart === 0 && metaEnd > 0, 'the meta block is not the first statement of the workflow')
const metaText = SOURCE.slice(metaStart + 'export const meta = '.length, metaEnd + 2)
// A pure literal: with the strings and the keys taken out, only braces, brackets, commas and colons are left.
const metaBare = metaText.replace(/'(?:[^'\\]|\\.)*'/g, "''").replace(/[A-Za-z_][A-Za-z_0-9]*\s*:/g, ':')
expect(/^[\s{}\[\],:']*$/.test(metaBare), `the meta block is not a pure literal, it holds: ${metaBare.replace(/[\s{}\[\],:']/g, '').slice(0, 40)}`)
let meta = null
try { meta = new Function('return (' + metaText + ')')() } catch (error) { expect(false, `the meta block does not evaluate: ${error.message}`) }
const used = []
for (const found of SOURCE.matchAll(/phase:\s*'([^']+)'/g)) if (!used.includes(found[1])) used.push(found[1])
expect(!!meta && JSON.stringify(meta.phases.map(p => p.title).sort()) === JSON.stringify(used.slice().sort()),
  `the phases of the meta block (${meta ? meta.phases.map(p => p.title).join(', ') : '?'}) are not the phase names the script uses (${used.join(', ')})`)

// The body as the runtime runs it: the export dropped, wrapped in an async function.
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor
const RETURN = 'return { rep: REP, cells: done }'
expect(SOURCE.split(RETURN).length === 2, 'the workflow no longer ends with its single return statement')
const body = SOURCE.replace('export const meta', 'const meta')
let compiled = null
try { compiled = new AsyncFunction('args', 'agent', 'parallel', 'log', body) } catch (error) { expect(false, `the workflow does not parse as the body of an async function: ${error.message}`) }
// The same body with a second return value, so that the questions and the helpers can be read out.
const peeking = compiled && new AsyncFunction('args', 'agent', 'parallel', 'log', body.replace(RETURN,
  'return { rep: REP, cells: done, inside: { Q, Q_B, Q_T, PACK_OF, HUB, SET_T, policy, baseArm, setOf, hasArm, bankOf, validName, sha256Text } }'))

// ---------------------------------------------------------------- a fake runtime

function fill(schema) {
  if (!schema) return null
  const type = Array.isArray(schema.type) ? schema.type.find(t => t !== 'null') : schema.type
  if (schema.enum) return schema.enum[0]
  if (type === 'object') return Object.fromEntries(Object.entries(schema.properties || {}).map(([key, value]) => [key, fill(value)]))
  if (type === 'array') return []
  if (type === 'integer' || type === 'number') return 1
  if (type === 'boolean') return true
  return 'x'
}

async function run(args) {
  const calls = []
  const agent = async (prompt, options) => {
    calls.push({ prompt, label: options.label, phase: options.phase, model: options.model, effort: options.effort })
    return fill(options.schema)
  }
  const parallel = async fns => Promise.all(fns.map(fn => fn()))
  const result = await peeking(args, agent, parallel, () => {})
  return { calls, result }
}

async function refused(args, pattern, what) {
  const started = []
  let message = ''
  try {
    await peeking(args, async (prompt, options) => { started.push(options.label); return fill(options.schema) }, async fns => Promise.all(fns.map(fn => fn())), () => {})
  } catch (error) { message = String(error.message) }
  expect(pattern.test(message), `${what}: expected an error matching ${pattern}, got ${JSON.stringify(message)}`)
  expect(started.length === 0, `${what}: ${started.length} agents started before the error`)
}

// ---------------------------------------------------------------- dummy data

const BASE = { repo: 'C:\\path\\to\\lxbench\\repo', repoPlain: 'C:\\path\\to\\lxbench\\repo-noemb', repoTeam: 'C:\\path\\to\\lxbench\\repo-team', runTag: 'dry' }
const KEYS = ['q1', 'q2', 'q3']
const sha = text => crypto.createHash('sha256').update(text, 'utf8').digest('hex')
const tokensOf = text => Math.ceil(Buffer.byteLength(text, 'utf8') / 4)
const clone = value => JSON.parse(JSON.stringify(value))

function dummyPacks(inside) {
  const packs = {}
  for (const variant of ['gs', 'g', 's']) {
    packs[variant] = {}
    for (const set of ['A', 'B']) {
      packs[variant][set] = {}
      for (const q of KEYS) {
        // A caller line, a call line and a non-ASCII character, so that a cut or a changed line has something to hit.
        const output = `DUMMYPACK ${variant}/${set}/${q}\n== find\nsome_function a/b.py:1-9 (x é€)\n== graph\nquery: 1 nodes\n<-- caller_one a/c.py:3-4\n<-- caller_two a/d.py:5-6`
        packs[variant][set][q] = { question: (set === 'A' ? inside.Q : inside.Q_B)[q], command: 'pgr pack', output, tokens: tokensOf(output), sha256: sha(output) }
      }
    }
  }
  return packs
}

function dummyLoadouts() {
  const tasks = { q1: 'cli-freeze', q2: 'ssrf-review', q3: 'merge-driver-audit' }
  const out = {}
  for (const q of KEYS) {
    // q1 cites a ticket and q2 holds a note with the stale mark, the two things a changed loadout would lose.
    const note = q === 'q1' ? 'Decision BLD-9999 keeps a name' : q === 'q2' ? 'A note about a function (0s ago, stale: code changed) [reviewer]' : 'A note'
    const output = `DUMMYLOADOUT ${q}\n== task ${tasks[q]}\n- ${note} #1\nloadout: 1 notes`
    out[q] = { task: tasks[q], output, tokens: tokensOf(output) }
  }
  return out
}

// The checksums that round6_args.py puts in args.sums: the SHA-256 of every text, keyed by its place.
function sumsOf(packs, loadouts) {
  const sums = {}
  for (const variant of Object.keys(packs)) for (const set of Object.keys(packs[variant])) for (const q of KEYS) sums[`packs/${variant}/${set}/${q}`] = sha(packs[variant][set][q].output)
  for (const q of KEYS) sums[`loadouts/${q}`] = sha(loadouts[q].output)
  return sums
}

const read = name => JSON.parse(fs.readFileSync(path.join(HERE, name), 'utf8'))
const cellKeys = matrix => matrix.flatMap(block => block.reps.flatMap(rep => block.arms.map(arm => [arm, block.setup, block.tier, rep].join('|'))))

;(async () => {
  if (!peeking) return report()
  const first = await run({ ...BASE, matrix: [] })
  const inside = first.result.inside
  const packs = dummyPacks(inside)
  const loadouts = dummyLoadouts()
  const sums = sumsOf(packs, loadouts)
  const full = extra => ({ ...BASE, packs, loadouts, sums, ...extra })

  // ---- the arm ids
  const base = id => inside.baseArm(id)
  expect(base('R6') === 'R6' && base('R6h') === 'R6', 'R6h does not resolve to R6')
  expect(base('THh') === 'TH' && base('TH') === 'TH', 'THh does not resolve to TH')
  expect(base('TPHh') === 'TPH' && base('TPH') === 'TPH', 'TPHh does not resolve to TPH')
  expect(base('TPh') === 'TP' && base('TPGh') === 'TPG' && base('TPSh') === 'TPS', 'TPh, TPGh or TPSh does not resolve')
  expect(inside.setOf({ arm: 'THh' }) === 'B' && inside.setOf({ arm: 'TPHh' }) === 'B' && inside.setOf({ arm: 'TH' }) === 'A', 'THh or TPHh is not on set B, or TH is not on set A')
  expect(['T0', 'TL', 'TI'].every(id => inside.setOf({ arm: id }) === 'T'), 'T0, TL or TI is not on set T')
  let threw = ''
  try { inside.baseArm('XYh') } catch (error) { threw = error.message }
  expect(/there is no arm/.test(threw), 'an h suffix on an unknown arm is accepted')
  for (const id of ['R6', 'TP', 'TPG', 'TPS', 'TH', 'TPH', 'T0', 'TL', 'TI']) expect(inside.hasArm(id), `${id} is not a registered arm`)

  // ---- R6 sends what RGS sends, in every setup and on both question sets
  const setups = ['single', 'parallel', 'orch', 'orch-notools', 'orch-mid', 'orch-low']
  const twin = await run(full({ matrix: setups.map(setup => ({ arms: ['R6', 'RGS', 'R6h', 'RGSh'], setup, tier: 'C', reps: ['r1'] })) }))
  const byLabel = {}
  for (const call of twin.calls) byLabel[call.label] = call
  const partner = { R6: 'RGS', R6h: 'RGSh' }
  let compared = 0
  for (const call of twin.calls) {
    const [arm, ...rest] = call.label.split('|')
    if (!partner[arm]) continue
    const other = byLabel[[partner[arm], ...rest].join('|')]
    expect(!!other, `no RGS prompt to compare with ${call.label}`)
    if (!other) continue
    compared += 1
    expect(other.prompt === call.prompt, `the prompt of ${call.label} differs from the prompt of ${other.label}`)
    expect(other.model === call.model && other.effort === call.effort, `the model of ${call.label} differs from ${other.label}`)
  }
  // single 1 agent, parallel 3, four orchestrated setups 5 each, on two sets
  expect(compared === (1 + 3 + 4 * 5) * 2, `compared ${compared} R6 prompts with RGS, expected 48`)

  // ---- the two round six matrices
  const matrices = { a: read('round6-a.json'), b: read('round6-b.json') }
  const everyCell = cellKeys(matrices.a).concat(cellKeys(matrices.b))
  expect(new Set(everyCell).size === everyCell.length, 'a cell is in both matrices, or twice in one')
  const dump = {}
  const banks = []
  for (const [half, matrix] of Object.entries(matrices)) {
    expect(Array.isArray(matrix) && matrix.every(block => Array.isArray(block.arms) && block.setup && block.tier && Array.isArray(block.reps) && block.reps.length === 3),
      `round6-${half}.json is not a list of {arms, setup, tier, reps} blocks with three repeats each`)
    const { calls, result } = await run(full({ matrix }))
    const perCell = {}
    for (const call of calls) {
      const [arm, setup, tier, role, rep] = call.label.split('|')
      const key = [arm, setup, tier, rep].join('|')
      ;(perCell[key] = perCell[key] || []).push({ ...call, role })
      dump[call.label] = call.prompt
    }
    expect(Object.keys(perCell).length === result.cells.length && result.cells.length === cellKeys(matrix).length, `round6-${half}: ${Object.keys(perCell).length} cells of agents for ${result.cells.length} cells`)
    for (const [key, agents] of Object.entries(perCell)) {
      const [arm, setup, tier, rep] = key.split('|')
      const id = base(arm)
      const set = inside.setOf({ arm })
      const Qs = set === 'A' ? inside.Q : set === 'B' ? inside.Q_B : inside.Q_T
      const expected = setup === 'single' ? 1 : setup === 'parallel' ? 3 : 5
      expect(agents.length === expected, `${key}: ${agents.length} agents, expected ${expected}`)
      const variant = inside.PACK_OF[id]
      const hub = !!inside.HUB[id]
      const bank = hub ? inside.bankOf({ arm, setup, tier, rep }) : null
      if (hub) banks.push(bank)
      for (const agent of agents) {
        const text = agent.prompt
        const where = agent.label
        if (agent.role === 'plan') continue
        expect(!/undefined|NaN|\[object/.test(text.replace(/Subagent answers:[\s\S]*$/, '')), `${where}: the prompt holds undefined, NaN or [object`)
        // the packs: only the variant, set and questions this agent is meant to see
        for (const v of ['gs', 'g', 's']) {
          for (const s of ['A', 'B']) {
            for (const q of KEYS) {
              const wanted = variant === v && s === set && (agent.role === 'all' || agent.role === 'verify' || agent.role === q || agent.role === 'w-' + q)
              expect(text.includes(`DUMMYPACK ${v}/${s}/${q}\n`) === wanted, `${where}: pack ${v}/${s}/${q} ${wanted ? 'is missing from' : 'must not be in'} the prompt`)
            }
          }
        }
        if (variant) {
          const labelled = agent.role === 'all' || agent.role === 'verify'
          expect(text.includes('Context looked up for you before you started (the harness ran pgr pack on the question; '), `${where}: the pack lead sentence is missing`)
          expect(text.includes(' tokens). It may be all you need. Anything it does not show you may look up under the tool policy.\n'), `${where}: the pack trailer is missing`)
          expect(labelled === /(^|\n)Q1\. Context looked up/.test(text), `${where}: the pack blocks ${labelled ? 'are not' : 'are'} labelled Q1 to Q3`)
          expect((text.match(/Context looked up for you/g) || []).length === (labelled ? 3 : 1), `${where}: wrong number of pack blocks`)
        } else {
          expect(!/Context looked up for you/.test(text), `${where}: a prompt without a pack holds one`)
        }
        // set T
        if (set === 'T') {
          expect(text.includes(BASE.repoTeam + ' ') && !text.includes(BASE.repo + ' '), `${where}: set T does not point at the team copy of the fixture`)
          expect(KEYS.every(q => text.includes(Qs[q])), `${where}: a question of set T is missing`)
          expect(text.includes('DUMMYLOADOUT q1') === (id === 'TI') && text.includes('DUMMYLOADOUT q3') === (id === 'TI'), `${where}: the loadout in the prompt of ${id}`)
          const team = /--bank team-notes --task NAME loadout/.test(text) && /--bank team-notes recall "<words>"/.test(text)
          expect(id === 'T0' ? !/loadout|recall|--bank/.test(text.replace(/DUMMYLOADOUT/g, '')) : team, `${where}: the team commands of ${id}`)
          continue
        }
        // the hub: a bank of its own in every command, none in an arm without the hub
        const mentioned = Array.from(new Set((text.match(/--bank ([^\s]+)/g) || []).map(m => m.slice(7))))
        if (hub) {
          const reviewer = agent.role === 'verify'
          expect(mentioned.length === 1 && mentioned[0] === bank, `${where}: names the banks ${JSON.stringify(mentioned)}, expected only ${bank}`)
          expect(text.includes('The hub commands write to a note bank outside the repository; that is allowed.'), `${where}: the hub sentence is missing`)
          expect(text.includes('It is read-only: never edit, create or delete anything in it'), `${where}: the read-only sentence is gone`)
          expect(reviewer === text.includes(`pgr --bank ${bank} --task lookups loadout`), `${where}: the loadout command is ${reviewer ? 'missing' : 'present'}`)
          expect(reviewer === text.includes('Start with the loadout: it shows what the subagents filed, each note checked against the code.'), `${where}: the reviewer instruction`)
          // The finding is shown in single quotes: PowerShell rewrites a backtick inside double quotes.
          expect(!reviewer === text.includes(`pgr --bank ${bank} --task lookups --from scout handoff '<finding>'`), `${where}: the handoff command is ${reviewer ? 'present' : 'missing'}`)
          expect(!text.includes('handoff "<finding>"'), `${where}: the handoff command shows the finding in double quotes`)
          expect(!reviewer === text.includes('Before you return, file what you found for the reviewer in one call: one sentence per finding, each naming the function and its path:line.'), `${where}: the worker instruction`)
        } else {
          expect(mentioned.length === 0 && !/handoff|loadout|note bank/.test(text), `${where}: a prompt without the hub mentions it`)
        }
      }
    }
  }
  expect(banks.length === 4 * 3, `${banks.length} hub cells, expected 12 (TH, THh, TPH and TPHh with three repeats)`)
  expect(new Set(banks).size === banks.length, 'two hub cells share a bank name')
  expect(banks.every(name => /^[A-Za-z0-9._-]{1,64}$/.test(name) && name === name.toLowerCase()), 'a bank name breaks the name rule or is not lower case')
  expect(banks.every(name => inside.validName(name)), 'validName refuses a bank name that was built')

  // ---- mistakes that must stop the round before an agent starts
  const hubCell = [{ arms: ['TH'], setup: 'orch', tier: 'C', reps: ['r1'] }]
  const packCell = [{ arms: ['TP'], setup: 'single', tier: 'C', reps: ['r1'] }]
  const withoutPack = JSON.parse(JSON.stringify(packs)); delete withoutPack.gs.A.q2
  await refused(full({ matrix: packCell, packs: withoutPack }), /no pack for variant gs, set A, q2/, 'a missing pack')
  const stale = JSON.parse(JSON.stringify(packs)); stale.gs.A.q1.question = 'another text'
  await refused(full({ matrix: packCell, packs: stale }), /another question text/, 'a stale pack')
  await refused(full({ matrix: [{ arms: ['TPGh'], setup: 'single', tier: 'C', reps: ['r1'] }], packs: { gs: packs.gs, s: packs.s } }), /no pack for variant g, set B/, 'a missing variant')
  await refused(full({ matrix: hubCell, runTag: undefined }), /args\.runTag is required/, 'a missing run tag')
  await refused(full({ matrix: hubCell, runTag: 'Bad Tag' }), /args\.runTag is required/, 'a bad run tag')
  await refused(full({ matrix: hubCell, runTag: 'a'.repeat(25) }), /args\.runTag is required/, 'a run tag of 25 characters')
  await refused(full({ matrix: [{ arms: ['TH', 'TH'], setup: 'orch', tier: 'C', reps: ['r1'] }] }), /belongs to another cell/, 'two cells with one bank')
  await refused(full({ matrix: [{ arms: ['TH'], setup: 'single', tier: 'C', reps: ['r1'] }] }), /hub arms run orchestrated/, 'a hub arm in the single setup')
  await refused(full({ matrix: [{ arms: ['TH'], setup: 'orch-notools', tier: 'C', reps: ['r1'] }] }), /hub arms run orchestrated/, 'a hub arm with a reviewer that has no tools')
  await refused(full({ matrix: [{ arms: ['T0h'], setup: 'single', tier: 'C', reps: ['r1'] }] }), /set T has no held-out set/, 'T0h')
  await refused(full({ matrix: [{ arms: ['T0'], setup: 'parallel', tier: 'C', reps: ['r1'] }] }), /set T is asked of one agent only/, 'set T in the parallel setup')
  await refused(full({ matrix: [{ arms: ['T0'], setup: 'single', tier: 'C', reps: ['r1'] }], repoTeam: undefined }), /needs args\.repoTeam/, 'set T without repoTeam')
  await refused(full({ matrix: [{ arms: ['TI'], setup: 'single', tier: 'C', reps: ['r1'] }], loadouts: {} }), /no loadout for q1/, 'TI without loadouts')
  await refused(full({ matrix: [{ arms: ['TI'], setup: 'single', tier: 'C', reps: ['r1'] }], loadouts: { ...loadouts, q2: { ...loadouts.q2, task: 'other-task' } } }), /which the question does not name/, 'a loadout of another task')
  await refused(full({ matrix: [{ arms: ['TH'], setup: 'orch', tier: 'C', reps: ['a repeat name that is far too long to ever fit into a sixty four character bank name'] }] }), /not valid/, 'a repeat name that breaks the bank name rule')
  await refused(full({ matrix: hubCell, teamBank: 'bad/name' }), /not a valid bank name/, 'a bad team bank')
  // ---- the args travel through the orchestrating model: a text that was cut, changed or moved is found before any agent starts
  const checksum = /does not match its checksum|has no checksum/
  const cell = (arm, setup = 'single') => [{ arms: [arm], setup, tier: 'C', reps: ['r1'] }]
  const tampered = async (what, matrix, change, pattern = checksum) => {
    const given = { packs: clone(packs), loadouts: clone(loadouts), sums: clone(sums) }
    change(given)
    await refused(full({ matrix, ...given }), pattern, what)
  }
  // A careful tamperer keeps the token count consistent, so that only the checksum can find the change.
  const retoken = pack => { pack.tokens = tokensOf(pack.output) }
  await tampered('a caller line dropped from a pack', cell('TP'), g => { g.packs.gs.A.q1.output = g.packs.gs.A.q1.output.replace('\n<-- caller_two a/d.py:5-6', ''); retoken(g.packs.gs.A.q1) })
  await tampered('a pack cut to its first 20 characters', cell('TP'), g => { g.packs.gs.A.q2.output = g.packs.gs.A.q2.output.slice(0, 20); retoken(g.packs.gs.A.q2) })
  await tampered('two packs swapped', cell('TPGh'), g => { const one = g.packs.g.B.q3; g.packs.g.B.q3 = g.packs.s.B.q3; g.packs.s.B.q3 = one })
  await tampered('a pack of set B replaced by the text of set A', cell('TPh'), g => { g.packs.gs.B.q1 = { ...clone(g.packs.gs.A.q1), question: inside.Q_B.q1 } })
  await tampered('a ticket changed in a loadout', cell('TI'), g => { g.loadouts.q1.output = g.loadouts.q1.output.replace('BLD-9999', 'BLD-9990'); retoken(g.loadouts.q1) })
  await tampered('a stale mark dropped from a loadout', cell('TI'), g => { g.loadouts.q2.output = g.loadouts.q2.output.replace(', stale: code changed', ''); retoken(g.loadouts.q2) })
  await tampered('a checksum left out', cell('TP'), g => { delete g.sums['packs/gs/A/q2'] })
  await tampered('no checksums at all', cell('TP'), g => { g.sums = undefined })
  await tampered('the checksum of another text', cell('TP'), g => { g.sums['packs/gs/A/q1'] = g.sums['packs/gs/A/q2'] })
  await tampered('a checksum that is not a SHA-256', cell('TP'), g => { g.sums['packs/gs/A/q3'] = 'x'.repeat(64) })
  await tampered('a loadout checksum left out', cell('TI'), g => { delete g.sums['loadouts/q3'] })
  await tampered('a token count that does not fit the text', cell('TP'), g => { g.packs.gs.A.q1.tokens += 1 }, /tokens/)
  // Only the texts a cell uses are held to their checksum: a half that does not run TI does not need the loadouts.
  const noLoadouts = await run(full({ matrix: cell('TP'), loadouts: undefined, sums: Object.fromEntries(Object.entries(sums).filter(([key]) => key.startsWith('packs/gs/A/'))) }))
  expect(noLoadouts.calls.length === 1, 'a pack cell with only the checksums of its own packs does not start')
  // The SHA-256 written in the script gives the digest of node's own, whatever the text.
  const texts = ['', 'abc', 'a'.repeat(55), 'a'.repeat(56), 'a'.repeat(63), 'a'.repeat(64), 'a'.repeat(65), 'b'.repeat(119), 'b'.repeat(120), 'é€😀 mixed \n\r\n text', Array.from({ length: 3000 }, (_, i) => `line ${i} é`).join('\n')]
  const differing = texts.filter(text => inside.sha256Text(text) !== sha(text))
  expect(differing.length === 0, `the script's SHA-256 differs from node's for ${differing.length} of ${texts.length} texts (first: ${JSON.stringify((differing[0] || '').slice(0, 20))})`)

  // two runs with different tags never share a bank
  const one = (await run(full({ matrix: hubCell, runTag: 'aa' }))).calls.map(c => c.prompt).join('\n')
  const two = (await run(full({ matrix: hubCell, runTag: 'bb' }))).calls.map(c => c.prompt).join('\n')
  expect(one.includes('r6-aa-th-orch-c-r1') && two.includes('r6-bb-th-orch-c-r1') && !one.includes('r6-bb-') && !two.includes('r6-aa-'), 'the bank name does not follow the run tag')
  expect(inside.policy('TH', { bank: 'BANK', task: 'lookups', role: 'worker' }).includes('pgr --bank BANK --task lookups --from scout handoff'), 'policy() does not use the hub it is given')

  if (flag('--dump')) fs.writeFileSync(flag('--dump'), JSON.stringify(dump, null, 1))
  if (flag('--inside')) fs.writeFileSync(flag('--inside'), JSON.stringify({ Q: inside.Q, Q_B: inside.Q_B, Q_T: inside.Q_T, PACK_OF: inside.PACK_OF, HUB: inside.HUB, SET_T: inside.SET_T }, null, 1))
  if (flag('--args')) {
    // The args file that round6_args.py wrote: the workflow must accept it and start every cell.
    const given = JSON.parse(fs.readFileSync(flag('--args'), 'utf8'))
    const ran = await run(given)
    expect(ran.result.cells.length > 0 && ran.calls.length > 0, 'the args file starts no cell')
    console.log(`args file: ${ran.result.cells.length} cells, ${ran.calls.length} agents`)
  }
  report()
})().catch(error => { failures.push(`the dry run failed: ${error.stack || error}`); report() })

function report() {
  if (failures.length) {
    console.log(`FAILED ${failures.length} of ${failures.length + passed} checks`)
    for (const line of failures) console.log('  ' + line)
    process.exit(1)
  }
  console.log(`ok: ${passed} checks passed`)
}
