# Benchmark results

787 agent runs on one repository: three lookup questions, 36 ways of finding the answer, three model tiers, three ways of organising the agents. 150.5 million tokens measured, 86% of them cache reads.

The charts are on the project site. This file is the same data as tables, written by `benchmark/harness/build_site.py`.

## Findings

- An agent starts with about 63k tokens of context before it reads one line of code, and every later turn re-reads all of it. Across the 787 agents measured, 86% of all tokens were cache reads, 13% were cache writes and 0.7% were output. What an index saves in tool output is small next to that. What it really buys is fewer turns.
- Tokens follow turns. Among methods run at least twice, one agent answering all three questions averaged from 2.0 turns (Shipped: lx ask, embeddings on, Tier B, 3 runs: 129k tokens) to 7.0 turns (graphify + semble, Tier C, 2 runs: 480k tokens).
- Cheapest tier, one agent, methods run at least three times, fewest tokens first: S1 Shipped: lx ask, default index: 130k tokens, 2.0 turns, 2 of 3 runs fully correct; S3 Shipped: lx ask, embeddings on: 130k tokens, 2.0 turns, 3 of 3 runs fully correct; L4 lx ask, one call: 130k tokens, 2.0 turns, 3 of 3 runs fully correct; C12 lx about + hybrid: 130k tokens, 2.0 turns, 3 of 3 runs fully correct; L3 lx + hybrid search: 130k tokens, 2.0 turns, 3 of 3 runs fully correct.
- Where the shipped build, the round 1 leaders and the control stand among those 29 methods: S1 Shipped: lx ask, default index: 130k tokens, 2.0 turns, 2 of 3 runs fully correct, place 1; S3 Shipped: lx ask, embeddings on: 130k tokens, 2.0 turns, 3 of 3 runs fully correct, place 2; A9 lx, as shipped after round 1: 197k tokens, 3.0 turns, 5 of 6 runs fully correct, place 19; R1 lx shipped + Rust embeddings: 176k tokens, 2.7 turns, 3 of 3 runs fully correct, place 15; A0 No index: 365k tokens, 5.0 turns, 4 of 5 runs fully correct, place 29.
- Two turns is the floor for this task: one to ask and one to answer. On Tier C, scout, 11 methods took exactly two turns and answered everything correctly in each of their 3 runs: S3 Shipped: lx ask, embeddings on, L4 lx ask, one call, C12 lx about + hybrid, L3 lx + hybrid search, C15 lx ask + token-goat read, C13 lx, every new command, S2 Shipped: all commands, default index, N1 lx ask, no Rust embeddings, N2 lx, every new command, no Rust embeddings, C14 lx, every new command + token-goat, C7 token-goat + semble.
- Two turns is the floor for this task: one to ask and one to answer. On Tier B, coder, 11 methods took exactly two turns and answered everything correctly in each of their 2 to 3 runs: C15 lx ask + token-goat read, L2 lx + cards, S3 Shipped: lx ask, embeddings on, L4 lx ask, one call, R1 lx shipped + Rust embeddings, C2 lx + Rust embeddings + token-goat, C12 lx about + hybrid, C11 lx about + Rust cards, S4 Shipped: all commands, embeddings on, C13 lx, every new command, C14 lx, every new command + token-goat.
- The shipped build on an index with Rust embeddings and on the default index without them. Tier C, one agent: Shipped: lx ask, embeddings on 130k (2 turns over 3 runs), against Shipped: lx ask, default index 130k (2 turns over 3 runs); Shipped: all commands, embeddings on 198k (2 to 4 turns over 3 runs), against Shipped: all commands, default index 130k (2 turns over 3 runs).
- The shipped build on an index with Rust embeddings and on the default index without them. Tier B, one agent: Shipped: lx ask, embeddings on 129k (2 turns over 3 runs), against Shipped: lx ask, default index 196k (3 turns over 6 runs); Shipped: all commands, embeddings on 130k (2 turns over 3 runs), against Shipped: all commands, default index 196k (3 turns over 3 runs).
- The shipped build on an index with Rust embeddings and on the default index without them. Tier A, one agent: Shipped: lx ask, embeddings on 199k (3 turns over 1 run), against Shipped: lx ask, default index 201k (3 turns over 1 run); Shipped: all commands, embeddings on 199k (3 turns over 1 run), against Shipped: all commands, default index 199k (3 turns over 1 run).
- What merging in the Rust index's name and doc search added on the default index: the shipped build against the round 2 build. Tier C, one agent: Shipped: lx ask, default index 130k (2 turns over 3 runs), against lx ask, no Rust embeddings 130k (2 turns over 3 runs); Shipped: all commands, default index 130k (2 turns over 3 runs), against lx, every new command, no Rust embeddings 131k (2 turns over 3 runs).
- What merging in the Rust index's name and doc search added on the default index: the shipped build against the round 2 build. Tier B, one agent: Shipped: lx ask, default index 196k (3 turns over 6 runs), against lx ask, no Rust embeddings 296k (4 to 5 turns over 2 runs); Shipped: all commands, default index 196k (3 turns over 3 runs), against lx, every new command, no Rust embeddings 196k (3 turns over 2 runs).
- Round 2, before that search was merged in: the same commands on an index built with Rust embeddings and without. Tier C, one agent: lx ask, one call 130k (2 turns over 3 runs), against lx ask, no Rust embeddings 130k (2 turns over 3 runs); lx, every new command 130k (2 turns over 3 runs), against lx, every new command, no Rust embeddings 131k (2 turns over 3 runs).
- Round 2, before that search was merged in: the same commands on an index built with Rust embeddings and without. Tier B, one agent: lx ask, one call 129k (2 turns over 2 runs), against lx ask, no Rust embeddings 296k (4 to 5 turns over 2 runs); lx, every new command 130k (2 turns over 2 runs), against lx, every new command, no Rust embeddings 196k (3 turns over 2 runs).
- Does adding token-goat beside lx change anything? Each pair is the same lx build without it and with it. Tier C, one agent: lx, as shipped after round 1 197k (2 to 5 turns over 6 runs), against lx shipped + token-goat 242k (3 to 5 turns over 3 runs); lx shipped + Rust embeddings 176k (2 to 3 turns over 3 runs), against lx + Rust embeddings + token-goat 177k (2 to 3 turns over 3 runs); lx, every new command 130k (2 turns over 3 runs), against lx, every new command + token-goat 131k (2 turns over 3 runs); lx ask, one call 130k (2 turns over 3 runs), against lx ask + token-goat read 130k (2 turns over 3 runs).
- Does adding token-goat beside lx change anything? Each pair is the same lx build without it and with it. Tier B, one agent: lx, as shipped after round 1 217k (3 to 4 turns over 3 runs), against lx shipped + token-goat 196k (3 turns over 2 runs); lx shipped + Rust embeddings 129k (2 turns over 2 runs), against lx + Rust embeddings + token-goat 129k (2 turns over 2 runs); lx, every new command 130k (2 turns over 2 runs), against lx, every new command + token-goat 130k (2 turns over 2 runs); lx ask, one call 129k (2 turns over 2 runs), against lx ask + token-goat read 129k (2 turns over 2 runs).
- Do the engines do as well without lx in front of them? lx with every command against the raw commands of the engines it wraps. Tier C, one agent: lx, every new command 130k (2 turns over 3 runs), against Rust index + graphify + semble 289k (3 to 5 turns over 3 runs); lx, every new command 130k (2 turns over 3 runs), against Rust index + token-goat + graphify + semble 311k (4 to 5 turns over 3 runs); lx, every new command 130k (2 turns over 3 runs), against Rust index + token-goat 282k (3 to 5 turns over 3 runs).
- Do the engines do as well without lx in front of them? lx with every command against the raw commands of the engines it wraps. Tier B, one agent: lx, every new command 130k (2 turns over 2 runs), against Rust index + graphify + semble 428k (5 to 7 turns over 2 runs); lx, every new command 130k (2 turns over 2 runs), against Rust index + token-goat + graphify + semble 314k (3 to 6 turns over 2 runs); lx, every new command 130k (2 turns over 2 runs), against Rust index + token-goat 393k (5 to 6 turns over 2 runs).
- Tier C, scout, one agent: no index used 365k tokens and $0.0074 over 5 runs. Among the 28 methods run at least twice and fully correct every time, the leanest was Shipped: lx ask, embeddings on at 130k tokens and $0.0019 over 3 runs, 64% fewer tokens.
- Tier B, coder, one agent: no index used 281k tokens and $0.103 over 3 runs. Among the 24 methods run at least twice and fully correct every time, the leanest was lx ask + token-goat read at 129k tokens and $0.0309 over 2 runs, 54% fewer tokens. 5 methods ran once on this tier and answered everything; the lowest of those single runs was lx improved + Rust embeddings at 128k tokens.
- Tier A, apex, one agent: no index used 298k tokens and $0.343 over 3 runs. Only one method was run twice on this tier and was fully correct both times: lx, as shipped after round 1 at 272k tokens and $0.164 over 2 runs, 9% fewer tokens. 32 methods ran once on this tier and answered everything; the lowest of those single runs was lx ask + token-goat read at 135k tokens.
- The model tier moves cost far more than the method does. With the start-up context cached, the same no-index job cost $0.0074 on Tier C (98% correct), $0.103 on Tier B (100%) and $0.343 on Tier A (100%). Send lookups to the cheapest tier.
- Orchestration buys accuracy and it is the expensive part. With Tier C subagents the orchestrator's two calls were 97% of the run's cost. Without a brief or a reviewer, 5 of 43 three-subagent runs returned a wrong detail; orchestrated on the same tier, 48 of 48 ended fully correct, 45 of them before the review changed anything.
- A cold start costs far more than a warm one. Writing the 63k start-up context to the cache costs $0.314 on Tier A against $0.0126 to read it back, 25 times as much. Subagents started together all pay the cold price; started one after another within five minutes they share the cached part.
- token-goat's hooks, installed in the client: with no index they ran in 9 of 9 runs, rewrote 3.7 tool results a run, and the runs used 2% more tokens, inside the 21% by which repeats of one prompt without the hooks differ; 5 of 9 of those runs ended fully correct, against 13 of 17 without the hooks. With index commands sent through PowerShell they ran in 6 of 32 runs. Through Bash they ran in 7 of 7 runs, rewrote nothing, and the runs used 4% more tokens, inside the 26% by which repeats of one prompt without the hooks differ.

## One agent

One agent answers all three questions. Fewest tokens first within each tier.

| Method | Tier | Runs | Turns | Start-up | Re-read | Added | Output | Total tokens | Cost warm | Cost cold | Correct |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| H1 Shipped lx ask, default index, hooks on | C | 3 | 2.0 | 62,891 | 62,889 | 3,300 | 262 | 129,342 | $0.0018 | $0.0090 | 100% |
| HB Shipped lx ask through Bash, hooks on | C | 3 | 2.0 | 62,899 | 62,897 | 3,264 | 474 | 129,533 | $0.0019 | $0.0091 | 100% |
| H3 Shipped lx ask, embeddings on, hooks on | C | 3 | 2.0 | 62,883 | 62,881 | 3,438 | 382 | 129,584 | $0.0019 | $0.0091 | 100% |
| S1 Shipped: lx ask, default index | C | 3 | 2.0 | 63,028 | 63,026 | 3,372 | 260 | 129,686 | $0.0018 | $0.0091 | 97% |
| S3 Shipped: lx ask, embeddings on | C | 3 | 2.0 | 63,020 | 63,018 | 3,442 | 474 | 129,954 | $0.0019 | $0.0092 | 100% |
| L4 lx ask, one call | C | 3 | 2.0 | 63,020 | 63,018 | 3,432 | 621 | 130,091 | $0.0020 | $0.0092 | 100% |
| C12 lx about + hybrid | C | 3 | 2.0 | 63,069 | 63,067 | 3,217 | 812 | 130,165 | $0.0021 | $0.0093 | 100% |
| L3 lx + hybrid search | C | 3 | 2.0 | 63,022 | 63,020 | 3,261 | 863 | 130,166 | $0.0021 | $0.0093 | 100% |
| C15 lx ask + token-goat read | C | 3 | 2.0 | 63,055 | 63,053 | 3,507 | 629 | 130,244 | $0.0020 | $0.0093 | 100% |
| C13 lx, every new command | C | 3 | 2.0 | 63,228 | 63,226 | 3,402 | 388 | 130,244 | $0.0019 | $0.0092 | 100% |
| S2 Shipped: all commands, default index | C | 3 | 2.0 | 63,179 | 63,177 | 3,312 | 641 | 130,309 | $0.0020 | $0.0093 | 100% |
| N1 lx ask, no Rust embeddings | C | 3 | 2.0 | 63,028 | 63,026 | 3,396 | 931 | 130,381 | $0.0022 | $0.0094 | 100% |
| N2 lx, every new command, no Rust embeddings | C | 3 | 2.0 | 63,179 | 63,177 | 3,238 | 960 | 130,554 | $0.0021 | $0.0094 | 100% |
| C14 lx, every new command + token-goat | C | 3 | 2.0 | 63,313 | 63,311 | 3,322 | 696 | 130,642 | $0.0020 | $0.0093 | 100% |
| C7 token-goat + semble | C | 3 | 2.0 | 63,042 | 63,040 | 6,821 | 1,143 | 134,047 | $0.0027 | $0.0099 | 100% |
| H9 Round 1 lx, hooks on | C | 3 | 2.3 | 62,896 | 84,920 | 3,300 | 778 | 151,894 | $0.0023 | $0.0095 | 100% |
| SB Shipped lx ask through Bash, hooks off | C | 3 | 2.3 | 63,186 | 85,267 | 3,552 | 354 | 152,360 | $0.0021 | $0.0094 | 100% |
| C11 lx about + Rust cards | C | 3 | 2.3 | 63,070 | 85,179 | 3,858 | 570 | 152,676 | $0.0022 | $0.0095 | 100% |
| L2 lx + cards | C | 3 | 2.3 | 63,028 | 85,233 | 3,779 | 952 | 152,991 | $0.0024 | $0.0097 | 100% |
| H5 token-goat commands, hooks on | C | 3 | 2.3 | 62,860 | 85,549 | 5,603 | 977 | 154,990 | $0.0027 | $0.0099 | 100% |
| A8 lx improved + Rust embeddings | C | 2 | 2.5 | 62,637 | 95,472 | 3,880 | 706 | 162,695 | $0.0024 | $0.0096 | 95% |
| R1 lx shipped + Rust embeddings | C | 3 | 2.7 | 63,012 | 106,959 | 4,197 | 1,368 | 175,536 | $0.0029 | $0.0102 | 100% |
| C2 lx + Rust embeddings + token-goat | C | 3 | 2.7 | 63,097 | 107,452 | 4,956 | 1,010 | 176,514 | $0.0028 | $0.0101 | 100% |
| A5 token-goat alone | C | 3 | 2.7 | 62,806 | 107,987 | 5,765 | 586 | 177,144 | $0.0027 | $0.0099 | 100% |
| C6 token-goat + graphify + semble | C | 3 | 2.7 | 63,076 | 108,610 | 7,437 | 1,006 | 180,129 | $0.0031 | $0.0104 | 100% |
| A4 lx first version + Rust embeddings | C | 2 | 3.0 | 62,635 | 128,320 | 3,676 | 837 | 195,468 | $0.0028 | $0.0100 | 100% |
| A9 lx, as shipped after round 1 | C | 6 | 3.0 | 62,852 | 129,520 | 3,899 | 773 | 197,045 | $0.0028 | $0.0100 | 99% |
| A6 token-goat + lx improved | C | 2 | 3.0 | 62,729 | 129,008 | 4,732 | 798 | 197,267 | $0.0029 | $0.0101 | 99% |
| S4 Shipped: all commands, embeddings on | C | 3 | 3.0 | 63,171 | 129,759 | 3,929 | 730 | 197,588 | $0.0028 | $0.0100 | 100% |
| C10 lx about + cards | C | 3 | 3.3 | 63,075 | 151,814 | 4,247 | 475 | 219,611 | $0.0029 | $0.0102 | 100% |
| L1 lx + about | C | 3 | 3.3 | 63,080 | 151,629 | 4,105 | 925 | 219,739 | $0.0031 | $0.0104 | 100% |
| C9 lx finds, token-goat reads | C | 3 | 3.3 | 63,068 | 151,943 | 5,147 | 712 | 220,870 | $0.0032 | $0.0104 | 100% |
| A7 lx, improved | C | 2 | 3.5 | 62,644 | 161,653 | 4,056 | 481 | 228,834 | $0.0030 | $0.0102 | 95% |
| C3 lx shipped + token-goat | C | 3 | 3.7 | 63,118 | 174,323 | 4,351 | 364 | 242,156 | $0.0031 | $0.0104 | 100% |
| C8 token-goat + graphify | C | 3 | 3.7 | 63,031 | 178,243 | 6,431 | 1,034 | 248,739 | $0.0037 | $0.0110 | 100% |
| C4 Rust index + token-goat | C | 3 | 4.0 | 63,075 | 202,897 | 14,703 | 914 | 281,589 | $0.0050 | $0.0122 | 100% |
| C5 Rust index + graphify + semble | C | 3 | 4.0 | 63,069 | 208,367 | 15,779 | 1,906 | 289,122 | $0.0056 | $0.0129 | 100% |
| A3 lx, first version | C | 2 | 4.5 | 62,630 | 229,832 | 6,220 | 1,524 | 300,206 | $0.0045 | $0.0117 | 100% |
| C1 Rust index + token-goat + graphify + semble | C | 3 | 4.3 | 63,154 | 232,189 | 14,429 | 823 | 310,595 | $0.0052 | $0.0124 | 99% |
| A2 Rust index alone, embeddings on | C | 2 | 5.0 | 62,629 | 273,990 | 13,282 | 1,794 | 351,694 | $0.0059 | $0.0131 | 100% |
| H0 No index, hooks on | C | 3 | 5.0 | 62,709 | 279,467 | 15,414 | 4,458 | 362,048 | $0.0076 | $0.0148 | 99% |
| A0 No index | C | 5 | 5.0 | 62,702 | 281,856 | 15,960 | 3,990 | 364,508 | $0.0074 | $0.0146 | 98% |
| A1 graphify + semble | C | 2 | 7.0 | 62,630 | 407,221 | 8,293 | 1,879 | 480,023 | $0.0067 | $0.0139 | 96% |
| A8 lx improved + Rust embeddings | B | 1 | 2.0 | 62,640 | 62,638 | 2,710 | 430 | 128,418 | $0.0361 | $0.180 | 100% |
| H1 Shipped lx ask, default index, hooks on | B | 2 | 2.0 | 62,894 | 64,340 | 1,450 | 264 | 128,946 | $0.0317 | $0.176 | 100% |
| H3 Shipped lx ask, embeddings on, hooks on | B | 2 | 2.0 | 62,886 | 64,356 | 1,474 | 339 | 129,056 | $0.0325 | $0.177 | 100% |
| L3 lx + hybrid search | B | 2 | 2.0 | 63,025 | 64,442 | 1,422 | 260 | 129,149 | $0.0316 | $0.177 | 97% |
| C15 lx ask + token-goat read | B | 2 | 2.0 | 63,058 | 64,484 | 1,430 | 182 | 129,154 | $0.0309 | $0.176 | 100% |
| L2 lx + cards | B | 2 | 2.0 | 63,031 | 64,452 | 1,424 | 267 | 129,174 | $0.0317 | $0.177 | 100% |
| S3 Shipped: lx ask, embeddings on | B | 3 | 2.0 | 63,023 | 64,952 | 967 | 263 | 129,205 | $0.0306 | $0.176 | 100% |
| L4 lx ask, one call | B | 2 | 2.0 | 63,023 | 64,449 | 1,430 | 310 | 129,212 | $0.0322 | $0.177 | 100% |
| R1 lx shipped + Rust embeddings | B | 2 | 2.0 | 63,015 | 64,372 | 1,362 | 483 | 129,232 | $0.0337 | $0.179 | 100% |
| C2 lx + Rust embeddings + token-goat | B | 2 | 2.0 | 63,100 | 64,458 | 1,362 | 320 | 129,239 | $0.0321 | $0.177 | 100% |
| C12 lx about + hybrid | B | 2 | 2.0 | 63,072 | 64,482 | 1,414 | 364 | 129,331 | $0.0327 | $0.178 | 100% |
| C11 lx about + Rust cards | B | 2 | 2.0 | 63,073 | 64,514 | 1,446 | 420 | 129,452 | $0.0333 | $0.178 | 100% |
| S4 Shipped: all commands, embeddings on | B | 3 | 2.0 | 63,174 | 65,135 | 984 | 265 | 129,558 | $0.0308 | $0.176 | 100% |
| C13 lx, every new command | B | 2 | 2.0 | 63,231 | 64,657 | 1,430 | 309 | 129,627 | $0.0322 | $0.178 | 100% |
| C14 lx, every new command + token-goat | B | 2 | 2.0 | 63,316 | 63,314 | 2,882 | 307 | 129,820 | $0.0356 | $0.181 | 100% |
| H5 token-goat commands, hooks on | B | 2 | 2.0 | 62,863 | 62,861 | 4,953 | 424 | 131,102 | $0.0418 | $0.186 | 100% |
| C6 token-goat + graphify + semble | B | 2 | 2.5 | 63,079 | 99,420 | 2,700 | 456 | 165,656 | $0.0438 | $0.189 | 100% |
| A5 token-goat alone | B | 3 | 2.7 | 62,980 | 109,943 | 3,779 | 583 | 177,285 | $0.0499 | $0.195 | 100% |
| A6 token-goat + lx improved | B | 1 | 3.0 | 62,732 | 128,176 | 3,072 | 271 | 194,251 | $0.0486 | $0.193 | 100% |
| H9 Round 1 lx, hooks on | B | 2 | 3.0 | 62,899 | 129,907 | 1,764 | 280 | 194,850 | $0.0458 | $0.190 | 100% |
| HB Shipped lx ask through Bash, hooks on | B | 2 | 3.0 | 62,902 | 130,062 | 1,850 | 278 | 195,092 | $0.0460 | $0.191 | 100% |
| C9 lx finds, token-goat reads | B | 2 | 3.0 | 63,071 | 130,251 | 1,800 | 390 | 195,512 | $0.0471 | $0.192 | 100% |
| S1 Shipped: lx ask, default index | B | 6 | 3.0 | 63,106 | 130,003 | 2,341 | 314 | 195,764 | $0.0476 | $0.193 | 100% |
| SB Shipped lx ask through Bash, hooks off | B | 2 | 3.0 | 63,189 | 130,636 | 1,860 | 262 | 195,948 | $0.0460 | $0.191 | 100% |
| S2 Shipped: all commands, default index | B | 3 | 3.0 | 63,182 | 131,103 | 1,368 | 300 | 195,954 | $0.0453 | $0.191 | 100% |
| C3 lx shipped + token-goat | B | 2 | 3.0 | 63,121 | 130,351 | 2,157 | 434 | 196,062 | $0.0484 | $0.194 | 100% |
| N2 lx, every new command, no Rust embeddings | B | 2 | 3.0 | 63,182 | 129,228 | 3,320 | 425 | 196,154 | $0.0510 | $0.196 | 100% |
| C7 token-goat + semble | B | 2 | 3.0 | 63,045 | 130,050 | 5,106 | 531 | 198,733 | $0.0567 | $0.202 | 100% |
| C8 token-goat + graphify | B | 2 | 3.0 | 63,034 | 134,227 | 3,363 | 406 | 201,030 | $0.0519 | $0.197 | 100% |
| A9 lx, as shipped after round 1 | B | 3 | 3.3 | 62,916 | 151,400 | 2,323 | 321 | 216,959 | $0.0519 | $0.197 | 100% |
| L1 lx + about | B | 2 | 3.5 | 63,083 | 163,347 | 1,820 | 280 | 228,529 | $0.0526 | $0.198 | 100% |
| A7 lx, improved | B | 1 | 4.0 | 62,647 | 193,715 | 3,503 | 415 | 260,280 | $0.0642 | $0.208 | 100% |
| C10 lx about + cards | B | 2 | 4.0 | 63,078 | 196,714 | 2,072 | 366 | 262,230 | $0.0608 | $0.206 | 100% |
| A1 graphify + semble | B | 1 | 4.0 | 62,633 | 197,221 | 5,763 | 456 | 266,073 | $0.0709 | $0.215 | 100% |
| A2 Rust index alone, embeddings on | B | 1 | 4.0 | 62,632 | 197,316 | 6,237 | 1,003 | 267,188 | $0.0776 | $0.222 | 100% |
| H0 No index, hooks on | B | 2 | 4.0 | 62,712 | 202,058 | 11,334 | 2,094 | 278,198 | $0.102 | $0.246 | 97% |
| A0 No index | B | 3 | 4.0 | 62,729 | 203,924 | 11,872 | 2,023 | 280,547 | $0.103 | $0.248 | 100% |
| N1 lx ask, no Rust embeddings | B | 2 | 4.5 | 63,031 | 230,031 | 2,362 | 606 | 296,030 | $0.0706 | $0.216 | 100% |
| C1 Rust index + token-goat + graphify + semble | B | 2 | 4.5 | 63,157 | 239,939 | 9,282 | 1,222 | 313,600 | $0.0960 | $0.241 | 99% |
| C4 Rust index + token-goat | B | 2 | 5.5 | 63,078 | 314,357 | 14,724 | 1,316 | 393,476 | $0.125 | $0.271 | 99% |
| C5 Rust index + graphify + semble | B | 2 | 6.0 | 63,072 | 349,728 | 13,704 | 1,226 | 427,730 | $0.129 | $0.274 | 99% |
| C15 lx ask + token-goat read | A | 1 | 2.0 | 63,052 | 63,050 | 3,307 | 5,377 | 134,786 | $0.149 | $0.452 | 100% |
| SB Shipped lx ask through Bash, hooks off | A | 1 | 2.0 | 63,183 | 63,181 | 3,127 | 6,683 | 136,174 | $0.175 | $0.478 | 100% |
| C14 lx, every new command + token-goat | A | 1 | 2.0 | 63,310 | 63,308 | 3,051 | 7,043 | 136,712 | $0.181 | $0.485 | 100% |
| N1 lx ask, no Rust embeddings | A | 1 | 3.0 | 63,025 | 129,379 | 4,647 | 1,429 | 198,480 | $0.0903 | $0.393 | 100% |
| C13 lx, every new command | A | 1 | 3.0 | 63,225 | 129,488 | 4,347 | 1,424 | 198,484 | $0.0888 | $0.392 | 100% |
| H1 Shipped lx ask, default index, hooks on | A | 1 | 3.0 | 62,888 | 128,789 | 4,758 | 2,077 | 198,512 | $0.104 | $0.406 | 100% |
| C10 lx about + cards | A | 1 | 3.0 | 63,072 | 129,392 | 4,817 | 1,532 | 198,813 | $0.0932 | $0.396 | 100% |
| S4 Shipped: all commands, embeddings on | A | 1 | 3.0 | 63,168 | 129,712 | 4,723 | 1,457 | 199,060 | $0.0913 | $0.395 | 100% |
| C11 lx about + Rust cards | A | 1 | 3.0 | 63,067 | 129,513 | 4,660 | 1,926 | 199,166 | $0.100 | $0.403 | 100% |
| S2 Shipped: all commands, default index | A | 1 | 3.0 | 63,176 | 129,621 | 4,770 | 1,662 | 199,229 | $0.0957 | $0.399 | 100% |
| S3 Shipped: lx ask, embeddings on | A | 1 | 3.0 | 63,017 | 129,481 | 5,054 | 1,719 | 199,271 | $0.0982 | $0.401 | 100% |
| H3 Shipped lx ask, embeddings on, hooks on | A | 1 | 3.0 | 62,880 | 129,100 | 5,333 | 2,044 | 199,357 | $0.106 | $0.408 | 100% |
| N2 lx, every new command, no Rust embeddings | A | 1 | 3.0 | 63,176 | 129,663 | 4,969 | 1,766 | 199,574 | $0.0987 | $0.402 | 100% |
| A8 lx improved + Rust embeddings | A | 1 | 3.0 | 62,634 | 128,573 | 6,307 | 2,271 | 199,785 | $0.115 | $0.416 | 100% |
| R1 lx shipped + Rust embeddings | A | 1 | 3.0 | 63,009 | 129,444 | 5,229 | 2,607 | 200,289 | $0.117 | $0.419 | 100% |
| S1 Shipped: lx ask, default index | A | 1 | 3.0 | 63,025 | 129,338 | 5,695 | 2,570 | 200,628 | $0.118 | $0.421 | 100% |
| C12 lx about + hybrid | A | 1 | 3.0 | 63,066 | 129,377 | 5,645 | 2,558 | 200,646 | $0.118 | $0.421 | 100% |
| L4 lx ask, one call | A | 1 | 3.0 | 63,017 | 129,347 | 5,608 | 2,931 | 200,903 | $0.125 | $0.428 | 100% |
| L2 lx + cards | A | 1 | 3.0 | 63,025 | 129,364 | 5,952 | 2,920 | 201,261 | $0.127 | $0.429 | 100% |
| L3 lx + hybrid search | A | 1 | 3.0 | 63,019 | 129,503 | 6,516 | 3,854 | 202,892 | $0.148 | $0.451 | 100% |
| C9 lx finds, token-goat reads | A | 1 | 3.0 | 63,065 | 129,500 | 5,556 | 5,076 | 203,197 | $0.168 | $0.471 | 100% |
| H9 Round 1 lx, hooks on | A | 1 | 3.0 | 62,893 | 129,022 | 7,303 | 6,012 | 205,230 | $0.195 | $0.497 | 100% |
| C8 token-goat + graphify | A | 1 | 3.0 | 63,028 | 131,409 | 8,756 | 4,820 | 208,013 | $0.179 | $0.482 | 100% |
| C2 lx + Rust embeddings + token-goat | A | 1 | 3.0 | 63,094 | 129,476 | 10,076 | 7,325 | 209,971 | $0.235 | $0.538 | 100% |
| H5 token-goat commands, hooks on | A | 1 | 3.0 | 62,857 | 131,114 | 18,329 | 12,204 | 224,504 | $0.375 | $0.676 | 100% |
| HB Shipped lx ask through Bash, hooks on | A | 1 | 4.0 | 62,896 | 196,091 | 6,326 | 2,305 | 267,618 | $0.130 | $0.431 | 100% |
| A9 lx, as shipped after round 1 | A | 2 | 4.0 | 62,850 | 197,928 | 7,034 | 3,828 | 271,640 | $0.164 | $0.466 | 100% |
| L1 lx + about | A | 1 | 4.0 | 63,077 | 198,984 | 7,597 | 3,695 | 273,353 | $0.164 | $0.467 | 100% |
| C3 lx shipped + token-goat | A | 1 | 4.0 | 63,115 | 198,803 | 8,848 | 4,091 | 274,857 | $0.178 | $0.481 | 100% |
| C4 Rust index + token-goat | A | 1 | 4.0 | 63,072 | 201,694 | 8,708 | 2,978 | 276,452 | $0.156 | $0.459 | 100% |
| C7 token-goat + semble | A | 1 | 4.0 | 63,039 | 200,306 | 17,224 | 9,400 | 289,969 | $0.327 | $0.629 | 100% |
| H0 No index, hooks on | A | 1 | 4.0 | 62,706 | 202,997 | 17,143 | 10,093 | 292,939 | $0.341 | $0.642 | 100% |
| A2 Rust index alone, embeddings on | A | 1 | 4.0 | 62,626 | 199,867 | 17,805 | 16,699 | 296,997 | $0.476 | $0.776 | 100% |
| A0 No index | A | 3 | 4.0 | 62,774 | 205,715 | 20,002 | 9,468 | 297,959 | $0.343 | $0.644 | 100% |
| A5 token-goat alone | A | 1 | 4.0 | 62,633 | 206,021 | 20,443 | 13,690 | 302,787 | $0.430 | $0.730 | 100% |
| A7 lx, improved | A | 1 | 5.0 | 62,641 | 264,473 | 6,352 | 1,508 | 334,974 | $0.127 | $0.428 | 100% |
| A6 token-goat + lx improved | A | 1 | 5.0 | 62,726 | 268,502 | 8,183 | 3,778 | 343,189 | $0.183 | $0.484 | 100% |
| C6 token-goat + graphify + semble | A | 1 | 5.0 | 63,073 | 273,393 | 12,147 | 1,855 | 350,468 | $0.165 | $0.468 | 100% |
| C1 Rust index + token-goat + graphify + semble | A | 1 | 5.0 | 63,151 | 273,848 | 12,203 | 3,590 | 352,792 | $0.200 | $0.503 | 100% |
| C5 Rust index + graphify + semble | A | 1 | 5.0 | 63,066 | 276,864 | 13,366 | 2,938 | 356,234 | $0.194 | $0.496 | 100% |
| A1 graphify + semble | A | 1 | 7.0 | 62,627 | 421,083 | 14,178 | 2,486 | 500,374 | $0.217 | $0.518 | 100% |

## Three subagents

Three subagents started together, one question each. Cheapest tier only, nobody reviews the answers. Fewest tokens first within each tier.

| Method | Tier | Runs | Turns | Start-up | Re-read | Added | Output | Total tokens | Cost warm | Cost cold | Correct |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| H3 Shipped lx ask, embeddings on, hooks on | C | 1 | 6.0 | 187,527 | 187,521 | 8,298 | 401 | 383,747 | $0.0050 | $0.0266 | 100% |
| H1 Shipped lx ask, default index, hooks on | C | 1 | 6.0 | 187,551 | 187,545 | 8,199 | 801 | 384,096 | $0.0052 | $0.0267 | 100% |
| HB Shipped lx ask through Bash, hooks on | C | 1 | 6.0 | 187,575 | 187,569 | 8,382 | 792 | 384,318 | $0.0052 | $0.0268 | 100% |
| N1 lx ask, no Rust embeddings | C | 1 | 6.0 | 187,962 | 187,956 | 8,380 | 676 | 384,974 | $0.0051 | $0.0268 | 100% |
| S3 Shipped: lx ask, embeddings on | C | 2 | 6.0 | 188,163 | 188,157 | 8,494 | 624 | 385,437 | $0.0051 | $0.0268 | 100% |
| S1 Shipped: lx ask, default index | C | 2 | 6.0 | 188,187 | 188,181 | 8,356 | 981 | 385,704 | $0.0053 | $0.0269 | 100% |
| C12 lx about + hybrid | C | 1 | 6.0 | 188,085 | 188,079 | 8,017 | 1,847 | 386,028 | $0.0057 | $0.0273 | 100% |
| C15 lx ask + token-goat read | C | 1 | 6.0 | 188,043 | 188,037 | 8,696 | 1,361 | 386,137 | $0.0055 | $0.0272 | 100% |
| H5 token-goat commands, hooks on | C | 1 | 6.0 | 187,458 | 187,452 | 10,126 | 1,390 | 386,426 | $0.0057 | $0.0273 | 100% |
| S4 Shipped: all commands, embeddings on | C | 1 | 6.0 | 188,391 | 188,385 | 8,835 | 1,061 | 386,672 | $0.0054 | $0.0271 | 100% |
| N2 lx, every new command, no Rust embeddings | C | 1 | 6.0 | 188,415 | 188,409 | 8,379 | 1,491 | 386,694 | $0.0056 | $0.0272 | 100% |
| SB Shipped lx ask through Bash, hooks off | C | 1 | 6.0 | 188,775 | 188,769 | 8,417 | 918 | 386,879 | $0.0053 | $0.0270 | 100% |
| C13 lx, every new command | C | 1 | 6.0 | 188,562 | 188,556 | 8,496 | 1,272 | 386,886 | $0.0055 | $0.0272 | 100% |
| C14 lx, every new command + token-goat | C | 1 | 6.0 | 188,817 | 188,811 | 8,272 | 1,072 | 386,972 | $0.0053 | $0.0271 | 100% |
| A5 token-goat alone | C | 2 | 6.5 | 187,552 | 220,908 | 10,263 | 942 | 419,665 | $0.0058 | $0.0274 | 100% |
| A4 lx first version + Rust embeddings | C | 1 | 7.0 | 186,783 | 251,676 | 8,890 | 838 | 448,187 | $0.0059 | $0.0274 | 92% |
| A8 lx improved + Rust embeddings | C | 1 | 7.0 | 186,789 | 251,776 | 9,097 | 973 | 448,635 | $0.0060 | $0.0275 | 100% |
| L3 lx + hybrid search | C | 1 | 7.0 | 187,944 | 253,465 | 8,413 | 1,419 | 451,241 | $0.0062 | $0.0278 | 100% |
| C11 lx about + Rust cards | C | 1 | 7.0 | 188,088 | 253,508 | 8,537 | 1,623 | 451,756 | $0.0063 | $0.0279 | 100% |
| C10 lx about + cards | C | 1 | 7.0 | 188,103 | 253,521 | 8,530 | 1,817 | 451,971 | $0.0064 | $0.0280 | 100% |
| L4 lx ask, one call | C | 1 | 7.0 | 187,938 | 253,731 | 9,470 | 875 | 452,014 | $0.0060 | $0.0277 | 100% |
| A6 token-goat + lx improved | C | 1 | 7.0 | 187,065 | 254,212 | 10,222 | 815 | 452,314 | $0.0061 | $0.0276 | 100% |
| S2 Shipped: all commands, default index | C | 1 | 7.0 | 188,415 | 254,213 | 9,100 | 844 | 452,572 | $0.0060 | $0.0277 | 100% |
| C9 lx finds, token-goat reads | C | 1 | 7.0 | 188,082 | 253,405 | 9,184 | 1,981 | 452,652 | $0.0066 | $0.0282 | 100% |
| C3 lx shipped + token-goat | C | 1 | 7.0 | 188,232 | 253,898 | 9,294 | 1,579 | 453,003 | $0.0064 | $0.0280 | 100% |
| C2 lx + Rust embeddings + token-goat | C | 1 | 7.0 | 188,169 | 255,464 | 10,678 | 1,134 | 455,445 | $0.0063 | $0.0280 | 100% |
| C8 token-goat + graphify | C | 1 | 7.0 | 187,971 | 255,096 | 10,818 | 1,617 | 455,502 | $0.0066 | $0.0282 | 100% |
| C6 token-goat + graphify + semble | C | 1 | 7.0 | 188,106 | 255,325 | 11,952 | 984 | 456,367 | $0.0064 | $0.0281 | 100% |
| A9 lx, as shipped after round 1 | C | 3 | 7.7 | 187,766 | 297,393 | 9,487 | 1,327 | 495,973 | $0.0067 | $0.0283 | 100% |
| A7 lx, improved | C | 1 | 8.0 | 186,810 | 316,948 | 9,051 | 911 | 513,720 | $0.0066 | $0.0281 | 100% |
| H9 Round 1 lx, hooks on | C | 1 | 8.0 | 187,566 | 318,297 | 8,707 | 739 | 515,309 | $0.0065 | $0.0281 | 100% |
| L2 lx + cards | C | 1 | 8.0 | 187,962 | 319,227 | 9,140 | 1,256 | 517,585 | $0.0068 | $0.0285 | 100% |
| R1 lx shipped + Rust embeddings | C | 1 | 8.0 | 187,914 | 318,885 | 9,230 | 1,628 | 517,657 | $0.0070 | $0.0286 | 100% |
| A2 Rust index alone, embeddings on | C | 1 | 8.0 | 186,765 | 318,893 | 11,496 | 2,396 | 519,550 | $0.0077 | $0.0292 | 100% |
| C7 token-goat + semble | C | 1 | 8.0 | 188,004 | 320,771 | 11,222 | 1,587 | 521,584 | $0.0073 | $0.0289 | 100% |
| C4 Rust index + token-goat | C | 1 | 8.0 | 188,103 | 322,610 | 14,328 | 2,251 | 527,292 | $0.0080 | $0.0297 | 100% |
| C5 Rust index + graphify + semble | C | 1 | 8.0 | 188,085 | 320,367 | 27,514 | 2,453 | 538,419 | $0.0098 | $0.0314 | 100% |
| A0 No index | C | 3 | 8.7 | 187,206 | 366,188 | 18,238 | 2,689 | 574,320 | $0.0092 | $0.0307 | 81% |
| L1 lx + about | C | 1 | 9.0 | 188,118 | 384,402 | 10,275 | 1,889 | 584,684 | $0.0080 | $0.0296 | 100% |
| H0 No index, hooks on | C | 1 | 9.0 | 187,005 | 383,450 | 17,965 | 3,858 | 592,278 | $0.0099 | $0.0314 | 89% |
| C1 Rust index + token-goat + graphify + semble | C | 1 | 9.0 | 188,340 | 388,808 | 14,829 | 2,473 | 594,450 | $0.0089 | $0.0305 | 100% |
| A3 lx, first version | C | 1 | 10.0 | 186,768 | 448,933 | 10,129 | 1,142 | 646,972 | $0.0082 | $0.0297 | 100% |
| A1 graphify + semble | C | 1 | 13.0 | 186,768 | 649,935 | 14,019 | 1,072 | 851,794 | $0.0107 | $0.0321 | 92% |

## Orchestrator + subagents

A top-tier orchestrator writes three briefs, three subagents answer, and the orchestrator checks the answers with the method's own tools and corrects them. The tier shown is the subagents' tier. Fewest tokens first within each tier.

| Method | Tier | Runs | Turns | Start-up | Re-read | Added | Output | Total tokens | Cost warm | Cost cold | Correct |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| C13 lx, every new command | C | 1 | 9.0 | 314,405 | 252,352 | 12,072 | 19,462 | 598,291 | $0.415 | $1.04 | 100% |
| S2 Shipped: all commands, default index | C | 1 | 10.0 | 314,228 | 318,889 | 13,650 | 8,703 | 655,470 | $0.244 | $0.868 | 100% |
| S1 Shipped: lx ask, default index | C | 3 | 10.7 | 314,318 | 362,534 | 14,142 | 9,649 | 700,644 | $0.242 | $0.866 | 100% |
| A8 lx improved + Rust embeddings | C | 1 | 11.0 | 312,041 | 381,303 | 15,521 | 9,343 | 718,208 | $0.247 | $0.868 | 100% |
| C15 lx ask + token-goat read | C | 1 | 11.0 | 313,716 | 384,593 | 13,520 | 8,498 | 720,327 | $0.215 | $0.838 | 100% |
| N1 lx ask, no Rust embeddings | C | 1 | 11.0 | 313,617 | 384,155 | 14,522 | 8,651 | 720,945 | $0.226 | $0.850 | 100% |
| S4 Shipped: all commands, embeddings on | C | 1 | 11.0 | 314,177 | 384,948 | 14,353 | 8,681 | 722,159 | $0.220 | $0.844 | 100% |
| C12 lx about + hybrid | C | 1 | 11.0 | 313,769 | 383,676 | 15,397 | 10,426 | 723,268 | $0.264 | $0.887 | 100% |
| L4 lx ask, one call | C | 1 | 11.0 | 313,573 | 384,098 | 16,277 | 9,822 | 723,770 | $0.260 | $0.883 | 100% |
| S3 Shipped: lx ask, embeddings on | C | 3 | 11.0 | 314,274 | 384,440 | 16,664 | 9,520 | 724,898 | $0.238 | $0.862 | 100% |
| H5 token-goat commands, hooks on | C | 2 | 11.0 | 312,935 | 385,745 | 18,421 | 9,049 | 726,150 | $0.242 | $0.865 | 100% |
| H1 Shipped lx ask, default index, hooks on | C | 2 | 11.5 | 313,069 | 416,266 | 15,068 | 8,823 | 753,226 | $0.235 | $0.858 | 100% |
| A5 token-goat alone | C | 3 | 11.7 | 313,701 | 429,840 | 19,682 | 10,990 | 774,213 | $0.280 | $0.903 | 100% |
| C9 lx finds, token-goat reads | C | 1 | 12.0 | 313,765 | 449,529 | 16,302 | 7,638 | 787,234 | $0.218 | $0.842 | 100% |
| N2 lx, every new command, no Rust embeddings | C | 1 | 12.0 | 314,221 | 450,351 | 15,050 | 8,972 | 788,594 | $0.230 | $0.854 | 100% |
| L3 lx + hybrid search | C | 1 | 12.0 | 313,581 | 449,020 | 15,705 | 10,389 | 788,695 | $0.249 | $0.872 | 100% |
| A9 lx, as shipped after round 1 | C | 3 | 12.0 | 313,494 | 449,364 | 16,019 | 10,021 | 788,899 | $0.263 | $0.886 | 100% |
| C11 lx about + Rust cards | C | 1 | 12.0 | 313,773 | 449,657 | 16,172 | 9,661 | 789,263 | $0.246 | $0.870 | 100% |
| A4 lx first version + Rust embeddings | C | 1 | 12.0 | 312,033 | 446,096 | 22,319 | 9,746 | 790,194 | $0.255 | $0.876 | 100% |
| C2 lx + Rust embeddings + token-goat | C | 1 | 12.0 | 313,881 | 451,961 | 17,831 | 8,795 | 792,468 | $0.237 | $0.860 | 100% |
| C14 lx, every new command + token-goat | C | 1 | 12.0 | 314,745 | 452,263 | 19,414 | 7,533 | 793,955 | $0.227 | $0.852 | 100% |
| C7 token-goat + semble | C | 1 | 12.0 | 313,661 | 453,165 | 17,244 | 12,043 | 796,113 | $0.267 | $0.890 | 100% |
| C8 token-goat + graphify | C | 1 | 12.0 | 313,617 | 455,629 | 20,362 | 9,825 | 799,433 | $0.259 | $0.883 | 100% |
| C6 token-goat + graphify + semble | C | 1 | 12.0 | 313,806 | 455,469 | 20,609 | 10,640 | 800,524 | $0.281 | $0.905 | 100% |
| L1 lx + about | C | 1 | 13.0 | 313,813 | 515,064 | 14,569 | 8,874 | 852,320 | $0.209 | $0.832 | 100% |
| A6 token-goat + lx improved | C | 1 | 13.0 | 312,409 | 516,538 | 16,609 | 8,083 | 853,639 | $0.243 | $0.864 | 100% |
| R1 lx shipped + Rust embeddings | C | 1 | 13.0 | 313,541 | 514,527 | 16,491 | 10,571 | 855,130 | $0.256 | $0.879 | 100% |
| C3 lx shipped + token-goat | C | 1 | 13.0 | 313,965 | 515,767 | 18,858 | 11,431 | 860,021 | $0.297 | $0.921 | 100% |
| A7 lx, improved | C | 1 | 14.0 | 312,069 | 579,796 | 15,860 | 9,114 | 916,839 | $0.266 | $0.887 | 100% |
| A3 lx, first version | C | 1 | 14.0 | 312,013 | 577,637 | 15,491 | 11,921 | 917,062 | $0.288 | $0.910 | 100% |
| C10 lx about + cards | C | 1 | 14.0 | 313,793 | 580,864 | 16,057 | 9,149 | 919,863 | $0.236 | $0.860 | 100% |
| L2 lx + cards | C | 1 | 14.0 | 313,605 | 582,323 | 16,539 | 9,772 | 922,239 | $0.255 | $0.879 | 100% |
| A2 Rust index alone, embeddings on | C | 1 | 15.0 | 312,009 | 650,852 | 41,109 | 15,483 | 1,019,453 | $0.398 | $1.02 | 100% |
| C1 Rust index + token-goat + graphify + semble | C | 1 | 15.0 | 314,109 | 661,188 | 45,357 | 8,230 | 1,028,884 | $0.262 | $0.885 | 100% |
| C5 Rust index + graphify + semble | C | 1 | 16.0 | 313,769 | 723,143 | 27,494 | 12,134 | 1,076,540 | $0.325 | $0.949 | 100% |
| C4 Rust index + token-goat | C | 1 | 16.0 | 313,793 | 729,036 | 41,229 | 9,074 | 1,093,132 | $0.275 | $0.899 | 100% |
| A0 No index | C | 3 | 17.3 | 312,646 | 818,431 | 45,460 | 23,118 | 1,199,654 | $0.572 | $1.19 | 100% |
| H0 No index, hooks on | C | 2 | 18.0 | 312,323 | 868,185 | 47,536 | 24,780 | 1,252,824 | $0.574 | $1.20 | 100% |
| A1 graphify + semble | C | 3 | 21.7 | 313,705 | 1,098,959 | 26,751 | 15,215 | 1,454,631 | $0.363 | $0.986 | 100% |
| S1 Shipped: lx ask, default index | B | 2 | 10.0 | 314,678 | 322,717 | 8,075 | 8,122 | 653,592 | $0.314 | $1.35 | 100% |
| A9 lx, as shipped after round 1 | B | 2 | 11.0 | 314,686 | 388,258 | 10,792 | 8,714 | 722,449 | $0.348 | $1.39 | 100% |
| A5 token-goat alone | B | 2 | 11.0 | 314,542 | 389,338 | 12,031 | 9,518 | 725,430 | $0.365 | $1.40 | 100% |
| S3 Shipped: lx ask, embeddings on | B | 2 | 12.5 | 314,634 | 486,534 | 12,234 | 11,254 | 824,656 | $0.419 | $1.46 | 100% |
| A7 lx, improved | B | 1 | 14.0 | 312,078 | 580,884 | 15,611 | 9,676 | 918,249 | $0.418 | $1.45 | 100% |
| A1 graphify + semble | B | 2 | 14.5 | 314,518 | 622,484 | 18,636 | 10,583 | 966,221 | $0.456 | $1.49 | 100% |
| A6 token-goat + lx improved | B | 1 | 15.0 | 312,418 | 643,462 | 18,061 | 9,887 | 983,828 | $0.439 | $1.47 | 100% |
| A0 No index | B | 2 | 15.0 | 312,690 | 678,904 | 39,639 | 16,021 | 1,047,254 | $0.654 | $1.69 | 100% |
| A5 token-goat alone | A | 1 | 11.0 | 314,524 | 388,793 | 18,003 | 10,043 | 731,363 | $0.432 | $1.94 | 100% |
| S3 Shipped: lx ask, embeddings on | A | 1 | 11.0 | 314,616 | 384,217 | 19,319 | 18,538 | 736,690 | $0.607 | $2.12 | 100% |
| A9 lx, as shipped after round 1 | A | 1 | 13.0 | 314,668 | 517,693 | 19,548 | 17,492 | 869,401 | $0.614 | $2.12 | 100% |
| S1 Shipped: lx ask, default index | A | 1 | 13.0 | 314,660 | 518,578 | 39,699 | 30,352 | 903,289 | $0.972 | $2.48 | 100% |
| A7 lx, improved | A | 1 | 15.0 | 312,060 | 650,087 | 30,955 | 22,063 | 1,015,165 | $0.788 | $2.29 | 100% |
| A6 token-goat + lx improved | A | 1 | 15.0 | 312,400 | 655,881 | 34,025 | 25,528 | 1,027,834 | $0.874 | $2.37 | 100% |
| A0 No index | A | 1 | 15.0 | 311,424 | 659,009 | 55,212 | 31,159 | 1,056,804 | $1.09 | $2.59 | 100% |
| A1 graphify + semble | A | 1 | 19.0 | 314,500 | 923,199 | 38,532 | 10,858 | 1,287,089 | $0.657 | $2.17 | 100% |

## Orchestrated, reviewer reads only

Orchestrated, but the top-tier reviewer may not look anything up: it sees the three questions and the three answers, with no tool policy and no repository path. Fewest tokens first within each tier.

| Method | Tier | Runs | Turns | Start-up | Re-read | Added | Output | Total tokens | Cost warm | Cost cold | Correct |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| S3 Shipped: lx ask, embeddings on | C | 2 | 8.5 | 314,270 | 222,087 | 9,168 | 8,094 | 553,620 | $0.166 | $0.789 | 100% |
| S1 Shipped: lx ask, default index | C | 2 | 8.0 | 314,310 | 189,061 | 8,438 | 47,173 | 558,982 | $0.951 | $1.57 | 100% |
| A0 No index | C | 2 | 12.0 | 313,746 | 454,348 | 19,816 | 10,684 | 798,594 | $0.174 | $0.797 | 96% |

## Orchestrated, mid-tier reviewer

Orchestrated, with the mid-tier model as reviewer, allowed the method's own tools. Fewest tokens first within each tier.

| Method | Tier | Runs | Turns | Start-up | Re-read | Added | Output | Total tokens | Cost warm | Cost cold | Correct |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| S3 Shipped: lx ask, embeddings on | C | 2 | 10.5 | 314,633 | 352,482 | 12,064 | 7,664 | 686,844 | $0.191 | $0.656 | 100% |
| S1 Shipped: lx ask, default index | C | 2 | 12.0 | 314,677 | 452,494 | 12,938 | 7,450 | 787,558 | $0.183 | $0.648 | 100% |
| A0 No index | C | 2 | 17.0 | 313,935 | 796,848 | 33,620 | 11,747 | 1,156,150 | $0.244 | $0.709 | 100% |

## Orchestrated, cheapest reviewer

Orchestrated, with the cheapest model as reviewer, allowed the method's own tools. Fewest tokens first within each tier.

| Method | Tier | Runs | Turns | Start-up | Re-read | Added | Output | Total tokens | Cost warm | Cost cold | Correct |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| S3 Shipped: lx ask, embeddings on | C | 2 | 9.5 | 314,628 | 286,709 | 10,122 | 7,778 | 619,237 | $0.140 | $0.466 | 100% |
| S1 Shipped: lx ask, default index | C | 2 | 10.0 | 314,678 | 320,263 | 11,070 | 8,006 | 654,018 | $0.140 | $0.467 | 100% |
| A0 No index | C | 2 | 16.5 | 313,932 | 762,536 | 34,750 | 14,522 | 1,125,739 | $0.151 | $0.478 | 100% |

## Index build and size

86 Python files, 2.97 MB (graphify 0.9.61 source). Windows 11, 8 cores. Seconds are wall clock.

| Index stack | First build, s | Nothing changed, s | One file changed, s | Disk, MB |
|---|---:|---:|---:|---:|
| graphify + semble | 21 | 3.9 | 9.3 | 19.9 |
| Rust index alone, embeddings off | 7.3 | 0.2 | 2.7 | 5.6 |
| Rust index alone, embeddings on | 100 | 1.9 | 17.4 | 7.1 |
| Combined (lx default) | 12.1 | 2.1 | 6.7 | 26.2 |
| Combined, graph on demand | 7.4 | 1.9 | 2.8 | 17.6 |
| Combined + Rust embeddings | 130 | 8.1 | 28.9 | 27.8 |
| Combined + Rust embeddings, no semble | 143 | 5 | 32.2 | 15.9 |
| token-goat index --embed | 1230 | not measured | not measured | 29.4 |

## Methods

| Code | Method | What the agent was given |
|---|---|---|
| A0 | No index | The client's own search and read tools. The control every other method is compared with. |
| A1 | graphify + semble | The Python stack before the Rust index: the code graph for symbols, semble for meaning. |
| A2 | Rust index alone, embeddings on | codanna's own commands with its output unfiltered. |
| A3 | lx, first version | Rust symbols, graphify and semble behind one short command, as it stood when the test began. |
| A4 | lx first version + Rust embeddings | The first version, searching by meaning in the Rust index. |
| A7 | lx, improved | lx def adds the signature and lx find adds function names. |
| A8 | lx improved + Rust embeddings | The improved build, searching by meaning in the Rust index. |
| A5 | token-goat alone | token-goat's symbol, callers, semantic and read commands. |
| A6 | token-goat + lx improved | Both command sets, the agent chooses. |
| A9 | lx, as shipped after round 1 | Adds caller counts and headers, several names per call, and the line each function starts on. |
| R1 | lx shipped + Rust embeddings | The shipped build, searching by meaning in the Rust index. Round 1 measured this pairing only on the improved build. |
| L1 | lx + about | New command: one answer holds a symbol's definition, signature, doc line and callers. |
| L2 | lx + cards | Search hits come back as definitions: name, range, signature and doc line, so a hit needs no file opened to be understood. |
| L3 | lx + hybrid search | semble and the Rust index answer the same search together and their rankings are merged. |
| L4 | lx ask, one call | One command takes every question at once and answers each under its own header. Its search uses both engines, because this index has Rust embeddings. |
| N1 | lx ask, no Rust embeddings | The same one-call command on the default index, built without Rust embeddings: its search is semble alone. |
| N2 | lx, every new command, no Rust embeddings | The round 2 build with def, callers, about, find --hybrid and ask, on the default index built without Rust embeddings: its search is semble alone. |
| S1 | Shipped: lx ask, default index | The final build. Its search merges semble with the Rust index's name and doc search, which needs no embeddings. One-call command, default index. |
| S2 | Shipped: all commands, default index | The final build with every command the shipped rules teach (def, callers, about, find --hybrid, ask), on the default index. |
| S3 | Shipped: lx ask, embeddings on | The final build's one-call command on an index with Rust embeddings: its search merges three engines. |
| S4 | Shipped: all commands, embeddings on | The final build with every command the shipped rules teach, on an index with Rust embeddings. |
| C1 | Rust index + token-goat + graphify + semble | All four engines with their own commands, nothing wrapped. |
| C2 | lx + Rust embeddings + token-goat | The shipped lx searching the Rust index, with token-goat beside it. |
| C3 | lx shipped + token-goat | The shipped lx with token-goat beside it. |
| C4 | Rust index + token-goat | codanna's own commands with token-goat. |
| C5 | Rust index + graphify + semble | The three engines lx wraps, used directly. |
| C6 | token-goat + graphify + semble | token-goat with the Python stack. |
| C7 | token-goat + semble | token-goat with semble for search by meaning. |
| C8 | token-goat + graphify | token-goat with the code graph. |
| C9 | lx finds, token-goat reads | The shipped lx for every lookup; code is read one symbol at a time through token-goat, never with the Read tool. |
| C10 | lx about + cards | The two new answer shapes together. |
| C11 | lx about + Rust cards | about for symbols, card-style search in the Rust index. |
| C12 | lx about + hybrid | about for symbols, merged two-engine search. |
| C13 | lx, every new command | def, callers, about, cards, hybrid and ask all offered at once. |
| C14 | lx, every new command + token-goat | Everything lx offers and everything token-goat offers. |
| C15 | lx ask + token-goat read | One-call lookups through lx; code is read one symbol at a time through token-goat, never with the Read tool. |
| H0 | No index, hooks on | The same prompt as A0 while token-goat's hooks were installed: they run on every Grep, Glob and Read the agent makes, and may rewrite what comes back or refuse the call. |
| H9 | Round 1 lx, hooks on | The same prompt as A9 with the hooks installed. lx runs through PowerShell, which the hooks do not match, so they act only on any file the agent then reads. |
| H1 | Shipped lx ask, default index, hooks on | The same prompt as S1 with the hooks installed. |
| H3 | Shipped lx ask, embeddings on, hooks on | The same prompt as S3 with the hooks installed. |
| H5 | token-goat commands, hooks on | The same prompt as A5 with the hooks installed: token-goat's own commands, and its hooks as well. |
| SB | Shipped lx ask through Bash, hooks off | The one-call command sent through the client's Bash tool instead of PowerShell, with token-goat's hooks uninstalled. The control for HB. |
| HB | Shipped lx ask through Bash, hooks on | The same through Bash with the hooks installed. Bash is a tool the hooks do match. |

## How it was measured

- Repository: 86 Python files, 2.97 MB, one commit, read-only for every agent. Ground truth for the three questions comes from Python's syntax tree, and every answer is graded against it by a script.
- Tiers: C is the Haiku class at low effort, B the Sonnet class at medium effort, A the Opus class at maximum effort. The planning call always ran at Tier A, and so did the review, except in the reviewer variants described below. Model names are resolved at run time by alias.
- Tokens are the usage fields the API returned for each request, summed per agent. Start-up context is the whole prompt of an agent's first request; re-read is cache-read tokens on later requests; added is new context written on later requests (tool results and the agent's own earlier output).
- Cost uses list prices per million tokens: Tier A $4 in, $20 out, $0.20 cache read; Tier B $2, $10, $0.20; Tier C $0.10, $0.50. Cache writes are priced at 1.25 times input and the Tier C cache read at a tenth of input, which are the published multipliers rather than figures printed for these models. Warm and cold differ only in how the start-up context is priced: all read from cache, or all written.
- The orchestrator's planning prompt is the same for every method and tier, so it was run 3 times and its briefs reused; a run that reused them is charged the mean of the measured planning calls.
- Reviewer variants. In the standard orchestrated run the reviewer is the Tier A model with the method's own tools. Three variants swap only the reviewer: Tier A with no tools, Tier B with tools and Tier C with tools. The planning briefs and the subagents are the same.
- Hooks. The runs without token-goat's hooks come from the rounds made before the hooks were ever installed and from a round made after they were uninstalled again; the runs with them were made in between, in one round. The client applies a hook change at once, so no restart separates them. A hook run that let a call through is a row in an agent's transcript, which is how runs and rewritten calls are counted; a call the hook refused leaves no row, only an error in place of the result, and each of those is counted as one hook run and one refused call. Methods measured with the hooks on carry "hooks on" in their name and are never averaged with their twins.
- One round was stopped part-way and resumed, and the resumed round ran most of its agents a second time. Where an agent has two transcripts the later one is counted and the earlier one is left out, so no run is counted twice and no half-finished one is graded.
- Limits. One repository, one language, three easy questions, and between one and 6 runs per cell: a difference of one turn between two methods is inside the noise, and a single-run cell is an example, not an average. Every agent carried the test machine's client, skills and connectors, so the start-up figure belongs to that machine and is not a constant. Wall-clock seconds depend on what else the machine was doing: round 2 and part of round 4 ran while other jobs held every processor core, and a row can mix runs from several rounds. Read seconds as a rough guide and never across rounds; token counts do not depend on machine load.
- lx changed during the test and each row names the build it was measured with: first version, improved, shipped after round 1, the round 2 commands (about, cards, hybrid, ask), and the build that ships, whose merged search also asks the Rust index's name and doc search. Rows marked default index ran on a copy of the repository indexed without Rust embeddings.

## Orchestrated runs

- Across 70 orchestrated runs with the standard reviewer, the subagents handed back 3 wrong answers out of 210. The reviewer corrected 3 of them, left 0 wrong, and changed 0 right answers into wrong ones. 70 of the 70 runs ended fully correct.
- With Tier C, scout subagents (48 runs over 36 methods), the orchestrator's planning and review were 33% of the tokens and 97% of the cost of a run, cache warm. The three subagents together averaged 569k tokens and the review 210k. Against one agent of that tier answering all three questions with the same method, the orchestrated run used 4.5 times the tokens (from 2.9 to 6.1) and 95 times the money.
- With Tier B, coder subagents (14 runs over 8 methods), the orchestrator's planning and review were 33% of the tokens and 65% of the cost of a run, cache warm. The three subagents together averaged 574k tokens and the review 213k. Against one agent of that tier answering all three questions with the same method, the orchestrated run used 4.1 times the tokens (from 3.3 to 6.4) and 8 times the money.
- With Tier A, apex subagents (8 runs over 8 methods), the orchestrator's planning and review were 28% of the tokens and 38% of the cost of a run, cache warm. The three subagents together averaged 687k tokens and the review 198k. Against one agent of that tier answering all three questions with the same method, the orchestrated run used 3.2 times the tokens (from 2.4 to 4.5) and 5 times the money.
- Three subagents with nobody reviewing them, Tier C: 38 of 43 runs were fully correct. The same 36 methods on that tier with the standard review: 48 of 48.
- Top tier, no tools: 6 runs on 3 methods. A review cost $0.0265 to $1.21 (median $0.0315) against $0.0877 to $0.441 (median $0.107) over 9 runs of the standard reviewer on the same methods, cheaper on average on 2 of the 3, and wrote 695 to 59,621 output tokens (median 940) against a median of 2,856. It was handed 1 wrong answer, corrected 0 and broke 0 right ones. 5 of 6 runs ended fully correct.
- Mid tier with the method's tools: 6 runs on 3 methods. A review cost $0.0357 to $0.101 (median $0.0602) against $0.0877 to $0.441 (median $0.107) over 9 runs of the standard reviewer on the same methods, cheaper on average on 3 of the 3, and wrote 267 to 1,814 output tokens (median 608) against a median of 2,856. Its subagents handed it no wrong answer, so these runs price the reviewer and do not show whether it catches a mistake. 6 of 6 runs ended fully correct.
- Cheapest tier with the method's tools: 6 runs on 3 methods. A review cost $0.0020 to $0.0061 (median $0.0025) against $0.0877 to $0.441 (median $0.107) over 9 runs of the standard reviewer on the same methods, cheaper on average on 3 of the 3, and wrote 768 to 3,790 output tokens (median 936) against a median of 2,856. Its subagents handed it no wrong answer, so these runs price the reviewer and do not show whether it catches a mistake. 6 of 6 runs ended fully correct.

### Who used what inside an orchestrated run

Means over the runs of each method and subagent tier. Cost is with the start-up context cached.

| Method | Subagent tier | Runs | Plan tokens | Subagent tokens | Review tokens | Total tokens | Plan cost | Subagent cost | Review cost | Total cost | Orchestrator share of cost | Review turns | Seconds |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| A0 No index | C | 3 | 67,681 | 823,791 | 308,182 | 1,199,654 | $0.125 | $0.0129 | $0.434 | $0.572 | 98% | 4.0 | 192 |
| A1 graphify + semble | C | 3 | 67,430 | 1,151,459 | 235,742 | 1,454,631 | $0.120 | $0.0150 | $0.228 | $0.363 | 96% | 3.3 | 154 |
| A2 Rust index alone, embeddings on | C | 1 | 68,025 | 668,535 | 282,893 | 1,019,453 | $0.132 | $0.0112 | $0.254 | $0.398 | 97% | 4.0 | 139 |
| A3 lx, first version | C | 1 | 70,841 | 647,595 | 198,626 | 917,062 | $0.188 | $0.0085 | $0.0915 | $0.288 | 97% | 3.0 | 122 |
| A4 lx first version + Rust embeddings | C | 1 | 68,025 | 521,977 | 200,192 | 790,194 | $0.132 | $0.0078 | $0.115 | $0.255 | 97% | 3.0 | 93 |
| A7 lx, improved | C | 1 | 68,025 | 580,234 | 268,580 | 916,839 | $0.132 | $0.0074 | $0.127 | $0.266 | 97% | 4.0 | 107 |
| A8 lx improved + Rust embeddings | C | 1 | 68,025 | 450,242 | 199,941 | 718,208 | $0.132 | $0.0063 | $0.109 | $0.247 | 97% | 3.0 | 92 |
| A5 token-goat alone | C | 3 | 68,025 | 545,419 | 160,769 | 774,213 | $0.132 | $0.0075 | $0.140 | $0.280 | 97% | 2.3 | 110 |
| A6 token-goat + lx improved | C | 1 | 68,025 | 515,874 | 269,740 | 853,639 | $0.132 | $0.0069 | $0.104 | $0.243 | 97% | 4.0 | 106 |
| A9 lx, as shipped after round 1 | C | 3 | 68,025 | 518,531 | 202,343 | 788,899 | $0.132 | $0.0070 | $0.124 | $0.263 | 97% | 3.0 | 136 |
| R1 lx shipped + Rust embeddings | C | 1 | 68,025 | 585,447 | 201,658 | 855,130 | $0.132 | $0.0082 | $0.116 | $0.256 | 97% | 3.0 | 119 |
| L1 lx + about | C | 1 | 68,025 | 652,695 | 131,600 | 852,320 | $0.132 | $0.0086 | $0.0682 | $0.209 | 96% | 2.0 | 176 |
| L2 lx + cards | C | 1 | 68,025 | 586,482 | 267,732 | 922,239 | $0.132 | $0.0081 | $0.115 | $0.255 | 97% | 4.0 | 142 |
| L3 lx + hybrid search | C | 1 | 68,025 | 520,081 | 200,589 | 788,695 | $0.132 | $0.0075 | $0.110 | $0.249 | 97% | 3.0 | 115 |
| L4 lx ask, one call | C | 1 | 68,025 | 453,980 | 201,765 | 723,770 | $0.132 | $0.0063 | $0.122 | $0.260 | 98% | 3.0 | 174 |
| N1 lx ask, no Rust embeddings | C | 1 | 68,025 | 453,607 | 199,313 | 720,945 | $0.132 | $0.0063 | $0.0881 | $0.226 | 97% | 3.0 | 149 |
| N2 lx, every new command, no Rust embeddings | C | 1 | 68,025 | 520,972 | 199,597 | 788,594 | $0.132 | $0.0072 | $0.0908 | $0.230 | 97% | 3.0 | 140 |
| S1 Shipped: lx ask, default index | C | 3 | 68,025 | 454,423 | 178,195 | 700,644 | $0.132 | $0.0063 | $0.104 | $0.242 | 97% | 2.7 | 120 |
| S2 Shipped: all commands, default index | C | 1 | 68,025 | 386,457 | 200,988 | 655,470 | $0.132 | $0.0051 | $0.107 | $0.244 | 98% | 3.0 | 124 |
| S3 Shipped: lx ask, embeddings on | C | 3 | 68,025 | 478,854 | 178,019 | 724,898 | $0.132 | $0.0069 | $0.0993 | $0.238 | 97% | 2.7 | 114 |
| S4 Shipped: all commands, embeddings on | C | 1 | 68,025 | 455,111 | 199,023 | 722,159 | $0.132 | $0.0065 | $0.0816 | $0.220 | 97% | 3.0 | 107 |
| C1 Rust index + token-goat + graphify + semble | C | 1 | 68,025 | 684,354 | 276,505 | 1,028,884 | $0.132 | $0.0117 | $0.118 | $0.262 | 96% | 4.0 | 175 |
| C2 lx + Rust embeddings + token-goat | C | 1 | 68,025 | 523,958 | 200,485 | 792,468 | $0.132 | $0.0073 | $0.0973 | $0.237 | 97% | 3.0 | 104 |
| C3 lx shipped + token-goat | C | 1 | 68,025 | 586,231 | 205,765 | 860,021 | $0.132 | $0.0079 | $0.157 | $0.297 | 97% | 3.0 | 157 |
| C4 Rust index + token-goat | C | 1 | 68,025 | 746,554 | 278,553 | 1,093,132 | $0.132 | $0.0119 | $0.131 | $0.275 | 96% | 4.0 | 141 |
| C5 Rust index + graphify + semble | C | 1 | 68,025 | 730,196 | 278,319 | 1,076,540 | $0.132 | $0.0104 | $0.183 | $0.325 | 97% | 4.0 | 129 |
| C6 token-goat + graphify + semble | C | 1 | 68,025 | 527,430 | 205,069 | 800,524 | $0.132 | $0.0075 | $0.142 | $0.281 | 97% | 3.0 | 119 |
| C7 token-goat + semble | C | 1 | 68,025 | 593,508 | 134,580 | 796,113 | $0.132 | $0.0084 | $0.126 | $0.267 | 97% | 2.0 | 140 |
| C8 token-goat + graphify | C | 1 | 68,025 | 528,018 | 203,390 | 799,433 | $0.132 | $0.0076 | $0.120 | $0.259 | 97% | 3.0 | 110 |
| C9 lx finds, token-goat reads | C | 1 | 68,025 | 518,873 | 200,336 | 787,234 | $0.132 | $0.0069 | $0.0793 | $0.218 | 97% | 3.0 | 128 |
| C10 lx about + cards | C | 1 | 68,025 | 652,412 | 199,426 | 919,863 | $0.132 | $0.0086 | $0.0957 | $0.236 | 96% | 3.0 | 107 |
| C11 lx about + Rust cards | C | 1 | 68,025 | 520,776 | 200,462 | 789,263 | $0.132 | $0.0072 | $0.107 | $0.246 | 97% | 3.0 | 97 |
| C12 lx about + hybrid | C | 1 | 68,025 | 453,518 | 201,725 | 723,268 | $0.132 | $0.0064 | $0.126 | $0.264 | 98% | 3.0 | 100 |
| C13 lx, every new command | C | 1 | 68,025 | 388,022 | 142,244 | 598,291 | $0.132 | $0.0057 | $0.278 | $0.415 | 99% | 2.0 | 202 |
| C14 lx, every new command + token-goat | C | 1 | 68,025 | 522,719 | 203,211 | 793,955 | $0.132 | $0.0071 | $0.0884 | $0.227 | 97% | 3.0 | 140 |
| C15 lx ask + token-goat read | C | 1 | 68,025 | 520,698 | 131,604 | 720,327 | $0.132 | $0.0068 | $0.0757 | $0.215 | 97% | 2.0 | 96 |
| A0 No index | B | 2 | 68,025 | 591,939 | 387,290 | 1,047,254 | $0.132 | $0.177 | $0.344 | $0.654 | 73% | 5.0 | 179 |
| A1 graphify + semble | B | 2 | 68,025 | 690,111 | 208,085 | 966,221 | $0.132 | $0.172 | $0.152 | $0.456 | 62% | 3.0 | 138 |
| A7 lx, improved | B | 1 | 68,025 | 580,327 | 269,897 | 918,249 | $0.132 | $0.146 | $0.141 | $0.418 | 65% | 4.0 | 110 |
| A5 token-goat alone | B | 2 | 68,025 | 523,057 | 134,348 | 725,430 | $0.132 | $0.130 | $0.103 | $0.365 | 64% | 2.0 | 103 |
| A6 token-goat + lx improved | B | 1 | 68,025 | 713,331 | 202,472 | 983,828 | $0.132 | $0.181 | $0.126 | $0.439 | 59% | 3.0 | 111 |
| A9 lx, as shipped after round 1 | B | 2 | 68,025 | 486,571 | 167,853 | 722,449 | $0.132 | $0.117 | $0.0988 | $0.348 | 66% | 2.5 | 108 |
| S1 Shipped: lx ask, default index | B | 2 | 68,025 | 419,535 | 166,032 | 653,592 | $0.132 | $0.0998 | $0.0817 | $0.314 | 68% | 2.5 | 94 |
| S3 Shipped: lx ask, embeddings on | B | 2 | 68,025 | 586,549 | 170,082 | 824,656 | $0.132 | $0.143 | $0.144 | $0.419 | 66% | 2.5 | 134 |
| A0 No index | A | 1 | 68,025 | 685,644 | 303,135 | 1,056,804 | $0.132 | $0.483 | $0.479 | $1.09 | 56% | 4.0 | 334 |
| A1 graphify + semble | A | 1 | 68,025 | 1,008,005 | 211,059 | 1,287,089 | $0.132 | $0.399 | $0.127 | $0.657 | 39% | 3.0 | 269 |
| A7 lx, improved | A | 1 | 68,025 | 747,136 | 200,004 | 1,015,165 | $0.132 | $0.547 | $0.109 | $0.788 | 31% | 3.0 | 222 |
| A5 token-goat alone | A | 1 | 68,025 | 530,924 | 132,414 | 731,363 | $0.132 | $0.236 | $0.0640 | $0.432 | 45% | 2.0 | 182 |
| A6 token-goat + lx improved | A | 1 | 68,025 | 758,420 | 201,389 | 1,027,834 | $0.132 | $0.624 | $0.119 | $0.874 | 29% | 3.0 | 262 |
| A9 lx, as shipped after round 1 | A | 1 | 68,025 | 597,895 | 203,481 | 869,401 | $0.132 | $0.355 | $0.127 | $0.614 | 42% | 3.0 | 145 |
| S1 Shipped: lx ask, default index | A | 1 | 68,025 | 634,209 | 201,055 | 903,289 | $0.132 | $0.735 | $0.105 | $0.972 | 24% | 3.0 | 290 |
| S3 Shipped: lx ask, embeddings on | A | 1 | 68,025 | 535,611 | 133,054 | 736,690 | $0.132 | $0.380 | $0.0947 | $0.607 | 37% | 2.0 | 184 |

### What the review changed

Counted per question: three questions a run. An answer counts as wrong when it is not fully right. The two score columns are mean scores, where a partly right answer earns part credit.

| Method | Subagent tier | Runs | Subagents, mean score | After review, mean score | Wrong before review | Corrected | Still wrong | Right answers broken | Runs fully correct |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| A0 No index | C | 3 | 100% | 100% | 0 | 0 | 0 | 0 | 3 of 3 |
| A1 graphify + semble | C | 3 | 100% | 100% | 0 | 0 | 0 | 0 | 3 of 3 |
| A2 Rust index alone, embeddings on | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| A3 lx, first version | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| A4 lx first version + Rust embeddings | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| A7 lx, improved | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| A8 lx improved + Rust embeddings | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| A5 token-goat alone | C | 3 | 100% | 100% | 0 | 0 | 0 | 0 | 3 of 3 |
| A6 token-goat + lx improved | C | 1 | 92% | 100% | 1 | 1 | 0 | 0 | 1 of 1 |
| A9 lx, as shipped after round 1 | C | 3 | 100% | 100% | 0 | 0 | 0 | 0 | 3 of 3 |
| R1 lx shipped + Rust embeddings | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| L1 lx + about | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| L2 lx + cards | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| L3 lx + hybrid search | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| L4 lx ask, one call | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| N1 lx ask, no Rust embeddings | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| N2 lx, every new command, no Rust embeddings | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| S1 Shipped: lx ask, default index | C | 3 | 100% | 100% | 0 | 0 | 0 | 0 | 3 of 3 |
| S2 Shipped: all commands, default index | C | 1 | 92% | 100% | 1 | 1 | 0 | 0 | 1 of 1 |
| S3 Shipped: lx ask, embeddings on | C | 3 | 100% | 100% | 0 | 0 | 0 | 0 | 3 of 3 |
| S4 Shipped: all commands, embeddings on | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| C1 Rust index + token-goat + graphify + semble | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| C2 lx + Rust embeddings + token-goat | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| C3 lx shipped + token-goat | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| C4 Rust index + token-goat | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| C5 Rust index + graphify + semble | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| C6 token-goat + graphify + semble | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| C7 token-goat + semble | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| C8 token-goat + graphify | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| C9 lx finds, token-goat reads | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| C10 lx about + cards | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| C11 lx about + Rust cards | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| C12 lx about + hybrid | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| C13 lx, every new command | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| C14 lx, every new command + token-goat | C | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| C15 lx ask + token-goat read | C | 1 | 83% | 100% | 1 | 1 | 0 | 0 | 1 of 1 |
| A0 No index | B | 2 | 100% | 100% | 0 | 0 | 0 | 0 | 2 of 2 |
| A1 graphify + semble | B | 2 | 100% | 100% | 0 | 0 | 0 | 0 | 2 of 2 |
| A7 lx, improved | B | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| A5 token-goat alone | B | 2 | 100% | 100% | 0 | 0 | 0 | 0 | 2 of 2 |
| A6 token-goat + lx improved | B | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| A9 lx, as shipped after round 1 | B | 2 | 100% | 100% | 0 | 0 | 0 | 0 | 2 of 2 |
| S1 Shipped: lx ask, default index | B | 2 | 100% | 100% | 0 | 0 | 0 | 0 | 2 of 2 |
| S3 Shipped: lx ask, embeddings on | B | 2 | 100% | 100% | 0 | 0 | 0 | 0 | 2 of 2 |
| A0 No index | A | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| A1 graphify + semble | A | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| A7 lx, improved | A | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| A5 token-goat alone | A | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| A6 token-goat + lx improved | A | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| A9 lx, as shipped after round 1 | A | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| S1 Shipped: lx ask, default index | A | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |
| S3 Shipped: lx ask, embeddings on | A | 1 | 100% | 100% | 0 | 0 | 0 | 0 | 1 of 1 |

### One agent, three subagents, orchestrated: the same method and tier

| Method | Tier | One agent: tokens | cost | correct | Three subagents: tokens | cost | correct | Orchestrated: tokens | cost | correct | Orchestrated over one agent, tokens | cost |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| A0 No index | C | 364,508 | $0.0074 | 98% | 574,320 | $0.0092 | 81% | 1,199,654 | $0.572 | 100% | 3.3x | 77x |
| A1 graphify + semble | C | 480,023 | $0.0067 | 96% | 851,794 | $0.0107 | 92% | 1,454,631 | $0.363 | 100% | 3.0x | 54x |
| A2 Rust index alone, embeddings on | C | 351,694 | $0.0059 | 100% | 519,550 | $0.0077 | 100% | 1,019,453 | $0.398 | 100% | 2.9x | 67x |
| A3 lx, first version | C | 300,206 | $0.0045 | 100% | 646,972 | $0.0082 | 100% | 917,062 | $0.288 | 100% | 3.1x | 65x |
| A4 lx first version + Rust embeddings | C | 195,468 | $0.0028 | 100% | 448,187 | $0.0059 | 92% | 790,194 | $0.255 | 100% | 4.0x | 91x |
| A7 lx, improved | C | 228,834 | $0.0030 | 95% | 513,720 | $0.0066 | 100% | 916,839 | $0.266 | 100% | 4.0x | 89x |
| A8 lx improved + Rust embeddings | C | 162,695 | $0.0024 | 95% | 448,635 | $0.0060 | 100% | 718,208 | $0.247 | 100% | 4.4x | 102x |
| A5 token-goat alone | C | 177,144 | $0.0027 | 100% | 419,665 | $0.0058 | 100% | 774,213 | $0.280 | 100% | 4.4x | 103x |
| A6 token-goat + lx improved | C | 197,267 | $0.0029 | 99% | 452,314 | $0.0061 | 100% | 853,639 | $0.243 | 100% | 4.3x | 83x |
| A9 lx, as shipped after round 1 | C | 197,045 | $0.0028 | 99% | 495,973 | $0.0067 | 100% | 788,899 | $0.263 | 100% | 4.0x | 94x |
| R1 lx shipped + Rust embeddings | C | 175,536 | $0.0029 | 100% | 517,657 | $0.0070 | 100% | 855,130 | $0.256 | 100% | 4.9x | 88x |
| L1 lx + about | C | 219,739 | $0.0031 | 100% | 584,684 | $0.0080 | 100% | 852,320 | $0.209 | 100% | 3.9x | 67x |
| L2 lx + cards | C | 152,991 | $0.0024 | 100% | 517,585 | $0.0068 | 100% | 922,239 | $0.255 | 100% | 6.0x | 105x |
| L3 lx + hybrid search | C | 130,166 | $0.0021 | 100% | 451,241 | $0.0062 | 100% | 788,695 | $0.249 | 100% | 6.1x | 119x |
| L4 lx ask, one call | C | 130,091 | $0.0020 | 100% | 452,014 | $0.0060 | 100% | 723,770 | $0.260 | 100% | 5.6x | 130x |
| N1 lx ask, no Rust embeddings | C | 130,381 | $0.0022 | 100% | 384,974 | $0.0051 | 100% | 720,945 | $0.226 | 100% | 5.5x | 105x |
| N2 lx, every new command, no Rust embeddings | C | 130,554 | $0.0021 | 100% | 386,694 | $0.0056 | 100% | 788,594 | $0.230 | 100% | 6.0x | 107x |
| S1 Shipped: lx ask, default index | C | 129,686 | $0.0018 | 97% | 385,704 | $0.0053 | 100% | 700,644 | $0.242 | 100% | 5.4x | 134x |
| S2 Shipped: all commands, default index | C | 130,309 | $0.0020 | 100% | 452,572 | $0.0060 | 100% | 655,470 | $0.244 | 100% | 5.0x | 122x |
| S3 Shipped: lx ask, embeddings on | C | 129,954 | $0.0019 | 100% | 385,437 | $0.0051 | 100% | 724,898 | $0.238 | 100% | 5.6x | 124x |
| S4 Shipped: all commands, embeddings on | C | 197,588 | $0.0028 | 100% | 386,672 | $0.0054 | 100% | 722,159 | $0.220 | 100% | 3.7x | 79x |
| C1 Rust index + token-goat + graphify + semble | C | 310,595 | $0.0052 | 99% | 594,450 | $0.0089 | 100% | 1,028,884 | $0.262 | 100% | 3.3x | 51x |
| C2 lx + Rust embeddings + token-goat | C | 176,514 | $0.0028 | 100% | 455,445 | $0.0063 | 100% | 792,468 | $0.237 | 100% | 4.5x | 84x |
| C3 lx shipped + token-goat | C | 242,156 | $0.0031 | 100% | 453,003 | $0.0064 | 100% | 860,021 | $0.297 | 100% | 3.6x | 96x |
| C4 Rust index + token-goat | C | 281,589 | $0.0050 | 100% | 527,292 | $0.0080 | 100% | 1,093,132 | $0.275 | 100% | 3.9x | 56x |
| C5 Rust index + graphify + semble | C | 289,122 | $0.0056 | 100% | 538,419 | $0.0098 | 100% | 1,076,540 | $0.325 | 100% | 3.7x | 58x |
| C6 token-goat + graphify + semble | C | 180,129 | $0.0031 | 100% | 456,367 | $0.0064 | 100% | 800,524 | $0.281 | 100% | 4.4x | 89x |
| C7 token-goat + semble | C | 134,047 | $0.0027 | 100% | 521,584 | $0.0073 | 100% | 796,113 | $0.267 | 100% | 5.9x | 99x |
| C8 token-goat + graphify | C | 248,739 | $0.0037 | 100% | 455,502 | $0.0066 | 100% | 799,433 | $0.259 | 100% | 3.2x | 69x |
| C9 lx finds, token-goat reads | C | 220,870 | $0.0032 | 100% | 452,652 | $0.0066 | 100% | 787,234 | $0.218 | 100% | 3.6x | 69x |
| C10 lx about + cards | C | 219,611 | $0.0029 | 100% | 451,971 | $0.0064 | 100% | 919,863 | $0.236 | 100% | 4.2x | 81x |
| C11 lx about + Rust cards | C | 152,676 | $0.0022 | 100% | 451,756 | $0.0063 | 100% | 789,263 | $0.246 | 100% | 5.2x | 110x |
| C12 lx about + hybrid | C | 130,165 | $0.0021 | 100% | 386,028 | $0.0057 | 100% | 723,268 | $0.264 | 100% | 5.6x | 128x |
| C13 lx, every new command | C | 130,244 | $0.0019 | 100% | 386,886 | $0.0055 | 100% | 598,291 | $0.415 | 100% | 4.6x | 221x |
| C14 lx, every new command + token-goat | C | 130,642 | $0.0020 | 100% | 386,972 | $0.0053 | 100% | 793,955 | $0.227 | 100% | 6.1x | 112x |
| C15 lx ask + token-goat read | C | 130,244 | $0.0020 | 100% | 386,137 | $0.0055 | 100% | 720,327 | $0.215 | 100% | 5.5x | 107x |
| A0 No index | B | 280,547 | $0.103 | 100% |  |  |  | 1,047,254 | $0.654 | 100% | 3.7x | 6x |
| A1 graphify + semble | B | 266,073 | $0.0709 | 100% |  |  |  | 966,221 | $0.456 | 100% | 3.6x | 6x |
| A7 lx, improved | B | 260,280 | $0.0642 | 100% |  |  |  | 918,249 | $0.418 | 100% | 3.5x | 7x |
| A5 token-goat alone | B | 177,285 | $0.0499 | 100% |  |  |  | 725,430 | $0.365 | 100% | 4.1x | 7x |
| A6 token-goat + lx improved | B | 194,251 | $0.0486 | 100% |  |  |  | 983,828 | $0.439 | 100% | 5.1x | 9x |
| A9 lx, as shipped after round 1 | B | 216,959 | $0.0519 | 100% |  |  |  | 722,449 | $0.348 | 100% | 3.3x | 7x |
| S1 Shipped: lx ask, default index | B | 195,764 | $0.0476 | 100% |  |  |  | 653,592 | $0.314 | 100% | 3.3x | 7x |
| S3 Shipped: lx ask, embeddings on | B | 129,205 | $0.0306 | 100% |  |  |  | 824,656 | $0.419 | 100% | 6.4x | 14x |
| A0 No index | A | 297,959 | $0.343 | 100% |  |  |  | 1,056,804 | $1.09 | 100% | 3.5x | 3x |
| A1 graphify + semble | A | 500,374 | $0.217 | 100% |  |  |  | 1,287,089 | $0.657 | 100% | 2.6x | 3x |
| A7 lx, improved | A | 334,974 | $0.127 | 100% |  |  |  | 1,015,165 | $0.788 | 100% | 3.0x | 6x |
| A5 token-goat alone | A | 302,787 | $0.430 | 100% |  |  |  | 731,363 | $0.432 | 100% | 2.4x | 1x |
| A6 token-goat + lx improved | A | 343,189 | $0.183 | 100% |  |  |  | 1,027,834 | $0.874 | 100% | 3.0x | 5x |
| A9 lx, as shipped after round 1 | A | 271,640 | $0.164 | 100% |  |  |  | 869,401 | $0.614 | 100% | 3.2x | 4x |
| S1 Shipped: lx ask, default index | A | 200,628 | $0.118 | 100% |  |  |  | 903,289 | $0.972 | 100% | 4.5x | 8x |
| S3 Shipped: lx ask, embeddings on | A | 199,271 | $0.0982 | 100% |  |  |  | 736,690 | $0.607 | 100% | 3.7x | 6x |

### What one correct answer cost

Three questions a run: the tokens and the money of a run divided by three times its mean score, so a partly right answer counts in part. Seconds is the wall clock of a run, with subagents running side by side; rounds ran under different machine load, so it is a rough guide.

| Method | Tier | Setup | Runs | Mean score | Tokens per correct answer | Cost per correct answer | Seconds |
|---|---|---|---:|---:|---:|---:|---:|
| A0 No index | C | One agent | 5 | 98% | 123,561 | $0.0025 | 25 |
| A0 No index | C | Three subagents | 3 | 81% | 237,647 | $0.0038 | 16 |
| A0 No index | C | Orchestrator + subagents | 3 | 100% | 399,885 | $0.191 | 192 |
| A1 graphify + semble | C | One agent | 2 | 96% | 166,962 | $0.0023 | 36 |
| A1 graphify + semble | C | Three subagents | 1 | 92% | 309,732 | $0.0039 | 33 |
| A1 graphify + semble | C | Orchestrator + subagents | 3 | 100% | 484,877 | $0.121 | 154 |
| A2 Rust index alone, embeddings on | C | One agent | 2 | 100% | 117,232 | $0.0020 | 26 |
| A2 Rust index alone, embeddings on | C | Three subagents | 1 | 100% | 173,183 | $0.0026 | 12 |
| A2 Rust index alone, embeddings on | C | Orchestrator + subagents | 1 | 100% | 339,818 | $0.133 | 139 |
| A3 lx, first version | C | One agent | 2 | 100% | 100,068 | $0.0015 | 25 |
| A3 lx, first version | C | Three subagents | 1 | 100% | 215,657 | $0.0027 | 20 |
| A3 lx, first version | C | Orchestrator + subagents | 1 | 100% | 305,687 | $0.0961 | 122 |
| A4 lx first version + Rust embeddings | C | One agent | 2 | 100% | 65,156 | $0.0009 | 14 |
| A4 lx first version + Rust embeddings | C | Three subagents | 1 | 92% | 162,971 | $0.0022 | 8 |
| A4 lx first version + Rust embeddings | C | Orchestrator + subagents | 1 | 100% | 263,398 | $0.0849 | 93 |
| A7 lx, improved | C | One agent | 2 | 95% | 80,675 | $0.0011 | 26 |
| A7 lx, improved | C | Three subagents | 1 | 100% | 171,240 | $0.0022 | 17 |
| A7 lx, improved | C | Orchestrator + subagents | 1 | 100% | 305,613 | $0.0887 | 107 |
| A8 lx improved + Rust embeddings | C | One agent | 2 | 95% | 57,358 | $0.0009 | 13 |
| A8 lx improved + Rust embeddings | C | Three subagents | 1 | 100% | 149,545 | $0.0020 | 10 |
| A8 lx improved + Rust embeddings | C | Orchestrator + subagents | 1 | 100% | 239,403 | $0.0824 | 92 |
| A5 token-goat alone | C | One agent | 3 | 100% | 59,048 | $0.0009 | 27 |
| A5 token-goat alone | C | Three subagents | 2 | 100% | 139,888 | $0.0019 | 11 |
| A5 token-goat alone | C | Orchestrator + subagents | 3 | 100% | 258,071 | $0.0932 | 110 |
| A6 token-goat + lx improved | C | One agent | 2 | 99% | 66,608 | $0.0010 | 29 |
| A6 token-goat + lx improved | C | Three subagents | 1 | 100% | 150,771 | $0.0020 | 13 |
| A6 token-goat + lx improved | C | Orchestrator + subagents | 1 | 100% | 284,546 | $0.0809 | 106 |
| A9 lx, as shipped after round 1 | C | One agent | 6 | 99% | 65,963 | $0.0009 | 58 |
| A9 lx, as shipped after round 1 | C | Three subagents | 3 | 100% | 165,324 | $0.0022 | 64 |
| A9 lx, as shipped after round 1 | C | Orchestrator + subagents | 3 | 100% | 262,966 | $0.0876 | 136 |
| R1 lx shipped + Rust embeddings | C | One agent | 3 | 100% | 58,512 | $0.0010 | 26 |
| R1 lx shipped + Rust embeddings | C | Three subagents | 1 | 100% | 172,552 | $0.0023 | 46 |
| R1 lx shipped + Rust embeddings | C | Orchestrator + subagents | 1 | 100% | 285,043 | $0.0852 | 119 |
| L1 lx + about | C | One agent | 3 | 100% | 73,246 | $0.0010 | 97 |
| L1 lx + about | C | Three subagents | 1 | 100% | 194,895 | $0.0027 | 110 |
| L1 lx + about | C | Orchestrator + subagents | 1 | 100% | 284,107 | $0.0696 | 176 |
| L2 lx + cards | C | One agent | 3 | 100% | 50,997 | $0.0008 | 75 |
| L2 lx + cards | C | Three subagents | 1 | 100% | 172,528 | $0.0023 | 200 |
| L2 lx + cards | C | Orchestrator + subagents | 1 | 100% | 307,413 | $0.0851 | 142 |
| L3 lx + hybrid search | C | One agent | 3 | 100% | 43,389 | $0.0007 | 50 |
| L3 lx + hybrid search | C | Three subagents | 1 | 100% | 150,414 | $0.0021 | 60 |
| L3 lx + hybrid search | C | Orchestrator + subagents | 1 | 100% | 262,898 | $0.0830 | 115 |
| L4 lx ask, one call | C | One agent | 3 | 100% | 43,364 | $0.0007 | 58 |
| L4 lx ask, one call | C | Three subagents | 1 | 100% | 150,671 | $0.0020 | 98 |
| L4 lx ask, one call | C | Orchestrator + subagents | 1 | 100% | 241,257 | $0.0867 | 174 |
| N1 lx ask, no Rust embeddings | C | One agent | 3 | 100% | 43,460 | $0.0007 | 94 |
| N1 lx ask, no Rust embeddings | C | Three subagents | 1 | 100% | 128,325 | $0.0017 | 81 |
| N1 lx ask, no Rust embeddings | C | Orchestrator + subagents | 1 | 100% | 240,315 | $0.0755 | 149 |
| N2 lx, every new command, no Rust embeddings | C | One agent | 3 | 100% | 43,518 | $0.0007 | 99 |
| N2 lx, every new command, no Rust embeddings | C | Three subagents | 1 | 100% | 128,898 | $0.0019 | 106 |
| N2 lx, every new command, no Rust embeddings | C | Orchestrator + subagents | 1 | 100% | 262,865 | $0.0766 | 140 |
| S1 Shipped: lx ask, default index | C | One agent | 3 | 97% | 44,463 | $0.0006 | 33 |
| S1 Shipped: lx ask, default index | C | Three subagents | 2 | 100% | 128,568 | $0.0018 | 56 |
| S1 Shipped: lx ask, default index | C | Orchestrator + subagents | 3 | 100% | 233,548 | $0.0807 | 120 |
| S2 Shipped: all commands, default index | C | One agent | 3 | 100% | 43,436 | $0.0007 | 39 |
| S2 Shipped: all commands, default index | C | Three subagents | 1 | 100% | 150,857 | $0.0020 | 58 |
| S2 Shipped: all commands, default index | C | Orchestrator + subagents | 1 | 100% | 218,490 | $0.0815 | 124 |
| S3 Shipped: lx ask, embeddings on | C | One agent | 3 | 100% | 43,318 | $0.0006 | 39 |
| S3 Shipped: lx ask, embeddings on | C | Three subagents | 2 | 100% | 128,479 | $0.0017 | 58 |
| S3 Shipped: lx ask, embeddings on | C | Orchestrator + subagents | 3 | 100% | 241,633 | $0.0794 | 114 |
| S4 Shipped: all commands, embeddings on | C | One agent | 3 | 100% | 65,863 | $0.0009 | 53 |
| S4 Shipped: all commands, embeddings on | C | Three subagents | 1 | 100% | 128,891 | $0.0018 | 64 |
| S4 Shipped: all commands, embeddings on | C | Orchestrator + subagents | 1 | 100% | 240,720 | $0.0734 | 107 |
| C1 Rust index + token-goat + graphify + semble | C | One agent | 3 | 99% | 104,423 | $0.0017 | 66 |
| C1 Rust index + token-goat + graphify + semble | C | Three subagents | 1 | 100% | 198,150 | $0.0030 | 50 |
| C1 Rust index + token-goat + graphify + semble | C | Orchestrator + subagents | 1 | 100% | 342,961 | $0.0872 | 175 |
| C2 lx + Rust embeddings + token-goat | C | One agent | 3 | 100% | 58,838 | $0.0009 | 44 |
| C2 lx + Rust embeddings + token-goat | C | Three subagents | 1 | 100% | 151,815 | $0.0021 | 47 |
| C2 lx + Rust embeddings + token-goat | C | Orchestrator + subagents | 1 | 100% | 264,156 | $0.0789 | 104 |
| C3 lx shipped + token-goat | C | One agent | 3 | 100% | 80,719 | $0.0010 | 104 |
| C3 lx shipped + token-goat | C | Three subagents | 1 | 100% | 151,001 | $0.0021 | 100 |
| C3 lx shipped + token-goat | C | Orchestrator + subagents | 1 | 100% | 286,674 | $0.0990 | 157 |
| C4 Rust index + token-goat | C | One agent | 3 | 100% | 93,863 | $0.0017 | 45 |
| C4 Rust index + token-goat | C | Three subagents | 1 | 100% | 175,764 | $0.0027 | 48 |
| C4 Rust index + token-goat | C | Orchestrator + subagents | 1 | 100% | 364,377 | $0.0917 | 141 |
| C5 Rust index + graphify + semble | C | One agent | 3 | 100% | 96,374 | $0.0019 | 52 |
| C5 Rust index + graphify + semble | C | Three subagents | 1 | 100% | 179,473 | $0.0033 | 71 |
| C5 Rust index + graphify + semble | C | Orchestrator + subagents | 1 | 100% | 358,847 | $0.108 | 129 |
| C6 token-goat + graphify + semble | C | One agent | 3 | 100% | 60,043 | $0.0010 | 62 |
| C6 token-goat + graphify + semble | C | Three subagents | 1 | 100% | 152,122 | $0.0021 | 40 |
| C6 token-goat + graphify + semble | C | Orchestrator + subagents | 1 | 100% | 266,841 | $0.0937 | 119 |
| C7 token-goat + semble | C | One agent | 3 | 100% | 44,682 | $0.0009 | 38 |
| C7 token-goat + semble | C | Three subagents | 1 | 100% | 173,861 | $0.0024 | 49 |
| C7 token-goat + semble | C | Orchestrator + subagents | 1 | 100% | 265,371 | $0.0890 | 140 |
| C8 token-goat + graphify | C | One agent | 3 | 100% | 82,913 | $0.0012 | 38 |
| C8 token-goat + graphify | C | Three subagents | 1 | 100% | 151,834 | $0.0022 | 83 |
| C8 token-goat + graphify | C | Orchestrator + subagents | 1 | 100% | 266,478 | $0.0865 | 110 |
| C9 lx finds, token-goat reads | C | One agent | 3 | 100% | 73,623 | $0.0010 | 89 |
| C9 lx finds, token-goat reads | C | Three subagents | 1 | 100% | 150,884 | $0.0022 | 170 |
| C9 lx finds, token-goat reads | C | Orchestrator + subagents | 1 | 100% | 262,411 | $0.0728 | 128 |
| C10 lx about + cards | C | One agent | 3 | 100% | 73,204 | $0.0010 | 84 |
| C10 lx about + cards | C | Three subagents | 1 | 100% | 150,657 | $0.0021 | 161 |
| C10 lx about + cards | C | Orchestrator + subagents | 1 | 100% | 306,621 | $0.0788 | 107 |
| C11 lx about + Rust cards | C | One agent | 3 | 100% | 50,892 | $0.0007 | 27 |
| C11 lx about + Rust cards | C | Three subagents | 1 | 100% | 150,585 | $0.0021 | 67 |
| C11 lx about + Rust cards | C | Orchestrator + subagents | 1 | 100% | 263,088 | $0.0821 | 97 |
| C12 lx about + hybrid | C | One agent | 3 | 100% | 43,388 | $0.0007 | 56 |
| C12 lx about + hybrid | C | Three subagents | 1 | 100% | 128,676 | $0.0019 | 61 |
| C12 lx about + hybrid | C | Orchestrator + subagents | 1 | 100% | 241,089 | $0.0880 | 100 |
| C13 lx, every new command | C | One agent | 3 | 100% | 43,415 | $0.0006 | 63 |
| C13 lx, every new command | C | Three subagents | 1 | 100% | 128,962 | $0.0018 | 109 |
| C13 lx, every new command | C | Orchestrator + subagents | 1 | 100% | 199,430 | $0.138 | 202 |
| C14 lx, every new command + token-goat | C | One agent | 3 | 100% | 43,547 | $0.0007 | 57 |
| C14 lx, every new command + token-goat | C | Three subagents | 1 | 100% | 128,991 | $0.0018 | 74 |
| C14 lx, every new command + token-goat | C | Orchestrator + subagents | 1 | 100% | 264,652 | $0.0758 | 140 |
| C15 lx ask + token-goat read | C | One agent | 3 | 100% | 43,415 | $0.0007 | 59 |
| C15 lx ask + token-goat read | C | Three subagents | 1 | 100% | 128,712 | $0.0018 | 109 |
| C15 lx ask + token-goat read | C | Orchestrator + subagents | 1 | 100% | 240,109 | $0.0715 | 96 |
| A0 No index | B | One agent | 3 | 100% | 93,516 | $0.0344 | 25 |
| A0 No index | B | Orchestrator + subagents | 2 | 100% | 349,084 | $0.218 | 179 |
| A1 graphify + semble | B | One agent | 1 | 100% | 88,691 | $0.0236 | 24 |
| A1 graphify + semble | B | Orchestrator + subagents | 2 | 100% | 322,074 | $0.152 | 138 |
| A7 lx, improved | B | One agent | 1 | 100% | 86,760 | $0.0214 | 31 |
| A7 lx, improved | B | Orchestrator + subagents | 1 | 100% | 306,083 | $0.139 | 110 |
| A5 token-goat alone | B | One agent | 3 | 100% | 59,095 | $0.0166 | 42 |
| A5 token-goat alone | B | Orchestrator + subagents | 2 | 100% | 241,810 | $0.122 | 103 |
| A6 token-goat + lx improved | B | One agent | 1 | 100% | 64,750 | $0.0162 | 23 |
| A6 token-goat + lx improved | B | Orchestrator + subagents | 1 | 100% | 327,943 | $0.146 | 111 |
| A9 lx, as shipped after round 1 | B | One agent | 3 | 100% | 72,320 | $0.0173 | 71 |
| A9 lx, as shipped after round 1 | B | Orchestrator + subagents | 2 | 100% | 240,816 | $0.116 | 108 |
| S1 Shipped: lx ask, default index | B | One agent | 6 | 100% | 65,255 | $0.0159 | 61 |
| S1 Shipped: lx ask, default index | B | Orchestrator + subagents | 2 | 100% | 217,864 | $0.105 | 94 |
| S3 Shipped: lx ask, embeddings on | B | One agent | 3 | 100% | 43,068 | $0.0102 | 23 |
| S3 Shipped: lx ask, embeddings on | B | Orchestrator + subagents | 2 | 100% | 274,885 | $0.140 | 134 |
| A0 No index | A | One agent | 3 | 100% | 99,320 | $0.114 | 101 |
| A0 No index | A | Orchestrator + subagents | 1 | 100% | 352,268 | $0.364 | 334 |
| A1 graphify + semble | A | One agent | 1 | 100% | 166,791 | $0.0724 | 98 |
| A1 graphify + semble | A | Orchestrator + subagents | 1 | 100% | 429,030 | $0.219 | 269 |
| A7 lx, improved | A | One agent | 1 | 100% | 111,658 | $0.0424 | 54 |
| A7 lx, improved | A | Orchestrator + subagents | 1 | 100% | 338,388 | $0.263 | 222 |
| A5 token-goat alone | A | One agent | 1 | 100% | 100,929 | $0.143 | 145 |
| A5 token-goat alone | A | Orchestrator + subagents | 1 | 100% | 243,788 | $0.144 | 182 |
| A6 token-goat + lx improved | A | One agent | 1 | 100% | 114,396 | $0.0609 | 62 |
| A6 token-goat + lx improved | A | Orchestrator + subagents | 1 | 100% | 342,611 | $0.291 | 262 |
| A9 lx, as shipped after round 1 | A | One agent | 2 | 100% | 90,546 | $0.0546 | 121 |
| A9 lx, as shipped after round 1 | A | Orchestrator + subagents | 1 | 100% | 289,800 | $0.205 | 145 |
| S1 Shipped: lx ask, default index | A | One agent | 1 | 100% | 66,876 | $0.0395 | 51 |
| S1 Shipped: lx ask, default index | A | Orchestrator + subagents | 1 | 100% | 301,096 | $0.324 | 290 |
| S3 Shipped: lx ask, embeddings on | A | One agent | 1 | 100% | 66,424 | $0.0327 | 47 |
| S3 Shipped: lx ask, embeddings on | A | Orchestrator + subagents | 1 | 100% | 245,563 | $0.202 | 184 |

### Which question the subagents got wrong

Standard reviewer, every method together. Each run asks each question once.

| Subagent tier | Question | Answers | Subagents fully right | After review fully right | Subagents, mean score | After review, mean score |
|---|---|---:|---:|---:|---:|---:|
| C | Q1 | 48 | 48 of 48 | 48 of 48 | 100% | 100% |
| C | Q2 | 48 | 47 of 48 | 48 of 48 | 99% | 100% |
| C | Q3 | 48 | 46 of 48 | 48 of 48 | 99% | 100% |
| B | Q1 | 14 | 14 of 14 | 14 of 14 | 100% | 100% |
| B | Q2 | 14 | 14 of 14 | 14 of 14 | 100% | 100% |
| B | Q3 | 14 | 14 of 14 | 14 of 14 | 100% | 100% |
| A | Q1 | 8 | 8 of 8 | 8 of 8 | 100% | 100% |
| A | Q2 | 8 | 8 of 8 | 8 of 8 | 100% | 100% |
| A | Q3 | 8 | 8 of 8 | 8 of 8 | 100% | 100% |

### Reviews, added up by reviewer

| Reviewer | Runs | Answers | Wrong before review | Corrected | Still wrong | Right answers broken | Runs fully correct |
|---|---:|---:|---:|---:|---:|---:|---:|
| Top tier with the method's tools (standard), every method and tier | 70 | 210 | 3 | 3 | 0 | 0 | 70 of 70 |
| Top tier with the method's tools (standard), on the methods and tier the variants ran on | 9 | 27 | 0 | 0 | 0 | 0 | 9 of 9 |
| Top tier, no tools | 6 | 18 | 1 | 0 | 1 | 0 | 5 of 6 |
| Mid tier with the method's tools | 6 | 18 | 0 | 0 | 0 | 0 | 6 of 6 |
| Cheapest tier with the method's tools | 6 | 18 | 0 | 0 | 0 | 0 | 6 of 6 |

### Who reviews, and with what

| Method | Subagent tier | Reviewer | Runs | Review tokens | Review output tokens | Review turns | Review cost | Total tokens | Total cost | Subagents, mean score | After review, mean score | Corrected | Still wrong | Broken | Runs fully correct |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| A0 No index | C | Top tier with the method's tools (standard) | 3 | 308,182 | 13,068 | 4.0 | $0.434 | 1,199,654 | $0.572 | 100% | 100% | 0 | 0 | 0 | 3 of 3 |
| A0 No index | C | Top tier, no tools | 2 | 64,123 | 931 | 1.0 | $0.0313 | 798,594 | $0.174 | 96% | 96% | 0 | 1 | 0 | 1 of 2 |
| A0 No index | C | Mid tier with the method's tools | 2 | 282,528 | 1,647 | 4.0 | $0.0991 | 1,156,150 | $0.244 | 100% | 100% | 0 | 0 | 0 | 2 of 2 |
| A0 No index | C | Cheapest tier with the method's tools | 2 | 284,434 | 3,674 | 4.0 | $0.0061 | 1,125,739 | $0.151 | 100% | 100% | 0 | 0 | 0 | 2 of 2 |
| S1 Shipped: lx ask, default index | C | Top tier with the method's tools (standard) | 3 | 178,195 | 2,390 | 2.7 | $0.104 | 700,644 | $0.242 | 100% | 100% | 0 | 0 | 0 | 3 of 3 |
| S1 Shipped: lx ask, default index | C | Top tier, no tools | 2 | 103,223 | 40,025 | 1.0 | $0.813 | 558,982 | $0.951 | 100% | 100% | 0 | 0 | 0 | 2 of 2 |
| S1 Shipped: lx ask, default index | C | Mid tier with the method's tools | 2 | 163,914 | 340 | 2.5 | $0.0433 | 787,558 | $0.183 | 100% | 100% | 0 | 0 | 0 | 2 of 2 |
| S1 Shipped: lx ask, default index | C | Cheapest tier with the method's tools | 2 | 164,588 | 936 | 2.5 | $0.0025 | 654,018 | $0.140 | 100% | 100% | 0 | 0 | 0 | 2 of 2 |
| S3 Shipped: lx ask, embeddings on | C | Top tier with the method's tools (standard) | 3 | 178,019 | 2,142 | 2.7 | $0.0993 | 724,898 | $0.238 | 100% | 100% | 0 | 0 | 0 | 3 of 3 |
| S3 Shipped: lx ask, embeddings on | C | Top tier, no tools | 2 | 63,966 | 772 | 1.0 | $0.0281 | 553,620 | $0.166 | 100% | 100% | 0 | 0 | 0 | 2 of 2 |
| S3 Shipped: lx ask, embeddings on | C | Mid tier with the method's tools | 2 | 197,956 | 556 | 3.0 | $0.0529 | 686,844 | $0.191 | 100% | 100% | 0 | 0 | 0 | 2 of 2 |
| S3 Shipped: lx ask, embeddings on | C | Cheapest tier with the method's tools | 2 | 130,825 | 792 | 2.0 | $0.0020 | 619,237 | $0.140 | 100% | 100% | 0 | 0 | 0 | 2 of 2 |

## token-goat hooks, off and on

- token-goat's hooks, installed in the client: with no index they ran in 9 of 9 runs, rewrote 3.7 tool results a run, and the runs used 2% more tokens, inside the 21% by which repeats of one prompt without the hooks differ; 5 of 9 of those runs ended fully correct, against 13 of 17 without the hooks. With index commands sent through PowerShell they ran in 6 of 32 runs. Through Bash they ran in 7 of 7 runs, rewrote nothing, and the runs used 4% more tokens, inside the 26% by which repeats of one prompt without the hooks differ.
- No index, where every lookup is one of the client's own Grep, Glob and Read calls: 9 runs with the hooks against 17 without, compared setup by setup and tier by tier (5 pairs, 2 of them with a single run on one side). The hooks ran in 9 of 9 runs, 27.7 times a run on average, rewrote 3.7 tool results a run, added 4.0 notes a run and refused 0.11 calls a run. Tokens a run: 547k without, 559k with (+2%, inside the 21% by which repeats of one prompt without the hooks differ; pair by pair from -2% to +4%). Turns: 7.8 and 8.0. Mean score: 97% and 98%. Runs fully correct: 13 of 17 and 5 of 9.
- The rewritten results carried 2.4 redaction markers a run, where the hook put a marker in place of source text it took for a secret. The agent that looked up the callers left one out in 5 of 9 runs with the hooks (triage_with_opus in 5), against 1 of 17 without them. In these runs the marker was `[REDACTED:generic_secret_assignment]`, written over the call itself on two lines of the fixture's prs.py that assign its result to something named api_key. Both lines sit in one function, and that function is the caller the incomplete answers lack. Traced in the transcripts: the lines an agent was shown, and the caller its answer left out.
- Index commands sent through PowerShell (A5, A9, S1, S3): 32 runs with the hooks against 50 without, compared setup by setup and tier by tier (18 pairs, 8 of them with a single run on one side). The hooks ran in 6 of 32 runs, 1.2 times a run on average, rewrote 0.2 tool results a run, added 0.6 notes a run and refused 0.00 calls a run. Tokens a run: 280k without, 260k with (-7%, inside the 25% by which repeats of one prompt without the hooks differ; pair by pair from -34% to +8%). Turns: 4.2 and 4.0. Mean score: 99% and 100%. Runs fully correct: 48 of 50 and 32 of 32.
- In 13 of those 18 method, setup and tier pairs no hook ran at all, because PowerShell is not a tool they match. 7 of the 13 still differ by 5% or more in tokens, all of them downward (the 13 run from -34% to +4%). No hook ran in those runs, so the hooks did not make that gap. The two sides were not otherwise identical: every run with the hooks comes from one round, and the agents of each round start from slightly different context (an agent began with 62,884 tokens with the hooks on against 62,958 without: the version of the rules file the session had loaded, the request it relays to its agents, the skill list). With 1 to 6 runs a side, these rows cannot separate that from chance.
- The one-call command sent through Bash, a tool the hooks do match: 7 runs with the hooks against 7 without, compared setup by setup and tier by tier (4 pairs, 2 of them with a single run on one side). The hooks ran in 7 of 7 runs, 4.1 times a run on average, rewrote nothing, added 0.4 notes a run and refused 0.00 calls a run. Tokens a run: 196k without, 204k with (+4%, inside the 26% by which repeats of one prompt without the hooks differ; pair by pair from -15% to +97%). Turns: 3.0 and 3.1. Mean score: 100% and 100%. Runs fully correct: 7 of 7 and 7 of 7.
- Installing the hooks also writes a routing block into each rules file (4,661 bytes in ~/.claude/CLAUDE.md, 4,731 bytes in ~/.codex/AGENTS.md, 4,564 bytes in ~/.copilot/copilot-instructions.md), about 1,200 tokens that every request of every agent then carries. The agents measured here did not carry it, so that cost is on top of the figures above.

Means per run. Hook runs, calls rewritten, notes added and calls refused are per run with the hooks on. Runs the hooks ran in counts the runs where a hook fired at least once. Seconds are left out: the two sides come from different rounds, run under different machine load.

| Method | Setup | Tier | Runs off / on | Tokens off | Tokens on | Change | Turns off | Turns on | Cost off | Cost on | Mean score off | Mean score on | Runs fully correct off | Runs fully correct on | Start-up tokens off | Start-up tokens on | Runs the hooks ran in | Hook runs | Calls rewritten | Redaction markers | Notes added | Calls refused |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| A0 No index | One agent | C | 5 / 3 | 364,508 | 362,048 | -1% | 5.0 | 5.0 | $0.0074 | $0.0076 | 98% | 99% | 4 of 5 | 2 of 3 | 62,702 | 62,709 | 3 of 3 | 22.0 | 3.7 | 2.0 | 2.7 | 0.00 |
| A0 No index | One agent | B | 3 / 2 | 280,547 | 278,198 | -1% | 4.0 | 4.0 | $0.103 | $0.102 | 100% | 97% | 3 of 3 | 0 of 2 | 62,729 | 62,712 | 2 of 2 | 16.0 | 1.5 | 2.0 | 1.5 | 0.00 |
| A0 No index | One agent | A | 3 / 1 | 297,959 | 292,939 | -2% | 4.0 | 4.0 | $0.343 | $0.341 | 100% | 100% | 3 of 3 | 1 of 1 | 62,774 | 62,706 | 1 of 1 | 22.0 | 2.0 | 2.0 | 3.0 | 0.00 |
| A0 No index | Three subagents | C | 3 / 1 | 574,320 | 592,278 | +3% | 8.7 | 9.0 | $0.0092 | $0.0099 | 81% | 89% | 0 of 3 | 0 of 1 | 187,206 | 187,005 | 1 of 1 | 22.0 | 3.0 | 2.0 | 2.0 | 0.00 |
| A0 No index | Orchestrator + subagents | C | 3 / 2 | 1,199,654 | 1,252,824 | +4% | 17.3 | 18.0 | $0.572 | $0.574 | 100% | 100% | 3 of 3 | 2 of 2 | 312,646 | 312,323 | 2 of 2 | 53.5 | 7.0 | 4.0 | 10.0 | 0.50 |
| A9 lx, as shipped after round 1 | One agent | C | 6 / 3 | 197,045 | 151,894 | -23% | 3.0 | 2.3 | $0.0028 | $0.0023 | 99% | 100% | 5 of 6 | 3 of 3 | 62,852 | 62,896 | 0 of 3 | 0.0 | 0.0 | 0.0 | 0.0 | 0.00 |
| A9 lx, as shipped after round 1 | One agent | B | 3 / 2 | 216,959 | 194,850 | -10% | 3.3 | 3.0 | $0.0519 | $0.0458 | 100% | 100% | 3 of 3 | 2 of 2 | 62,916 | 62,899 | 0 of 2 | 0.0 | 0.0 | 0.0 | 0.0 | 0.00 |
| A9 lx, as shipped after round 1 | One agent | A | 2 / 1 | 271,640 | 205,230 | -24% | 4.0 | 3.0 | $0.164 | $0.195 | 100% | 100% | 2 of 2 | 1 of 1 | 62,850 | 62,893 | 1 of 1 | 4.0 | 1.0 | 0.0 | 2.0 | 0.00 |
| A9 lx, as shipped after round 1 | Three subagents | C | 3 / 1 | 495,973 | 515,309 | +4% | 7.7 | 8.0 | $0.0067 | $0.0065 | 100% | 100% | 3 of 3 | 1 of 1 | 187,766 | 187,566 | 0 of 1 | 0.0 | 0.0 | 0.0 | 0.0 | 0.00 |
| S1 Shipped: lx ask, default index | One agent | C | 3 / 3 | 129,686 | 129,342 | -0% | 2.0 | 2.0 | $0.0018 | $0.0018 | 97% | 100% | 2 of 3 | 3 of 3 | 63,028 | 62,891 | 0 of 3 | 0.0 | 0.0 | 0.0 | 0.0 | 0.00 |
| S1 Shipped: lx ask, default index | One agent | B | 6 / 2 | 195,764 | 128,946 | -34% | 3.0 | 2.0 | $0.0476 | $0.0317 | 100% | 100% | 6 of 6 | 2 of 2 | 63,106 | 62,894 | 0 of 2 | 0.0 | 0.0 | 0.0 | 0.0 | 0.00 |
| S1 Shipped: lx ask, default index | One agent | A | 1 / 1 | 200,628 | 198,512 | -1% | 3.0 | 3.0 | $0.118 | $0.104 | 100% | 100% | 1 of 1 | 1 of 1 | 63,025 | 62,888 | 1 of 1 | 4.0 | 0.0 | 0.0 | 2.0 | 0.00 |
| S1 Shipped: lx ask, default index | Three subagents | C | 2 / 1 | 385,704 | 384,096 | -0% | 6.0 | 6.0 | $0.0053 | $0.0052 | 100% | 100% | 2 of 2 | 1 of 1 | 188,187 | 187,551 | 0 of 1 | 0.0 | 0.0 | 0.0 | 0.0 | 0.00 |
| S1 Shipped: lx ask, default index | Orchestrator + subagents | C | 3 / 2 | 700,644 | 753,226 | +8% | 10.7 | 11.5 | $0.242 | $0.235 | 100% | 100% | 3 of 3 | 2 of 2 | 314,318 | 313,069 | 2 of 2 | 8.0 | 2.0 | 0.0 | 4.0 | 0.00 |
| S3 Shipped: lx ask, embeddings on | One agent | C | 3 / 3 | 129,954 | 129,584 | -0% | 2.0 | 2.0 | $0.0019 | $0.0019 | 100% | 100% | 3 of 3 | 3 of 3 | 63,020 | 62,883 | 0 of 3 | 0.0 | 0.0 | 0.0 | 0.0 | 0.00 |
| S3 Shipped: lx ask, embeddings on | One agent | B | 3 / 2 | 129,205 | 129,056 | -0% | 2.0 | 2.0 | $0.0306 | $0.0325 | 100% | 100% | 3 of 3 | 2 of 2 | 63,023 | 62,886 | 0 of 2 | 0.0 | 0.0 | 0.0 | 0.0 | 0.00 |
| S3 Shipped: lx ask, embeddings on | One agent | A | 1 / 1 | 199,271 | 199,357 | +0% | 3.0 | 3.0 | $0.0982 | $0.106 | 100% | 100% | 1 of 1 | 1 of 1 | 63,017 | 62,880 | 1 of 1 | 4.0 | 0.0 | 0.0 | 2.0 | 0.00 |
| S3 Shipped: lx ask, embeddings on | Three subagents | C | 2 / 1 | 385,437 | 383,747 | -0% | 6.0 | 6.0 | $0.0051 | $0.0050 | 100% | 100% | 2 of 2 | 1 of 1 | 188,163 | 187,527 | 0 of 1 | 0.0 | 0.0 | 0.0 | 0.0 | 0.00 |
| A5 token-goat alone | One agent | C | 3 / 3 | 177,144 | 154,990 | -13% | 2.7 | 2.3 | $0.0027 | $0.0027 | 100% | 100% | 3 of 3 | 3 of 3 | 62,806 | 62,860 | 0 of 3 | 0.0 | 0.0 | 0.0 | 0.0 | 0.00 |
| A5 token-goat alone | One agent | B | 3 / 2 | 177,285 | 131,102 | -26% | 2.7 | 2.0 | $0.0499 | $0.0418 | 100% | 100% | 3 of 3 | 2 of 2 | 62,980 | 62,863 | 0 of 2 | 0.0 | 0.0 | 0.0 | 0.0 | 0.00 |
| A5 token-goat alone | One agent | A | 1 / 1 | 302,787 | 224,504 | -26% | 4.0 | 3.0 | $0.430 | $0.375 | 100% | 100% | 1 of 1 | 1 of 1 | 62,633 | 62,857 | 1 of 1 | 12.0 | 0.0 | 0.0 | 6.0 | 0.00 |
| A5 token-goat alone | Three subagents | C | 2 / 1 | 419,665 | 386,426 | -8% | 6.5 | 6.0 | $0.0058 | $0.0057 | 100% | 100% | 2 of 2 | 1 of 1 | 187,552 | 187,458 | 0 of 1 | 0.0 | 0.0 | 0.0 | 0.0 | 0.00 |
| A5 token-goat alone | Orchestrator + subagents | C | 3 / 2 | 774,213 | 726,150 | -6% | 11.7 | 11.0 | $0.280 | $0.242 | 100% | 100% | 3 of 3 | 2 of 2 | 313,701 | 312,935 | 0 of 2 | 0.0 | 0.0 | 0.0 | 0.0 | 0.00 |
| SB Shipped lx ask through Bash, hooks off | One agent | C | 3 / 3 | 152,360 | 129,533 | -15% | 2.3 | 2.0 | $0.0021 | $0.0019 | 100% | 100% | 3 of 3 | 3 of 3 | 63,186 | 62,899 | 3 of 3 | 2.0 | 0.0 | 0.0 | 0.0 | 0.00 |
| SB Shipped lx ask through Bash, hooks off | One agent | B | 2 / 2 | 195,948 | 195,092 | -0% | 3.0 | 3.0 | $0.0460 | $0.0460 | 100% | 100% | 2 of 2 | 2 of 2 | 63,189 | 62,902 | 2 of 2 | 4.0 | 0.0 | 0.0 | 0.0 | 0.00 |
| SB Shipped lx ask through Bash, hooks off | One agent | A | 1 / 1 | 136,174 | 267,618 | +97% | 2.0 | 4.0 | $0.175 | $0.130 | 100% | 100% | 1 of 1 | 1 of 1 | 63,183 | 62,896 | 1 of 1 | 10.0 | 0.0 | 0.0 | 3.0 | 0.00 |
| SB Shipped lx ask through Bash, hooks off | Three subagents | C | 1 / 1 | 386,879 | 384,318 | -1% | 6.0 | 6.0 | $0.0053 | $0.0052 | 100% | 100% | 1 of 1 | 1 of 1 | 188,775 | 187,575 | 1 of 1 | 5.0 | 0.0 | 0.0 | 0.0 | 0.00 |

### What the hooks are

- Version: token-goat 2.9.30, installed with `token-goat install --no-index`, and again with `--codex` and `--copilot`.
- Licence: PolyForm Noncommercial: not for commercial use.
- Events hooked in Claude Code: SessionStart, UserPromptSubmit, PreToolUse, PostToolUse, PostToolUseFailure, PreCompact, PostCompact, SubagentStop.
- Tools matched before a call: `^Read$|^Grep$|^Glob$|^Write$|^WebFetch$|^Skill$|^Bash$|^mcp__|^WebSearch$|^Agent$|^task$|^Task$`.
- Not matched: PowerShell. On Windows the client sends shell commands through its PowerShell tool unless told otherwise, so the hooks never see lx, token-goat or any other command run that way.
- The routing block token-goat adds to each rules file is read on every request of every agent. The benchmark agents did not carry it: the client had loaded the rules before the block was written. Its cost is therefore an estimate, at about four characters a token, and is not in any measured figure.
- In these runs the marker was `[REDACTED:generic_secret_assignment]`, written over the call itself on two lines of the fixture's prs.py that assign its result to something named api_key. Both lines sit in one function, and that function is the caller the incomplete answers lack. Traced in the transcripts: the lines an agent was shown, and the caller its answer left out.
- The install also adds a token-goat skill to the client. Its name, without a description, was in the skill list of every agent measured with the hooks on: a few tokens of start-up context their twins did not have.
- The two sides of a pair share one prompt, word for word, but not one start-up context. Every round's agents inherit what the session had loaded when the round ran: the version of the global rules file, the request the session relays to its agents, the skill list. The runs with the hooks were all made in one round, the runs without them before and after it, and the start-up context differs by a few hundred tokens between rounds. The start-up columns of the table show it pair by pair. The Bash pair has one more difference from the PowerShell methods, the same on both of its sides: on this machine two unrelated hooks of the owner's own setup are launched before every Bash call, which adds to a Bash method's seconds and not to its tokens.
- How this was measured: The runs without the hooks come from the earlier rounds, made before the hooks were ever installed, and from a round made after they were uninstalled again. The runs with the hooks were made in between, in one round. The client applies hook changes at once, so no restart was needed. A hook run that let a call through is a row in the agent's transcript, and its answer says whether it rewrote the result. A note the hook added beside a result is a row of its own. A call the hook refused leaves no hook row: the agent gets an error in place of the result, starting `PreToolUse:<Tool> hook error`, and each of those is counted as one hook run and one refused call.
