use super::{DirectInputRuntime32, objects::ObjectKind32};
use crate::{
    cpu::dbt::GateSpec,
    input::dinput::{DirectInputResult32 as H, InterfaceKind32},
    memory::AddressSpace,
    windows::com::{IID_IUNKNOWN, InterfaceId},
};
impl DirectInputRuntime32 {
    pub(in crate::process) fn activate_class(
        &mut self,
        memory: &mut AddressSpace,
        gates: &[GateSpec],
        version8: bool,
        iid: InterfaceId,
        out: u32,
    ) -> Result<u32, H> {
        let interface = if iid == IID_IUNKNOWN {
            if version8 {
                InterfaceKind32::Input8A
            } else {
                InterfaceKind32::Input1A
            }
        } else {
            InterfaceKind32::from_iid(iid)
                .filter(|kind| !kind.device() && (kind.generation() == 8) == version8)
                .ok_or(H::NO_INTERFACE)?
        };
        self.initialize_methods(gates, version8)?;
        self.publish_iid(memory, interface, 0, ObjectKind32::Input, out, None, iid)?;
        Ok(0)
    }
}
