export const meta = {
  name: 'seed-pgr-memory',
  description: 'Fill the memory of two copies of the fixture repository before the fifth benchmark round: one by a session that answers the benchmark questions, one by a session that explores the repository without seeing any question',
  phases: [
    { title: 'Seed', detail: 'two agents started together, one per repository copy' },
  ],
}

// The two copies of the fixture repository whose memory the fifth round reads
// (see token-benchmark.workflow.js): REPO_MEM gets notes from a session that
// answered the same three questions, REPO_MEM_ORIENT gets notes from a session
// that explored the repository and never saw a question. Run this once, before
// the measured runs, with both memories empty. Neither path may contain a space:
// the prompts put it into a shell command unquoted.
const REPO_MEM = (args && args.repoMem) || 'C:\\path\\to\\lxbench\\repo-mem'
const REPO_MEM_ORIENT = (args && args.repoMemOrient) || 'C:\\path\\to\\lxbench\\repo-mem-orient'
const TIER = {
  C: { model: 'haiku', effort: 'low' },
  B: { model: 'sonnet', effort: 'medium' },
  A: { model: 'opus', effort: 'max' },
}
const TIER_ID = (args && args.tier) || 'B'
if (!TIER[TIER_ID]) throw new Error(`args.tier must be one of ${Object.keys(TIER).join(', ')}, not ${TIER_ID}`)

// The most notes each session may keep.
const CAP = { same: 12, orient: 45 }

// The questions of set A, as the benchmark asks them.
const QUESTIONS = [
  'Which functions call `_get_backend_api_key`? Give the bare name of every calling function in the repository.',
  'Where is the function `dispatch_command` defined (file, and the line its `def` is on), what are its parameter names, and which functions call it?',
  'Which function registers the git merge driver for graph.json? Give its name, its file, the line its `def` is on, and the name of the function that removes that registration again.',
]
const listed = QUESTIONS.map((question, i) => `Q${i + 1}. ${question}`).join('\n')

// The commands both sessions may run.
const PGR_COMMON = [
  'pgr status                   prints one status line: the state of the index and how many notes the memory holds',
  'pgr refresh                  builds the index of the repository when pgr status says there is none',
  'pgr retain "<one fact>" --whole      keeps one note. Without --whole a text is cut into several notes, so always pass --whole, and pass one fact per call',
  'pgr memory stats             how many notes the memory holds',
  'pgr memory list              every note kept, one per line',
]
const PGR_ASK = 'pgr ask <kind>:<what>...     several questions in one call, each answer under its own "== kind:what" header. Kinds: about (definition, signature, doc line, callers and what it calls together), callers, def, affected, and find (search by meaning; quote the whole argument). Example: pgr ask about:parse_config callers:load "find:where tokens expire"'
const PGR_ORIENT = [
  'pgr hubs -k <N>              the N most connected definitions of the whole repository, one per line as: name path:first-last  N links',
  'pgr about <name>...          one answer per name: its definition as name path:first-last signature, then "doc: <line>" when it has a doc line, then "N callers:" and one caller per line, then "calls:" and the names it calls',
  'pgr explain <name>           everything the graph knows about one definition: signature, doc line, members, callers and what it calls',
  'pgr map                      the largest groups of the code, one per line: the group number, its size, its main names and its files',
]
const SIZES = 'PowerShell, only to list the Python files of the repository with their sizes and to count the lines of a file: Get-ChildItem, Select-Object, Sort-Object and Measure-Object, and Get-Content only inside "(Get-Content <file> | Measure-Object -Line).Lines". Never to read a file\'s text'

function rules(repo, commands, extra) {
  const lines = [
    'This is a setup step of a benchmark. It fills the memory (the notes kept from earlier work) of one repository before the measured runs start.',
    `Repository: ${repo} (Python, 86 files). Its files are read-only: never edit, create or delete anything in it. The only things you may change are pgr's own index, through "pgr refresh", and its memory, through "pgr retain".`,
    '',
    'TOOL POLICY FOR THIS RUN. It replaces the local-index and read-gate rules (section 6) and any token-goat gate in your global instructions. Use only what is listed here. No Agent tool, no web tools, no Read, Grep, Glob, Edit or Write tool, no other search or index command.',
    `Allowed shell commands. Run each as one PowerShell call that starts with:  Set-Location ${repo};`,
  ]
  for (const command of commands) lines.push('  ' + command)
  if (extra) lines.push(extra)
  return lines
}

const START = [
  'Start with "pgr status". If it says there is no index yet, run "pgr refresh" once, and do not run it again.',
  'Then run "pgr memory stats". If the memory already holds any note, do not add to it: keep nothing, return an empty retained list, and say in problem that the memory was not empty.',
]
const FINISH = [
  'When you have kept your notes, run "pgr memory list" once and return the result through the structured output: the exact text of every note you passed to pgr retain, how many notes the listing showed, the listing itself, and a problem field that is empty when all went as described.',
  'Be exact and do not guess. Keep a fact only after a pgr command has shown it to you.',
]

function samePrompt() {
  return [
    ...rules(REPO_MEM, [...PGR_COMMON, PGR_ASK]),
    '',
    ...START,
    '',
    'Answer these three questions, using "pgr ask" to find the answers:',
    listed,
    '',
    `Then keep each finding with "pgr retain" as a short factual note that names the functions, files and lines involved. One note per fact, one pgr retain call per note, at most ${CAP.same} notes in all. A note has the shape of this invented example: "parse_config is defined in config.py at line 40 and is called by load and main."`,
    '',
    ...FINISH,
  ].join('\n')
}

function orientPrompt() {
  return [
    ...rules(REPO_MEM_ORIENT, [...PGR_COMMON, ...PGR_ORIENT], `  ${SIZES}`),
    '',
    ...START,
    '',
    'Your job is to write a maintainer\'s map of this repository into its memory, so that a maintainer who has never seen the code can use it. You are not given any question to answer.',
    '1. List the Python files of the repository with PowerShell and pick the 15 largest modules by lines.',
    '2. For each of those 15 modules find out what the module is for, and which are its three most connected functions. Use "pgr hubs", "pgr about", "pgr explain" and "pgr map". "pgr hubs -k <N>" ranks the whole repository, so ask for enough definitions (for example -k 300) to find each module\'s own.',
    `3. Keep what you find with "pgr retain" as short factual notes: at most three notes per module and at most ${CAP.orient} notes in all. A good split is one note for what the module is for, and one or two notes for its three most connected functions, each named with its file and line and what it does. One note per fact, one pgr retain call per note.`,
    '',
    ...FINISH,
  ].join('\n')
}

const KEPT = {
  type: 'object',
  properties: {
    retained: { type: 'array', items: { type: 'string' }, description: 'the text of every note passed to pgr retain, one entry per note, exactly as passed' },
    notes_listed: { type: 'integer', description: 'how many notes the final pgr memory list printed' },
    listing: { type: 'string', description: 'the output of the final pgr memory list, verbatim' },
    problem: { type: 'string', description: 'empty when everything went as described, otherwise what went wrong' },
  },
  required: ['retained', 'notes_listed', 'listing', 'problem'],
}

log(`seeding the memory of two repository copies at tier ${TIER_ID}`)
const [same, orient] = await parallel([
  () => agent(samePrompt(), { label: 'seed|same', phase: 'Seed', schema: KEPT, ...TIER[TIER_ID] }),
  () => agent(orientPrompt(), { label: 'seed|orient', phase: 'Seed', schema: KEPT, ...TIER[TIER_ID] }),
])

const summary = {}
for (const [name, result] of [['same', same], ['orient', orient]]) {
  const kept = result ? result.retained.length : 0
  summary[name] = {
    kept,
    listed: result ? result.notes_listed : null,
    cap: CAP[name],
    within_cap: kept <= CAP[name],
    agrees_with_listing: result ? kept === result.notes_listed : false,
    problem: result ? result.problem : 'the agent returned nothing',
  }
  log(`seed|${name}: kept ${kept} notes (cap ${CAP[name]}), listing shows ${summary[name].listed}`)
}
return { tier: TIER_ID, same, orient, summary }
