pub(super) mod actions;
mod format;
mod information;
mod names;
#[cfg(feature = "observability")]
mod observation;
mod properties;
mod reading;
use super::DirectInputRuntime32;
use crate::{
    input::{
        dinput::{DirectInputResult32 as H, InterfaceKind32, device::CooperativeLevel32},
        source::InputSource32,
    },
    memory::{AddressSpace, GuestAddress},
    windows::{
        synchronization::SynchronizationContext32,
        window::{WindowContext32, WindowHandle32},
    },
};
impl DirectInputRuntime32 {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::process::windows::dinput) fn device_call(
        &mut self,
        memory: &mut AddressSpace,
        windows: Option<&WindowContext32>,
        mut sync: Option<&mut SynchronizationContext32>,
        source: &InputSource32,
        interface: InterfaceKind32,
        args: &[u32],
        slot: u8,
    ) -> Result<u32, H> {
        let pointer = GuestAddress(args[0]);
        match slot {
            3 => self.capabilities(memory, pointer, args[1]),
            5 | 6 => self.property(memory, pointer, args, slot == 6, interface),
            7 => {
                let active = windows
                    .and_then(WindowContext32::active)
                    .map_or(0, WindowHandle32::get);
                let device = self.device(pointer)?;
                if device
                    .native
                    .cooperative()
                    .is_some_and(|level| level.exclusive)
                {
                    let kind = device.kind;
                    if self.records.iter().flatten().any(|record|matches!(&record.kind,super::objects::ObjectKind32::Device(other) if record.pointer!=self.object(pointer).unwrap().pointer&&other.kind==kind&&other.native.acquisition()==crate::input::dinput::device::Acquisition32::Acquired&&other.native.cooperative().is_some_and(|level|level.exclusive))) {return Err(H::OTHER_APP);}
                }
                Ok(self.device_mut(pointer)?.native.acquire(source, active)?.0)
            }
            8 => {
                let device = self.device_mut(pointer)?;
                let result = device.native.unacquire().0;
                if let Some(haptics) = &mut device.haptics {
                    haptics.lose_acquisition();
                }
                Ok(result)
            }
            9 => self.get_state(memory, pointer, args[1], args[2]),
            10 => self.get_data(memory, pointer, args, interface),
            11 => self.set_format(memory, pointer, args[1]),
            12 => {
                let device = self.device(pointer)?;
                device.native.check_unacquired()?;
                let old = device.notification;
                let handles = if args[1] == 0 { &[][..] } else { &args[1..2] };
                let release = old.as_slice();
                if handles.is_empty() && release.is_empty() {
                    return Ok(0);
                }
                let context = sync.as_mut().ok_or(H::HANDLE)?;
                let prepared = context
                    .prepare_event_bindings(handles, release)
                    .map_err(|_| H::HANDLE)?;
                let next = prepared.bindings().first().copied();
                context.commit_event_bindings(prepared);
                self.device_mut(pointer)?.notification = next;
                Ok(0)
            }
            13 => {
                let kind = self.device(pointer)?.kind;
                let level = CooperativeLevel32::new(args[1], args[2], kind)?;
                if args[1] != 0 {
                    let windows = windows.ok_or(H::HANDLE)?;
                    let handle = WindowHandle32::try_from(args[1]).map_err(|_| H::HANDLE)?;
                    windows.get(handle).map_err(|_| H::HANDLE)?;
                }
                self.device_mut(pointer)?.native.set_cooperative(level)?;
                Ok(0)
            }
            14 => self.object_info(memory, pointer, args[1], args[2], args[3], interface),
            15 => self.device_info(memory, pointer, args[1], interface),
            16 => Err(H::UNSUPPORTED),
            17 => {
                super::dispatch::validate_initialization(interface, args[1], args[2])?;
                let device = self.device(pointer)?;
                device.native.check_unacquired()?;
                let guid = super::memory::read::<16>(memory, args[3])?;
                if guid != self.device_guid(device.kind, false) {
                    return Err(H::DEVICE_NOT_REGISTERED);
                }
                Ok(1)
            }
            18 => self.create_effect(memory, pointer, args),
            20 => self.effect_information(memory, pointer, interface, args),
            21 => {
                let state = self.force_device(pointer)?.state(self.now);
                super::memory::output(memory, args[1], 4)?;
                memory.write_words32(&[super::memory::word(args[1], state)])?;
                Ok(0)
            }
            22 => {
                let exclusive = self.force_exclusive(pointer)?;
                let now = self.now;
                self.force_device_mut(pointer)?
                    .command(args[1], exclusive, now)?;
                Ok(0)
            }
            24 | 26 | 27 | 28 | 31 => Err(H::UNSUPPORTED),
            25 => {
                self.device(pointer)?.native.check_acquired()?;
                Ok(0)
            }
            29 | 30 => self.action_map(memory, pointer, args, interface, slot == 30),
            _ => Err(H::INVALID_PARAM),
        }
    }
}
