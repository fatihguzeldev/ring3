use crate::{
    graphics::d3d8::{api::AdapterProfile32, device::DeviceLimits},
    memory::PageRange,
};

#[derive(Clone, Debug)]
pub struct D3D8Config32 {
    pub objects: PageRange,
    pub locks: PageRange,
    pub max_objects: u32,
    pub max_devices: u32,
    pub max_locks: u32,
    pub adapter: AdapterProfile32,
    pub device: DeviceLimits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum D3D8Error32 {
    NotConfigured,
    AlreadyConfigured,
    InvalidConfiguration,
    Allocation,
    InvalidDevice,
    Busy,
    StaleReply,
    Device(crate::graphics::d3d8::device::DeviceError),
}
