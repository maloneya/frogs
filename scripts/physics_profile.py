#!/usr/bin/env python3
"""Summarize an opt-in perf-headless --profile artifact; no benchmark gate changes."""
import argparse
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("profile", type=Path)
    parser.add_argument("--plain", type=Path, help="Matching uninstrumented trial for overhead comparison")
    args = parser.parse_args()
    data = json.loads(args.profile.read_text())
    profile = data["physics_profile"]
    if profile["version"] != 1:
        parser.error("unsupported physics profile version")
    ticks = list(profile["ticks"].values())
    steps = list(data["step_ns"].values())
    if not ticks or len(ticks) != len(steps):
        parser.error("profile must cover every sampled step")
    total = sum(steps)
    count = len(steps)
    print(f"{count} ticks, {data['start_enemies']} → {data['end_enemies']} enemies; "
          f"mean {total / count / 1e6:.3f} ms, max {max(steps) / 1e6:.3f} ms")
    phases = {key: sum(t['phase_ns'][key] for t in ticks) for key in ticks[0]['phase_ns']}
    phases['outside_physics'] = total - sum(phases.values())
    print("\nExclusive phase              mean ms     step %")
    for key, value in sorted(phases.items(), key=lambda item: -item[1]):
        print(f"{key:27} {value / count / 1e6:9.3f} {100 * value / total:10.2f}")
    print("\nWork                         mean/tick       max/tick")
    for key in ticks[0]['work']:
        values = [float(t['work'][key]) for t in ticks]
        print(f"{key:27} {sum(values) / count:13.5g} {max(values):14.5g}")
    if args.plain:
        plain = json.loads(args.plain.read_text())
        for key in ('warmup_ticks', 'sample_ticks', 'input', 'initial_enemies', 'start_enemies',
                    'end_enemies', 'population_sum', 'current_tick', 'finite', 'final_hash'):
            if plain[key] != data[key]:
                parser.error(f"uninstrumented trial differs in {key}; cannot compare overhead")
        if 'physics_profile' in plain:
            parser.error("--plain must be an uninstrumented trial")
        baseline = sum(plain['step_ns'].values()) / len(plain['step_ns'])
        print(f"\nSame outcomes/hash; plain {baseline / 1e6:.3f} ms, "
              f"observed difference {(total / count / baseline - 1) * 100:+.2f}% "
              "(includes run-to-run noise).")


if __name__ == '__main__':
    main()
