mod adaptive;
mod identity;
mod reference;
mod samples;

pub use adaptive::CaptureProcessor;
pub use identity::{CaptureEffectKind, class_id, instance_id, interface_id};
pub use reference::PreparedCaptureReference;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AecParameters {
    pub enabled: bool,
    pub noise_fill: bool,
}
impl Default for AecParameters {
    fn default() -> Self {
        Self {
            enabled: true,
            noise_fill: false,
        }
    }
}

#[cfg(test)]
mod tests;
