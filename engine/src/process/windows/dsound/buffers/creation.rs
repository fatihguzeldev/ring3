use super::super::{
    DSoundRuntime32,
    lifetime::Publication32,
    memory::{output, read, wave, words},
    objects::{Buffer32, ObjectKind32},
};
use crate::{
    audio::{
        AudioContext,
        dsound::{Buffer, BufferDescription, HResult, InterfaceKind},
    },
    memory::{AddressSpace, GuestAddress},
};
impl DSoundRuntime32 {
    pub(in crate::process::windows::dsound) fn create_buffer(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        device: GuestAddress,
        input: u32,
        out: u32,
        outer: u32,
    ) -> Result<u32, HResult> {
        output(memory, out, 4)?;
        if outer != 0 {
            return Err(HResult::NO_AGGREGATION);
        }
        let header = words::<5>(memory, input)?;
        if !matches!(header[0], 20 | 36) || header[3] != 0 {
            return Err(HResult::INVALID_PARAM);
        }
        let description = BufferDescription {
            flags: header[1],
            bytes: header[2],
            format: if header[4] == 0 {
                None
            } else {
                Some(wave(memory, header[4])?)
            },
            algorithm: if header[0] == 36 {
                read(memory, input.checked_add(20).ok_or(HResult::INVALID_PARAM)?)?
            } else {
                [0; 16]
            },
        };
        let device_record = self.device(device)?;
        let description = description.validate(device_record.native.version8)?;
        let result = description.spatial_algorithm()?.creation_result().0;
        if description.flags & BufferDescription::PRIMARY != 0
            && let Some(primary) = device_record.primary
        {
            self.acquire(memory, primary, out, InterfaceKind::Buffer)?;
            return Ok(result);
        }
        let version8 = device_record.native.version8;
        let native = Buffer::create(audio, &device_record.native, description)?;
        native.apply_controls(audio, &device_record.native)?;
        let primary = native.primary;
        let canonical = self.object(device)?.pointer;
        let mut interfaces = [InterfaceKind::Buffer; 4];
        let mut count = 1;
        if version8 && !primary {
            interfaces[count] = InterfaceKind::Buffer8;
            count += 1;
        }
        if description.flags & BufferDescription::NOTIFY != 0 {
            interfaces[count] = InterfaceKind::Notify;
            count += 1;
        }
        if description.flags & BufferDescription::CONTROL_3D != 0 {
            interfaces[count] = if primary {
                InterfaceKind::Listener
            } else {
                InterfaceKind::Buffer3D
            };
            count += 1;
        }
        let pointer = self.publish_object(
            memory,
            audio,
            ObjectKind32::Buffer(Buffer32 {
                native,
                device: canonical,
                version8: version8 && !primary,
                notifications: Vec::new(),
            }),
            Publication32 {
                interfaces: &interfaces[..count],
                output: Some((out, InterfaceKind::Buffer)),
                retain: &[canonical],
            },
        )?;
        if primary {
            let slot = self.slot(canonical)?;
            let ObjectKind32::Device(record) = &mut self.records[slot].as_mut().unwrap().kind
            else {
                unreachable!()
            };
            record.primary = Some(pointer);
        }
        Ok(result)
    }
    pub(in crate::process::windows::dsound) fn duplicate_buffer(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        device: GuestAddress,
        source: GuestAddress,
        out: u32,
    ) -> Result<u32, HResult> {
        output(memory, out, 4)?;
        self.com.preflight(memory, source)?;
        let ObjectKind32::Buffer(buffer) = &self.object(source)?.kind else {
            return Err(HResult::INVALID_PARAM);
        };
        let canonical = self.object(device)?.pointer;
        if buffer.device != canonical {
            return Err(HResult::INVALID_PARAM);
        }
        let native = buffer.native.duplicate(audio)?;
        native.apply_controls(audio, &self.device(canonical)?.native)?;
        let flags = native.description.flags;
        let version8 = buffer.version8;
        let mut interfaces = [InterfaceKind::Buffer; 4];
        let mut count = 1;
        if flags & BufferDescription::NOTIFY != 0 {
            interfaces[count] = InterfaceKind::Notify;
            count += 1;
        }
        if flags & BufferDescription::CONTROL_3D != 0 {
            interfaces[count] = InterfaceKind::Buffer3D;
            count += 1;
        }
        self.publish_object(
            memory,
            audio,
            ObjectKind32::Buffer(Buffer32 {
                native,
                device: canonical,
                version8: false,
                notifications: Vec::new(),
            }),
            Publication32 {
                interfaces: &interfaces[..count],
                output: Some((out, InterfaceKind::Buffer)),
                retain: &[canonical],
            },
        )?;
        let _ = version8;
        Ok(0)
    }
}
