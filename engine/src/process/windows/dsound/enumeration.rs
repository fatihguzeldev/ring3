use super::{
    DSoundRuntime32,
    device::{CAPTURE_GUID, PLAYBACK_GUID},
    memory::output,
};
use crate::{
    audio::dsound::HResult,
    memory::{Access, AddressSpace, GuestAddress, Permissions},
    process::{WindowsCallback32, WindowsCallbackProvider32, WindowsProviderOutcome32},
};

#[derive(Clone, Copy)]
pub(super) struct EnumerationData32 {
    address: GuestAddress,
    memory: u64,
    mapping: u64,
}
#[derive(Clone, Copy)]
pub(super) struct PendingEnumeration32 {
    id: u64,
    callback: u32,
    context: u32,
    capture: bool,
    wide: bool,
    index: u8,
}
impl PendingEnumeration32 {
    pub(in crate::process::windows::dsound) fn id(&self) -> u64 {
        self.id
    }
}
impl DSoundRuntime32 {
    pub(in crate::process::windows::dsound) fn enumerate(
        &mut self,
        memory: &mut AddressSpace,
        callback: u32,
        context: u32,
        capture: bool,
        wide: bool,
    ) -> Result<WindowsProviderOutcome32, HResult> {
        if callback == 0 {
            return Err(HResult::INVALID_PARAM);
        }
        memory.check_span(GuestAddress(callback), 1, Access::Execute)?;
        if capture && !self.config.capture_available {
            return Ok(WindowsProviderOutcome32::Return(0));
        }
        if self.enumerations.len() == 33 {
            return Err(HResult::OUT_OF_MEMORY);
        }
        let id = self.sequence.checked_add(1).ok_or(HResult::INVALID_CALL)?;
        self.enumeration_page(memory)?;
        let pending = PendingEnumeration32 {
            id,
            callback,
            context,
            capture,
            wide,
            index: 0,
        };
        let request = self.enumeration_request(memory, pending)?;
        self.enumerations.push(pending);
        self.sequence = id;
        Ok(WindowsProviderOutcome32::Callback(request))
    }
    pub(in crate::process) fn resume_enumeration(
        &mut self,
        memory: &AddressSpace,
        id: u64,
        result: u32,
    ) -> Result<WindowsProviderOutcome32, HResult> {
        let index = self
            .enumerations
            .iter()
            .position(|pending| pending.id == id)
            .ok_or(HResult::INVALID_CALL)?;
        let mut pending = self.enumerations[index];
        if result == 0 || pending.index == 1 {
            self.enumerations.remove(index);
            return Ok(WindowsProviderOutcome32::Return(0));
        }
        pending.index = 1;
        let request = self.enumeration_request(memory, pending)?;
        self.enumerations[index] = pending;
        Ok(WindowsProviderOutcome32::Callback(request))
    }
    fn enumeration_request(
        &self,
        memory: &AddressSpace,
        pending: PendingEnumeration32,
    ) -> Result<WindowsCallback32, HResult> {
        let data = self.enumeration_data.ok_or(HResult::INVALID_CALL)?;
        if data.memory != memory.identity()
            || memory.mapping_version_at(data.address)? != data.mapping
            || memory.permissions_at(data.address)? != Permissions::READ
        {
            return Err(HResult::INVALID_CALL);
        }
        let entry = usize::from(pending.capture) * 2 + pending.index as usize;
        let description = data.address.0
            + if pending.wide {
                512 + entry as u32 * 256
            } else {
                32 + entry as u32 * 128
            };
        let mut arguments = [0; crate::process::WINDOWS_CALLBACK_ARGUMENTS32];
        arguments[..4].copy_from_slice(&[
            if pending.index == 0 {
                0
            } else {
                data.address.0 + u32::from(pending.capture) * 16
            },
            description,
            data.address.0
                + if pending.index == 0 {
                    1900
                } else if pending.wide {
                    1820
                } else {
                    1800
                },
            pending.context,
        ]);
        Ok(WindowsCallback32 {
            procedure: GuestAddress(pending.callback),
            arguments,
            count: 4,
            continuation: pending.id,
            provider: WindowsCallbackProvider32::DirectSound,
        })
    }
    fn enumeration_page(&mut self, memory: &mut AddressSpace) -> Result<(), HResult> {
        if self.enumeration_data.is_some() {
            return Ok(());
        }
        let mut bytes = [0; 4096];
        bytes[..16].copy_from_slice(&PLAYBACK_GUID);
        bytes[16..32].copy_from_slice(&CAPTURE_GUID);
        for (index, string) in [
            "Primary Sound Driver",
            "Ring3 Software Audio",
            "Primary Sound Capture Driver",
            "Ring3 Audio Capture",
        ]
        .into_iter()
        .enumerate()
        {
            let start = 32 + index * 128;
            bytes[start..start + string.len()].copy_from_slice(string.as_bytes());
            let start = 512 + index * 256;
            for (offset, unit) in string.encode_utf16().enumerate() {
                bytes[start + offset * 2..start + offset * 2 + 2]
                    .copy_from_slice(&unit.to_le_bytes());
            }
        }
        bytes[1800..1805].copy_from_slice(b"ring3");
        for (index, unit) in "ring3".encode_utf16().enumerate() {
            bytes[1820 + index * 2..1822 + index * 2].copy_from_slice(&unit.to_le_bytes());
        }
        let (address, mapping) = self.locks.reserve_readonly_page(memory, &bytes)?;
        self.enumeration_data = Some(EnumerationData32 {
            address,
            memory: memory.identity(),
            mapping,
        });
        Ok(())
    }
    pub(in crate::process::windows::dsound) fn get_device_id(
        &self,
        memory: &mut AddressSpace,
        input: u32,
        out: u32,
    ) -> Result<u32, HResult> {
        output(memory, out, 16)?;
        let guid = super::memory::read::<16>(memory, input)?;
        let result = if guid == PLAYBACK_GUID {
            PLAYBACK_GUID
        } else if guid == CAPTURE_GUID {
            if !self.config.capture_available {
                return Err(HResult::NO_DRIVER);
            }
            CAPTURE_GUID
        } else {
            let value = u32::from_le_bytes(guid[..4].try_into().unwrap());
            if guid[4..]
                != [
                    0x6d, 0x9c, 0xed, 0x47, 0xaa, 0xf1, 0x4d, 0xda, 0x8f, 0x2b, 0x5c, 3,
                ]
            {
                return Err(HResult::INVALID_PARAM);
            }
            match value {
                0xdef00000 | 0xdef00002 => PLAYBACK_GUID,
                0xdef00001 | 0xdef00003 if self.config.capture_available => CAPTURE_GUID,
                _ => return Err(HResult::NO_DRIVER),
            }
        };
        memory.write(GuestAddress(out), &result)?;
        Ok(0)
    }
}
