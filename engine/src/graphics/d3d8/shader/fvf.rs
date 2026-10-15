use super::{Element, ElementType, ShaderError};

#[derive(Debug, PartialEq, Eq)]
pub struct Fvf {
    pub value: u32,
    pub stride: u32,
    pub transformed: bool,
    pub blend_weights: u8,
    pub indexed_blending: bool,
    pub weight_offsets: [Option<u32>; 5],
    elements: Vec<Element>,
}

impl Fvf {
    pub fn parse(value: u32) -> Result<Self, ShaderError> {
        if value & 0xe001 != 0 {
            return Err(ShaderError::InvalidFvf);
        }
        let position = value & 14;
        let transformed = position == 4;
        let betas = match position {
            2 | 4 => 0,
            6 => 1,
            8 => 2,
            10 => 3,
            12 => 4,
            14 => 5,
            _ => return Err(ShaderError::InvalidFvf),
        };
        let indexed_blending = value & 0x1000 != 0;
        if (transformed && value & 16 != 0) || (indexed_blending && betas == 0) {
            return Err(ShaderError::InvalidFvf);
        }
        let texture_count = ((value >> 8) & 15) as usize;
        if texture_count > 8 || (texture_count < 8 && value >> (16 + texture_count * 2) != 0) {
            return Err(ShaderError::InvalidFvf);
        }
        let mut elements = Vec::new();
        elements
            .try_reserve_exact(17)
            .map_err(|_| ShaderError::Allocation)?;
        let mut stride = 0;
        append(
            &mut elements,
            &mut stride,
            0,
            if transformed {
                ElementType::Float4
            } else {
                ElementType::Float3
            },
        );
        let blend_weights = betas - u8::from(indexed_blending);
        let weight_offsets = std::array::from_fn(|index| {
            (index < blend_weights as usize).then_some(stride + index as u32 * 4)
        });
        if blend_weights > 0 {
            let kind = match blend_weights {
                1 => ElementType::Float1,
                2 => ElementType::Float2,
                3 => ElementType::Float3,
                _ => ElementType::Float4,
            };
            append(&mut elements, &mut stride, 1, kind);
            // the fifth unindexed beta is retained in the stream even though the blend register has four lanes.
            if blend_weights == 5 {
                stride += 4;
            }
        }
        if indexed_blending {
            append(&mut elements, &mut stride, 2, ElementType::Ubyte4);
        }
        if value & 16 != 0 {
            append(&mut elements, &mut stride, 3, ElementType::Float3);
        }
        if value & 32 != 0 {
            append(&mut elements, &mut stride, 4, ElementType::Float1);
        }
        if value & 64 != 0 {
            append(&mut elements, &mut stride, 5, ElementType::Color);
        }
        if value & 128 != 0 {
            append(&mut elements, &mut stride, 6, ElementType::Color);
        }
        for stage in 0..texture_count {
            let kind = match (value >> (16 + stage * 2)) & 3 {
                0 => ElementType::Float2,
                1 => ElementType::Float3,
                2 => ElementType::Float4,
                _ => ElementType::Float1,
            };
            append(&mut elements, &mut stride, 7 + stage as u8, kind);
        }
        Ok(Self {
            value,
            stride,
            transformed,
            blend_weights,
            indexed_blending,
            weight_offsets,
            elements,
        })
    }

    pub fn elements(&self) -> &[Element] {
        &self.elements
    }
}

fn append(elements: &mut Vec<Element>, stride: &mut u32, register: u8, kind: ElementType) {
    elements.push(Element {
        stream: 0,
        offset: *stride,
        register,
        kind,
    });
    *stride += kind.bytes();
}
