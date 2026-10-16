use super::super::PatchError;
use crate::graphics::d3d8::shader::{Element, Tessellation};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputPlan {
    words: [u32; 128],
}

impl InputPlan {
    pub fn compile(elements: &[Element], generated: &[Tessellation]) -> Result<Self, PatchError> {
        if elements.is_empty() || elements.len() > 17 || generated.len() > 17 {
            return Err(PatchError);
        }
        let mut words = [0; 128];
        words[0] = elements.len() as u32;
        words[1] = generated.len() as u32;
        let mut used = 0u32;
        for (i, element) in elements.iter().enumerate() {
            if element.stream > 15
                || element.register > 16
                || used & (1 << element.register) != 0
                || element.offset.checked_add(element.kind.bytes()).is_none()
            {
                return Err(PatchError);
            }
            used |= 1 << element.register;
            words[4 + i * 4..8 + i * 4].copy_from_slice(&[
                element.offset,
                element.stream as u32,
                element.kind as u32,
                element.register as u32,
            ]);
        }
        let inputs = used;
        for (i, operation) in generated.iter().enumerate() {
            let (kind, input, output) = match *operation {
                Tessellation::Normal { input, output } => {
                    if input > 15 || inputs & (1 << input) == 0 {
                        return Err(PatchError);
                    }
                    (0, input, output)
                }
                Tessellation::Coordinates { output } => (1, 0, output),
            };
            if output > 16 || used & (1 << output) != 0 {
                return Err(PatchError);
            }
            used |= 1 << output;
            words[72 + i * 3..75 + i * 3].copy_from_slice(&[kind, input as u32, output as u32]);
        }
        Ok(Self { words })
    }
    pub fn with_fifth_blend_weight(mut self, offset: u32) -> Result<Self, PatchError> {
        let count = self.words[0] as usize;
        if count >= 17
            || offset.checked_add(4).is_none()
            || (0..count).any(|i| self.words[7 + i * 4] == 16)
        {
            return Err(PatchError);
        }
        self.words[4 + count * 4..8 + count * 4].copy_from_slice(&[offset, 0, 0, 16]);
        self.words[0] += 1;
        self.words[3] = 1;
        Ok(self)
    }
    pub fn words(&self) -> &[u32; 128] {
        &self.words
    }
}
