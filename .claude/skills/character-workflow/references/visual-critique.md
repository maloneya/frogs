# Independent visual critique

Use a fresh subagent without inherited conversation history. Its input is visual
evidence and a neutral critique instruction. Keep implementation context with the
author: source code, tests, effort spent and claimed improvements can anchor the
review. A fresh context reduces that anchoring; using the same model does not
establish an independent measurement of quality.

## Give the critic useful evidence

Choose a compact set of views that exposes the current question. Keep camera,
scale and light matched between candidates. Include the equipped character and
the relevant diagnostic view: side/skeleton for body travel, front/back for
garment volume, or contact close-ups from opposing sides for a grasp. Let the
critic request the next view when projection or occlusion makes a finding
uncertain, rather than supplying an exhaustive gallery by default.

Use the player's [review launcher](../../../../assets/characters/basic-player/README.md#review-motion)
to produce this evidence. Pass selected images from `critic/`, plus references
when relevant; keep `evidence.json` and `render.log` with the author. The packet's
neutral A/B labels preserve input order without claiming which is better.

Provide agreed visual references when judging target adherence. Without them,
limit the verdict to visible defects and clearly identified taste; the critic
cannot reconstruct the user's intended style from candidate images alone.
Label alternatives neutrally. Avoid before/after, approved/fixed or authorship
labels. For a consequential but uncertain pairwise preference, repeat with the
display order swapped; an inconsistent preference is inconclusive.

For motion, include samples at action phases and relevant intermediate times.
Label timestamps and playback cadence. Pose strips establish pose differences;
they do not certify acceleration, foot planting or feel. Request normal-speed
playback when the reviewing tool supports temporal inspection; otherwise report
the missing temporal coverage and leave it to the author's playback/game review.

## Neutral critic brief

> Inspect only the supplied images; do not read source, logs, other files or chat
> history. Identify at most three high-impact visible issues. For each, give the
> image and location, visible evidence, confidence, and whether another view is
> needed. Separate a clear defect, an ambiguous projection and a style preference.
> Do not assume candidate order implies quality. State what looks convincing,
> what cannot be judged from these images, and the most useful next observation.
> Do not infer hidden topology, implementation causes or motion quality from
> stills. With reference images, judge adherence to their relevant visible
> properties without importing unrelated details.

## Close the loop

The author evaluates findings against the brief, obtains missing views where
needed and fixes the responsible shared relationship. Do not turn a speculative
critique into an unquestioned requirement. Re-render the same evidence and verify
whether the visible issue actually changed; an explanation of the code change
does not close a visual finding. Keep rejected candidates and the strongest
provisional version available. If the same defect survives repeated revisions,
change the observation or method rather than extending the same adjustment loop.

Use one critic per meaningful checkpoint, with a focused follow-up for unresolved
findings. Do not create a standing committee, score threshold or endless review
loop. Technical admission, runtime evidence and user judgment retain their own
roles; a critic's approval does not replace them or imply user approval.

## Research behind this protocol

- [BlenderAlchemy](https://arxiv.org/html/2404.17672v3): compare rendered
  hypotheses and retain the best prior candidate when a new edit is worse.
- [LL3M](https://arxiv.org/html/2508.08228v1): render-based criticism and subsequent
  verification are distinct jobs; its limitations include missed spatial defects.
- [VIGA](https://arxiv.org/html/2601.11109v1): actively choose viewpoints and keep
  bounded iteration memory instead of relying on one final render.
- [3D-DefectBench](https://arxiv.org/abs/2607.10826): rendering and rubric affect
  judge reliability, and tested judges still trail trained human labelers.
- [Judging to Improve](https://arxiv.org/html/2606.20364v1): presentation order and
  image overload can distort pairwise judgments in the studied setup.

These studies motivate the protocol; they do not establish its performance on
this game's characters or animations. Calibrate changes to the protocol against
retained examples of known defects and accepted work before claiming a reduction
in user corrections.
