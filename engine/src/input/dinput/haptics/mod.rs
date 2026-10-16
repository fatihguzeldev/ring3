mod coordinates;
mod device;
mod evaluation;
pub mod files;
mod types;

pub use device::{EffectDevice32, PreparedEffect32};
pub use types::{
    Condition32, Coordinates32, EffectEnvelope32, EffectKind32, EffectParameters32,
    EffectPayload32, HapticCapabilities32, HapticFrame32,
};

pub const ALL_PARAMETERS: u32 = 0x3ff;
pub const REQUIRED_PARAMETERS: u32 = 0x165;

#[cfg(test)]
mod tests;
