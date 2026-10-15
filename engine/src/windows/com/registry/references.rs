use super::{
    ComError32, ComIdentity32, ComRegistry32, GuestAddress, ReferenceState32, ReferenceTransition32,
};

#[derive(Clone, Copy)]
struct ReferenceChange32 {
    slot: usize,
    references: i64,
    holds: i64,
}

#[cfg(test)]
mod tests;

struct ReferencePlan32 {
    changes: Vec<ReferenceChange32>,
    destroyed: Vec<ComIdentity32>,
    transitions: Vec<ReferenceTransition32>,
}

pub(crate) struct OwnedReferenceChanges32 {
    plan: OwnedReferencePlan32,
}

enum OwnedReferencePlan32 {
    Inline {
        changes: [ReferenceChange32; 2],
        transitions: [ReferenceTransition32; 2],
        count: usize,
        destroyed: Vec<ComIdentity32>,
    },
    Heap(ReferencePlan32),
}

impl OwnedReferenceChanges32 {
    pub(crate) fn destroyed(&self) -> &[ComIdentity32] {
        match &self.plan {
            OwnedReferencePlan32::Inline { destroyed, .. } => destroyed,
            OwnedReferencePlan32::Heap(plan) => &plan.destroyed,
        }
    }
    pub(crate) fn transitions(&self) -> &[ReferenceTransition32] {
        match &self.plan {
            OwnedReferencePlan32::Inline {
                transitions, count, ..
            } => &transitions[..*count],
            OwnedReferencePlan32::Heap(plan) => &plan.transitions,
        }
    }
}

impl ComRegistry32 {
    pub(crate) fn prepare_owned_reference_changes(
        &self,
        retain: &[GuestAddress],
        release: &[GuestAddress],
        hold: &[GuestAddress],
        unhold: &[GuestAddress],
    ) -> Result<OwnedReferenceChanges32, ComError32> {
        let count = retain
            .len()
            .checked_add(release.len())
            .and_then(|n| n.checked_add(hold.len()))
            .and_then(|n| n.checked_add(unhold.len()))
            .ok_or(ComError32::Capacity)?;
        if count > 2 {
            return Ok(OwnedReferenceChanges32 {
                plan: OwnedReferencePlan32::Heap(
                    self.reference_plan(retain, release, hold, unhold, true)?,
                ),
            });
        }
        let mut changes = [ReferenceChange32 {
            slot: 0,
            references: 0,
            holds: 0,
        }; 2];
        let mut used = 0;
        for (pointers, references, holds) in [
            (retain, 1, 0),
            (release, -1, 0),
            (hold, 0, 1),
            (unhold, 0, -1),
        ] {
            for pointer in pointers {
                let slot = self.slot(*pointer)?;
                let index = changes[..used]
                    .iter()
                    .position(|change| change.slot == slot)
                    .unwrap_or_else(|| {
                        used += 1;
                        used - 1
                    });
                changes[index].slot = slot;
                changes[index].references += references;
                changes[index].holds += holds;
            }
        }
        let placeholder = ReferenceTransition32 {
            identity: ComIdentity32 {
                id: super::ComObjectId32::new(0),
                identity_pointer: GuestAddress(0),
                payload: 0,
            },
            before: ReferenceState32 {
                references: 0,
                holds: 0,
            },
            after: ReferenceState32 {
                references: 0,
                holds: 0,
            },
        };
        let mut transitions = [placeholder; 2];
        let mut destroyed = Vec::new();
        for (index, change) in changes[..used].iter_mut().enumerate() {
            let object = self.objects[change.slot].as_ref().unwrap();
            change.references += i64::from(object.references);
            change.holds += i64::from(object.holds);
            if change.references < 0 || change.holds < 0 {
                return Err(ComError32::ReferenceUnderflow);
            }
            if change.references > i64::from(u32::MAX) || change.holds > i64::from(u32::MAX) {
                return Err(ComError32::Exhausted);
            }
            transitions[index] = ReferenceTransition32 {
                identity: object.identity,
                before: ReferenceState32 {
                    references: object.references,
                    holds: object.holds,
                },
                after: ReferenceState32 {
                    references: change.references as u32,
                    holds: change.holds as u32,
                },
            };
            if change.references == 0 && change.holds == 0 {
                destroyed
                    .try_reserve(1)
                    .map_err(|_| ComError32::Allocation)?;
                destroyed.push(object.identity);
            }
        }
        transitions[..used].sort_unstable_by_key(|transition| transition.identity.id);
        destroyed.sort_unstable_by_key(|identity| identity.id);
        Ok(OwnedReferenceChanges32 {
            plan: OwnedReferencePlan32::Inline {
                changes,
                transitions,
                count: used,
                destroyed,
            },
        })
    }

    pub(crate) fn can_commit_owned_reference_changes(
        &self,
        prepared: &OwnedReferenceChanges32,
    ) -> Result<(), ComError32> {
        for transition in prepared.transitions() {
            let object = self.object(transition.identity.identity_pointer)?;
            if object.identity != transition.identity
                || object.references != transition.before.references
                || object.holds != transition.before.holds
            {
                return Err(ComError32::InvalidPointer);
            }
        }
        Ok(())
    }

    pub(crate) fn commit_owned_reference_changes(
        &mut self,
        prepared: OwnedReferenceChanges32,
    ) -> Vec<ComIdentity32> {
        match prepared.plan {
            OwnedReferencePlan32::Heap(plan) => {
                self.publish_reference_changes(&plan.changes, plan.destroyed)
            }
            OwnedReferencePlan32::Inline {
                changes,
                count,
                destroyed,
                ..
            } => self.publish_reference_changes(&changes[..count], destroyed),
        }
    }

    fn publish_reference_changes(
        &mut self,
        changes: &[ReferenceChange32],
        destroyed: Vec<ComIdentity32>,
    ) -> Vec<ComIdentity32> {
        for change in changes {
            if change.references == 0 && change.holds == 0 {
                let object = self.objects[change.slot].take().unwrap();
                self.interfaces -= object.interfaces.len();
                self.live -= 1;
            } else {
                let object = self.objects[change.slot].as_mut().unwrap();
                object.references = change.references as u32;
                object.holds = change.holds as u32;
            }
        }
        if !destroyed.is_empty() {
            let objects = &self.objects;
            self.pointers
                .retain(|pointer| objects[pointer.slot].is_some());
        }
        destroyed
    }

    pub(super) fn creation_retains(
        &self,
        retain: &[GuestAddress],
    ) -> Result<Vec<(usize, u32)>, ComError32> {
        let plan = self.reference_plan(retain, &[], &[], &[], false)?;
        let mut retained = Vec::new();
        retained
            .try_reserve_exact(plan.changes.len())
            .map_err(|_| ComError32::Allocation)?;
        retained.extend(
            plan.changes
                .into_iter()
                .map(|change| (change.slot, change.references as u32)),
        );
        Ok(retained)
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
