mod creation;
mod descriptions;
mod interfaces;
mod private;
mod views;

pub(super) use private::{PendingPrivateCall, PrivateCallbackRequest32};

use super::DispatchError32;
use crate::graphics::d3d8::{device::DeviceError, resources::ResourceError};

fn error(error: ResourceError) -> DispatchError32 {
    DeviceError::Resource(error).into()
}

#[cfg(test)]
mod tests;
