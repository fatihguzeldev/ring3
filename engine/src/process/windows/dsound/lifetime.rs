use super::{
    DSoundRuntime32,
    memory::{output, read, word},
    objects::{Object32, ObjectKind32},
};
use crate::{
    audio::{
        AudioContext, AudioError32,
        dsound::{HResult, InterfaceKind},
    },
    memory::{AddressSpace, GuestAddress, PageRange},
    process::windows::objects::{GuestComCreation32, InterfaceLayout32},
    windows::{
        com::{IID_IUNKNOWN, InterfaceId},
        synchronization::SynchronizationContext32,
    },
};

pub(super) struct Publication32<'a> {
    pub interfaces: &'a [InterfaceKind],
    pub output: Option<(u32, InterfaceKind)>,
    pub retain: &'a [GuestAddress],
}
impl DSoundRuntime32 {
    pub(in crate::process::windows::dsound) fn publish_object(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        kind: ObjectKind32,
        publication: Publication32<'_>,
    ) -> Result<GuestAddress, HResult> {
        let result = (|| {
            let slot = self.vacant()?;
            let mut layouts = [InterfaceLayout32 {
                iid: IID_IUNKNOWN,
                methods: &[],
            }; 5];
            layouts[0] = InterfaceLayout32 {
                iid: IID_IUNKNOWN,
                methods: &self.methods[InterfaceKind::Unknown as usize - 1],
            };
            let mut count = 1;
            for interface in publication.interfaces {
                if matches!(
                    interface,
                    InterfaceKind::Buffer8 | InterfaceKind::CaptureBuffer8
                ) {
                    continue;
                }
                if count == layouts.len() {
                    return Err(HResult::INVALID_PARAM);
                }
                let method_interface = if *interface == InterfaceKind::Buffer
                    && publication.interfaces.contains(&InterfaceKind::Buffer8)
                {
                    InterfaceKind::Buffer8
                } else if *interface == InterfaceKind::CaptureBuffer
                    && publication
                        .interfaces
                        .contains(&InterfaceKind::CaptureBuffer8)
                {
                    InterfaceKind::CaptureBuffer8
                } else {
                    *interface
                };
                layouts[count] = InterfaceLayout32 {
                    iid: interface.iid(),
                    methods: &self.methods[method_interface as usize - 1],
                };
                count += 1;
            }
            let identity = self.com.create_with_lifetime(
                memory,
                slot as u64,
                &layouts[..count],
                GuestComCreation32 {
                    output: publication
                        .output
                        .map(|(pointer, interface)| (GuestAddress(pointer), interface.iid())),
                    retain: publication.retain,
                    ..Default::default()
                },
            )?;
            Ok((slot, identity.identity_pointer))
        })();
        match result {
            Ok((slot, pointer)) => {
                self.records[slot] = Some(Object32 {
                    pointer,
                    counts: [1, 0, 0, 0],
                    kind,
                });
                Ok(pointer)
            }
            Err(error) => {
                discard(audio, &kind);
                Err(error)
            }
        }
    }
    pub(in crate::process::windows::dsound) fn target(
        &self,
        memory: &AddressSpace,
        pointer: GuestAddress,
        interface: InterfaceKind,
    ) -> Result<GuestAddress, HResult> {
        let actual = match (&self.object(pointer)?.kind, interface) {
            (ObjectKind32::Buffer(buffer), InterfaceKind::Buffer8) if buffer.version8 => {
                InterfaceKind::Buffer
            }
            (ObjectKind32::CaptureBuffer(buffer), InterfaceKind::CaptureBuffer8)
                if buffer.version8 =>
            {
                InterfaceKind::CaptureBuffer
            }
            _ => interface,
        };
        Ok(self.com.query_target(memory, pointer, actual.iid())?)
    }
    pub(in crate::process::windows::dsound) fn add_ref(
        &mut self,
        memory: &AddressSpace,
        pointer: GuestAddress,
        interface: InterfaceKind,
    ) -> Result<u32, HResult> {
        let index = self.slot(pointer)?;
        let group = group(interface);
        let count = self.records[index].as_ref().unwrap().counts[group]
            .checked_add(1)
            .ok_or(HResult::INVALID_CALL)?;
        self.com.add_ref(memory, pointer)?;
        self.records[index].as_mut().unwrap().counts[group] = count;
        Ok(count)
    }
    pub(in crate::process::windows::dsound) fn query_interface(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        iid: u32,
        out: u32,
    ) -> Result<u32, HResult> {
        output(memory, out, 4)?;
        let iid = InterfaceId(read(memory, iid)?);
        let requested = InterfaceKind::ALL
            .into_iter()
            .find(|kind| kind.iid() == iid);
        let target = requested.and_then(|kind| {
            self.target(memory, pointer, kind)
                .ok()
                .map(|pointer| (pointer, kind))
        });
        let retained = [pointer];
        let retain = if target.is_some() { &retained[..] } else { &[] };
        let slot = self.slot(pointer)?;
        let increment = target
            .map(|(_, kind)| {
                let group = group(kind);
                self.records[slot].as_ref().unwrap().counts[group]
                    .checked_add(1)
                    .map(|count| (group, count))
                    .ok_or(HResult::INVALID_CALL)
            })
            .transpose()?;
        let prepared = self.com.prepare_lifetime_changes(
            memory,
            retain,
            &[],
            &[],
            &[],
            &[word(out, target.map_or(0, |(pointer, _)| pointer.0))],
        )?;
        self.com.commit_lifetime_changes(memory, prepared)?;
        if let Some((group, count)) = increment {
            self.records[slot].as_mut().unwrap().counts[group] = count;
        }
        Ok(if target.is_some() {
            0
        } else {
            HResult::NO_INTERFACE.0
        })
    }
    pub(in crate::process::windows::dsound) fn acquire(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        out: u32,
        interface: InterfaceKind,
    ) -> Result<u32, HResult> {
        output(memory, out, 4)?;
        let target = self.target(memory, pointer, interface)?;
        let index = self.slot(pointer)?;
        let group = group(interface);
        let count = self.records[index].as_ref().unwrap().counts[group]
            .checked_add(1)
            .ok_or(HResult::INVALID_CALL)?;
        let prepared = self.com.prepare_lifetime_changes(
            memory,
            &[pointer],
            &[],
            &[],
            &[],
            &[word(out, target.0)],
        )?;
        self.com.commit_lifetime_changes(memory, prepared)?;
        self.records[index].as_mut().unwrap().counts[group] = count;
        Ok(0)
    }
    pub(in crate::process::windows::dsound) fn release(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        sync: Option<&mut SynchronizationContext32>,
        pointer: GuestAddress,
        interface: InterfaceKind,
    ) -> Result<u32, HResult> {
        self.com.preflight(memory, pointer)?;
        let index = self.slot(pointer)?;
        let group = group(interface);
        let remaining = self.records[index].as_ref().unwrap().counts[group]
            .checked_sub(1)
            .ok_or(HResult::INVALID_CALL)?;
        if self.com.references(pointer)? > 1 {
            self.com.release(memory, pointer)?;
            self.records[index].as_mut().unwrap().counts[group] = remaining;
            return Ok(remaining);
        }
        let mut release = Vec::new();
        release
            .try_reserve_exact(self.records.len().min(4))
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        release.push(pointer);
        let mut handled = Vec::new();
        handled
            .try_reserve_exact(self.records.len())
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        loop {
            let preview = self
                .com
                .prepare_reference_receipt(memory, &[], &release, &[], &[])?;
            let mut changed = false;
            for identity in preview.destroyed() {
                if handled.contains(&identity.id) {
                    continue;
                }
                handled.push(identity.id);
                let slot = identity.payload as usize;
                match &self.records[slot]
                    .as_ref()
                    .ok_or(HResult::INVALID_PARAM)?
                    .kind
                {
                    ObjectKind32::Buffer(buffer) => {
                        release.try_reserve(1).map_err(|_| HResult::OUT_OF_MEMORY)?;
                        release.push(buffer.device);
                        changed = true;
                    }
                    ObjectKind32::CaptureBuffer(buffer) => {
                        release.try_reserve(1).map_err(|_| HResult::OUT_OF_MEMORY)?;
                        release.push(buffer.device);
                        changed = true;
                    }
                    ObjectKind32::FullDuplex { device, capture } => {
                        release.try_reserve(2).map_err(|_| HResult::OUT_OF_MEMORY)?;
                        release.extend([*device, *capture]);
                        changed = true;
                    }
                    _ => {}
                }
            }
            if !changed {
                break;
            }
        }
        let references = self
            .com
            .prepare_reference_receipt(memory, &[], &release, &[], &[])?;
        let destroyed = references.destroyed();
        let mut ranges: Vec<PageRange> = Vec::new();
        let mut bindings = Vec::new();
        ranges
            .try_reserve_exact(self.locks.entries.len())
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        bindings
            .try_reserve_exact(self.config.max_notifications as usize)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        for lock in &self.locks.entries {
            if destroyed.iter().any(|identity| identity.id == lock.object) {
                self.locks.validate(memory, lock)?;
                ranges.push(lock.range);
            }
        }
        for identity in destroyed {
            match &self.records[identity.payload as usize]
                .as_ref()
                .unwrap()
                .kind
            {
                ObjectKind32::Buffer(buffer) => {
                    audio.voice_state(buffer.native.voice)?;
                    bindings.extend(
                        buffer
                            .notifications
                            .iter()
                            .map(|notification| notification.binding),
                    );
                }
                ObjectKind32::CaptureBuffer(buffer) => bindings.extend(
                    buffer
                        .notifications
                        .iter()
                        .map(|notification| notification.binding),
                ),
                _ => {}
            }
        }
        ranges.sort_unstable_by_key(|range| range.first);
        ranges.dedup();
        let event_receipt = if bindings.is_empty() {
            None
        } else {
            Some(
                sync.as_deref()
                    .ok_or(HResult::INVALID_CALL)?
                    .prepare_event_bindings(&[], &bindings)
                    .map_err(|_| HResult::INVALID_CALL)?,
            )
        };
        let prepared = self
            .com
            .prepare_lifetime_receipt(memory, references, &[], &ranges)?;
        let destroyed = self.com.commit_lifetime_changes(memory, prepared)?;
        if let Some(prepared) = event_receipt {
            sync.unwrap().commit_event_bindings(prepared);
        }
        self.locks
            .entries
            .retain(|lock| !destroyed.iter().any(|identity| identity.id == lock.object));
        for identity in destroyed {
            let object = self.records[identity.payload as usize].take().unwrap();
            if let ObjectKind32::Buffer(buffer) = &object.kind
                && buffer.native.primary
                && let Ok(slot) = self.slot(buffer.device)
                && let ObjectKind32::Device(device) = &mut self.records[slot].as_mut().unwrap().kind
            {
                device.primary = None;
            }
            discard(audio, &object.kind);
        }
        Ok(0)
    }
}

pub(in crate::process::windows::dsound) fn discard(audio: &mut AudioContext, kind: &ObjectKind32) {
    if let ObjectKind32::Buffer(buffer) = kind {
        audio
            .release_stream_voice(buffer.native.voice)
            .expect("sound voice disposal was preflighted");
        match audio.release_stream_buffer(buffer.native.storage) {
            Ok(()) | Err(AudioError32::BufferInUse) => {}
            Err(error) => panic!("sound backing disposal failed: {error:?}"),
        }
    }
}

pub(in crate::process::windows::dsound) fn group(interface: InterfaceKind) -> usize {
    match interface {
        InterfaceKind::Unknown => 1,
        InterfaceKind::Listener | InterfaceKind::Buffer3D => 2,
        InterfaceKind::Notify => 3,
        _ => 0,
    }
}
