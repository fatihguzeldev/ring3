use crate::{
    input::dinput::{DeviceKind32, InterfaceKind32, device::DirectInputDevice32},
    memory::GuestAddress,
    windows::synchronization::EventBinding32,
};
pub(super) struct Object32 {
    pub pointer: GuestAddress,
    pub interface: InterfaceKind32,
    pub version: u32,
    pub kind: ObjectKind32,
}
pub(super) enum ObjectKind32 {
    Input,
    Device(OwnedDevice32),
    Effect { device: GuestAddress, id: u64 },
}
pub(super) struct Device32 {
    pub native: DirectInputDevice32,
    pub parent: GuestAddress,
    pub notification: Option<EventBinding32>,
    pub kind: DeviceKind32,
    pub transfer: Vec<u8>,
    pub haptics: Option<crate::input::dinput::haptics::EffectDevice32>,
}

impl ObjectKind32 {
    pub(super) fn device(
        device: Device32,
    ) -> Result<Self, crate::input::dinput::DirectInputResult32> {
        let mut allocation = Vec::new();
        allocation
            .try_reserve_exact(1)
            .map_err(|_| crate::input::dinput::DirectInputResult32::OUT_OF_MEMORY)?;
        allocation.push(device);
        let owned = match Box::<[Device32; 1]>::try_from(allocation.into_boxed_slice()) {
            Ok(owned) => owned,
            Err(_) => unreachable!("one preallocated device"),
        };
        Ok(Self::Device(OwnedDevice32(owned)))
    }
}

pub(super) struct OwnedDevice32(Box<[Device32; 1]>);
impl std::ops::Deref for OwnedDevice32 {
    type Target = Device32;
    fn deref(&self) -> &Self::Target {
        &self.0[0]
    }
}
impl std::ops::DerefMut for OwnedDevice32 {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0[0]
    }
}
