---
name: caveman-ultra-compact
description: Aggressively compress ONE natural-language file (CLAUDE.md, AGENTS.md, notes, todos, prompts) into caveman-speak, targeting 40%+ byte reduction while preserving code, commands, URLs, paths and headings byte-for-byte. Use when caveman-compress was too gentle, or when a context file is large enough that its token cost matters every session. Do NOT use on documents a human will be graded on or that ship to end users.
---

# Caveman Ultra Compact

Maximum-compression pass over a single natural-language file. Harsher than
`caveman-compress`: it restructures prose into fragments and bullets instead of
only deleting filler.

## When NOT to run

Refuse and say why:

- Graded, published, or customer-facing prose (reports, papers, READMEs that
  represent the author's writing, marketing copy, legal text).
- Files matching the sensitive denylist: `.env*`, `credentials*`, `secret*`,
  `password*`, `id_rsa*`, `*.pem`, `*.key`, anything under `.ssh/`, `.aws/`,
  `.gnupg/`, `.kube/`, `.docker/`.
- Source or config: `.py .js .ts .json .yaml .yml .toml .lock .css .html .xml .sql .sh`.
- Any `*.original.md` backup produced by a previous run.
- Files under 400 bytes — the savings do not repay the risk.

## Procedure

1. **Read the target.** Record its byte size.

2. **Back up out of tree, verify the copy, then overwrite.** The backup must not
   live beside the source, or skill auto-loaders re-ingest it as a live file.

   ```bash
   DST="$LOCALAPPDATA/caveman-compress/backups/$(basename "$(dirname "$FILE")")"   # Windows
   DST="${XDG_DATA_HOME:-$HOME/.local/share}/caveman-compress/backups/$(basename "$(dirname "$FILE")")"  # POSIX
   mkdir -p "$DST" && cp "$FILE" "$DST/$(basename "${FILE%.*}").original.md"
   ```

   Abort if a backup already exists — it may hold content from before an earlier
   compression, and overwriting it destroys the only clean copy.

3. **Compress the body.** Leave YAML frontmatter untouched; split it off first
   and re-prepend it verbatim.

4. **Validate before declaring success** (see Verification).

5. **Report** original bytes, new bytes, percent saved, and the backup path.

## Compression rules

### Preserve byte-for-byte — never touch

Fenced and indented code blocks, inline `backtick` spans, URLs, markdown link
targets, file paths, shell commands, environment variables, YAML frontmatter,
every heading's exact text, version numbers, dates, numeric values, proper
nouns, technical terms, and exact error strings.

### Delete

- Articles (a, an, the) and filler (just, really, basically, actually, simply,
  essentially, generally, quite, very).
- Pleasantries and hedging ("it might be worth", "you could consider",
  "I'd recommend", "please note that").
- Connective fluff (however, furthermore, additionally, in addition, that said).
- Restatements: a sentence that repeats the heading above it.
- Duplicate bullets that say one thing two ways — keep the clearer one.
- Extra examples that demonstrate the same pattern — keep one.

### Restructure — this is what makes it *ultra*

- Prose paragraph listing 3+ items becomes a bullet list.
- "You should X because Y" becomes "X. Y." Keep the *why* whenever the rule is
  a safety, security, or data-loss rule; drop it for style preferences.
- Instruction sentences lose their subject: "Run tests before push", not "You
  should always make sure to run the tests before pushing".
- Tables keep their structure; only cell prose compresses.
- Nested bullets keep their nesting level.

### Never abbreviate

Do not invent short forms (`cfg`, `impl`, `req`, `res`, `fn`). The tokenizer
splits them the same as the full word: zero tokens saved, reader still decodes.
Full words are cheaper *and* clearer. Standard acronyms (DB, API, HTTP) are fine.

## Verification — required, not optional

Run all four and report the results. Restore from backup if any fail.

```bash
# 1. Code blocks identical
diff <(sed -n '/```/,/```/p' "$BACKUP") <(sed -n '/```/,/```/p' "$FILE") && echo "CODE OK"

# 2. Headings identical
diff <(grep '^#' "$BACKUP") <(grep '^#' "$FILE") && echo "HEADINGS OK"

# 3. Every URL survived
comm -23 <(grep -oE 'https?://[^ )"]+' "$BACKUP" | sort -u) \
         <(grep -oE 'https?://[^ )"]+' "$FILE" | sort -u)   # must print nothing

# 4. Size actually dropped
wc -c "$BACKUP" "$FILE"
```

A run that saves less than 15% is a failed run: restore the backup and tell the
user the file was already dense. Do not report a rewrite as a win when it only
moved words around.

## Pattern

Before:

> You should always make sure to run the test suite before pushing any changes
> to the main branch. This is important because it helps catch bugs early and
> prevents broken builds from being deployed to production.

After:

> Run tests before push to main. Catches bugs early, prevents broken prod deploys.

Before:

> The application uses a microservices architecture with the following
> components. The API gateway handles all incoming requests and routes them to
> the appropriate service. The authentication service is responsible for
> managing user sessions and JWT tokens.

After:

> Microservices architecture:
> - API gateway routes all incoming requests to services
> - Auth service manages user sessions + JWT tokens

## Related

- `caveman-compress` — gentler pass, deletes filler without restructuring.
- `caveman-ultra-compact-repo` — same rules applied across a whole repository.
