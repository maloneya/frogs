//! Optional per-tick physics timing. No clock or measurement enters simulation.
use arpg_core::Report;
use arpg_sim::{CrowdWork, PhysicsCounter, PhysicsPhase, StepObserver};
use std::time::Instant;

#[derive(Default)]
pub(super) struct Profile {
    phase: Option<(PhysicsPhase, Instant)>,
    nanos: [u64; PhysicsPhase::ALL.len()],
    counts: [u64; PhysicsCounter::ALL.len()],
}
impl Profile {
    fn close_phase(&mut self, now: Instant) {
        if let Some((phase, began)) = self.phase.take() {
            self.nanos[phase as usize] += now.duration_since(began).as_nanos() as u64;
        }
    }
    pub(super) fn include(&mut self, other: &Self) {
        assert!(self.phase.is_none() && other.phase.is_none());
        for (sum, value) in self.nanos.iter_mut().zip(other.nanos) {
            *sum += value;
        }
        for (sum, value) in self.counts.iter_mut().zip(other.counts) {
            *sum += value;
        }
    }
    pub(super) fn report(&self, out: &mut Report) {
        assert!(self.phase.is_none(), "profile ends at the crowd boundary");
        out.object("phase_ns", |out| {
            for phase in PhysicsPhase::ALL {
                out.int(phase.label(), self.nanos[phase as usize]);
            }
        });
        out.object("work", |out| {
            for count in PhysicsCounter::ALL {
                out.int(count.label(), self.counts[count as usize]);
            }
        });
    }
}
impl StepObserver for Profile {
    const PHYSICS_DETAILS: bool = true;
    fn crowd_started(&mut self) {
        assert!(self.phase.is_none());
        self.phase = Some((PhysicsPhase::Other, Instant::now()));
    }
    fn crowd_finished(&mut self, _: CrowdWork) {
        self.close_phase(Instant::now());
    }
    fn physics_phase(&mut self, phase: PhysicsPhase) {
        assert!(self.phase.is_some());
        let now = Instant::now();
        self.close_phase(now);
        self.phase = Some((phase, now));
    }
    fn physics_count(&mut self, counter: PhysicsCounter, amount: u64) {
        self.counts[counter as usize] += amount;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn exclusive_phases_and_multiple_crowd_passes_share_one_tick() {
        let mut profile = Profile::default();
        let start = Instant::now();
        profile.phase = Some((PhysicsPhase::Broadphase, start));
        profile.close_phase(start + Duration::from_nanos(20));
        profile.phase = Some((PhysicsPhase::Solve, start));
        profile.close_phase(start + Duration::from_nanos(30));
        profile.phase = Some((PhysicsPhase::Broadphase, start));
        profile.close_phase(start + Duration::from_nanos(7));
        assert_eq!(profile.nanos.iter().sum::<u64>(), 57);
        assert_eq!(profile.nanos[PhysicsPhase::Broadphase as usize], 27);
        profile.physics_count(PhysicsCounter::CollisionPasses, 2);
        let mut total = Profile::default();
        total.include(&profile);
        total.include(&profile);
        assert_eq!(total.nanos.iter().sum::<u64>(), 114);
        assert_eq!(total.counts[PhysicsCounter::CollisionPasses as usize], 4);
        let mut report = Report::default();
        total.report(&mut report);
        assert!(report.finish().contains("\"collision_passes\":4"));
    }
}
