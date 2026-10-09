---
name: verify-before-complete
description: Use before claiming any work is done, fixed, passing, deployed, or working - requires running the actual check and quoting real output. Blocks success claims that rest on inference rather than evidence.
---

# Verify Before Complete

## The rule

Do not claim a result you have not observed.

"The tests should pass now" is a hypothesis. "The tests pass" is a claim about the world.
Only make the second one after seeing the run.

## The three sentences that require evidence

Before any of these leaves your output, you must have run something and read the result:

| Claim | Required evidence |
|---|---|
| "Fixed" / "Works now" | The failing case re-run, now passing |
| "Tests pass" | Test-runner output, with the count |
| "Builds cleanly" | Build command exit 0 |
| "Deployed" | Deploy output plus a check against the live endpoint |
| "No other usages" | The search command and its output |
| "Removed all X" | A search proving zero remain |
| "Faster" / "Smaller" | Before and after measurements |
| "Endpoint returns Y" | The actual request and response |

## Procedure

1. **State what would prove it.** Before running anything: "This is done if `<command>` exits 0
   and prints `<expectation>`."
2. **Run it.** The real command, in the real environment, not a simplified stand-in.
3. **Read the whole output.** Not just the exit code. A suite that reports `0 passed, 0 failed`
   exits 0 and proves nothing.
4. **Quote the decisive line.** Shortest excerpt that settles it.
5. **Only then claim.**

## Failure modes to check for explicitly

**The vacuous pass.** Zero tests collected, a filter that matched nothing, a skipped suite.
Confirm the test you care about actually *ran*.

```
Bad evidence:  "42 passed"                       (was the new test among them?)
Good evidence: "test_retry_resets_backoff PASSED"  (the specific one)
```

**Cached or stale results.** A build server replaying an old green. A test runner using a
cached compile. Force a clean run when the claim matters.

**Wrong environment.** Passing locally is not passing in CI. Say which one you observed.

**Partial completion reported as total.** If five things were asked for and four are done,
the honest report is "four done, one blocked, here is the blocker" — not "done."

**The fix that was never applied.** Confirm the edit actually landed in the file on disk
before testing the theory that it works.

## When you cannot verify

Say so, precisely. This is not a failure — hiding it is.

```
Implemented; NOT verified. No test runner configured in this repo.
To verify: `pytest tests/test_retry.py -k backoff`
Risk if wrong: retry storms under sustained 503s.
```

Never substitute confidence for evidence. "I'm confident this is correct" carries no
information the reader can check.

## Reporting honestly

If verification fails, report the failure with the output. Do not:

- Retry silently until something passes and report only that
- Narrow the test until it passes and call it fixed
- Describe a failure as a "minor issue" when it invalidates the claim
- Bury the failure at the end of a long success narrative

If tests fail, the first line says tests fail.

## Grounding

This is the operational form of the verification step in
[Dhuliawala et al., *Chain-of-Verification*, arXiv:2309.11495](https://arxiv.org/abs/2309.11495) —
draft, then generate and independently answer checks against the draft, then finalise. CoVe's
measured gain comes specifically from answering verification questions *independently* of the
original draft, which is why the check here must be a real command run rather than a re-read of
your own reasoning.

## Self-check

- [ ] Did I run the check, or infer the result?
- [ ] Did the specific case I care about actually execute?
- [ ] Am I quoting real output, or paraphrasing what I expect it said?
- [ ] Is any part of this incomplete and reported as complete?
- [ ] If it failed, is the failure the first thing I say?
