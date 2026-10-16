use crate::{
    memory::{Access, AddressSpace, GuestAddress},
    process::windows::objects::GuestComError32,
    windows::{com::InterfaceId, ole32::HResult32},
};
pub(super) fn iid(memory: &AddressSpace, address: u32) -> Result<InterfaceId, HResult32> {
    if address == 0 {
        return Err(HResult32::POINTER);
    }
    let mut bytes = [0; 16];
    memory
        .read(GuestAddress(address), &mut bytes)
        .map_err(|_| HResult32::POINTER)?;
    Ok(InterfaceId(bytes))
}
pub(super) fn output(memory: &AddressSpace, address: u32) -> Result<(), HResult32> {
    if address == 0 {
        return Err(HResult32::POINTER);
    }
    memory
        .check_span(GuestAddress(address), 4, Access::Write)
        .map_err(|_| HResult32::POINTER)
}
pub(super) fn com_error(error: GuestComError32) -> HResult32 {
    match error {
        GuestComError32::Allocation | GuestComError32::Capacity => HResult32::OUT_OF_MEMORY,
        GuestComError32::Com(crate::windows::com::ComError32::NoInterface) => {
            HResult32::NO_INTERFACE
        }
        _ => HResult32::UNEXPECTED,
    }
}
pub(super) fn failure(
    memory: &mut AddressSpace,
    address: u32,
    error: HResult32,
) -> Result<u32, HResult32> {
    output(memory, address)?;
    memory
        .write(GuestAddress(address), &0u32.to_le_bytes())
        .map_err(|_| HResult32::POINTER)?;
    Ok(error.0)
}
