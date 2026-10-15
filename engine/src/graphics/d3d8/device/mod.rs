mod context;
mod draw;
mod lifetime;
mod pipeline;
mod preparation;
mod presentation;
mod resources;
mod targets;
mod transfers;
mod types;
mod validation;

pub use bindings::{Binding, PreparedBinding, PreparedBlockChange};
pub use context::Device;
pub use lifetime::{PreparedClose, PreparedDestruction, PreparedReset};
pub use preparation::{
    PreparedDeviceResource, PreparedDeviceShader, PreparedResources, PreparedStateBlock,
};
pub use targets::PreparedTargets;
pub use types::{DeviceDescription, DeviceError, DeviceLimits, DeviceStatus, Outcome};

mod bindings;
#[cfg(test)]
mod tests;
