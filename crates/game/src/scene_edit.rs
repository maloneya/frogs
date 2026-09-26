//! Relationship-safe operations on authored gameplay.
use crate::{GameScene, EncounterAction};
impl GameScene {
    /// Remove an unreferenced door component, keeping its physical body.
    pub fn remove_door(&mut self, index: usize) -> Result<(), &'static str> {
        if index >= self.doors.len() { return Err("door no longer exists"); }
        self.remove_action_target(EncounterAction::UnlockDoor(index))?;
        self.doors.remove(index);
        Ok(())
    }

    /// Remove an unreferenced chest component, keeping its physical body.
    pub fn remove_chest(&mut self, index: usize) -> Result<(), &'static str> {
        if index >= self.chests.len() { return Err("chest no longer exists"); }
        self.remove_action_target(EncounterAction::UnlockChest(index))?;
        self.chests.remove(index);
        Ok(())
    }

    fn remove_action_target(&mut self, target: EncounterAction) -> Result<(), &'static str> {
        if self.encounters.iter().any(|encounter| {
            [&encounter.on_start, &encounter.on_success, &encounter.on_failure]
                .iter().any(|actions| actions.contains(&target))
        }) {
            return Err("This object is used by an encounter action and cannot be removed.");
        }
        for encounter in &mut self.encounters {
            for action in encounter.on_start.iter_mut()
                .chain(&mut encounter.on_success).chain(&mut encounter.on_failure)
            {
                match (action, target) {
                    (EncounterAction::UnlockDoor(index), EncounterAction::UnlockDoor(removed))
                    | (EncounterAction::UnlockChest(index), EncounterAction::UnlockChest(removed))
                        if *index > removed => { *index -= 1; }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}
