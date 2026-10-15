"""Blender-frame samples and their presentation times for character reviews."""
import math

MELEE = {'AttackCleave': (6, 6, 10), 'AttackSlam': (9, 5, 16)}


def sampling(name, frame_range, native, fps, poses=False, requested=None):
    first, last = frame_range
    if not all(math.isfinite(n) for n in (first, last, fps)) or last <= first or fps <= 0:
        raise ValueError(f'{name}: expected a finite, nonempty action range and positive cadence')
    melee = None if native else MELEE.get(name)
    if melee:
        startup, active, recovery = melee
        total = startup + active + recovery
        seconds, looping = total / 60, False

        def frame(tick):
            phase = (tick / startup * .4 if tick < startup else
                     .4 + (tick - startup) / active * .2 if tick < startup + active else
                     .6 + (tick - startup - active) / recovery * .4)
            return first + phase * (last - first)

        def time(value):
            phase = (value - first) / (last - first)
            tick = (phase / .4 * startup if phase < .4 else
                    startup + (phase - .4) / .2 * active if phase < .6 else
                    startup + active + (phase - .6) / .4 * recovery)
            return tick / 60

        ticks = [0, startup, startup + active, startup + active + recovery / 2, total]
        landmarks = [(frame(tick), label) for tick, label in zip(
            ticks, ['Ready', 'Loaded', 'Release end', 'Recovery', 'Ready'])]
        frames = [frame(tick) for tick in range(total + 1)]
    else:
        seconds = (last - first) / fps
        looping = not native or 'Loop' in name or name.startswith(('Running', 'Walking'))
        count = max(2, round(seconds * (24 if native else 30)))
        frames = [first + (last - first) * i / (count if looping else count - 1) for i in range(count)]
        time = lambda value: (value - first) / fps
        landmarks = [(first + (last - first) * i / 4, f'{i * 25}%') for i in range(5)]
    playback = not poses and requested is None
    if requested is not None:
        frames = list(requested)
        if (not frames or any(not math.isfinite(f) or not first <= f <= last for f in frames)
                or any(a >= b for a, b in zip(frames, frames[1:]))):
            raise ValueError(f'{name}: --frames must increase strictly within {first:g}..{last:g}')
    elif poses:
        frames = [frame for frame, _ in landmarks]
    pose_indices = sorted({min(range(len(frames)), key=lambda i: abs(frames[i] - value))
                           for value, _ in landmarks})
    labels = {}
    for value, label in landmarks:
        index = min(range(len(frames)), key=lambda i: abs(frames[i] - value))
        if abs(frames[index] - value) < .00001:
            labels[str(index)] = label
    return {'frames': frames, 'times': [time(f) for f in frames], 'seconds': seconds,
            'loop': looping, 'playback': playback, 'pose_indices': pose_indices,
            'labels': labels if requested is None else {}}


def gif_durations(clip, multiplier=1):
    """Round cumulative times, preserving total cadence instead of per-frame drift."""
    times = clip['times'] + ([clip['seconds']] if clip['loop'] else [])
    delays = [(round(b * 100 * multiplier) - round(a * 100 * multiplier)) * 10
              for a, b in zip(times, times[1:])]
    if not clip['loop']:
        delays.append(800)
    if any(delay <= 0 for delay in delays):
        raise ValueError('GIF cadence is below its 10 ms resolution')
    return delays
