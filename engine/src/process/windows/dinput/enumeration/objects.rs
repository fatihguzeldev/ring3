use super::super::DirectInputRuntime32;
use crate::{
    input::dinput::{DirectInputResult32 as H, InterfaceKind32},
    memory::{AddressSpace, GuestAddress},
    process::WindowsProviderOutcome32,
};
impl DirectInputRuntime32 {
    pub(in crate::process::windows::dinput) fn enum_objects(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        interface: InterfaceKind32,
        procedure: u32,
        context: u32,
        flags: u32,
    ) -> Result<WindowsProviderOutcome32, H> {
        if flags & !0x03ff_ff1f != 0 {
            return Err(H::INVALID_PARAM);
        }
        let mut rows = Vec::new();
        let device = self.device(pointer)?;
        rows.try_reserve_exact(device.native.description.objects().count())
            .map_err(|_| H::OUT_OF_MEMORY)?;
        for object in device.native.description.objects() {
            let actuator = matches!(object,crate::input::dinput::format::DeviceObject32::Axis{index,aspect:0} if device.haptics.as_ref().is_some_and(|haptics|index<haptics.capabilities().axes));
            let trigger = matches!(
                object,
                crate::input::dinput::format::DeviceObject32::Button(_)
            ) && device.haptics.is_some();
            if (flags & 255 == 0 || object.object_type(device.native.relative()) & flags & 255 != 0)
                && (flags & 0x01000000 == 0 || actuator)
                && (flags & 0x02000000 == 0 || trigger)
            {
                rows.push(self.object_bytes(
                    pointer,
                    object,
                    interface,
                    if interface.wide() { 576 } else { 316 },
                )?);
            }
        }
        self.start_enumeration(memory, procedure, context, interface, rows)
    }
}
