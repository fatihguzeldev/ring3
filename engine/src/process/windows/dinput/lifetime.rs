use super::{
    DirectInputRuntime32,
    memory::{output, read, word},
    objects::{Object32, ObjectKind32},
};
use crate::{
    input::dinput::{DirectInputResult32 as H, InterfaceKind32},
    memory::{AddressSpace, GuestAddress, PageRange},
    process::windows::objects::{GuestComCreation32, InterfaceLayout32},
    windows::{
        com::{ComIdentity32, IID_IUNKNOWN, InterfaceId},
        synchronization::{EventBinding32, SynchronizationContext32},
    },
};
pub(in crate::process::windows::dinput) struct PreparedRelease32 {
    pub lifetime: crate::process::windows::objects::PreparedGuestLifetime32,
    pub bindings: Vec<EventBinding32>,
    effects: Vec<(usize, u64)>,
}
impl DirectInputRuntime32 {
    pub(super) fn publish(
        &mut self,
        memory: &mut AddressSpace,
        interface: InterfaceKind32,
        version: u32,
        kind: ObjectKind32,
        out: u32,
        parent: Option<GuestAddress>,
    ) -> Result<GuestAddress, H> {
        self.publish_iid(
            memory,
            interface,
            version,
            kind,
            out,
            parent,
            interface.iid(),
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn publish_iid(
        &mut self,
        memory: &mut AddressSpace,
        interface: InterfaceKind32,
        version: u32,
        kind: ObjectKind32,
        out: u32,
        parent: Option<GuestAddress>,
        requested: InterfaceId,
    ) -> Result<GuestAddress, H> {
        output(memory, out, 4)?;
        let slot = self
            .records
            .iter()
            .position(Option::is_none)
            .ok_or(H::OUT_OF_MEMORY)?;
        let mut layouts = [InterfaceLayout32 {
            iid: IID_IUNKNOWN,
            methods: &[],
        }; 6];
        let mut count = 0;
        for candidate in InterfaceKind32::ALL
            .into_iter()
            .filter(|candidate| interface.compatible(*candidate))
        {
            layouts[count] = InterfaceLayout32 {
                iid: candidate.iid(),
                methods: &self.methods[candidate as usize - 1],
            };
            count += 1;
        }
        let retain = parent.as_slice();
        let identity = self.com.create_with_lifetime(
            memory,
            slot as u64,
            &layouts[..count],
            GuestComCreation32 {
                output: Some((GuestAddress(out), requested)),
                retain,
                ..Default::default()
            },
        )?;
        self.records[slot] = Some(Object32 {
            pointer: identity.identity_pointer,
            interface,
            version,
            kind,
        });
        Ok(identity.identity_pointer)
    }
    pub(super) fn query(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        iid: u32,
        out: u32,
    ) -> Result<u32, H> {
        output(memory, out, 4)?;
        let iid = InterfaceId(read(memory, iid)?);
        let target = match self.com.query_target(memory, pointer, iid) {
            Ok(target) => Some(target),
            Err(crate::process::windows::objects::GuestComError32::Com(
                crate::windows::com::ComError32::NoInterface,
            )) => None,
            Err(error) => return Err(error.into()),
        };
        let retained = target.map(|_| pointer);
        let prepared = self.com.prepare_lifetime_changes(
            memory,
            retained.as_slice(),
            &[],
            &[],
            &[],
            &[word(out, target.map_or(0, |target| target.0))],
        )?;
        self.com.commit_lifetime_changes(memory, prepared)?;
        Ok(if target.is_some() {
            0
        } else {
            H::NO_INTERFACE.0
        })
    }
    pub(super) fn release(
        &mut self,
        memory: &mut AddressSpace,
        sync: Option<&mut SynchronizationContext32>,
        pointer: GuestAddress,
    ) -> Result<u32, H> {
        self.release_with_unmaps(memory, sync, pointer, &[], &[])
    }
    pub(super) fn release_with_unmaps(
        &mut self,
        memory: &mut AddressSpace,
        sync: Option<&mut SynchronizationContext32>,
        pointer: GuestAddress,
        ranges: &[PageRange],
        extra_release: &[GuestAddress],
    ) -> Result<u32, H> {
        self.com.preflight(memory, pointer)?;
        let remaining = self.com.references(pointer)? - 1;
        if remaining > 0 && ranges.is_empty() && extra_release.is_empty() {
            return Ok(self.com.release(memory, pointer)?.remaining);
        }
        let mut releases = Vec::new();
        releases
            .try_reserve_exact(1 + extra_release.len())
            .map_err(|_| H::OUT_OF_MEMORY)?;
        releases.push(pointer);
        releases.extend_from_slice(extra_release);
        self.release_references_with_unmaps(memory, sync, &releases, ranges)?;
        Ok(remaining)
    }

    pub(in crate::process::windows::dinput) fn release_effect_enumeration(
        &mut self,
        memory: &mut AddressSpace,
        sync: Option<&mut SynchronizationContext32>,
        retained: &[GuestAddress],
        range: PageRange,
    ) -> Result<(), H> {
        self.release_references_with_unmaps(memory, sync, retained, &[range])
    }

    fn release_references_with_unmaps(
        &mut self,
        memory: &mut AddressSpace,
        sync: Option<&mut SynchronizationContext32>,
        initial: &[GuestAddress],
        ranges: &[PageRange],
    ) -> Result<(), H> {
        let prepared = self.prepare_releases(memory, initial, ranges, &[])?;
        let events = if prepared.bindings.is_empty() {
            None
        } else {
            Some(
                sync.as_ref()
                    .ok_or(H::HANDLE)?
                    .prepare_event_bindings(&[], &prepared.bindings)
                    .map_err(|_| H::HANDLE)?,
            )
        };
        self.commit_releases(memory, prepared)?;
        if let Some(events) = events {
            sync.unwrap().commit_event_bindings(events);
        }
        Ok(())
    }
    pub(in crate::process::windows::dinput) fn prepare_releases(
        &self,
        memory: &AddressSpace,
        initial: &[GuestAddress],
        ranges: &[PageRange],
        words: &[crate::memory::WordWrite32],
    ) -> Result<PreparedRelease32, H> {
        let mut releases = Vec::new();
        releases
            .try_reserve_exact(initial.len() + self.records.len())
            .map_err(|_| H::OUT_OF_MEMORY)?;
        releases.extend_from_slice(initial);
        let mut handled = Vec::new();
        handled
            .try_reserve_exact(self.records.len())
            .map_err(|_| H::OUT_OF_MEMORY)?;
        let references = loop {
            let references =
                self.com
                    .prepare_reference_receipt(memory, &[], &releases, &[], &[])?;
            let mut changed = false;
            for identity in references.destroyed() {
                if handled.contains(&identity.id) {
                    continue;
                }
                handled.push(identity.id);
                match &self.records[identity.payload as usize]
                    .as_ref()
                    .ok_or(H::INVALID_PARAM)?
                    .kind
                {
                    ObjectKind32::Device(device) => {
                        releases.push(device.parent);
                        changed = true;
                    }
                    ObjectKind32::Effect { device, .. } => {
                        releases.push(*device);
                        changed = true;
                    }
                    _ => {}
                }
            }
            if !changed {
                break references;
            }
        };
        let mut bindings = Vec::new();
        bindings
            .try_reserve_exact(references.destroyed().len())
            .map_err(|_| H::OUT_OF_MEMORY)?;
        let mut effects = Vec::new();
        effects
            .try_reserve_exact(references.destroyed().len())
            .map_err(|_| H::OUT_OF_MEMORY)?;
        for identity in references.destroyed() {
            match &self.records[identity.payload as usize]
                .as_ref()
                .ok_or(H::INVALID_PARAM)?
                .kind
            {
                ObjectKind32::Device(device) => bindings.extend(device.notification),
                ObjectKind32::Effect { device, id } => {
                    self.force_device(*device)?.kind(*id)?;
                    effects.push((self.slot(*device)?, *id));
                }
                _ => {}
            }
        }
        let prepared = self
            .com
            .prepare_lifetime_receipt(memory, references, words, ranges)?;
        Ok(PreparedRelease32 {
            lifetime: prepared,
            bindings,
            effects,
        })
    }
    pub(in crate::process::windows::dinput) fn commit_releases(
        &mut self,
        memory: &mut AddressSpace,
        prepared: PreparedRelease32,
    ) -> Result<(), H> {
        let destroyed = self
            .com
            .commit_lifetime_changes(memory, prepared.lifetime)?;
        for (slot, id) in prepared.effects {
            let ObjectKind32::Device(device) = &mut self.records[slot].as_mut().unwrap().kind
            else {
                unreachable!()
            };
            device
                .haptics
                .as_mut()
                .unwrap()
                .delete(id)
                .expect("effect disposal was preflighted");
        }
        self.retire_destroyed(destroyed);
        Ok(())
    }
    fn retire_destroyed(&mut self, destroyed: Vec<ComIdentity32>) {
        for identity in destroyed {
            self.records[identity.payload as usize] = None;
        }
    }
}
