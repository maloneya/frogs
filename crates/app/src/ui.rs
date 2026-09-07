//! Interaction state for the attack panel. Drawing only borrows this state;
//! edits leave it as typed requests, consumed before the next simulation tick.

use arpg_core::Report;
use arpg_sim::RecoveryTicks;

#[derive(Clone, Copy)]
pub(crate) enum MenuKey {
    Toggle,
    Close,
    Decrease,
    Increase,
    Reset,
}

/// There is one focused control today. Add selection state when a second
/// control needs it; the panel description itself is rebuilt every frame.
#[derive(Default)]
pub(crate) struct Menu {
    open: bool,
    /// Coalesced until a tick: two increments before that tick still mean +2.
    pending: Option<RecoveryTicks>,
}

impl Menu {
    pub(crate) fn open(&self) -> bool {
        self.open
    }

    pub(crate) fn pending(&self) -> Option<RecoveryTicks> {
        self.pending
    }

    pub(crate) fn recovery(&self, applied: RecoveryTicks) -> RecoveryTicks {
        self.pending.unwrap_or(applied)
    }

    pub(crate) fn on_key(&mut self, key: MenuKey, applied: RecoveryTicks) {
        match key {
            MenuKey::Toggle => self.open = !self.open,
            MenuKey::Close => self.open = false,
            _ if !self.open => {}
            MenuKey::Reset => self.pending = Some(RecoveryTicks::default()),
            MenuKey::Decrease | MenuKey::Increase => {
                let current = self.recovery(applied).get();
                let next = match key {
                    MenuKey::Decrease => current.saturating_sub(1).max(RecoveryTicks::MIN),
                    _ => current.saturating_add(1).min(RecoveryTicks::MAX),
                };
                self.pending = Some(RecoveryTicks::try_from(next).expect("bounded stepper"));
            }
        }
    }

    /// Called only at the input/sim boundary. A zero-tick frame leaves it alone.
    pub(crate) fn take_recovery(&mut self) -> Option<RecoveryTicks> {
        self.pending.take()
    }

    pub(crate) fn report(&self, out: &mut Report) {
        let Self { open, pending } = self;
        out.bool("attack_menu_open", *open);
        out.bool("captures_gameplay", *open);
        out.bool("recovery_pending", pending.is_some());
        out.int("pending_recovery_ticks", u64::from(pending.map_or(0, RecoveryTicks::get)));
    }
}
