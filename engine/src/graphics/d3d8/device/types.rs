use super::super::{
    commands::{CommandError, CommandLimits, Progress},
    resources::{Format, ResourceError},
    shader::registry::{RegistryError, ShaderLimits},
    state::StateError,
};
use crate::graphics::GraphicsLimits;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceDescription {
    pub width: u32,
    pub height: u32,
    pub format: Format,
    pub back_buffers: u32,
    pub multisample: u32,
    pub depth: Option<Format>,
    pub window: u32,
    pub interval: u32,
    pub swap_effect: u32,
    pub lockable_back_buffer: bool,
    pub behavior: u32,
}

impl DeviceDescription {
    pub(super) fn validate(self) -> Result<(), DeviceError> {
        if self.width == 0
            || self.height == 0
            || self.width > 16384
            || self.height > 16384
            || self.back_buffers == 0
            || self.back_buffers > 3
            || self.window == 0
            || !matches!(self.swap_effect, 1..=4)
            || (self.swap_effect >= 3 && self.back_buffers != 1)
            || !matches!(self.multisample, 0 | 2..=16)
            || (self.lockable_back_buffer && self.multisample != 0)
            || self.format.is_depth()
            || self.format.is_buffer()
            || self.format.block().0 != 1
            || self.depth.is_some_and(|format| !format.is_depth())
            || !matches!(self.behavior & 0xe0, 0x20 | 0x40 | 0x80)
        {
            return Err(DeviceError::InvalidCall);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceLimits {
    pub graphics: GraphicsLimits,
    pub commands: CommandLimits,
    pub shaders: ShaderLimits,
    pub state_blocks: u32,
    pub patches: u32,
    pub readbacks: u32,
}

impl Default for DeviceLimits {
    fn default() -> Self {
        let graphics = GraphicsLimits {
            resources: 1024,
            resource_bytes: 256 * 1024 * 1024,
            batch_bytes: 256 * 1024,
        };
        Self {
            graphics,
            commands: graphics.into(),
            shaders: ShaderLimits::default(),
            state_blocks: 256,
            patches: 256,
            readbacks: 64,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceStatus {
    Operational,
    Lost,
    Resettable,
    Removed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceError {
    InvalidCall,
    NotAvailable,
    Lost,
    NotReset,
    Removed,
    PendingReadback,
    StaleReadback,
    Resource(ResourceError),
    State(StateError),
    Shader(RegistryError),
    Command(CommandError),
    Capacity,
    IdentityExhausted,
    Allocation,
}

impl From<ResourceError> for DeviceError {
    fn from(error: ResourceError) -> Self {
        Self::Resource(error)
    }
}
impl From<StateError> for DeviceError {
    fn from(error: StateError) -> Self {
        Self::State(error)
    }
}
impl From<RegistryError> for DeviceError {
    fn from(error: RegistryError) -> Self {
        Self::Shader(error)
    }
}
impl From<CommandError> for DeviceError {
    fn from(error: CommandError) -> Self {
        Self::Command(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome<T> {
    pub value: T,
    pub progress: Progress,
}

impl<T> Outcome<T> {
    pub(super) fn new(value: T, progress: Progress) -> Self {
        Self { value, progress }
    }
}
