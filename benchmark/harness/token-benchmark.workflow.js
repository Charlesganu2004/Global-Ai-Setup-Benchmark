export const meta = {
  name: 'index-token-benchmark',
  description: 'Ask the same lookup questions through each local index method, model tier and agent setup, so token use can be compared',
  phases: [
    { title: 'Single', detail: 'one agent answers all three questions' },
    { title: 'Parallel', detail: 'three agents, one question each' },
    { title: 'Orchestrated', detail: 'apex plan, three workers, apex verify' },
  ],
}

// The fixture: a git repository with every index built (see benchmark/README.md).
// Pass its path as args.repo, and as args.repoPlain the same repository indexed
// without Rust embeddings. The published runs used ...\\lxbench\\repo and
// ...\\lxbench\\repo-noemb under the user's Downloads folder.
const REPO = (args && args.repo) || 'C:\\path\\to\\lxbench\\repo'
const TIER = {
  C: { model: 'haiku', effort: 'low' },
  B: { model: 'sonnet', effort: 'medium' },
  A: { model: 'opus', effort: 'max' },
}
const APEX = TIER.A
const REP = (args && args.rep) || 'r1'
// Cells are listed one by one in args.cells, or as blocks in args.matrix:
// {arms: [...], setup, tier, reps: [...]} is every arm at every repeat.
const CELLS = ((args && args.cells) || []).concat(((args && args.matrix) || []).flatMap(block =>
  block.reps.flatMap(rep => block.arms.map(arm => ({ arm, setup: block.setup, tier: block.tier, rep })))))

const LX = [
  'lx def <name>                where a function or class is defined, printed as: name path:first-last',
  'lx callers <name>            every function that calls it, one per line: name path:first-last',
]
// lx as improved during this benchmark: the definition line carries the
// signature and a search hit carries the names of the functions it lies in.
const LX2 = [
  'lx def <name>                where a function or class is defined, printed as: name path:first-last signature',
  'lx callers <name>            every function that calls it, one per line: name path:first-last',
]
const LX2_FIND = 'lx find "<description>"      search by meaning, prints: path:first-last, then the functions those lines belong to'
const TOKEN_GOAT = [
  'token-goat symbol <name>               where a symbol is defined',
  'token-goat callers <name>              every function that calls it',
  'token-goat semantic "<description>"    search by meaning',
  'token-goat read "<file>::<symbol>"     the body of one symbol',
]
// lx as shipped after the first round: several names per call, a count and
// header on callers, and the line each function starts on beside a search hit.
const LX3_DEF = 'lx def <name>...             where each is defined, printed as: name path:first-last signature'
const LX3_CALLERS = 'lx callers <name>...         prints "N callers of <name>:" and then one caller per line: name path:first-last'
const LX3_FIND = 'lx find "<description>"      search by meaning, prints: path:first-last of the matching lines, then name:line for each function those lines sit in, where line is the line that function starts on'
// What the second round adds.
const LX_RUST = 'lx find --rust "<description>"      search by meaning in the Rust index, prints: name path:first-last'
const LX_ABOUT = 'lx about <name>...           one answer per name: its definition as name path:first-last signature, its doc line, then "N callers:" and one caller per line'
const LX_CARDS = 'lx find --cards "<description>"      search by meaning, prints each hit as the definition it sits in: name path:first-last signature, then that definition\'s doc line'
const LX_RUST_CARDS = 'lx find --rust --cards "<description>"      search by meaning in the Rust index, prints: name path:first-last signature, then the doc line'
const LX_HYBRID = 'lx find --hybrid "<description>"      search by meaning with two engines merged, prints: name path:first-last signature, then the doc line'
const LX_ASK = 'lx ask <kind>:<what>...      several questions in one call, each answer under its own "== kind:what" header. Kinds: def, callers, about (definition, signature, doc line and callers together) and find (search by meaning; quote the whole argument). Example: lx ask about:parse_config callers:load "find:where tokens expire"'
const GRAPHIFY = 'graphify explain "<symbol>"            definition line, then every caller (<--) and callee (-->)'
const SEMBLE = 'semble search "<description>" . -k 5 --max-snippet-lines 0 --format text      search by meaning, prints path:first-last'
const RUST_RAW = [
  'codanna retrieve symbol <name>         definition, signature and callers',
  'codanna retrieve callers <name>        every function that calls it',
  'codanna mcp semantic_search_docs "query:<description>" limit:5      search by meaning',
]
const TG_READ = 'token-goat read "<file>::<symbol>"     the body of one symbol'
const ARMS = {
  A0: null,
  A1: [
    'graphify explain "<symbol>"            definition line, then every caller (<--) and callee (-->)',
    'semble search "<description>" . -k 5 --max-snippet-lines 0 --format text      search by meaning, prints path:first-last',
  ],
  A2: [
    'codanna retrieve symbol <name>         definition, signature and callers',
    'codanna retrieve callers <name>        every function that calls it',
    'codanna mcp semantic_search_docs "query:<description>" limit:5      search by meaning',
  ],
  A3: LX.concat(['lx find "<description>"      search by meaning, prints path:first-last']),
  A4: LX.concat(['lx find --rust "<description>"      search by meaning, prints: name path:first-last']),
  A5: TOKEN_GOAT,
  A6: TOKEN_GOAT.concat(LX2, [LX2_FIND]),
  A7: LX2.concat([LX2_FIND]),
  A8: LX2.concat(['lx find --rust "<description>"      search by meaning, prints: name path:first-last']),
  // lx as shipped: several names per call, a count and header on callers, and
  // the line each function starts on beside a search hit.
  A9: [LX3_DEF, LX3_CALLERS, LX3_FIND],

  // Second round. R1 is the shipped lx searching the Rust index's embeddings.
  R1: [LX3_DEF, LX3_CALLERS, LX_RUST],
  // Four improved versions of lx.
  L1: [LX3_DEF, LX3_CALLERS, LX_ABOUT, LX3_FIND],
  L2: [LX3_DEF, LX3_CALLERS, LX_CARDS],
  L3: [LX3_DEF, LX3_CALLERS, LX_HYBRID],
  L4: [LX_ASK],
  // Fifteen combinations.
  C1: RUST_RAW.concat(TOKEN_GOAT, [GRAPHIFY, SEMBLE]),
  C2: [LX3_DEF, LX3_CALLERS, LX_RUST].concat(TOKEN_GOAT),
  C3: [LX3_DEF, LX3_CALLERS, LX3_FIND].concat(TOKEN_GOAT),
  C4: RUST_RAW.concat(TOKEN_GOAT),
  C5: RUST_RAW.concat([GRAPHIFY, SEMBLE]),
  C6: TOKEN_GOAT.concat([GRAPHIFY, SEMBLE]),
  C7: TOKEN_GOAT.concat([SEMBLE]),
  C8: TOKEN_GOAT.concat([GRAPHIFY]),
  C9: [LX3_DEF, LX3_CALLERS, LX3_FIND, TG_READ],
  C10: [LX3_DEF, LX3_CALLERS, LX_ABOUT, LX_CARDS],
  C11: [LX3_DEF, LX3_CALLERS, LX_ABOUT, LX_RUST_CARDS],
  C12: [LX3_DEF, LX3_CALLERS, LX_ABOUT, LX_HYBRID],
  C13: [LX3_DEF, LX3_CALLERS, LX_ABOUT, LX_CARDS, LX_HYBRID, LX_ASK],
  C14: [LX3_DEF, LX3_CALLERS, LX_ABOUT, LX_CARDS, LX_HYBRID, LX_ASK].concat(TOKEN_GOAT),
  C15: [LX_ASK, TG_READ],
}
// Arms that read code through token-goat instead of the Read tool.
const READ_VIA_TOKEN_GOAT = { C9: true, C15: true }
// The same repository indexed without Rust embeddings, the default: lx ask and
// lx find --hybrid then search with semble alone.
const REPO_PLAIN = (args && args.repoPlain) || 'C:\\path\\to\\lxbench\\repo-noemb'
const LX_HYBRID_PLAIN = 'lx find --hybrid "<description>"      search by meaning, prints: name path:first-last signature, then the doc line'
ARMS.N1 = [LX_ASK]
ARMS.N2 = [LX3_DEF, LX3_CALLERS, LX_ABOUT, LX_HYBRID_PLAIN, LX_ASK]
// Third round: the shipped lx, whose merged search also asks the Rust index's
// name and doc search, which needs no embeddings. S1 and S2 run on the default
// index (no embeddings), S3 and S4 on the index with embeddings.
ARMS.S1 = [LX_ASK]
ARMS.S2 = [LX3_DEF, LX3_CALLERS, LX_ABOUT, LX_HYBRID_PLAIN, LX_ASK]
ARMS.S3 = [LX_ASK]
ARMS.S4 = [LX3_DEF, LX3_CALLERS, LX_ABOUT, LX_HYBRID_PLAIN, LX_ASK]
const REPO_OF = { N1: REPO_PLAIN, N2: REPO_PLAIN, S1: REPO_PLAIN, S2: REPO_PLAIN }

function policy(arm) {
  const repo = REPO_OF[arm] || REPO
  const lines = [
    'This is one run of a benchmark that measures how many tokens a code lookup costs.',
    `Repository under test: ${repo} (Python, 86 files). It is read-only: never edit, create or delete anything in it, and never build or refresh an index.`,
    '',
    'TOOL POLICY FOR THIS RUN. It replaces the local-index and read-gate rules (section 6) and any token-goat gate in your global instructions. Use only what is listed here. No Agent tool, no web tools, no other search or index command.',
  ]
  if (!ARMS[arm]) {
    lines.push('Allowed: the Grep, Glob and Read tools only, with absolute paths inside the repository. No shell commands at all.')
  } else {
    lines.push(`Allowed shell commands. Run each as one PowerShell call that starts with:  Set-Location ${repo};`)
    for (const command of ARMS[arm]) lines.push('  ' + command)
    lines.push(READ_VIA_TOKEN_GOAT[arm]
      ? 'To see code, use token-goat read on a symbol a command returned, and only when the answer needs it. Do not use the Read tool. Use Grep only when the listed commands return nothing useful.'
      : 'After a command answers, use the Read tool only on a line range it returned, and only when the answer needs it. Use Grep only when the listed commands return nothing useful.')
  }
  lines.push('Be exact and do not guess. Stop as soon as the work is answered, then return the answer through the structured output.')
  return lines.join('\n')
}

const Q = {
  q1: 'Which functions call `_get_backend_api_key`? Give the bare name of every calling function in the repository.',
  q2: 'Where is the function `dispatch_command` defined (file, and the line its `def` is on), what are its parameter names, and which functions call it?',
  q3: 'Which function registers the git merge driver for graph.json? Give its name, its file, the line its `def` is on, and the name of the function that removes that registration again.',
}
const NAMES = { type: 'array', items: { type: 'string' }, description: 'bare function names' }
const S = {
  q1: { type: 'object', properties: { callers: NAMES }, required: ['callers'] },
  q2: {
    type: 'object',
    properties: {
      file: { type: 'string', description: 'path relative to the repository' },
      line: { type: 'integer', description: 'line of the def' },
      parameters: { type: 'array', items: { type: 'string' }, description: 'parameter names only' },
      called_by: NAMES,
    },
    required: ['file', 'line', 'parameters', 'called_by'],
  },
  q3: {
    type: 'object',
    properties: {
      function: { type: 'string', description: 'bare function name' },
      file: { type: 'string', description: 'path relative to the repository' },
      line: { type: 'integer', description: 'line of the def' },
      removed_by: { type: 'string', description: 'bare name of the function that undoes it' },
    },
    required: ['function', 'file', 'line', 'removed_by'],
  },
}
const ALL = { type: 'object', properties: { q1: S.q1, q2: S.q2, q3: S.q3 }, required: ['q1', 'q2', 'q3'] }
const BRIEFS = {
  type: 'object',
  properties: { q1: { type: 'string' }, q2: { type: 'string' }, q3: { type: 'string' } },
  required: ['q1', 'q2', 'q3'],
}
const KEYS = ['q1', 'q2', 'q3']
const listed = KEYS.map(k => `${k.toUpperCase()}. ${Q[k]}`).join('\n')
const tag = (c, role) => `${c.arm}|${c.setup}|${c.tier}|${role}|${c.rep || REP}`
// The planning prompt is the same for every method and tier, so it is measured
// a few times (cells marked ownPlan) and its briefs are reused everywhere else.
const SHARED_BRIEFS = args && args.briefs

async function single(c) {
  const answer = await agent(
    `${policy(c.arm)}\n\nAnswer all three questions.\n${listed}`,
    { label: tag(c, 'all'), phase: 'Single', schema: ALL, ...TIER[c.tier] })
  return { ...c, answer }
}

async function fanOut(c) {
  const got = await parallel(KEYS.map(k => () => agent(
    `${policy(c.arm)}\n\nAnswer this question.\n${Q[k]}`,
    { label: tag(c, k), phase: 'Parallel', schema: S[k], ...TIER[c.tier] })))
  return { ...c, answer: { q1: got[0], q2: got[1], q3: got[2] } }
}

async function orchestrated(c) {
  const plan = (SHARED_BRIEFS && !c.ownPlan) ? SHARED_BRIEFS : await agent(
    `You are the orchestrator of one benchmark run. Three independent lookup questions about the repository at ${REPO} are listed below. Do not answer them, and call no tool except the structured output. Write one brief per question for a subagent that starts with no other context: the question, what a complete answer contains, and when to stop. Keep each brief under 80 words. The harness appends the tool policy to every brief, so leave tools out.\n\n${listed}`,
    { label: tag(c, 'plan'), phase: 'Orchestrated', schema: BRIEFS, ...APEX })
  if (!plan) return { ...c, answer: null, failed: 'plan' }
  const workers = await parallel(KEYS.map(k => () => agent(
    // The reused briefs name the main fixture; an arm on the other copy is pointed at its own.
    `${policy(c.arm)}\n\nBrief from the orchestrator:\n${plan[k].split(REPO).join(REPO_OF[c.arm] || REPO)}`,
    { label: tag(c, 'w-' + k), phase: 'Orchestrated', schema: S[k], ...TIER[c.tier] })))
  const reported = { q1: workers[0], q2: workers[1], q3: workers[2] }
  const answer = await agent(
    `${policy(c.arm)}\n\nYou are the orchestrator and lead reviewer of this run. Three subagents each answered one question; their answers are below as JSON, with null for a subagent that returned nothing. Check each answer under the tool policy with as few calls as it takes, correct anything wrong or missing, and return the final answers for all three questions.\n\n${listed}\n\nSubagent answers:\n${JSON.stringify(reported)}`,
    { label: tag(c, 'verify'), phase: 'Orchestrated', schema: ALL, ...APEX })
  return { ...c, workers: reported, answer }
}

const RUN = { single, parallel: fanOut, orch: orchestrated }
log(`${CELLS.length} cells, repeat ${REP}`)
const done = await parallel(CELLS.map(c => () => RUN[c.setup](c)))
return { rep: REP, cells: done }
