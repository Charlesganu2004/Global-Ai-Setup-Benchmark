# Benchmark results

502 agent runs on one repository: three lookup questions, 36 ways of finding the answer, three model tiers, three ways of organising the agents. 97.9 million tokens measured, 85% of them cache reads.

The charts are on the project site. This file is the same data as tables, written by `benchmark/harness/build_site.py`.

## Findings

- An agent starts with about 63k tokens of context before it reads one line of code, and every later turn re-reads all of it. Across the 502 agents measured, 85% of all tokens were cache reads, 14% were cache writes and 0.6% were output. What an index saves in tool output is small next to that. What it really buys is fewer turns.
- Tokens follow turns. Among methods run at least twice, one agent answering all three questions averaged from 2.0 turns (Shipped: lx ask, embeddings on, Tier B, 3 runs: 129k tokens) to 7.0 turns (graphify + semble, Tier C, 2 runs: 480k tokens).
- Cheapest tier, one agent, methods run at least three times, fewest tokens first: S1 Shipped: lx ask, default index: 130k tokens, 2.0 turns, 2 of 3 runs fully correct; S3 Shipped: lx ask, embeddings on: 130k tokens, 2.0 turns, 3 of 3 runs fully correct; L4 lx ask, one call: 130k tokens, 2.0 turns, 3 of 3 runs fully correct; C12 lx about + hybrid: 130k tokens, 2.0 turns, 3 of 3 runs fully correct; L3 lx + hybrid search: 130k tokens, 2.0 turns, 3 of 3 runs fully correct.
- Where the shipped build, the round 1 leaders and the control stand among those 28 methods: S1 Shipped: lx ask, default index: 130k tokens, 2.0 turns, 2 of 3 runs fully correct, place 1; S3 Shipped: lx ask, embeddings on: 130k tokens, 2.0 turns, 3 of 3 runs fully correct, place 2; A9 lx, as shipped after round 1: 197k tokens, 3.0 turns, 5 of 6 runs fully correct, place 18; R1 lx shipped + Rust embeddings: 176k tokens, 2.7 turns, 3 of 3 runs fully correct, place 15; A0 No index: 365k tokens, 5.0 turns, 4 of 5 runs fully correct, place 28.
- Two turns is the floor for this task: one to ask and one to answer. On Tier C, scout, 11 methods took exactly two turns and answered everything correctly in each of their 3 runs: S3 Shipped: lx ask, embeddings on, L4 lx ask, one call, C12 lx about + hybrid, L3 lx + hybrid search, C15 lx ask + token-goat read, C13 lx, every new command, S2 Shipped: all commands, default index, N1 lx ask, no Rust embeddings, N2 lx, every new command, no Rust embeddings, C14 lx, every new command + token-goat, C7 token-goat + semble.
- Two turns is the floor for this task: one to ask and one to answer. On Tier B, coder, 11 methods took exactly two turns and answered everything correctly in each of their 2 to 3 runs: C15 lx ask + token-goat read, L2 lx + cards, S3 Shipped: lx ask, embeddings on, L4 lx ask, one call, R1 lx shipped + Rust embeddings, C2 lx + Rust embeddings + token-goat, C12 lx about + hybrid, C11 lx about + Rust cards, S4 Shipped: all commands, embeddings on, C13 lx, every new command, C14 lx, every new command + token-goat.
- The shipped build on an index with Rust embeddings and on the default index without them. Tier C, one agent: Shipped: lx ask, embeddings on 130k (2 turns over 3 runs), against Shipped: lx ask, default index 130k (2 turns over 3 runs); Shipped: all commands, embeddings on 198k (2 to 4 turns over 3 runs), against Shipped: all commands, default index 130k (2 turns over 3 runs).
- The shipped build on an index with Rust embeddings and on the default index without them. Tier B, one agent: Shipped: lx ask, embeddings on 129k (2 turns over 3 runs), against Shipped: lx ask, default index 196k (3 turns over 3 runs); Shipped: all commands, embeddings on 130k (2 turns over 3 runs), against Shipped: all commands, default index 196k (3 turns over 3 runs).
- The shipped build on an index with Rust embeddings and on the default index without them. Tier A, one agent: Shipped: lx ask, embeddings on 199k (3 turns over 1 run), against Shipped: lx ask, default index 201k (3 turns over 1 run); Shipped: all commands, embeddings on 199k (3 turns over 1 run), against Shipped: all commands, default index 199k (3 turns over 1 run).
- What merging in the Rust index's name and doc search added on the default index: the shipped build against the round 2 build. Tier C, one agent: Shipped: lx ask, default index 130k (2 turns over 3 runs), against lx ask, no Rust embeddings 130k (2 turns over 3 runs); Shipped: all commands, default index 130k (2 turns over 3 runs), against lx, every new command, no Rust embeddings 131k (2 turns over 3 runs).
- What merging in the Rust index's name and doc search added on the default index: the shipped build against the round 2 build. Tier B, one agent: Shipped: lx ask, default index 196k (3 turns over 3 runs), against lx ask, no Rust embeddings 296k (4 to 5 turns over 2 runs); Shipped: all commands, default index 196k (3 turns over 3 runs), against lx, every new command, no Rust embeddings 196k (3 turns over 2 runs).
- Round 2, before that search was merged in: the same commands on an index built with Rust embeddings and without. Tier C, one agent: lx ask, one call 130k (2 turns over 3 runs), against lx ask, no Rust embeddings 130k (2 turns over 3 runs); lx, every new command 130k (2 turns over 3 runs), against lx, every new command, no Rust embeddings 131k (2 turns over 3 runs).
- Round 2, before that search was merged in: the same commands on an index built with Rust embeddings and without. Tier B, one agent: lx ask, one call 129k (2 turns over 2 runs), against lx ask, no Rust embeddings 296k (4 to 5 turns over 2 runs); lx, every new command 130k (2 turns over 2 runs), against lx, every new command, no Rust embeddings 196k (3 turns over 2 runs).
- Does adding token-goat beside lx change anything? Each pair is the same lx build without it and with it. Tier C, one agent: lx, as shipped after round 1 197k (2 to 5 turns over 6 runs), against lx shipped + token-goat 242k (3 to 5 turns over 3 runs); lx shipped + Rust embeddings 176k (2 to 3 turns over 3 runs), against lx + Rust embeddings + token-goat 177k (2 to 3 turns over 3 runs); lx, every new command 130k (2 turns over 3 runs), against lx, every new command + token-goat 131k (2 turns over 3 runs); lx ask, one call 130k (2 turns over 3 runs), against lx ask + token-goat read 130k (2 turns over 3 runs).
- Does adding token-goat beside lx change anything? Each pair is the same lx build without it and with it. Tier B, one agent: lx, as shipped after round 1 217k (3 to 4 turns over 3 runs), against lx shipped + token-goat 196k (3 turns over 2 runs); lx shipped + Rust embeddings 129k (2 turns over 2 runs), against lx + Rust embeddings + token-goat 129k (2 turns over 2 runs); lx, every new command 130k (2 turns over 2 runs), against lx, every new command + token-goat 130k (2 turns over 2 runs); lx ask, one call 129k (2 turns over 2 runs), against lx ask + token-goat read 129k (2 turns over 2 runs).
- Do the engines do as well without lx in front of them? lx with every command against the raw commands of the engines it wraps. Tier C, one agent: lx, every new command 130k (2 turns over 3 runs), against Rust index + graphify + semble 289k (3 to 5 turns over 3 runs); lx, every new command 130k (2 turns over 3 runs), against Rust index + token-goat + graphify + semble 311k (4 to 5 turns over 3 runs); lx, every new command 130k (2 turns over 3 runs), against Rust index + token-goat 282k (3 to 5 turns over 3 runs).
- Do the engines do as well without lx in front of them? lx with every command against the raw commands of the engines it wraps. Tier B, one agent: lx, every new command 130k (2 turns over 2 runs), against Rust index + graphify + semble 428k (5 to 7 turns over 2 runs); lx, every new command 130k (2 turns over 2 runs), against Rust index + token-goat + graphify + semble 314k (3 to 6 turns over 2 runs); lx, every new command 130k (2 turns over 2 runs), against Rust index + token-goat 393k (5 to 6 turns over 2 runs).
- Tier C, scout, one agent: no index used 365k tokens and $0.0074 over 5 runs. Among the 28 methods run at least twice and fully correct every time, the leanest was Shipped: lx ask, embeddings on at 130k tokens and $0.0019 over 3 runs, 64% fewer tokens.
- Tier B, coder, one agent: no index used 281k tokens and $0.103 over 3 runs. Among the 23 methods run at least twice and fully correct every time, the leanest was lx ask + token-goat read at 129k tokens and $0.0309 over 2 runs, 54% fewer tokens. Most methods ran once on this tier; the lowest single run was lx improved + Rust embeddings at 128k tokens.
- Tier A, apex, one agent: no index used 293k tokens and $0.294 over 2 runs. Only one method was run twice on this tier and was fully correct both times: lx, as shipped after round 1 at 272k tokens and $0.164 over 2 runs, 7% fewer tokens. Most methods ran once on this tier; the lowest single run was lx ask + token-goat read at 135k tokens.
- The model tier moves cost far more than the method does. With the start-up context cached, the same no-index job cost $0.0074 on Tier C (98% correct), $0.103 on Tier B (100%) and $0.294 on Tier A (100%). Send lookups to the cheapest tier.
- Orchestration buys accuracy and it is the expensive part. With Tier C subagents the orchestrator's two calls were 97% of the run's cost. Without a reviewer, 4 of 38 three-subagent runs returned a wrong detail; with the orchestrator checking, 44 of 44 orchestrated runs ended fully correct.
- A cold start costs far more than a warm one. Writing the 63k start-up context to the cache costs $0.314 on Tier A against $0.0126 to read it back, 25 times as much. Subagents started together all pay the cold price; started one after another within five minutes they share the cached part.

## One agent

One agent answers all three questions. Fewest tokens first within each tier.

| Method | Tier | Runs | Turns | Start-up | Re-read | Added | Output | Total tokens | Cost warm | Cost cold | Correct |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
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
| C11 lx about + Rust cards | C | 3 | 2.3 | 63,070 | 85,179 | 3,858 | 570 | 152,676 | $0.0022 | $0.0095 | 100% |
| L2 lx + cards | C | 3 | 2.3 | 63,028 | 85,233 | 3,779 | 952 | 152,991 | $0.0024 | $0.0097 | 100% |
| A8 lx improved + Rust embeddings | C | 2 | 2.5 | 62,637 | 95,472 | 3,880 | 706 | 162,695 | $0.0024 | $0.0096 | 95% |
| R1 lx shipped + Rust embeddings | C | 3 | 2.7 | 63,012 | 106,959 | 4,197 | 1,368 | 175,536 | $0.0029 | $0.0102 | 100% |
| C2 lx + Rust embeddings + token-goat | C | 3 | 2.7 | 63,097 | 107,452 | 4,956 | 1,010 | 176,514 | $0.0028 | $0.0101 | 100% |
| C6 token-goat + graphify + semble | C | 3 | 2.7 | 63,076 | 108,610 | 7,437 | 1,006 | 180,129 | $0.0031 | $0.0104 | 100% |
| A4 lx first version + Rust embeddings | C | 2 | 3.0 | 62,635 | 128,320 | 3,676 | 837 | 195,468 | $0.0028 | $0.0100 | 100% |
| A9 lx, as shipped after round 1 | C | 6 | 3.0 | 62,852 | 129,520 | 3,899 | 773 | 197,045 | $0.0028 | $0.0100 | 99% |
| A6 token-goat + lx improved | C | 2 | 3.0 | 62,729 | 129,008 | 4,732 | 798 | 197,267 | $0.0029 | $0.0101 | 99% |
| S4 Shipped: all commands, embeddings on | C | 3 | 3.0 | 63,171 | 129,759 | 3,929 | 730 | 197,588 | $0.0028 | $0.0100 | 100% |
| A5 token-goat alone | C | 2 | 3.0 | 62,636 | 130,408 | 5,981 | 264 | 199,290 | $0.0028 | $0.0100 | 100% |
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
| A0 No index | C | 5 | 5.0 | 62,702 | 281,856 | 15,960 | 3,990 | 364,508 | $0.0074 | $0.0146 | 98% |
| A1 graphify + semble | C | 2 | 7.0 | 62,630 | 407,221 | 8,293 | 1,879 | 480,023 | $0.0067 | $0.0139 | 96% |
| A8 lx improved + Rust embeddings | B | 1 | 2.0 | 62,640 | 62,638 | 2,710 | 430 | 128,418 | $0.0361 | $0.180 | 100% |
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
| C6 token-goat + graphify + semble | B | 2 | 2.5 | 63,079 | 99,420 | 2,700 | 456 | 165,656 | $0.0438 | $0.189 | 100% |
| A6 token-goat + lx improved | B | 1 | 3.0 | 62,732 | 128,176 | 3,072 | 271 | 194,251 | $0.0486 | $0.193 | 100% |
| C9 lx finds, token-goat reads | B | 2 | 3.0 | 63,071 | 130,251 | 1,800 | 390 | 195,512 | $0.0471 | $0.192 | 100% |
| S1 Shipped: lx ask, default index | B | 3 | 3.0 | 63,031 | 130,801 | 1,416 | 277 | 195,526 | $0.0451 | $0.190 | 100% |
| S2 Shipped: all commands, default index | B | 3 | 3.0 | 63,182 | 131,103 | 1,368 | 300 | 195,954 | $0.0453 | $0.191 | 100% |
| C3 lx shipped + token-goat | B | 2 | 3.0 | 63,121 | 130,351 | 2,157 | 434 | 196,062 | $0.0484 | $0.194 | 100% |
| N2 lx, every new command, no Rust embeddings | B | 2 | 3.0 | 63,182 | 129,228 | 3,320 | 425 | 196,154 | $0.0510 | $0.196 | 100% |
| C7 token-goat + semble | B | 2 | 3.0 | 63,045 | 130,050 | 5,106 | 531 | 198,733 | $0.0567 | $0.202 | 100% |
| A5 token-goat alone | B | 1 | 3.0 | 62,639 | 130,177 | 5,469 | 747 | 199,032 | $0.0597 | $0.204 | 100% |
| C8 token-goat + graphify | B | 2 | 3.0 | 63,034 | 134,227 | 3,363 | 406 | 201,030 | $0.0519 | $0.197 | 100% |
| A9 lx, as shipped after round 1 | B | 3 | 3.3 | 62,916 | 151,400 | 2,323 | 321 | 216,959 | $0.0519 | $0.197 | 100% |
| L1 lx + about | B | 2 | 3.5 | 63,083 | 163,347 | 1,820 | 280 | 228,529 | $0.0526 | $0.198 | 100% |
| A7 lx, improved | B | 1 | 4.0 | 62,647 | 193,715 | 3,503 | 415 | 260,280 | $0.0642 | $0.208 | 100% |
| C10 lx about + cards | B | 2 | 4.0 | 63,078 | 196,714 | 2,072 | 366 | 262,230 | $0.0608 | $0.206 | 100% |
| A1 graphify + semble | B | 1 | 4.0 | 62,633 | 197,221 | 5,763 | 456 | 266,073 | $0.0709 | $0.215 | 100% |
| A2 Rust index alone, embeddings on | B | 1 | 4.0 | 62,632 | 197,316 | 6,237 | 1,003 | 267,188 | $0.0776 | $0.222 | 100% |
| A0 No index | B | 3 | 4.0 | 62,729 | 203,924 | 11,872 | 2,023 | 280,547 | $0.103 | $0.248 | 100% |
| N1 lx ask, no Rust embeddings | B | 2 | 4.5 | 63,031 | 230,031 | 2,362 | 606 | 296,030 | $0.0706 | $0.216 | 100% |
| C1 Rust index + token-goat + graphify + semble | B | 2 | 4.5 | 63,157 | 239,939 | 9,282 | 1,222 | 313,600 | $0.0960 | $0.241 | 99% |
| C4 Rust index + token-goat | B | 2 | 5.5 | 63,078 | 314,357 | 14,724 | 1,316 | 393,476 | $0.125 | $0.271 | 99% |
| C5 Rust index + graphify + semble | B | 2 | 6.0 | 63,072 | 349,728 | 13,704 | 1,226 | 427,730 | $0.129 | $0.274 | 99% |
| C15 lx ask + token-goat read | A | 1 | 2.0 | 63,052 | 63,050 | 3,307 | 5,377 | 134,786 | $0.149 | $0.452 | 100% |
| C14 lx, every new command + token-goat | A | 1 | 2.0 | 63,310 | 63,308 | 3,051 | 7,043 | 136,712 | $0.181 | $0.485 | 100% |
| N1 lx ask, no Rust embeddings | A | 1 | 3.0 | 63,025 | 129,379 | 4,647 | 1,429 | 198,480 | $0.0903 | $0.393 | 100% |
| C13 lx, every new command | A | 1 | 3.0 | 63,225 | 129,488 | 4,347 | 1,424 | 198,484 | $0.0888 | $0.392 | 100% |
| C10 lx about + cards | A | 1 | 3.0 | 63,072 | 129,392 | 4,817 | 1,532 | 198,813 | $0.0932 | $0.396 | 100% |
| S4 Shipped: all commands, embeddings on | A | 1 | 3.0 | 63,168 | 129,712 | 4,723 | 1,457 | 199,060 | $0.0913 | $0.395 | 100% |
| C11 lx about + Rust cards | A | 1 | 3.0 | 63,067 | 129,513 | 4,660 | 1,926 | 199,166 | $0.100 | $0.403 | 100% |
| S2 Shipped: all commands, default index | A | 1 | 3.0 | 63,176 | 129,621 | 4,770 | 1,662 | 199,229 | $0.0957 | $0.399 | 100% |
| S3 Shipped: lx ask, embeddings on | A | 1 | 3.0 | 63,017 | 129,481 | 5,054 | 1,719 | 199,271 | $0.0982 | $0.401 | 100% |
| N2 lx, every new command, no Rust embeddings | A | 1 | 3.0 | 63,176 | 129,663 | 4,969 | 1,766 | 199,574 | $0.0987 | $0.402 | 100% |
| A8 lx improved + Rust embeddings | A | 1 | 3.0 | 62,634 | 128,573 | 6,307 | 2,271 | 199,785 | $0.115 | $0.416 | 100% |
| R1 lx shipped + Rust embeddings | A | 1 | 3.0 | 63,009 | 129,444 | 5,229 | 2,607 | 200,289 | $0.117 | $0.419 | 100% |
| S1 Shipped: lx ask, default index | A | 1 | 3.0 | 63,025 | 129,338 | 5,695 | 2,570 | 200,628 | $0.118 | $0.421 | 100% |
| C12 lx about + hybrid | A | 1 | 3.0 | 63,066 | 129,377 | 5,645 | 2,558 | 200,646 | $0.118 | $0.421 | 100% |
| L4 lx ask, one call | A | 1 | 3.0 | 63,017 | 129,347 | 5,608 | 2,931 | 200,903 | $0.125 | $0.428 | 100% |
| L2 lx + cards | A | 1 | 3.0 | 63,025 | 129,364 | 5,952 | 2,920 | 201,261 | $0.127 | $0.429 | 100% |
| L3 lx + hybrid search | A | 1 | 3.0 | 63,019 | 129,503 | 6,516 | 3,854 | 202,892 | $0.148 | $0.451 | 100% |
| C9 lx finds, token-goat reads | A | 1 | 3.0 | 63,065 | 129,500 | 5,556 | 5,076 | 203,197 | $0.168 | $0.471 | 100% |
| C8 token-goat + graphify | A | 1 | 3.0 | 63,028 | 131,409 | 8,756 | 4,820 | 208,013 | $0.179 | $0.482 | 100% |
| C2 lx + Rust embeddings + token-goat | A | 1 | 3.0 | 63,094 | 129,476 | 10,076 | 7,325 | 209,971 | $0.235 | $0.538 | 100% |
| A9 lx, as shipped after round 1 | A | 2 | 4.0 | 62,850 | 197,928 | 7,034 | 3,828 | 271,640 | $0.164 | $0.466 | 100% |
| L1 lx + about | A | 1 | 4.0 | 63,077 | 198,984 | 7,597 | 3,695 | 273,353 | $0.164 | $0.467 | 100% |
| C3 lx shipped + token-goat | A | 1 | 4.0 | 63,115 | 198,803 | 8,848 | 4,091 | 274,857 | $0.178 | $0.481 | 100% |
| C4 Rust index + token-goat | A | 1 | 4.0 | 63,072 | 201,694 | 8,708 | 2,978 | 276,452 | $0.156 | $0.459 | 100% |
| C7 token-goat + semble | A | 1 | 4.0 | 63,039 | 200,306 | 17,224 | 9,400 | 289,969 | $0.327 | $0.629 | 100% |
| A0 No index | A | 2 | 4.0 | 62,662 | 205,018 | 18,320 | 7,426 | 293,426 | $0.294 | $0.594 | 100% |
| A2 Rust index alone, embeddings on | A | 1 | 4.0 | 62,626 | 199,867 | 17,805 | 16,699 | 296,997 | $0.476 | $0.776 | 100% |
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
| N1 lx ask, no Rust embeddings | C | 1 | 6.0 | 187,962 | 187,956 | 8,380 | 676 | 384,974 | $0.0051 | $0.0268 | 100% |
| S3 Shipped: lx ask, embeddings on | C | 1 | 6.0 | 187,938 | 187,932 | 8,464 | 829 | 385,163 | $0.0052 | $0.0268 | 100% |
| S1 Shipped: lx ask, default index | C | 1 | 6.0 | 187,962 | 187,956 | 8,286 | 1,192 | 385,396 | $0.0054 | $0.0270 | 100% |
| C12 lx about + hybrid | C | 1 | 6.0 | 188,085 | 188,079 | 8,017 | 1,847 | 386,028 | $0.0057 | $0.0273 | 100% |
| C15 lx ask + token-goat read | C | 1 | 6.0 | 188,043 | 188,037 | 8,696 | 1,361 | 386,137 | $0.0055 | $0.0272 | 100% |
| S4 Shipped: all commands, embeddings on | C | 1 | 6.0 | 188,391 | 188,385 | 8,835 | 1,061 | 386,672 | $0.0054 | $0.0271 | 100% |
| N2 lx, every new command, no Rust embeddings | C | 1 | 6.0 | 188,415 | 188,409 | 8,379 | 1,491 | 386,694 | $0.0056 | $0.0272 | 100% |
| C13 lx, every new command | C | 1 | 6.0 | 188,562 | 188,556 | 8,496 | 1,272 | 386,886 | $0.0055 | $0.0272 | 100% |
| C14 lx, every new command + token-goat | C | 1 | 6.0 | 188,817 | 188,811 | 8,272 | 1,072 | 386,972 | $0.0053 | $0.0271 | 100% |
| A4 lx first version + Rust embeddings | C | 1 | 7.0 | 186,783 | 251,676 | 8,890 | 838 | 448,187 | $0.0059 | $0.0274 | 92% |
| A8 lx improved + Rust embeddings | C | 1 | 7.0 | 186,789 | 251,776 | 9,097 | 973 | 448,635 | $0.0060 | $0.0275 | 100% |
| L3 lx + hybrid search | C | 1 | 7.0 | 187,944 | 253,465 | 8,413 | 1,419 | 451,241 | $0.0062 | $0.0278 | 100% |
| A5 token-goat alone | C | 1 | 7.0 | 186,786 | 253,502 | 10,443 | 681 | 451,412 | $0.0060 | $0.0275 | 100% |
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
| A7 lx, improved | C | 1 | 8.0 | 186,810 | 316,948 | 9,051 | 911 | 513,720 | $0.0066 | $0.0281 | 100% |
| L2 lx + cards | C | 1 | 8.0 | 187,962 | 319,227 | 9,140 | 1,256 | 517,585 | $0.0068 | $0.0285 | 100% |
| R1 lx shipped + Rust embeddings | C | 1 | 8.0 | 187,914 | 318,885 | 9,230 | 1,628 | 517,657 | $0.0070 | $0.0286 | 100% |
| A2 Rust index alone, embeddings on | C | 1 | 8.0 | 186,765 | 318,893 | 11,496 | 2,396 | 519,550 | $0.0077 | $0.0292 | 100% |
| C7 token-goat + semble | C | 1 | 8.0 | 188,004 | 320,771 | 11,222 | 1,587 | 521,584 | $0.0073 | $0.0289 | 100% |
| C4 Rust index + token-goat | C | 1 | 8.0 | 188,103 | 322,610 | 14,328 | 2,251 | 527,292 | $0.0080 | $0.0297 | 100% |
| C5 Rust index + graphify + semble | C | 1 | 8.0 | 188,085 | 320,367 | 27,514 | 2,453 | 538,419 | $0.0098 | $0.0314 | 100% |
| A9 lx, as shipped after round 1 | C | 2 | 8.5 | 187,436 | 351,878 | 10,084 | 1,429 | 550,828 | $0.0074 | $0.0289 | 100% |
| A0 No index | C | 2 | 8.5 | 186,874 | 353,722 | 17,254 | 2,200 | 560,051 | $0.0087 | $0.0302 | 75% |
| L1 lx + about | C | 1 | 9.0 | 188,118 | 384,402 | 10,275 | 1,889 | 584,684 | $0.0080 | $0.0296 | 100% |
| C1 Rust index + token-goat + graphify + semble | C | 1 | 9.0 | 188,340 | 388,808 | 14,829 | 2,473 | 594,450 | $0.0089 | $0.0305 | 100% |
| A3 lx, first version | C | 1 | 10.0 | 186,768 | 448,933 | 10,129 | 1,142 | 646,972 | $0.0082 | $0.0297 | 100% |
| A1 graphify + semble | C | 1 | 13.0 | 186,768 | 649,935 | 14,019 | 1,072 | 851,794 | $0.0107 | $0.0321 | 92% |

## Orchestrator + subagents

A top-tier orchestrator writes three briefs, three subagents answer, the orchestrator checks and corrects. The tier shown is the subagents' tier. Fewest tokens first within each tier.

| Method | Tier | Runs | Turns | Start-up | Re-read | Added | Output | Total tokens | Cost warm | Cost cold | Correct |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| C13 lx, every new command | C | 1 | 9.0 | 314,405 | 252,352 | 12,072 | 19,462 | 598,291 | $0.415 | $1.04 | 100% |
| S2 Shipped: all commands, default index | C | 1 | 10.0 | 314,228 | 318,889 | 13,650 | 8,703 | 655,470 | $0.244 | $0.868 | 100% |
| A8 lx improved + Rust embeddings | C | 1 | 11.0 | 312,041 | 381,303 | 15,521 | 9,343 | 718,208 | $0.247 | $0.868 | 100% |
| C15 lx ask + token-goat read | C | 1 | 11.0 | 313,716 | 384,593 | 13,520 | 8,498 | 720,327 | $0.215 | $0.838 | 100% |
| N1 lx ask, no Rust embeddings | C | 1 | 11.0 | 313,617 | 384,155 | 14,522 | 8,651 | 720,945 | $0.226 | $0.850 | 100% |
| S4 Shipped: all commands, embeddings on | C | 1 | 11.0 | 314,177 | 384,948 | 14,353 | 8,681 | 722,159 | $0.220 | $0.844 | 100% |
| C12 lx about + hybrid | C | 1 | 11.0 | 313,769 | 383,676 | 15,397 | 10,426 | 723,268 | $0.264 | $0.887 | 100% |
| L4 lx ask, one call | C | 1 | 11.0 | 313,573 | 384,098 | 16,277 | 9,822 | 723,770 | $0.260 | $0.883 | 100% |
| C9 lx finds, token-goat reads | C | 1 | 12.0 | 313,765 | 449,529 | 16,302 | 7,638 | 787,234 | $0.218 | $0.842 | 100% |
| S1 Shipped: lx ask, default index | C | 1 | 12.0 | 313,617 | 449,442 | 15,157 | 9,140 | 787,356 | $0.227 | $0.850 | 100% |
| N2 lx, every new command, no Rust embeddings | C | 1 | 12.0 | 314,221 | 450,351 | 15,050 | 8,972 | 788,594 | $0.230 | $0.854 | 100% |
| L3 lx + hybrid search | C | 1 | 12.0 | 313,581 | 449,020 | 15,705 | 10,389 | 788,695 | $0.249 | $0.872 | 100% |
| C11 lx about + Rust cards | C | 1 | 12.0 | 313,773 | 449,657 | 16,172 | 9,661 | 789,263 | $0.246 | $0.870 | 100% |
| A4 lx first version + Rust embeddings | C | 1 | 12.0 | 312,033 | 446,096 | 22,319 | 9,746 | 790,194 | $0.255 | $0.876 | 100% |
| C2 lx + Rust embeddings + token-goat | C | 1 | 12.0 | 313,881 | 451,961 | 17,831 | 8,795 | 792,468 | $0.237 | $0.860 | 100% |
| C14 lx, every new command + token-goat | C | 1 | 12.0 | 314,745 | 452,263 | 19,414 | 7,533 | 793,955 | $0.227 | $0.852 | 100% |
| S3 Shipped: lx ask, embeddings on | C | 1 | 12.0 | 313,573 | 449,514 | 22,669 | 9,371 | 795,127 | $0.238 | $0.861 | 100% |
| C7 token-goat + semble | C | 1 | 12.0 | 313,661 | 453,165 | 17,244 | 12,043 | 796,113 | $0.267 | $0.890 | 100% |
| C8 token-goat + graphify | C | 1 | 12.0 | 313,617 | 455,629 | 20,362 | 9,825 | 799,433 | $0.259 | $0.883 | 100% |
| C6 token-goat + graphify + semble | C | 1 | 12.0 | 313,806 | 455,469 | 20,609 | 10,640 | 800,524 | $0.281 | $0.905 | 100% |
| A9 lx, as shipped after round 1 | C | 2 | 12.5 | 312,903 | 481,202 | 15,707 | 9,943 | 819,755 | $0.256 | $0.878 | 100% |
| L1 lx + about | C | 1 | 13.0 | 313,813 | 515,064 | 14,569 | 8,874 | 852,320 | $0.209 | $0.832 | 100% |
| A5 token-goat alone | C | 1 | 13.0 | 312,037 | 514,903 | 16,658 | 9,495 | 853,093 | $0.228 | $0.849 | 100% |
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
| A0 No index | C | 2 | 17.0 | 312,174 | 793,674 | 43,316 | 22,606 | 1,171,770 | $0.565 | $1.19 | 100% |
| A1 graphify + semble | C | 1 | 23.0 | 312,098 | 1,184,154 | 25,103 | 12,426 | 1,533,781 | $0.308 | $0.929 | 100% |
| A7 lx, improved | B | 1 | 14.0 | 312,078 | 580,884 | 15,611 | 9,676 | 918,249 | $0.418 | $1.45 | 100% |
| A0 No index | B | 1 | 14.0 | 311,442 | 588,067 | 36,692 | 10,645 | 946,846 | $0.518 | $1.55 | 100% |
| A6 token-goat + lx improved | B | 1 | 15.0 | 312,418 | 643,462 | 18,061 | 9,887 | 983,828 | $0.439 | $1.47 | 100% |
| A7 lx, improved | A | 1 | 15.0 | 312,060 | 650,087 | 30,955 | 22,063 | 1,015,165 | $0.788 | $2.29 | 100% |
| A6 token-goat + lx improved | A | 1 | 15.0 | 312,400 | 655,881 | 34,025 | 25,528 | 1,027,834 | $0.874 | $2.37 | 100% |
| A0 No index | A | 1 | 15.0 | 311,424 | 659,009 | 55,212 | 31,159 | 1,056,804 | $1.09 | $2.59 | 100% |

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

## How it was measured

- Repository: 86 Python files, 2.97 MB, one commit, read-only for every agent. Ground truth for the three questions comes from Python's syntax tree, and every answer is graded against it by a script.
- Tiers: C is the Haiku class at low effort, B the Sonnet class at medium effort, A the Opus class at maximum effort. Orchestrators always ran at Tier A. Model names are resolved at run time by alias.
- Tokens are the usage fields the API returned for each request, summed per agent. Start-up context is the whole prompt of an agent's first request; re-read is cache-read tokens on later requests; added is new context written on later requests (tool results and the agent's own earlier output).
- Cost uses list prices per million tokens: Tier A $4 in, $20 out, $0.20 cache read; Tier B $2, $10, $0.20; Tier C $0.10, $0.50. Cache writes are priced at 1.25 times input and the Tier C cache read at a tenth of input, which are the published multipliers rather than figures printed for these models. Warm and cold differ only in how the start-up context is priced: all read from cache, or all written.
- The orchestrator's planning prompt is the same for every method and tier, so it was run 3 times and its briefs reused; a run that reused them is charged the mean of the measured planning calls.
- Limits. One repository, one language, three easy questions, and between one and six runs per cell: a difference of one turn between two methods is inside the noise, and a single-run cell is an example, not an average. Every agent carried the test machine's client, skills and connectors, so the start-up figure belongs to that machine and is not a constant. Wall-clock seconds in round 2 were taken while another job held every processor core, so only round 1 timings mean anything; token counts do not depend on machine load.
- lx changed during the test and each row names the build it was measured with: first version, improved, shipped after round 1, the round 2 commands (about, cards, hybrid, ask), and the build that ships, whose merged search also asks the Rust index's name and doc search. Rows marked default index ran on a copy of the repository indexed without Rust embeddings.
