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
        com::{IID_IUNKNOWN, InterfaceId},
        synchronization::SynchronizationContext32,
    },
};
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
                output: Some((GuestAddress(out), interface.iid())),
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
        let slot = self.slot(pointer)?;
        let mut releases = [pointer, GuestAddress(0), GuestAddress(0)];
        let (mut length, binding) = match &self.records[slot].as_ref().unwrap().kind {
            ObjectKind32::Device(device) if remaining == 0 => {
                releases[1] = device.parent;
                (2, device.notification)
            }
            _ => (1, None),
        };
        if extra_release.len() > 1 {
            return Err(H::INVALID_PARAM);
        }
        for &extra in extra_release {
            releases[length] = extra;
            length += 1;
        }
        let prepared = self.com.prepare_lifetime_changes_with_unmaps(
            memory,
            &[],
            &releases[..length],
            &[],
            &[],
            &[],
            ranges,
        )?;
        let mut sync = sync;
        let events = if let Some(binding) = binding {
            Some(
                sync.as_ref()
                    .ok_or(H::HANDLE)?
                    .prepare_event_bindings(&[], &[binding])
                    .map_err(|_| H::HANDLE)?,
            )
        } else {
            None
        };
        let destroyed = self.com.commit_lifetime_changes(memory, prepared)?;
        if let Some(events) = events {
            sync.as_mut().unwrap().commit_event_bindings(events);
        }
        for identity in destroyed {
            self.records[identity.payload as usize] = None;
        }
        Ok(remaining)
    }
}
