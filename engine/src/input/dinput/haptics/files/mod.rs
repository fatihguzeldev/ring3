mod decoding;
mod encoding;
mod layout;

use super::EffectEnvelope32;
pub use decoding::decode;
pub use encoding::encode;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpaqueChunk32 {
    pub tag: [u8; 4],
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEffect32 {
    pub guid: [u8; 16],
    pub name: Vec<u8>,
    pub flags: u32,
    pub duration: u32,
    pub sample_period: u32,
    pub gain: u32,
    pub trigger: u32,
    pub trigger_repeat: u32,
    pub axes: Vec<u32>,
    pub direction: Vec<i32>,
    pub envelope: Option<EffectEnvelope32>,
    // custom force stores the literal pe32 dicustomforce header, including its
    // raw guest sample pointer. live effect creation owns and validates samples.
    pub type_specific: Vec<u8>,
    pub opaque_chunks: Vec<OpaqueChunk32>,
    pub start_delay: u32,
}
impl FileEffect32 {
    pub fn validate(&self) -> Result<(), crate::input::dinput::DirectInputResult32> {
        use crate::input::dinput::DirectInputResult32 as H;
        if self.name.len() > 259
            || self.name.contains(&0)
            || self.axes.len() > 32
            || self.axes.is_empty()
            || self.axes.len() != self.direction.len()
            || self.gain > 10000
            || !matches!(self.flags & 3, 1 | 2)
            || !matches!(self.flags & 0x70, 0x10 | 0x20 | 0x40)
            || self.flags & !0x73 != 0
            || self.type_specific.len() > 131072
            || self.opaque_chunks.len() > 256
            || self
                .opaque_chunks
                .iter()
                .any(|chunk| chunk.bytes.len() > 131072)
        {
            return Err(H::INVALID_PARAM);
        }
        if let Some(envelope) = self.envelope
            && (envelope.attack_level > 10000 || envelope.fade_level > 10000)
        {
            return Err(H::INVALID_PARAM);
        }
        if let Some(kind) = super::EffectKind32::from_guid(self.guid) {
            if !self.opaque_chunks.is_empty() {
                return Err(H::UNSUPPORTED);
            }
            let valid = match kind.family() {
                1 => self.type_specific.len() == 4,
                2 => self.type_specific.len() == 8,
                3 => self.type_specific.len() == 16,
                4 => {
                    self.type_specific.len() == 24
                        || self.type_specific.len() == self.axes.len() * 24
                }
                _ => self.type_specific.len() == 16,
            };
            if !valid {
                return Err(H::INVALID_PARAM);
            }
        }
        Ok(())
    }
    pub fn standard(&self) -> bool {
        super::EffectKind32::from_guid(self.guid).is_some()
    }
}
#[cfg(test)]
mod tests;
