use super::{Action, PendingPrivateCall, Phase};
use crate::process::windows::d3d8::{
    D3D8Runtime32, DispatchError32,
    lifetime::LifetimeChange32,
    memory::{check_output, read_bytes, read_guid, read_words, word},
    objects::PrivateUnknown32,
};
use crate::{
    graphics::{
        ResourceId,
        d3d8::{api::HResult, device::DeviceError},
    },
    memory::{AddressSpace, GuestAddress},
};

impl D3D8Runtime32 {
    pub(in crate::process::windows::d3d8) fn private_resource_call(
        &mut self,
        memory: &mut AddressSpace,
        object: GuestAddress,
        device: GuestAddress,
        resource: ResourceId,
        slot: u8,
        args: &[u32],
    ) -> Result<u32, DispatchError32> {
        let key = read_guid(memory, args[1])?;
        let namespace = self.com.preflight(memory, object)?.id.get();
        let record = self.record_slot(object)?;
        let old = self.records[record]
            .as_ref()
            .unwrap()
            .private_unknowns
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| entry.pointer);
        match slot {
            4 => {
                if args[4] & !1 != 0 || (args[4] == 1 && (args[3] != 4 || args[2] == 0)) {
                    return Err(HResult::INVALID_CALL.into());
                }
                let unknown = (args[4] == 1).then_some(GuestAddress(args[2]));
                if let Some(pointer) = unknown {
                    self.private_reference_procedure(memory, pointer, 1)?;
                    self.private_reference_procedure(memory, pointer, 2)?;
                }
                if let Some(pointer) = old {
                    self.private_reference_procedure(memory, pointer, 2)?;
                }
                if args[3] > self.config.device.graphics.resource_bytes {
                    return Err(HResult::OUT_OF_VIDEO_MEMORY.into());
                }
                let bytes = if unknown.is_some() {
                    args[2].to_le_bytes().to_vec()
                } else {
                    read_bytes(memory, args[2], args[3] as usize)?
                };
                if unknown.is_some() && old.is_none() {
                    self.records[record]
                        .as_mut()
                        .unwrap()
                        .private_unknowns
                        .try_reserve(1)
                        .map_err(|_| HResult::OUT_OF_MEMORY)?;
                }
                let prepared = self
                    .device_mut(device)?
                    .resources_mut()?
                    .prepare_private_data(resource, namespace, key, &bytes)
                    .map_err(DeviceError::from)?;
                if unknown
                    .into_iter()
                    .chain(old)
                    .all(|pointer| self.com.lookup(pointer).is_ok())
                {
                    let retain = unknown
                        .map(|pointer| self.counter_pointer(pointer))
                        .transpose()?
                        .into_iter()
                        .collect();
                    let release = old
                        .map(|pointer| self.counter_pointer(pointer))
                        .transpose()?
                        .into_iter()
                        .collect();
                    let lifetime = self.prepare_lifetime(
                        memory,
                        LifetimeChange32 {
                            retain,
                            release,
                            ..Default::default()
                        },
                        &[],
                    )?;
                    self.publish_private_action(
                        memory,
                        Action::Set {
                            object,
                            device,
                            resource,
                            namespace,
                            key,
                            prepared,
                            unknown,
                        },
                    )?;
                    self.commit_lifetime(memory, lifetime);
                    return Ok(0);
                }
                self.start_private_call(
                    memory,
                    PendingPrivateCall {
                        continuation: 0,
                        action: Some(Action::Set {
                            object,
                            device,
                            resource,
                            namespace,
                            key,
                            prepared,
                            unknown,
                        }),
                        phase: Phase::Add,
                        acquired: unknown,
                        releases: old.into_iter().collect(),
                        result: 0,
                    },
                )
            }
            5 => {
                let bytes = self
                    .device(device)?
                    .resources()
                    .private_data_for(resource, namespace, key)
                    .map_err(DeviceError::from)?;
                let required = bytes.len() as u32;
                let [capacity] = read_words(memory, args[3])?;
                check_output(memory, args[3], 4)?;
                if args[2] == 0 || capacity < required {
                    memory.write_words32(&[word(args[3], required)])?;
                    return Ok(if args[2] == 0 {
                        0
                    } else {
                        HResult::MORE_DATA.0
                    });
                }
                check_output(memory, args[2], bytes.len())?;
                if let Some(pointer) = old.filter(|pointer| self.com.lookup(*pointer).is_ok()) {
                    let retain = vec![self.counter_pointer(pointer)?];
                    let lifetime = self.prepare_lifetime(
                        memory,
                        LifetimeChange32 {
                            retain,
                            ..Default::default()
                        },
                        &[word(args[2], pointer.0), word(args[3], required)],
                    )?;
                    self.commit_lifetime(memory, lifetime);
                    return Ok(0);
                }
                let mut copied = Vec::new();
                copied
                    .try_reserve_exact(bytes.len())
                    .map_err(|_| HResult::OUT_OF_MEMORY)?;
                copied.extend_from_slice(bytes);
                if let Some(pointer) = old {
                    self.private_reference_procedure(memory, pointer, 1)?;
                    self.private_reference_procedure(memory, pointer, 2)?;
                }
                let memory_identity = memory.identity();
                let mappings = output_mappings(memory, args[2], required, args[3])?;
                self.start_private_call(
                    memory,
                    PendingPrivateCall {
                        continuation: 0,
                        action: Some(Action::Get {
                            bytes: copied,
                            output: args[2],
                            size: args[3],
                            memory_identity,
                            mappings,
                        }),
                        phase: Phase::Add,
                        acquired: old,
                        releases: Vec::new(),
                        result: 0,
                    },
                )
            }
            6 => {
                self.device(device)?
                    .resources()
                    .private_data_for(resource, namespace, key)
                    .map_err(DeviceError::from)?;
                if let Some(pointer) = old {
                    self.private_reference_procedure(memory, pointer, 2)?;
                }
                if old.is_none_or(|pointer| self.com.lookup(pointer).is_ok()) {
                    let release = old
                        .map(|pointer| self.counter_pointer(pointer))
                        .transpose()?
                        .into_iter()
                        .collect();
                    let lifetime = self.prepare_lifetime(
                        memory,
                        LifetimeChange32 {
                            release,
                            ..Default::default()
                        },
                        &[],
                    )?;
                    self.publish_private_action(
                        memory,
                        Action::Free {
                            object,
                            device,
                            resource,
                            namespace,
                            key,
                        },
                    )?;
                    self.commit_lifetime(memory, lifetime);
                    return Ok(0);
                }
                self.start_private_call(
                    memory,
                    PendingPrivateCall {
                        continuation: 0,
                        action: Some(Action::Free {
                            object,
                            device,
                            resource,
                            namespace,
                            key,
                        }),
                        phase: Phase::Add,
                        acquired: None,
                        releases: old.into_iter().collect(),
                        result: 0,
                    },
                )
            }
            _ => Err(HResult::INVALID_CALL.into()),
        }
    }

    pub(super) fn publish_private_action(
        &mut self,
        memory: &mut AddressSpace,
        action: Action,
    ) -> Result<(), DispatchError32> {
        match action {
            Action::Set {
                object,
                device,
                resource,
                namespace,
                key,
                prepared,
                unknown,
            } => {
                if self.com.preflight(memory, object)?.id.get() != namespace {
                    return Err(HResult::INVALID_CALL.into());
                }
                let record = self.record_slot(object)?;
                self.device(device)?
                    .resources()
                    .desc(resource)
                    .map_err(DeviceError::from)?;
                self.device(device)?
                    .resources()
                    .can_commit_private_data(&prepared)
                    .map_err(DeviceError::from)?;
                self.device_mut(device)?
                    .resources_mut()?
                    .commit_private_data(prepared)
                    .map_err(DeviceError::from)?;
                let entries = &mut self.records[record].as_mut().unwrap().private_unknowns;
                if let Some(index) = entries.iter().position(|entry| entry.key == key) {
                    entries.remove(index);
                }
                if let Some(pointer) = unknown {
                    entries.push(PrivateUnknown32 { key, pointer });
                }
            }
            Action::Free {
                object,
                device,
                resource,
                namespace,
                key,
            } => {
                if self.com.preflight(memory, object)?.id.get() != namespace {
                    return Err(HResult::INVALID_CALL.into());
                }
                let record = self.record_slot(object)?;
                self.device_mut(device)?
                    .resources_mut()?
                    .free_private_data_for(resource, namespace, key)
                    .map_err(DeviceError::from)?;
                self.records[record]
                    .as_mut()
                    .unwrap()
                    .private_unknowns
                    .retain(|entry| entry.key != key);
            }
            Action::Get {
                bytes,
                output,
                size,
                memory_identity,
                mappings,
            } => {
                if memory.identity() != memory_identity
                    || mappings.iter().any(|(address, version)| {
                        memory.mapping_version_at(*address) != Ok(*version)
                    })
                {
                    return Err(HResult::INVALID_CALL.into());
                }
                check_output(memory, output, bytes.len())?;
                check_output(memory, size, 4)?;
                memory.write_spans(&[
                    (GuestAddress(output), &bytes),
                    (GuestAddress(size), &(bytes.len() as u32).to_le_bytes()),
                ])?;
            }
        }
        Ok(())
    }

    pub(super) fn owned_private_reference(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        add: bool,
    ) -> Result<(), DispatchError32> {
        let pointer = self.counter_pointer(pointer)?;
        self.change_lifetime(
            memory,
            if add {
                LifetimeChange32 {
                    retain: vec![pointer],
                    ..Default::default()
                }
            } else {
                LifetimeChange32 {
                    release: vec![pointer],
                    ..Default::default()
                }
            },
            &[],
        )?;
        Ok(())
    }
}

fn output_mappings(
    memory: &AddressSpace,
    output: u32,
    length: u32,
    size: u32,
) -> Result<Vec<(GuestAddress, u64)>, DispatchError32> {
    let mut mappings = Vec::new();
    let pages = if length == 0 {
        0
    } else {
        (u64::from(output) + u64::from(length) - 1) / u64::from(crate::memory::PAGE_SIZE)
            - u64::from(output / crate::memory::PAGE_SIZE)
            + 1
    };
    mappings
        .try_reserve_exact(pages as usize + 2)
        .map_err(|_| HResult::OUT_OF_MEMORY)?;
    for (pointer, length) in [(output, length), (size, 4)] {
        if length == 0 {
            continue;
        }
        let first = pointer / crate::memory::PAGE_SIZE;
        let last = ((u64::from(pointer) + u64::from(length) - 1)
            / u64::from(crate::memory::PAGE_SIZE)) as u32;
        for page in first..=last {
            let address = GuestAddress(page * crate::memory::PAGE_SIZE);
            mappings.push((address, memory.mapping_version_at(address)?));
        }
    }
    Ok(mappings)
}
