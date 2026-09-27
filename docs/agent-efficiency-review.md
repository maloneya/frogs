# Codex session efficiency review — 26 September 2026

The largest remaining opportunity is to make **verification operate on stable inputs while humans and agents keep working**. The recurring costs were shared-checkout interference, editable scenes doubling as test fixtures, and repeated investigation of infrastructure failures. Smaller gains come from bounded output, less polling, and scaling review effort to the change.

Follow-up decision: the user chose to drop automated testing of playable scenes
entirely. Tests own their setups, with a small set of complex test-only
compositions. This supersedes the fixture-protection UI proposed below; no editor
warning or special save workflow is needed.

This is the original audit report, not a specification for that follow-up change. It distinguishes observed incidents, fixes already present, and proposed work. Repository state reviewed: `955c6d9`, with existing edits to `scenes/single_enemy.ron` and `scenes/two_torches.ron`.

**Evidence and scope**

Reviewed 13 root Codex sessions beginning September 23–26 in America/New_York, including their user requests, tool calls/results, and completion messages. Earlier September sessions were screened for context. Inspected all 69 retained `target/verify/run-*/verification.json` records and relevant logs. This is the available local sample, not a complete history of every attempt or deleted artifact. Related child sessions were inspected selectively; aggregate token/tool numbers below cover roots only.

| Observed signal | Count / recorded time | Interpretation |
|---|---:|---|
| Verification runs invalidated by changing inputs | 19 runs; 917 seconds of uncached checks | About 15.3 minutes of check execution in runs that could not certify the final source |
| Runs containing failures in the two single-enemy scenarios | 8 runs; 718 seconds of uncached checks | About 12 minutes spent in affected runs; the failing fast-suite checks themselves consumed only 15 seconds |
| Runs encountering missing `current_corpses` benchmark telemetry | 8 runs; 1,477 seconds in benchmark checks | About 24.6 minutes of benchmark execution encountering an infrastructure problem; this path is now fixed |
| Root tool calls | 1,420 | Workload size, not a measure of waste |
| Calls containing `write_stdin` | 375 | 26% of calls contained polling; many waits were legitimate |
| Tool-result messages containing the explicit truncation warning | 123 | Evidence that output selection repeatedly exceeded its budget |

Times sum saved, non-cached check durations, not elapsed user waiting time. Runs can overlap; these are not additive savings estimates. Successful checks within an affected run can still provide useful evidence. No dollar savings claim is justified by these data.

The root response-level usage records contain 181.3 million input tokens, of which 176.1 million were cached (97.2%), and 666,116 output tokens. Deduplicated by response ID; cumulative thread counters were not summed. These numbers include repeated context across requests, exclude child-agent usage, and are not a bill. The uncached input count is about 5.16 million. A model switch cannot be evaluated from this sample without task-quality and pricing comparisons.

**1. Protect test fixtures from ordinary scene experimentation — highest priority**

In **Scenario editor**, the same two scenarios repeatedly failed after the playable `single_enemy.ron` acquired five enemies. Agents preserved the user's changes, manually copied implementation files into temporary checkouts, substituted the committed fixture, and repeated verification. Both later **Audit Repo for Vibe-Code Failures** sessions encountered the problem again. It remains present: both scenarios still load `../scenes/single_enemy.ron`.

This is an ownership problem. A file optimized for frequent human experimentation also supplies exact automated predictions. The current instruction to use Save as helps, but relies on the human and every agent remembering it.

**Proposed change:** give the two rule-level scenarios a small, dedicated fixture under a fixture directory excluded from scenario discovery and the normal editor catalog. Keep genuine tests of authored game content attached to their real scene files. For those scenes, derive “used by these scenarios” information from scenario references and make an experimental copy the normal editing path. Put any save protection in the shared authoring boundary so the companion and CLI behave consistently; provide a deliberate route for intentional content-and-prediction updates.

**Acceptance:** changing the ordinary playable single-enemy scene no longer breaks the rule-level movement/damage scenarios. Tests intended to certify actual game content still detect content changes. The user can retain existing experiments without an agent manufacturing a different test checkout.

Owners: [scenario inputs](../scenarios/a_single_enemy_damages_the_player.ron), [movement scenario](../scenarios/a_hit_does_not_interrupt_movement.ron), [authoring](scene-authoring.md), [editor](scene-editor.md).

**2. Provide one supported way to verify a frozen candidate — highest priority**

**Add campfire and simple trees** and **Improve horde clear speed** worked in the same checkout at the same time. Each saw failures from the other's unfinished changes; performance measurements also encountered changed inputs or another benchmark process. The scene-editor session later copied files into temporary verification checkouts, then changed those copies while a check was running, invalidating that attempt too. The recent audit recorded another invalidation during a 100-second workspace test check.

The existing source fingerprint correctly refuses to certify moving inputs. Keep that safeguard. The missing capability is a convenient, trustworthy input snapshot.

**Proposed change:** add a snapshot option to the existing verification runner. Capture a manifest of the intended candidate, including new files and deletions, materialize it once, and verify without subsequent writes. Detect changes during capture itself. Default to the complete current candidate; selecting a commit or staged candidate must be explicit and reported. Never silently replace dirty test data with HEAD. Report the verified digest and whether it still matches the live checkout.

Use isolated worktrees for independently authorized coding tasks, or a single integration owner for shared edits. Reuse build artifacts only where safe; retain the existing per-checkout Swift cache isolation. If duplicate verification jobs remain common, share results for an identical running key rather than launching another copy.

**Acceptance:** editing the live checkout during a check cannot alter the snapshot. The completed report certifies that snapshot only. A changed live checkout is visibly unverified. No ad hoc copy lists, fixture restoration, or source-hash exemptions are needed.

**Implemented:** `python3 scripts/verify.py full --snapshot` captures current inputs,
checks them in an isolated checkout with a reusable private build cache, and saves
the manifest and source archive. Reports distinguish snapshot coverage from whether
the live checkout still matches. See [verification](verification.md#verify-while-work-continues).

Owner: [verification runner](../scripts/verify.py). Estimated implementation effort: medium; likely highest ongoing payoff alongside fixture protection.

**3. Make verification failures explain themselves in one response**

The runner currently prints a status, log path, and omitted checks. Sessions then use `tail`, `rg`, and more polling calls to discover whether the cause is a compiler error, a changed fixture, input drift, stale infrastructure, or timing noise. These have different remedies.

**Proposed change:** extend the existing result with a compact failure summary: failing check, first actionable diagnostic, affected scenario/input paths where known, source-change information, evidence path, and the narrow retry command. Summarize only supported classifications; retain an “unclassified” result and full logs. Surface known repeated failures without marking them passed or suppressing correctness execution. For source drift, a path-level manifest can explain which inputs changed without dumping file contents.

For agents, return bounded summaries and artifact paths. Wait on running checks for a useful interval, usually 10–30 seconds, rather than repeatedly requesting one-second polls. Continue independent work only when it cannot mutate the candidate under test.

**Acceptance:** an agent can identify the cause and correct next command from the runner's first completion response. Measure extra diagnostic tool calls per failed check before and after. Implementation effort: small to medium.

**Implemented:** the runner prints bounded failure summaries and stores them in
`verification.json`, including diagnostic locations, known affected paths and a
focused retry command. It distinguishes source drift, missing evidence, compiler
and scenario/test failures, and structured benchmark outcomes; unknown failures
remain unclassified. Regression tests require useful completion output without a
separate log read. Session-level savings have not yet been measured.

**4. Preserve the benchmark fixes already shipped**

The corpse change added required telemetry. Older baseline records lacked it. Expensive measurements ran before baseline discovery, and the error path could discard measurements. Later work repeatedly encountered the same incompatibility. This was not evidence of a performance regression.

**Fix benchmark validation loop** addressed this in `cd2ad93`: preflight baseline eligibility, retire incompatible evidence, preserve measurements after late failures, defer unchanged classified infrastructure failures, and maintain independent headless/live checkpoints. `847fa1a` subsequently made live benchmarking an explicit foreground session. Current [performance code](../scripts/perf.py) and [verification policy](verification.md) contain these mechanisms.

These are successful examples of replacing repeated agent judgment with an executable rule. Do not rebuild them or loosen budgets. Remaining work is to expose their status through the compact failure summary above and monitor recurrence. Genuine noisy timing still needs an inconclusive result; it must not inherit the infrastructure-retry exemption.

**5. Test a small interaction before expanding a mechanic**

In **Vary horde movement physics**, the first motor design changed damping and broke an existing knockback/chest interaction. Per-monster gait events overflowed diagnostic history in the 10,000-enemy case. Several movement expectations were discovered through successive failures. The agent's own review identified starting expensive baseline collection before establishing whether a meaningful comparison was possible.

The same session made durable improvements: authoritative spawn outcomes, selected-event assertions, bounded capture, scenario timing, fast/extended suites, and verification reuse. It reported an unchanged quick rerun taking about 0.2 seconds. Those capabilities already exist.

**Remaining workflow change:** predict a tiny Game-driven interaction first—free movement, two-body pushing, knockback recovery—then test the smallest case exercising the scale bound. Expand presentation and authoring after the mechanism composes correctly. Search affected expectations before editing them. Preserve independent predictions and exact boundary checks; do not derive expected outcomes from the new implementation merely to clear failures.

This is targeted use of existing capabilities, not a proposal to add another test framework or remove coverage.

**6. Avoid building a complete tool before settling its boundary**

In **Research World-Class Game Design**, a handwritten tuning UI and GPU surface were built and verified, then replaced with SwiftUI after discussion. **Scenario editor** subsequently moved launch ownership out of the game and extracted authoring operations from the game executable into the shared headless API.

Some of this was legitimate design evolution. It would be unfair to count all of it as agent error. The avoidable risk was extending the UI before demonstrating the full boundary: offline edit → validate → save → play, with the game consuming playable data and the launcher owning processes.

The [dependency guard](../build_support/dependencies.rs) already enforces crate boundaries. Future editor work should prove one complete authoring operation through both clients before expanding the palette.

**7. Reduce review and context overhead selectively**

The local [simplify skill](../.agents/skills/simplify/SKILL.md) always requests three review agents, each receiving the full diff. This explains some fan-out: agents were following instructions, not spontaneously wasting work. Reviews also found real defects, including the sword-beam wall-clipping regression and stale inspector selection. Preserve that value.

**Proposed skill change:** one review for a small local diff; focused independent reviews for substantial changes crossing owners. For larger work, give each reviewer the relevant diff and boundary context, and assign one owner to overlapping fixes. Measure accepted findings, elapsed time, and aggregate parent-plus-child usage. No evidence here establishes an ideal numerical diff-size threshold or a better default model.

The 123 explicit output truncations support narrower searches and reads: discover filenames before assuming paths, request the relevant section, and save full test output as an artifact. The logs include missing-path/glob errors and repeated oversized source dumps. Avoid solving this with another mandatory repository tour or a broad injected hook.

Current official OpenAI guidance also recommends narrowly triggered skills and contextual documentation, and cautions that blanket reading/testing instructions can cause unnecessary work. That supports selective guidance, not weakening this project's deterministic and lifecycle checks. [Official guidance](https://developers.openai.com/blog/rethinking-skills-and-prompts-for-gpt-6-astra).

**8. Prevent formatting and native-tool detours at their owners**

In **Tune lighting, zoom, and controls**, removing formatting churn required temporary source trees, a custom token-based “unformat” script, a failed patch across a symlink, a `KeyError`, and a missing-`rustfmt` retry. An output showed 80 changed files before recovery reduced the diff to 64. This is clear avoidable repair work, although its full cost was not separately measured.

Choose a repository formatting policy explicitly. Either establish canonical formatting in a dedicated change, or provide a supported scoped formatting command that does not recursively rewrite unrelated modules. Do not invent token-based restoration during a feature review. Keep any formatting transition separate from semantic edits.

Native companion work also tried an explicit Foundation import for a stale Swift-cache error before isolating caches by canonical checkout path. That issue and native capture limitations now have owner fixes and entries in [traps](traps.md). Treat them as resolved safeguards or documented operational constraints, not new backlog items.

**Recommended implementation order and measurement**

| Order | Work | Effort judgment | Success measure |
|---|---|---|---|
| 1 | Separate rule-test fixtures from editable scene experiments | Small | No recurrence of these two incidental fixture failures |
| 2 | Frozen candidate verification in the existing runner | Medium | No input-drift invalidation inside a successfully captured snapshot |
| 3 | Compact failure reports and narrow retry commands | Small–medium | Fewer diagnostic reads and polls per failure |
| 4 | Scale simplify review to the change | Small | Lower total review usage with comparable accepted findings and defect detection |
| 5 | Settle scoped/canonical formatting | Small policy decision; cleanup scope varies | No unrelated formatting repair during feature work |

For the next ten comparable completed changes, record first focused-check result, invalidated verification seconds, failure-to-diagnosis tool calls, repeated infrastructure signatures, explicit truncations, and parent-plus-child usage split into cached input, uncached input, and output. Also record defects found in review so cheaper execution does not hide quality loss. Compare similar work; scene tuning and a physics rewrite are not interchangeable samples. These are proposed measurements, not promised percentage savings.

**Evidence pointers and counting method**

Session titles below are the actual app titles. JSONL line numbers locate the cited observation or action; later app metadata may append to these files.

| Session | Evidence |
|---|---|
| Tune lighting, zoom, and controls | [Formatting recovery calls](/Users/avm/.codex/sessions/2026/09/23/rollout-2026-09-23T18-53-23-01a0d079-4521-7b92-9e14-023e386ffd4e.jsonl:793); wall regression at line 722 |
| Vary horde movement physics | [Agent retrospective](/Users/avm/.codex/sessions/2026/09/23/rollout-2026-09-23T21-25-55-01a0d104-eb94-72b0-b316-31b5b36fc577.jsonl:473); verification reuse at line 1069 |
| Add campfire and simple trees | [Concurrent-edit interference](/Users/avm/.codex/sessions/2026/09/23/rollout-2026-09-23T23-02-12-01a0d15d-10fb-7b62-9c70-4a4af1933fb7.jsonl:566) |
| Improve horde clear speed | [Concurrent woodland failures](/Users/avm/.codex/sessions/2026/09/23/rollout-2026-09-23T23-02-40-01a0d15d-7d67-72e1-8c46-9fc03f1adcfa.jsonl:591) |
| Add budgeted enemy corpses | [Repeated baseline incompatibility](/Users/avm/.codex/sessions/2026/09/23/rollout-2026-09-23T23-43-42-01a0d183-0ffb-7e51-b536-fa9e2dd0c0a9.jsonl:551) |
| Research World-Class Game Design | [Replacement of handwritten panel](/Users/avm/.codex/sessions/2026/09/26/rollout-2026-09-26T12-28-07-01a0de8b-9ea3-7a01-a391-cc92f744f2a0.jsonl:684); repeated benchmark problem at line 991 |
| Fix benchmark validation loop | [Root cause and repair](/Users/avm/.codex/sessions/2026/09/26/rollout-2026-09-26T16-07-15-01a0df54-3f48-7071-8189-27eda3aa99b5.jsonl:43) |
| Scenario editor | [Fixture and cache problems](/Users/avm/.codex/sessions/2026/09/26/rollout-2026-09-26T16-09-45-01a0df56-8755-7171-9ae2-83f901428d73.jsonl:1382); snapshot changed during verification at line 1420; repeated fixture failure at line 2424 |
| Audit Repo for Vibe-Code Failures — audit session | [Source changed during checks](/Users/avm/.codex/sessions/2026/09/26/rollout-2026-09-26T21-55-38-01a0e093-32c9-7e30-b673-0469576981d7.jsonl:463) |
| Audit Repo for Vibe-Code Failures — follow-up fixes | [Same fixture failures](/Users/avm/.codex/sessions/2026/09/26/rollout-2026-09-26T23-00-33-01a0e0ce-a2ef-7db0-b362-4378044d6441.jsonl:127) |

Other root sessions included in the quantitative sample: **Let’s add a very primitive hud for player health and bound abilities**, **Find performance testing guidance**, and **Tour Repo Lines by Crate**. Long conversations contain multiple user-authorized changes; a large call count alone is not evidence of inefficiency.

Verification counting: enumerate retained result files once; count a run as drift when its top-level or check reason contains `inputs changed`; count fixture incidents from exact failing scenario names in evidence logs; count corpse-telemetry incidents from logs containing `current_corpses` and an error/inconclusive marker. Sum `seconds` only for checks without `cached: true`; benchmark totals include only the two performance check names. Representative evidence: [drift](../target/verify/run-s3856rhr/verification.json), [fixture](../target/verify/run-m30uw46i/verification.json), [old baseline](../target/verify/run-95_db0mg/verification.json). These local build artifacts may later be cleaned.

No gameplay checks were run for this report. Recommendations are based on historical execution evidence and current source inspection; the existing scene edits were preserved.
