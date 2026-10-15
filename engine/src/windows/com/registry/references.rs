use super::{
    ComError32, ComIdentity32, ComRegistry32, GuestAddress, ReferenceState32, ReferenceTransition32,
};

struct ReferenceChange32 {
    slot: usize,
    references: i64,
    holds: i64,
}

pub(crate) struct PreparedReferenceChanges32<'a> {
    registry: &'a mut ComRegistry32,
    changes: Vec<ReferenceChange32>,
    destroyed: Vec<ComIdentity32>,
}

struct ReferencePlan32 {
    changes: Vec<ReferenceChange32>,
    destroyed: Vec<ComIdentity32>,
    transitions: Vec<ReferenceTransition32>,
}

impl PreparedReferenceChanges32<'_> {
    pub(crate) fn destroyed(&self) -> &[ComIdentity32] {
        &self.destroyed
    }

    pub(crate) fn publish(self) -> Vec<ComIdentity32> {
        let Self {
            registry,
            changes,
            destroyed,
        } = self;
        for change in changes {
            if change.references == 0 && change.holds == 0 {
                let object = registry.objects[change.slot].take().unwrap();
                registry.interfaces -= object.interfaces.len();
                registry.live -= 1;
            } else {
                let object = registry.objects[change.slot].as_mut().unwrap();
                object.references = change.references as u32;
                object.holds = change.holds as u32;
            }
        }
        if !destroyed.is_empty() {
            let objects = &registry.objects;
            registry
                .pointers
                .retain(|pointer| objects[pointer.slot].is_some());
        }
        destroyed
    }
}

impl ComRegistry32 {
    pub(crate) fn prepare_reference_changes(
        &mut self,
        retain: &[GuestAddress],
        release: &[GuestAddress],
        hold: &[GuestAddress],
        unhold: &[GuestAddress],
    ) -> Result<PreparedReferenceChanges32<'_>, ComError32> {
        let plan = self.reference_plan(retain, release, hold, unhold, false)?;
        Ok(PreparedReferenceChanges32 {
            registry: self,
            changes: plan.changes,
            destroyed: plan.destroyed,
        })
    }

    pub(crate) fn preview_reference_changes(
        &self,
        retain: &[GuestAddress],
        release: &[GuestAddress],
        hold: &[GuestAddress],
        unhold: &[GuestAddress],
    ) -> Result<Vec<ReferenceTransition32>, ComError32> {
        Ok(self
            .reference_plan(retain, release, hold, unhold, true)?
            .transitions)
    }

    fn reference_plan(
        &self,
        retain: &[GuestAddress],
        release: &[GuestAddress],
        hold: &[GuestAddress],
        unhold: &[GuestAddress],
        with_transitions: bool,
    ) -> Result<ReferencePlan32, ComError32> {
        let count = retain
            .len()
            .checked_add(release.len())
            .and_then(|count| count.checked_add(hold.len()))
            .and_then(|count| count.checked_add(unhold.len()))
            .ok_or(ComError32::Capacity)?;
        let mut changes = Vec::new();
        changes
            .try_reserve_exact(count)
            .map_err(|_| ComError32::Allocation)?;
        for (pointers, references, holds) in [
            (retain, 1, 0),
            (release, -1, 0),
            (hold, 0, 1),
            (unhold, 0, -1),
        ] {
            for &pointer in pointers {
                changes.push(ReferenceChange32 {
                    slot: self.slot(pointer)?,
                    references,
                    holds,
                });
            }
        }
        changes.sort_unstable_by_key(|change| change.slot);
        let mut used = 0;
        for index in 0..changes.len() {
            if used != 0 && changes[used - 1].slot == changes[index].slot {
                changes[used - 1].references += changes[index].references;
                changes[used - 1].holds += changes[index].holds;
            } else {
                changes.swap(used, index);
                used += 1;
            }
        }
        changes.truncate(used);
        let mut transitions = Vec::new();
        if with_transitions {
            transitions
                .try_reserve_exact(changes.len())
                .map_err(|_| ComError32::Allocation)?;
        }
        for change in &mut changes {
            let object = self.objects[change.slot].as_ref().unwrap();
            change.references += i64::from(object.references);
            change.holds += i64::from(object.holds);
            if change.references < 0 || change.holds < 0 {
                return Err(ComError32::ReferenceUnderflow);
            }
            if change.references > i64::from(u32::MAX) || change.holds > i64::from(u32::MAX) {
                return Err(ComError32::Exhausted);
            }
            if with_transitions {
                transitions.push(ReferenceTransition32 {
                    identity: object.identity,
                    before: ReferenceState32 {
                        references: object.references,
                        holds: object.holds,
                    },
                    after: ReferenceState32 {
                        references: change.references as u32,
                        holds: change.holds as u32,
                    },
                });
            }
        }
        let mut destroyed = Vec::new();
        destroyed
            .try_reserve_exact(
                changes
                    .iter()
                    .filter(|change| change.references == 0 && change.holds == 0)
                    .count(),
            )
            .map_err(|_| ComError32::Allocation)?;
        destroyed.extend(
            changes
                .iter()
                .filter(|change| change.references == 0 && change.holds == 0)
                .map(|change| self.objects[change.slot].as_ref().unwrap().identity),
        );
        destroyed.sort_unstable_by_key(|identity| identity.id);
        transitions.sort_unstable_by_key(|transition| transition.identity.id);
        Ok(ReferencePlan32 {
            changes,
            destroyed,
            transitions,
        })
    }
}
