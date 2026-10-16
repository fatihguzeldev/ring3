use super::*;
use crate::windows::synchronization::context::Wait32;

impl SynchronizationContext32 {
    pub(crate) fn with_current_wait_objects(
        &mut self,
        process: u32,
        handles: &[u32],
        wait: impl FnOnce(&mut Self) -> Result<WaitOutcome32, SynchronizationError32>,
    ) -> Result<WaitOutcome32, SynchronizationError32> {
        type Error = SynchronizationError32;
        if !handles
            .iter()
            .any(|handle| matches!(*handle, u32::MAX | 0xffff_fffe))
        {
            return wait(self);
        }
        if process == 0 || self.pseudo_wait_slots.iter().any(Option::is_some) {
            return Err(Error::InvalidWait);
        }
        let mut slots = [None; 2];
        let mut new: [Option<Object32>; 2] = [None, None];
        let previous_object = self.next_object;
        let mut next_object = previous_object;
        for (index, handle) in [u32::MAX, 0xffff_fffe].into_iter().enumerate() {
            if !handles.contains(&handle) {
                continue;
            }
            let kind = if index == 0 {
                ObjectKind32::Process {
                    id: process,
                    terminated: false,
                }
            } else {
                ObjectKind32::Thread {
                    id: self.current_thread,
                    terminated: false,
                }
            };
            let existing = self.objects.iter().position(|object| {
                object.as_ref().is_some_and(|object| {
                    matches!((object.kind, kind),
                    (ObjectKind32::Process{id:a,..}, ObjectKind32::Process{id:b,..})
                    | (ObjectKind32::Thread{id:a,..}, ObjectKind32::Thread{id:b,..}) if a==b)
                })
            });
            let slot = if let Some(slot) = existing {
                self.objects[slot]
                    .as_ref()
                    .unwrap()
                    .bindings
                    .checked_add(1)
                    .ok_or(Error::Exhausted)?;
                slot
            } else {
                let slot = self
                    .objects
                    .iter()
                    .enumerate()
                    .find(|(slot, object)| object.is_none() && !slots.contains(&Some(*slot)))
                    .map(|(slot, _)| slot)
                    .ok_or(Error::Capacity)?;
                next_object = next_object.checked_add(1).ok_or(Error::Exhausted)?;
                new[index] = Some(Object32 {
                    id: next_object,
                    name: None,
                    manual: true,
                    handles: 0,
                    bindings: 0,
                    pseudo_only: true,
                    kind,
                });
                slot
            };
            slots[index] = Some(slot);
        }
        for (index, slot) in slots.into_iter().enumerate() {
            if let Some(slot) = slot {
                if let Some(object) = new[index].take() {
                    self.objects[slot] = Some(object);
                    self.typed_objects += 1;
                    set(&mut self.signaled, slot, false);
                }
                self.objects[slot].as_mut().unwrap().bindings += 1;
            }
        }
        self.next_object = next_object;
        self.pseudo_wait_slots = slots;
        let result = wait(self);
        self.pseudo_wait_slots = [None; 2];
        if !matches!(result, Ok(WaitOutcome32::Pending(_))) {
            self.release_pseudo_objects(slots);
            if result.is_err() {
                self.next_object = previous_object;
            }
        }
        result
    }

    pub(in crate::windows::synchronization) fn release_wait_objects(&mut self, wait: Wait32) {
        self.release_pseudo_objects(wait.pseudo_objects);
    }

    fn release_pseudo_objects(&mut self, slots: [Option<usize>; 2]) {
        for slot in slots.into_iter().flatten() {
            self.objects[slot]
                .as_mut()
                .expect("owned pseudo wait object")
                .bindings -= 1;
            self.reclaim_event(slot);
        }
    }
}
