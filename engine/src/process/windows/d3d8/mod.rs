mod thread;
pub(in crate::process) use thread::D3D8ThreadReply32;
mod bindings;
mod context;
mod dispatch;
mod draw;
mod factory;
mod fpu;
mod garbage;
mod lifetime;
mod locks;
mod memory;
mod objects;
mod presentation;
mod readback_input;
mod resources;
mod runtime;
mod shaders;
mod state;
mod termination;
mod transfers;
mod transport;
mod types;
pub(in crate::process) use runtime::{D3D8ReadbackOwner32, D3D8Reply32};

#[cfg(test)]
mod tests;

pub(in crate::process) use context::D3D8Runtime32;
pub use transport::D3D8DeviceInfo32;
pub use types::{D3D8Config32, D3D8Error32};

use crate::graphics::d3d8::api::HResult;
use crate::process::windows::objects::GuestComError32;

impl From<GuestComError32> for HResult {
    fn from(error: GuestComError32) -> Self {
        use GuestComError32::*;
        match error {
            Allocation | Capacity => Self::OUT_OF_MEMORY,
            Com(crate::windows::com::ComError32::NoInterface) => Self::NO_INTERFACE,
            _ => Self::INVALID_CALL,
        }
    }
}

impl From<crate::memory::MemoryError> for HResult {
    fn from(_: crate::memory::MemoryError) -> Self {
        Self::INVALID_CALL
    }
}

impl From<crate::graphics::d3d8::state::StateError> for HResult {
    fn from(error: crate::graphics::d3d8::state::StateError) -> Self {
        use crate::graphics::d3d8::state::StateError;
        match error {
            StateError::Allocation | StateError::LightCapacity => Self::OUT_OF_MEMORY,
            _ => Self::INVALID_CALL,
        }
    }
}

impl From<crate::graphics::d3d8::device::DeviceError> for HResult {
    fn from(error: crate::graphics::d3d8::device::DeviceError) -> Self {
        use crate::graphics::d3d8::{
            device::DeviceError as D, resources::ResourceError as R,
            shader::registry::RegistryError as S,
        };
        match error {
            D::Lost | D::Removed => Self::DEVICE_LOST,
            D::NotReset => Self::DEVICE_NOT_RESET,
            D::NotAvailable | D::Resource(R::NotAvailable) => Self::NOT_AVAILABLE,
            D::Resource(R::NotFound) => Self::NOT_FOUND,
            D::Resource(R::Allocation | R::ByteCapacity | R::ResourceCapacity) => {
                Self::OUT_OF_VIDEO_MEMORY
            }
            D::Allocation
            | D::Capacity
            | D::Shader(S::Allocation | S::ObjectCapacity | S::ByteCapacity) => Self::OUT_OF_MEMORY,
            D::State(state) => state.into(),
            _ => Self::INVALID_CALL,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum DispatchError32 {
    Api(HResult),
    Device(crate::graphics::d3d8::device::DeviceError),
    Readback {
        id: crate::graphics::d3d8::commands::ReadbackId,
    },
    LockReadback {
        id: crate::graphics::d3d8::commands::ReadbackId,
    },
    Callback(super::d3d8::resources::PrivateCallbackRequest32),
}
impl From<HResult> for DispatchError32 {
    fn from(error: HResult) -> Self {
        Self::Api(error)
    }
}
impl From<crate::memory::MemoryError> for DispatchError32 {
    fn from(error: crate::memory::MemoryError) -> Self {
        Self::Api(error.into())
    }
}
impl From<crate::graphics::d3d8::device::DeviceError> for DispatchError32 {
    fn from(error: crate::graphics::d3d8::device::DeviceError) -> Self {
        Self::Device(error)
    }
}
impl From<GuestComError32> for DispatchError32 {
    fn from(error: GuestComError32) -> Self {
        Self::Api(error.into())
    }
}
impl From<crate::graphics::d3d8::shader::registry::RegistryError> for DispatchError32 {
    fn from(error: crate::graphics::d3d8::shader::registry::RegistryError) -> Self {
        Self::Device(crate::graphics::d3d8::device::DeviceError::Shader(error))
    }
}
