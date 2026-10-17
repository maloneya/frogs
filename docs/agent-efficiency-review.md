# Codex session efficiency review — 2026-09-26

This is a historical audit of 13 root sessions beginning September 23–26,
reviewed at `955c6d9` with dirty scene edits. It is not a current worklist.
The original session files and ignored verification logs were local evidence;
they are not portable repository artifacts and may no longer exist. The full
original report and its evidence locators remain in Git history. The counts
below are retained observations, not a new measurement of the current workflow.

## Observed costs

The audit inspected 69 retained verification reports and selected child sessions.
Aggregate tool/token counts covered roots only.

| Signal | Recorded count / time | Interpretation |
|---|---:|---|
| Runs invalidated by changing inputs | 19; 917 seconds of uncached checks | Checks could not certify the final source |
| Runs containing failures in two single-enemy scenarios | 8; 718 seconds of uncached checks | Playable edits changed exact test inputs; failing fast checks themselves took 15 seconds |
| Runs encountering missing `current_corpses` baseline telemetry | 8; 1,477 seconds in benchmark checks | An infrastructure incompatibility, not evidence of a timing regression |
| Root tool calls | 1,420 | Workload size, not waste |
| Calls containing `write_stdin` | 375 | Many waits were legitimate |
| Explicit tool-output truncations | 123 | Oversized output repeatedly lost evidence |

Check times overlap and cannot be added into elapsed savings. Successful checks
inside a failed run can still provide evidence. Root response records contained
181.3 million input tokens, including 176.1 million cached, and 666,116 output
tokens, deduplicated by response ID. They exclude child usage and are not a bill
or evidence for choosing a different model.

Counting used one enumeration of retained reports: `inputs changed` in reasons
for drift, exact scenario failure names for fixture incidents, and
`current_corpses` plus an error/inconclusive marker for baseline incidents.
Only non-cached check seconds were summed; performance totals used the two
performance checks. These historical counts cannot be reproduced from the
committed repository alone. No gameplay checks were run for the original audit.

## Disposition

- **Playable fixtures:** the user chose to remove automated testing of playable
  scenes. Scenarios and benchmarks now own independent setups. The proposed
  fixture warnings and protected-save workflow were superseded; see
  [scenario ownership](verification.md#scenario-coverage-and-evidence).
- **Stable verification inputs:** `full --snapshot` now captures an isolated
  candidate and reports whether the live checkout still matches. The current
  [snapshot contract](verification.md#verify-while-work-continues) owns usage.
- **Failure diagnosis and retries:** bounded summaries, focused reproduction
  commands, exact-input correctness reuse and classified infrastructure deferral
  are implemented. [Verification](verification.md#reuse-and-retry) and
  [performance](performance.md#evidence-and-baselines) own their rules.
- **Small interaction tests and authoring boundaries:** the durable guidance is
  in [repository rules](../CLAUDE.md), the
  [sim-pass guide](../.claude/skills/add-sim-pass/SKILL.md) and the
  [shared authoring API](scene-authoring.md).
- **Review scaling and formatting:** the audit proposed adapting review breadth
  and choosing a formatting policy. Those proposals were not implemented by the
  audit; the [simplify skill](../.claude/skills/simplify/SKILL.md) still specifies
  three review agents. This historical proposal does not override that skill.

Session-level savings and the proposed ten-change follow-up study were not
measured here. Workflow mechanisms passing tests do not establish their savings.
