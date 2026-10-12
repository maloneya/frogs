---
name: investigate-performance
description: Investigate an arpg performance alert when the user requests diagnosis. Gather causal evidence, research published solutions and present feature and implementation alternatives for the user's decision before fixes.
---

# Investigate Performance

Use only when the user triggers a performance investigation. A feature agent
detecting a concern follows the [stop and handoff policy](../../../docs/performance.md#feature-work-stop-and-handoff)
and ends its work. This workflow produces a decision brief; it does not implement
an optimization or change feature scope. The user chooses the next action.

Read [repository rules](../../../CLAUDE.md), the alert, relevant evidence,
[performance measurement policy](../../../docs/performance.md), and symptom
matches in [traps](../../../docs/traps.md). Follow the current telemetry, workload
and baseline owners rather than inventing a parallel benchmark framework.

## Establish the question and research early

Identify the intended player-visible effect, changed feature, failing metric,
population, conditions and missing evidence. Separate a budget breach, a
relative regression and an invalid measurement. Inspect metric rows even when
the aggregate verdict is inconclusive. Do not infer causation from sequence or
call a suspected bottleneck established.

Before new experiments or code changes, search for published approaches to the
observed problem class. Use primary papers, engine/vendor documentation and
conference talks from their authors; read the sources and cite direct links.
Explain the mechanism and tradeoffs, not just the technique name. Revisit the
research as evidence narrows the bottleneck. Consider established algorithms
within the handwritten engine, preserving the macOS/M4/Metal target. Check
hardware/API assumptions: an NVIDIA result is not evidence of an Apple speedup.
If research access fails, report that gap instead of substituting recollection.

## Gather bounded, causal evidence

State competing hypotheses and the smallest measurement that would distinguish
them. Begin with retained raw trials, source identities and existing coarse
telemetry. Separate simulation, presentation preparation, render submission,
GPU execution and frame pacing. Renderer CPU time includes waits and does not
measure GPU execution. Follow the [GPU investigation recipe](../../../docs/performance.md#gpu-investigation)
for GPU claims; if unavailable, keep
the GPU cause unconfirmed. Use the existing opt-in physics profiler for a
simulation bottleneck. Report instrumentation overhead and unavailable coverage.

Retain the failing candidate and dirty edits. Use isolated source copies for
diagnostic changes; never reset someone else's working tree or overwrite its
evidence. Compare compatible pre-feature and candidate states under identical
release workloads, assets except the tested feature, camera, population, inputs,
present mode and measurement conditions. Dirty work needs an exact source
manifest or snapshot, not just a commit name. If the pre-feature state cannot be
reconstructed, state that limitation; an ancestor may contain unrelated changes.

Use one-variable ablations to attribute cost to geometry, shadow generation,
filtering, weather or the relevant mechanic. An ablation is a disposable
diagnostic variant, not a shipping fix. Record its changed inputs and effects;
its results do not certify the complete feature. Explain interactions when costs
are not additive. Distinguish measured attribution from inference and unknowns.

Bound the investigation before running: by default, at most three focused
experiment batches, each testing one stated hypothesis with the existing trial
repetitions. Existing evidence review and research are outside that measurement
limit. Allow one replacement batch for a demonstrated infrastructure/condition
failure, after correcting it; count timing noise as evidence, not infrastructure.
Never rerun unchanged conditions merely to seek a pass. Live measurements need a
reserved foreground session through ARPG_HARNESS, without competing builds or
companion polling. Follow the [playtest guide](../playtest/SKILL.md).

Stop measuring when evidence discriminates the plausible options or that bound
is reached. If unresolved, deliver the evidence so far, the uncertainty and the
single next experiment that would help the user's decision. Do not build a new
profiling subsystem, benchmark framework or production optimization as part of
diagnosis. Temporary diagnostic probes must stay isolated and narrowly scoped.

## Present options and return the decision

Deliver a concise brief with linked raw artifacts and source identities:

- **Finding:** what fails, trial values versus baseline/limit, measurement
  validity, and what is proved, inferred or still unknown.
- **Feature connection:** the player-visible requirement, feature work that
  adds cost, causal ablation results and remaining interactions or gaps.
- **Published approaches:** cited mechanisms, evidence of their use, assumptions
  and applicability to this engine and hardware.
- **Options:** a comparison table covering preserving the effect with a known
  optimization, approximating or limiting the effect, redesigning/removing the
  costly feature, and deferring it. Include only plausible alternatives. For
  each, state visible/gameplay consequences, expected cost reduction and its
  evidence or uncertainty, complexity/maintenance cost, risks and validation.
- **Recommendation:** a reasoned choice, an alternative if the user prioritizes
  fidelity or simplicity, and what the evidence cannot yet justify.

End by asking which approach the user wants. Diagnosis ends there. Do not choose
on their behalf, automatically start another agent or continue implementing.
The boundary comes from the user's requested process, recorded in the repository
stop policy. Further investigation also requires the user's direction.

Once the user selects a path, implement only that scope. Agree on observable
acceptance criteria before edits, retain the failing evidence, and validate
correctness plus affected performance under compatible conditions. Do not relax
gates or replace baselines without an explicit user decision about requirements.
A new regression triggers the feature-work stop rule again.
