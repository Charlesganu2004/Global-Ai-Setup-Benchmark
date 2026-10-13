export const meta = {
  name: 'index-token-benchmark',
  description: 'Ask the same lookup questions through each local index method, model tier and agent setup, so token use can be compared',
  phases: [
    { title: 'Single', detail: 'one agent answers all three questions' },
    { title: 'Parallel', detail: 'three agents, one question each' },
    { title: 'Orchestrated', detail: 'apex plan, three workers, a reviewer chosen by the setup' },
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
// Fourth round: the same prompts as A0, A9, S1, S3 and A5, run while
// token-goat's hooks were installed in the client. Nothing in the prompt
// differs: the hooks act on the client's own Read, Grep, Glob and Bash calls.
ARMS.H0 = null
ARMS.H9 = [LX3_DEF, LX3_CALLERS, LX3_FIND]
ARMS.H1 = [LX_ASK]
ARMS.H3 = [LX_ASK]
ARMS.H5 = TOKEN_GOAT
// The same one-call command sent through the Bash tool instead of PowerShell,
// because the hooks match Bash and never see a PowerShell call: SB with the
// hooks off, HB with them on.
ARMS.SB = [LX_ASK]
ARMS.HB = [LX_ASK]
const VIA_BASH = { SB: true, HB: true }
const posix = path => path.replace(/^([A-Za-z]):/, (all, drive) => '/' + drive.toLowerCase()).replace(/\\/g, '/')
const REPO_OF = { N1: REPO_PLAIN, N2: REPO_PLAIN, S1: REPO_PLAIN, S2: REPO_PLAIN, H1: REPO_PLAIN, SB: REPO_PLAIN, HB: REPO_PLAIN }
// Fifth round: the engines rewritten in Rust, one binary (pgr), in every
// combination of its three parts: G the code graph (definitions, callers, what a
// change reaches), S the hybrid search, M the memory (notes kept from earlier
// sessions). An arm with M runs on a copy of the fixture whose memory was filled
// beforehand: args.repoMem by a session that answered the same three questions,
// args.repoMemOrient by a session that explored the repository without seeing them.
const PGR_G = [
  'pgr about <name>...          one answer per name: its definition as name path:first-last signature, then "doc: <line>" when it has a doc line, then "N callers:" and one caller per line as name path:first-last @line (the line the call is on), then "calls:" and the names it calls. A name that is not defined prints "<name>: not found"',
  'pgr affected <name> [--depth N]   everything that reaches <name> through calls, nearest first. Prints "N affected by <name> (depth D):" and one line per hit: dK name path:first-last via calls @line, where K is the number of calls between them. The default depth is 2',
]
const PGR_S = 'pgr find "<description>"     search by meaning and by words together, prints each hit as the definition it sits in: name path:first-last signature, then its doc line when it has one'
// How a kept note is printed, by pgr recall and by the closing section of pgr ask.
const PGR_NOTE = 'one per line as: - text  (age ago) #id. A note ends its brackets with "stale: code changed" when code it names has changed since, and "gone: code removed" when that code no longer exists'
const PGR_M = 'pgr recall "<question>"      notes kept from earlier work in this repository, most relevant first, ' + PGR_NOTE + '. Prints "no notes" when none match'
const PGR_ASK = 'pgr ask <kind>:<what>...     several questions in one call, each answer under its own "== kind:what" header. Kinds: about (definition, signature, doc line, callers and what it calls together), callers, def, affected, and find (search by meaning; quote the whole argument). Example: pgr ask about:parse_config callers:load "find:where tokens expire"'
const PGR_ASK_M = PGR_ASK + ' When notes kept from earlier work bear on the questions, up to four follow under a closing "== memory" line, ' + PGR_NOTE + '.'
ARMS.RG = PGR_G
ARMS.RS = [PGR_S]
ARMS.RM = [PGR_M]
ARMS.RGS = [PGR_ASK]
ARMS.RGM = PGR_G.concat([PGR_M])
ARMS.RSM = [PGR_S, PGR_M]
ARMS.RGSM = [PGR_ASK_M]
ARMS.RGSO = [PGR_ASK_M]
const REPO_MEM = (args && args.repoMem) || 'C:\\path\\to\\lxbench\\repo-mem'
const REPO_MEM_ORIENT = (args && args.repoMemOrient) || 'C:\\path\\to\\lxbench\\repo-mem-orient'
for (const arm of ['RM', 'RGM', 'RSM', 'RGSM']) REPO_OF[arm] = REPO_MEM
REPO_OF.RGSO = REPO_MEM_ORIENT

// Sixth round: team work (see benchmark/harness/ROUND6.md). R6 is RGS run again with the same
// commands and the same policy text, so that every comparison inside the round is same-day. The
// arms with P in the name also get the output of `pgr pack` for each question in the prompt:
// it costs no model call, so the packs are computed before the run (prepare_packs.py) and passed
// in args.packs[variant][set][q], variant gs (graph and search), g or s. TH and TPH add the hub:
// the subagents file what they found with `pgr handoff` and the reviewer starts from `pgr loadout`.
// T0, TL and TI ask set T, a demonstration set that the builders wrote to show one feature: its
// answers need a note that an earlier agent left behind (prepare_team.py files the notes).
const REPO_TEAM = (args && args.repoTeam) || 'C:\\path\\to\\lxbench\\repo-team'
const NAME_RULE = /^[A-Za-z0-9._-]{1,64}$/
const validName = name => NAME_RULE.test(name) && name !== '.' && name !== '..'
const TEAM_BANK = (args && args.teamBank) || 'team-notes'
if (!validName(TEAM_BANK)) throw new Error(`args.teamBank ${JSON.stringify(TEAM_BANK)} is not a valid bank name (1 to 64 characters of A-Z a-z 0-9 . _ -)`)
const HUB_TASK = 'lookups'
// How the hub prints a note: the recall line with the roles that filed it before the id.
const PGR_HUB_NOTE = 'one per line as: - text  (age ago) [roles] #id, where roles are the roles that filed it joined by +. A note ends its brackets with "stale: code changed" when code it names has changed since, and "gone: code removed" when that code no longer exists'
// The finding is shown in single quotes: PowerShell rewrites a backtick inside double quotes, which would cut the first letter off a name.
const hubHandoff = (bank, task) => `pgr --bank ${bank} --task ${task} --from scout handoff '<finding>'      files what you found as notes in the shared note bank, for the reviewer. Prints "kept <id>" for each note filed and a last line "handoff ${task}: N notes from scout, K name code"`
const hubLoadout = (bank, task) => `pgr --bank ${bank} --task ${task} loadout      what the subagents filed for this task, ${PGR_HUB_NOTE}. A last line counts the notes. Prints "no notes" when nothing was filed`
const HUB_WORKER_RULE = 'Before you return, file what you found for the reviewer in one call: one sentence per finding, each naming the function and its path:line.'
const HUB_REVIEWER_RULE = 'Start with the loadout: it shows what the subagents filed, each note checked against the code.'
const PGR_TEAM_LOADOUT = `pgr --bank ${TEAM_BANK} --task NAME loadout      the notes the team filed for the task NAME, ${PGR_HUB_NOTE}. A last line counts the notes. Prints "no notes" when there are none`
const PGR_TEAM_RECALL = `pgr --bank ${TEAM_BANK} recall "<words>"      notes of the team bank that match some words, most relevant first, ${PGR_NOTE}. Prints "no notes" when none match`
ARMS.R6 = ARMS.RGS
ARMS.TP = [PGR_ASK]
ARMS.TPG = PGR_G
ARMS.TPS = [PGR_S]
ARMS.TH = [PGR_ASK]
ARMS.TPH = [PGR_ASK]
ARMS.T0 = [PGR_ASK]
ARMS.TL = [PGR_ASK, PGR_TEAM_LOADOUT, PGR_TEAM_RECALL]
ARMS.TI = [PGR_ASK, PGR_TEAM_LOADOUT, PGR_TEAM_RECALL]
for (const arm of ['T0', 'TL', 'TI']) REPO_OF[arm] = REPO_TEAM
// What an arm adds to R6: the variant of pack in its prompts, the hub, the set it asks.
const PACK_OF = { TP: 'gs', TPG: 'g', TPS: 's', TPH: 'gs' }
const HUB = { TH: true, TPH: true }
const SET_T = { T0: true, TL: true, TI: true }

// Held-out questions. The builders of pgr saw questions q1 to q3 (set A), so a
// second set B exists that nobody tuned for. An arm id plus "h" runs that arm's
// method on set B: "RGSh" is "RGS" on set B, "A0h" is "A0" on set B. Policy text,
// tiers, setups and labels are the same; the label keeps the id with its "h".
const hasArm = id => Object.prototype.hasOwnProperty.call(ARMS, id)
// An arm id plus "r" is that arm run again on the day of a later round, on the
// same set A: "A0r" is "A0" again. It gets its own id so that its runs are not
// mixed with the earlier ones.
const baseArm = id => {
  if (!/[hr]$/.test(id) || hasArm(id)) return id
  if (!hasArm(id.slice(0, -1))) throw new Error(`arm ${id}: there is no arm ${id.slice(0, -1)} to run again or to run on the held-out questions`)
  return id.slice(0, -1)
}
// Set T has no held-out version: its arms ask their own questions, and an id ending in h would not mean set B.
const setOf = c => {
  if (SET_T[baseArm(c.arm)]) {
    if (/h$/.test(c.arm) && !hasArm(c.arm)) throw new Error(`arm ${c.arm}: set T has no held-out set`)
    return 'T'
  }
  return /h$/.test(c.arm) && !hasArm(c.arm) ? 'B' : 'A'
}

// Ways of running the reviewer at the end of an orchestrated run. The setup
// name picks one: who reviews, and whether the reviewer may look anything up.
const REVIEW = {
  orch: { who: APEX, tools: true },          // top tier, may check with the method's own tools
  'orch-notools': { who: APEX, tools: false },   // top tier, reads the answers only
  'orch-mid': { who: TIER.B, tools: true },      // middle tier reviews
  'orch-low': { who: TIER.C, tools: true },      // cheapest tier reviews
}

// hub is null for every arm without the hub; for TH and TPH it is {bank, task, role}, role worker or reviewer.
function policy(armId, hub) {
  const arm = baseArm(armId)
  const repo = REPO_OF[arm] || REPO
  const lines = [
    'This is one run of a benchmark that measures how many tokens a code lookup costs.',
    `Repository under test: ${repo} (Python, 86 files). It is read-only: never edit, create or delete anything in it, and never build or refresh an index.${hub ? ' The hub commands write to a note bank outside the repository; that is allowed.' : ''}`,
    '',
    'TOOL POLICY FOR THIS RUN. It replaces the local-index and read-gate rules (section 6) and any token-goat gate in your global instructions. Use only what is listed here. No Agent tool, no web tools, no other search or index command.',
  ]
  if (!ARMS[arm]) {
    lines.push('Allowed: the Grep, Glob and Read tools only, with absolute paths inside the repository. No shell commands at all.')
  } else {
    lines.push(VIA_BASH[arm]
      ? `Allowed shell commands. Run each as one call of the Bash tool (not PowerShell) that starts with:  cd ${posix(repo)} &&`
      : `Allowed shell commands. Run each as one PowerShell call that starts with:  Set-Location ${repo};`)
    for (const command of ARMS[arm]) lines.push('  ' + command)
    if (hub) lines.push('  ' + (hub.role === 'reviewer' ? hubLoadout(hub.bank, hub.task) : hubHandoff(hub.bank, hub.task)))
    lines.push(READ_VIA_TOKEN_GOAT[arm]
      ? 'To see code, use token-goat read on a symbol a command returned, and only when the answer needs it. Do not use the Read tool. Use Grep only when the listed commands return nothing useful.'
      : 'After a command answers, use the Read tool only on a line range it returned, and only when the answer needs it. Use Grep only when the listed commands return nothing useful.')
    if (hub) lines.push(hub.role === 'reviewer' ? HUB_REVIEWER_RULE : HUB_WORKER_RULE)
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
// Set B, the held-out questions: the same three shapes on other functions of the
// same repository. They are stored under the keys q1 to q3 like set A's, and the
// answer to the third names its fourth field read_by instead of removed_by.
const Q_B = {
  q1: 'Which functions call `write_json_atomic`? Give the bare name of every calling function in the repository.',
  q2: 'Where is the function `validate_url` defined (file, and the line its `def` is on), what are its parameter names, and which functions call it?',
  q3: 'Which function decides whether a path should be ignored under the .graphifyignore patterns? Give its name, its file, the line its `def` is on, and the name of the function that reads the .graphifyignore files.',
}
const S_B = {
  q1: S.q1,
  q2: S.q2,
  q3: {
    type: 'object',
    properties: {
      function: { type: 'string', description: 'bare function name' },
      file: { type: 'string', description: 'path relative to the repository' },
      line: { type: 'integer', description: 'line of the def' },
      read_by: { type: 'string', description: 'bare name of the function that reads the .graphifyignore files' },
    },
    required: ['function', 'file', 'line', 'read_by'],
  },
}
const ALL_B = { type: 'object', properties: { q1: S_B.q1, q2: S_B.q2, q3: S_B.q3 }, required: ['q1', 'q2', 'q3'] }
// Set T, a demonstration set that the builders wrote to show one feature of the hub: each question
// needs one lookup and one note that an earlier agent filed under the named task (prepare_team.py
// files them). A run that cannot see the notes may answer unknown; it is graded wrong on that field.
// The first question's lookup is the function of set A's second question, the second question's is
// set B's second question, the third question's is set A's third question, so the lookup fields are
// graded by the rules of those sets.
const Q_T = {
  q1: 'Task `cli-freeze`: the team recorded a decision about `dispatch_command`. Which ticket does the decision cite, may the parameters of the function be renamed (true or false), and in which file is it defined and on which line is its `def`? Where you cannot find something out, say so: the word unknown for the ticket, and leave the true or false out.',
  q2: 'Task `ssrf-review`: one note of this task describes what `validate_url` does. Is that note still current, or has the code of the function changed since the note was filed (current, changed, or unknown if you cannot tell)? Which role filed it? And which functions call `validate_url`? Give the bare name of every caller.',
  q3: 'Task `merge-driver-audit`: two roles filed the same fact about the function that registers the git merge driver for graph.json. Give the name of that function, the two roles that filed the fact (sorted; an empty list if you cannot tell), its file and the line its `def` is on.',
}
const S_T = {
  q1: {
    type: 'object',
    properties: {
      ticket: { type: 'string', description: 'the ticket id the decision cites, exactly as written; the word unknown if you cannot tell' },
      // Left out, not null, when the agent cannot tell: a plain boolean is the one schema shape every runtime accepts.
      may_rename: { type: 'boolean', description: 'true if the parameters may be renamed, false if they may not; leave this field out if you cannot tell' },
      file: { type: 'string', description: 'path relative to the repository' },
      line: { type: 'integer', description: 'line of the def' },
    },
    required: ['ticket', 'file', 'line'],
  },
  q2: {
    type: 'object',
    properties: {
      status: { type: 'string', enum: ['current', 'changed', 'unknown'], description: 'changed if the code of the function changed since the note was filed, current if it did not, unknown if you cannot tell' },
      filed_by: { type: 'string', description: 'the role that filed the note (scout, builder or reviewer); the word unknown if you cannot tell' },
      called_by: NAMES,
    },
    required: ['status', 'filed_by', 'called_by'],
  },
  q3: {
    type: 'object',
    properties: {
      function: { type: 'string', description: 'bare function name' },
      roles: { type: 'array', items: { type: 'string' }, description: 'the roles that filed the fact (scout, builder or reviewer), sorted; empty if you cannot tell' },
      file: { type: 'string', description: 'path relative to the repository' },
      line: { type: 'integer', description: 'line of the def' },
    },
    required: ['function', 'roles', 'file', 'line'],
  },
}
const ALL_T = { type: 'object', properties: { q1: S_T.q1, q2: S_T.q2, q3: S_T.q3 }, required: ['q1', 'q2', 'q3'] }
const BRIEFS = {
  type: 'object',
  properties: { q1: { type: 'string' }, q2: { type: 'string' }, q3: { type: 'string' } },
  required: ['q1', 'q2', 'q3'],
}
const KEYS = ['q1', 'q2', 'q3']
const listedOf = questions => KEYS.map(k => `${k.toUpperCase()}. ${questions[k]}`).join('\n')
// What a cell asks, by question set: the questions, one answer schema per
// question, the schema of all three, and the numbered list for a prompt.
const SETS = {
  A: { Q, S, ALL, listed: listedOf(Q) },
  B: { Q: Q_B, S: S_B, ALL: ALL_B, listed: listedOf(Q_B) },
  T: { Q: Q_T, S: S_T, ALL: ALL_T, listed: listedOf(Q_T) },
}
const tag = (c, role) => `${c.arm}|${c.setup}|${c.tier}|${role}|${c.rep || REP}`
// The planning prompt is the same for every method and tier, so it is measured
// a few times (cells marked ownPlan) and its briefs are reused everywhere else.
const SHARED_BRIEFS = args && args.briefs

// Round six: what a prompt holds besides the policy and the questions.
const PACKS = (args && args.packs) || {}
const LOADOUTS = (args && args.loadouts) || {}
const RUN_TAG = args && args.runTag
const RUN_TAG_RULE = /^[a-z0-9][a-z0-9._-]{0,23}$/
// The packs and loadouts reach this script through args, which the orchestrating model has to type out as a
// tool argument, so a text could be cut or changed on the way. args.sums holds the SHA-256 of each text as
// prepare_packs.py and prepare_team.py recorded it, keyed by place (packs/VARIANT/SET/Q and loadouts/Q), and
// check() computes it again. A script cannot import a hash, so this one is written out;
// check_round6_prompts.js compares it with node's on a set of texts.
const SUMS = (args && args.sums) || {}
const K256 = [
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
]
// The bytes of a text in UTF-8, as the files that prepare_packs.py and prepare_team.py hash were written.
function utf8Bytes(text) {
  const bytes = []
  for (let i = 0; i < text.length; i++) {
    let c = text.charCodeAt(i)
    if (c >= 0xd800 && c <= 0xdbff && i + 1 < text.length) {
      const d = text.charCodeAt(i + 1)
      if (d >= 0xdc00 && d <= 0xdfff) { c = 0x10000 + ((c - 0xd800) << 10) + (d - 0xdc00); i += 1 }
    }
    if (c < 0x80) bytes.push(c)
    else if (c < 0x800) bytes.push(0xc0 | (c >> 6), 0x80 | (c & 63))
    else if (c < 0x10000) bytes.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63))
    else bytes.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63))
  }
  return bytes
}
function sha256Hex(bytes) {
  const h = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19]
  const padded = bytes.concat([0x80])
  while (padded.length % 64 !== 56) padded.push(0)
  // The length in bits, as 64 bits, high word first.
  for (const word of [Math.floor(bytes.length / 0x20000000), (bytes.length * 8) >>> 0]) padded.push((word >>> 24) & 255, (word >>> 16) & 255, (word >>> 8) & 255, word & 255)
  const w = new Array(64)
  for (let offset = 0; offset < padded.length; offset += 64) {
    for (let t = 0; t < 16; t++) w[t] = (padded[offset + 4 * t] << 24) | (padded[offset + 4 * t + 1] << 16) | (padded[offset + 4 * t + 2] << 8) | padded[offset + 4 * t + 3]
    for (let t = 16; t < 64; t++) {
      const x = w[t - 15]
      const y = w[t - 2]
      const s0 = ((x >>> 7) | (x << 25)) ^ ((x >>> 18) | (x << 14)) ^ (x >>> 3)
      const s1 = ((y >>> 17) | (y << 15)) ^ ((y >>> 19) | (y << 13)) ^ (y >>> 10)
      w[t] = (w[t - 16] + s0 + w[t - 7] + s1) | 0
    }
    let [a, b, c, d, e, f, g, k] = h
    for (let t = 0; t < 64; t++) {
      const big1 = ((e >>> 6) | (e << 26)) ^ ((e >>> 11) | (e << 21)) ^ ((e >>> 25) | (e << 7))
      const choose = (e & f) ^ (~e & g)
      const first = (k + big1 + choose + K256[t] + w[t]) | 0
      const big0 = ((a >>> 2) | (a << 30)) ^ ((a >>> 13) | (a << 19)) ^ ((a >>> 22) | (a << 10))
      const majority = (a & b) ^ (a & c) ^ (b & c)
      const second = (big0 + majority) | 0
      k = g; g = f; f = e; e = (d + first) | 0; d = c; c = b; b = a; a = (first + second) | 0
    }
    const mixed = [a, b, c, d, e, f, g, k]
    for (let i = 0; i < 8; i++) h[i] = (h[i] + mixed[i]) | 0
  }
  return h.map(word => (word >>> 0).toString(16).padStart(8, '0')).join('')
}
const sha256Text = text => sha256Hex(utf8Bytes(text))
// What a text measures, once per place: its SHA-256 and its estimated tokens (four bytes to a token).
const measured = {}
const measure = (key, text) => {
  if (!measured[key]) {
    const bytes = utf8Bytes(text)
    measured[key] = { sum: sha256Hex(bytes), tokens: Math.ceil(bytes.length / 4) }
  }
  return measured[key]
}
const packOf = c => PACK_OF[baseArm(c.arm)] || null
const PACK_LEAD = 'Context looked up for you before you started (the harness ran pgr pack on the question; '
const packBlock = (c, k, labelled) => {
  const pack = PACKS[packOf(c)][setOf(c)][k]
  return `${labelled ? k.toUpperCase() + '. ' : ''}${PACK_LEAD}${pack.tokens} tokens). It may be all you need. Anything it does not show you may look up under the tool policy.\n${pack.output}`
}
const LOAD_LEAD = 'Notes filed earlier for the task of this question (the harness ran pgr loadout for the task '
const loadBlock = (c, k, labelled) => {
  const load = LOADOUTS[k]
  return `${labelled ? k.toUpperCase() + '. ' : ''}${LOAD_LEAD}${load.task} before you started; ${load.tokens} tokens). They may be all you need from the notes; look up the code itself under the tool policy.\n${load.output}`
}
// The blocks that follow the question or questions in a prompt, one per question and labelled Q1, Q2,
// Q3 when there are several; nothing for an arm that has no pack and no loadout in its prompts.
const context = (c, keys) => {
  const make = packOf(c) ? packBlock : baseArm(c.arm) === 'TI' ? loadBlock : null
  return make ? '\n\n' + keys.map(k => make(c, k, keys.length > 1)).join('\n\n') : ''
}
// The bank of one hub cell is new for every cell and repeat, so that no run can read another run's notes.
const bankOf = c => {
  if (typeof RUN_TAG !== 'string' || !RUN_TAG_RULE.test(RUN_TAG)) throw new Error('args.runTag is required for the hub arms: 1 to 24 characters of a-z 0-9 . _ -, starting with a letter or a digit')
  const name = ['r6', RUN_TAG, c.arm, c.setup, c.tier, c.rep || REP].join('-').toLowerCase()
  if (!validName(name)) throw new Error(`bank name ${JSON.stringify(name)} is not valid (1 to 64 characters of A-Z a-z 0-9 . _ -): shorten args.runTag or the repeat name`)
  return name
}
const hubOf = (c, role) => (HUB[baseArm(c.arm)] ? { bank: bankOf(c), task: HUB_TASK, role } : null)
// Everything a cell needs that the harness has to bring, checked before any agent starts.
const banksSeen = {}
function check(c) {
  const arm = baseArm(c.arm)
  const set = setOf(c)
  const where = tag(c, 'all')
  // A text and its token count against the checksum that was recorded for its place.
  const proven = (key, text, tokens, what) => {
    const want = SUMS[key]
    if (typeof want !== 'string' || !/^[0-9a-f]{64}$/.test(want)) throw new Error(`cell ${where}: args.sums has no checksum for ${what} (${key}); write the args again with round6_args.py`)
    const seen = measure(key, text)
    if (seen.sum !== want) throw new Error(`cell ${where}: ${what} does not match its checksum in args.sums (${key}): the text was cut or changed on the way; write the args again with round6_args.py and pass the file as it is`)
    if (seen.tokens !== tokens) throw new Error(`cell ${where}: the tokens of ${what} (${tokens}) are not the estimate (${seen.tokens}) of its text`)
  }
  if (SET_T[arm]) {
    if (c.setup !== 'single') throw new Error(`cell ${where}: set T is asked of one agent only (setup single)`)
    if (!(args && args.repoTeam)) throw new Error(`cell ${where}: set T needs args.repoTeam, the copy of the fixture that prepare_team.py made`)
  }
  if (HUB[arm] && (String(c.setup).indexOf('orch') !== 0 || c.setup === 'orch-notools')) throw new Error(`cell ${where}: the hub arms run orchestrated, with a reviewer that may use tools`)
  if (PACK_OF[arm]) {
    for (const k of KEYS) {
      const pack = PACKS[PACK_OF[arm]] && PACKS[PACK_OF[arm]][set] && PACKS[PACK_OF[arm]][set][k]
      if (!pack || typeof pack.output !== 'string' || !pack.output || !Number.isInteger(pack.tokens)) throw new Error(`cell ${where}: no pack for variant ${PACK_OF[arm]}, set ${set}, ${k} in args.packs (run prepare_packs.py)`)
      if (pack.question !== SETS[set].Q[k]) throw new Error(`cell ${where}: the pack for variant ${PACK_OF[arm]}, set ${set}, ${k} was made for another question text than this script asks (run prepare_packs.py again)`)
      proven(`packs/${PACK_OF[arm]}/${set}/${k}`, pack.output, pack.tokens, `the pack for variant ${PACK_OF[arm]}, set ${set}, ${k}`)
    }
  }
  if (arm === 'TI') {
    for (const k of KEYS) {
      const load = LOADOUTS[k]
      if (!load || typeof load.task !== 'string' || typeof load.output !== 'string' || !load.output || !Number.isInteger(load.tokens)) throw new Error(`cell ${where}: no loadout for ${k} in args.loadouts (run prepare_team.py)`)
      if (Q_T[k].indexOf('`' + load.task + '`') < 0) throw new Error(`cell ${where}: the loadout for ${k} is for task ${load.task}, which the question does not name`)
      proven(`loadouts/${k}`, load.output, load.tokens, `the loadout for ${k}`)
    }
  }
  if (HUB[arm]) {
    const bank = bankOf(c)
    if (banksSeen[bank]) throw new Error(`cell ${where}: bank ${bank} belongs to another cell; every hub cell and repeat needs a bank of its own`)
    banksSeen[bank] = true
  }
}

async function single(c) {
  const set = SETS[setOf(c)]
  const answer = await agent(
    `${policy(c.arm)}\n\nAnswer all three questions.\n${set.listed}${context(c, KEYS)}`,
    { label: tag(c, 'all'), phase: 'Single', schema: set.ALL, ...TIER[c.tier] })
  return { ...c, set: setOf(c), answer }
}

async function fanOut(c) {
  const set = SETS[setOf(c)]
  const got = await parallel(KEYS.map(k => () => agent(
    `${policy(c.arm)}\n\nAnswer this question.\n${set.Q[k]}${context(c, [k])}`,
    { label: tag(c, k), phase: 'Parallel', schema: set.S[k], ...TIER[c.tier] })))
  return { ...c, set: setOf(c), answer: { q1: got[0], q2: got[1], q3: got[2] } }
}

async function orchestrated(c) {
  const set = SETS[setOf(c)]
  // The shared briefs were written for set A's questions; a cell on any other set plans for itself.
  const plan = (SHARED_BRIEFS && !c.ownPlan && setOf(c) === 'A') ? SHARED_BRIEFS : await agent(
    `You are the orchestrator of one benchmark run. Three independent lookup questions about the repository at ${REPO} are listed below. Do not answer them, and call no tool except the structured output. Write one brief per question for a subagent that starts with no other context: the question, what a complete answer contains, and when to stop. Keep each brief under 80 words. The harness appends the tool policy to every brief, so leave tools out.\n\n${set.listed}`,
    { label: tag(c, 'plan'), phase: 'Orchestrated', schema: BRIEFS, ...APEX })
  if (!plan) return { ...c, set: setOf(c), answer: null, failed: 'plan' }
  const workers = await parallel(KEYS.map(k => () => agent(
    // The reused briefs name the main fixture; an arm on the other copy is pointed at its own.
    `${policy(c.arm, hubOf(c, 'worker'))}\n\nBrief from the orchestrator:\n${plan[k].split(REPO).join(REPO_OF[baseArm(c.arm)] || REPO)}${context(c, [k])}`,
    { label: tag(c, 'w-' + k), phase: 'Orchestrated', schema: set.S[k], ...TIER[c.tier] })))
  const reported = { q1: workers[0], q2: workers[1], q3: workers[2] }
  const review = REVIEW[c.setup]
  const answer = await agent(
    review.tools
      ? `${policy(c.arm, hubOf(c, 'reviewer'))}\n\nYou are the orchestrator and lead reviewer of this run. Three subagents each answered one question; their answers are below as JSON, with null for a subagent that returned nothing. Check each answer under the tool policy with as few calls as it takes, correct anything wrong or missing, and return the final answers for all three questions.\n\n${set.listed}${context(c, KEYS)}\n\nSubagent answers:\n${JSON.stringify(reported)}`
      : `This is one run of a benchmark that measures how many tokens a code lookup costs.\n\nYou are the orchestrator and lead reviewer of this run. Three subagents each answered one question about a repository; their answers are below as JSON, with null for a subagent that returned nothing. Call no tool except the structured output: you cannot look anything up. Check each answer against its question for what can be seen from the answer alone (a missing field, an answer to a different question, a list that plainly cannot be complete), correct only what you can correct that way, and return the final answers for all three questions.\n\n${set.listed}\n\nSubagent answers:\n${JSON.stringify(reported)}`,
    { label: tag(c, 'verify'), phase: 'Orchestrated', schema: set.ALL, ...review.who })
  return { ...c, set: setOf(c), workers: reported, answer }
}

const RUN = { single, parallel: fanOut }
for (const name of Object.keys(REVIEW)) RUN[name] = orchestrated
// A held-out id with no base arm fails here, before any agent starts.
for (const c of CELLS) baseArm(c.arm)
// A missing pack, loadout, run tag or team fixture also fails here, before any agent starts.
for (const c of CELLS) check(c)
log(`${CELLS.length} cells, repeat ${REP}`)
if (CELLS.some(c => setOf(c) === 'B')) log(`${CELLS.filter(c => setOf(c) === 'B').length} of them on the held-out question set B`)
if (CELLS.some(c => setOf(c) === 'T')) log(`${CELLS.filter(c => setOf(c) === 'T').length} of them on set T, the demonstration set`)
const done = await parallel(CELLS.map(c => () => RUN[c.setup](c)))
return { rep: REP, cells: done }
