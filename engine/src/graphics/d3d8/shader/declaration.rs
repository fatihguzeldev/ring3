use super::{MAX_DECLARATION_TOKENS, ShaderError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ElementType {
    Float1 = 0,
    Float2 = 1,
    Float3 = 2,
    Float4 = 3,
    Color = 4,
    Ubyte4 = 5,
    Short2 = 6,
    Short4 = 7,
}

impl ElementType {
    pub fn bytes(self) -> u32 {
        match self {
            Self::Float1 | Self::Color | Self::Ubyte4 | Self::Short2 => 4,
            Self::Float2 | Self::Short4 => 8,
            Self::Float3 => 12,
            Self::Float4 => 16,
        }
    }

    fn from_token(value: u32) -> Result<Self, ShaderError> {
        match value {
            0 => Ok(Self::Float1),
            1 => Ok(Self::Float2),
            2 => Ok(Self::Float3),
            3 => Ok(Self::Float4),
            4 => Ok(Self::Color),
            5 => Ok(Self::Ubyte4),
            6 => Ok(Self::Short2),
            7 => Ok(Self::Short4),
            _ => Err(ShaderError::InvalidDeclaration),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Element {
    pub stream: u8,
    pub offset: u32,
    pub register: u8,
    pub kind: ElementType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tessellation {
    Normal { input: u8, output: u8 },
    Coordinates { output: u8 },
}

#[derive(Debug)]
pub struct Declaration {
    tokens: Vec<u32>,
    elements: Vec<Element>,
    strides: [u32; 16],
    constants: Vec<(u8, [u32; 4])>,
    tessellation: Vec<Tessellation>,
}

impl Declaration {
    pub fn parse(words: &[u32]) -> Result<Self, ShaderError> {
        if words.len() > MAX_DECLARATION_TOKENS {
            return Err(ShaderError::TokenLimit);
        }
        let mut elements = Vec::new();
        elements
            .try_reserve_exact(17)
            .map_err(|_| ShaderError::Allocation)?;
        let mut constants = Vec::new();
        constants
            .try_reserve_exact(96)
            .map_err(|_| ShaderError::Allocation)?;
        let mut tessellation = Vec::new();
        tessellation
            .try_reserve_exact(17)
            .map_err(|_| ShaderError::Allocation)?;
        let mut used_registers = 0u32;
        let mut strides = [0u32; 16];
        let mut stream = None;
        let mut stream_tessellation = false;
        let mut cursor = 0;
        let mut ended = false;
        while cursor < words.len() {
            let token = words[cursor];
            cursor += 1;
            if token == u32::MAX {
                if cursor != words.len() {
                    return Err(ShaderError::InvalidDeclaration);
                }
                ended = true;
                break;
            }
            match token >> 29 {
                0 if token == 0 => {}
                1 => {
                    if token & !0x3000_000f != 0 {
                        return Err(ShaderError::InvalidDeclaration);
                    }
                    stream_tessellation = token & 0x1000_0000 != 0;
                    if stream_tessellation && token & 15 != 0 {
                        return Err(ShaderError::InvalidDeclaration);
                    }
                    stream = Some((token & 15) as u8);
                }
                2 => {
                    let stream = stream.ok_or(ShaderError::InvalidDeclaration)?;
                    if stream_tessellation {
                        return Err(ShaderError::InvalidDeclaration);
                    }
                    if token & 0x1000_0000 != 0 {
                        if token & !0x500f_0000 != 0 {
                            return Err(ShaderError::InvalidDeclaration);
                        }
                        let count = (token >> 16) & 15;
                        if count == 0 {
                            return Err(ShaderError::InvalidDeclaration);
                        }
                        strides[stream as usize] += count * 4;
                    } else {
                        if token & !0x400f_001f != 0 {
                            return Err(ShaderError::InvalidDeclaration);
                        }
                        let register = (token & 31) as u8;
                        if register > 16 || used_registers & (1 << register) != 0 {
                            return Err(ShaderError::InvalidDeclaration);
                        }
                        let kind = ElementType::from_token((token >> 16) & 15)?;
                        elements.push(Element {
                            stream,
                            offset: strides[stream as usize],
                            register,
                            kind,
                        });
                        strides[stream as usize] += kind.bytes();
                        used_registers |= 1 << register;
                    }
                }
                3 => {
                    if !stream_tessellation || stream.is_none() {
                        return Err(ShaderError::InvalidDeclaration);
                    }
                    let output = (token & 31) as u8;
                    if output > 16 || used_registers & (1 << output) != 0 {
                        return Err(ShaderError::InvalidDeclaration);
                    }
                    let operation = if token & 0x1000_0000 != 0 {
                        if token & !0x7001_001f != 0 || token & 0x000f_0000 != 0x0001_0000 {
                            return Err(ShaderError::InvalidDeclaration);
                        }
                        Tessellation::Coordinates { output }
                    } else {
                        if token & !0x60f2_001f != 0 || token & 0x000f_0000 != 0x0002_0000 {
                            return Err(ShaderError::InvalidDeclaration);
                        }
                        Tessellation::Normal {
                            input: ((token >> 20) & 15) as u8,
                            output,
                        }
                    };
                    tessellation.push(operation);
                    used_registers |= 1 << output;
                }
                4 => {
                    if token & !0x9e00_007f != 0 {
                        return Err(ShaderError::InvalidDeclaration);
                    }
                    let count = ((token >> 25) & 15) as usize;
                    let start = (token & 127) as usize;
                    if count == 0 || start + count > 96 || constants.len() + count > 96 {
                        return Err(ShaderError::InvalidDeclaration);
                    }
                    for index in start..start + count {
                        let values = words
                            .get(cursor..cursor + 4)
                            .ok_or(ShaderError::Truncated)?;
                        constants.push((index as u8, values.try_into().unwrap()));
                        cursor += 4;
                    }
                }
                5 => return Err(ShaderError::InvalidDeclaration),
                _ => return Err(ShaderError::InvalidDeclaration),
            }
        }
        if !ended {
            return Err(ShaderError::Truncated);
        }
        let mut tokens = Vec::new();
        tokens
            .try_reserve_exact(words.len())
            .map_err(|_| ShaderError::Allocation)?;
        tokens.extend_from_slice(words);
        Ok(Self {
            tokens,
            elements,
            strides,
            constants,
            tessellation,
        })
    }

    pub fn tokens(&self) -> &[u32] {
        &self.tokens
    }
    pub fn elements(&self) -> &[Element] {
        &self.elements
    }
    pub fn strides(&self) -> &[u32; 16] {
        &self.strides
    }
    pub fn constants(&self) -> &[(u8, [u32; 4])] {
        &self.constants
    }
    pub fn tessellation(&self) -> &[Tessellation] {
        &self.tessellation
    }

    pub(super) fn retained_bytes(&self) -> usize {
        self.tokens.capacity() * std::mem::size_of::<u32>()
            + self.elements.capacity() * std::mem::size_of::<Element>()
            + self.constants.capacity() * std::mem::size_of::<(u8, [u32; 4])>()
            + self.tessellation.capacity() * std::mem::size_of::<Tessellation>()
    }
}
