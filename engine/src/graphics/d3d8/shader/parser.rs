use super::{
    Instruction, MAX_SHADER_TOKENS, Operation, ShaderError, Stage, Version, registers, validation,
};

#[derive(Debug)]
pub struct Program {
    version: Version,
    tokens: Vec<u32>,
    instructions: Vec<Instruction>,
}

impl Program {
    pub fn parse(words: &[u32], stage: Stage) -> Result<Self, ShaderError> {
        let token = *words.first().ok_or(ShaderError::Truncated)?;
        let minor = (token & 255) as u8;
        if token & 0xffff_ff00
            != match stage {
                Stage::Vertex => 0xfffe_0100,
                Stage::Pixel => 0xffff_0100,
            }
            || match stage {
                Stage::Vertex => minor != 1,
                Stage::Pixel => minor > 4,
            }
        {
            return Err(ShaderError::InvalidVersion);
        }
        if words.len() > MAX_SHADER_TOKENS {
            return Err(ShaderError::TokenLimit);
        }
        let version = Version { stage, minor };
        let mut instructions = Vec::new();
        instructions
            .try_reserve_exact(256)
            .map_err(|_| ShaderError::Allocation)?;
        let mut cursor = 1;
        let mut ended = false;
        while cursor < words.len() {
            let token = words[cursor];
            cursor += 1;
            let opcode = (token & 0xffff) as u16;
            if opcode == 0xffff {
                if token != 0xffff || cursor != words.len() {
                    return Err(ShaderError::InvalidToken);
                }
                ended = true;
                break;
            }
            if opcode == 0xfffe {
                if token & 0x8000_0000 != 0 {
                    return Err(ShaderError::InvalidToken);
                }
                cursor = cursor
                    .checked_add(((token >> 16) & 0x7fff) as usize)
                    .filter(|end| *end <= words.len())
                    .ok_or(ShaderError::Truncated)?;
                continue;
            }
            if token & !0x4000_ffff != 0 {
                return Err(ShaderError::InvalidToken);
            }
            let operation = Operation::decode(opcode, version)?;
            if instructions.len() == 256 {
                return Err(ShaderError::InstructionLimit);
            }
            let mut instruction = Instruction {
                operation,
                destination: None,
                sources: [None; 3],
                immediate: None,
                coissue: token & 0x4000_0000 != 0,
            };
            if operation.has_destination() {
                let token = *words.get(cursor).ok_or(ShaderError::Truncated)?;
                cursor += 1;
                let destination = registers::destination(token, version, operation)?;
                instruction.destination = Some(destination);
                if operation == Operation::Define {
                    let values = words
                        .get(cursor..cursor + 4)
                        .ok_or(ShaderError::Truncated)?;
                    instruction.immediate = Some(values.try_into().unwrap());
                    cursor += 4;
                } else {
                    for source in instruction
                        .sources
                        .iter_mut()
                        .take(operation.source_count(version))
                    {
                        let token = *words.get(cursor).ok_or(ShaderError::Truncated)?;
                        cursor += 1;
                        *source = Some(registers::source(token, version, operation, destination)?);
                    }
                }
            }
            instructions.push(instruction);
        }
        if !ended {
            return Err(ShaderError::Truncated);
        }
        validation::validate(version, &instructions)?;
        let mut tokens = Vec::new();
        tokens
            .try_reserve_exact(words.len())
            .map_err(|_| ShaderError::Allocation)?;
        tokens.extend_from_slice(words);
        Ok(Self {
            version,
            tokens,
            instructions,
        })
    }

    pub fn version(&self) -> Version {
        self.version
    }
    pub fn tokens(&self) -> &[u32] {
        &self.tokens
    }
    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    pub fn bind_constants(&self) -> impl Iterator<Item = (u16, [u32; 4])> + '_ {
        self.instructions
            .iter()
            .filter(|instruction| instruction.operation == Operation::Define)
            .map(|instruction| {
                (
                    instruction.destination.unwrap().register.index,
                    instruction.immediate.unwrap(),
                )
            })
    }

    pub fn executable_instructions(&self) -> impl Iterator<Item = &Instruction> + '_ {
        self.instructions
            .iter()
            .filter(|instruction| instruction.operation != Operation::Define)
    }

    pub(super) fn retained_bytes(&self) -> usize {
        self.tokens.capacity() * std::mem::size_of::<u32>()
            + self.instructions.capacity() * std::mem::size_of::<Instruction>()
    }

    pub fn validate_declaration(
        &self,
        declaration: &super::Declaration,
    ) -> Result<(), ShaderError> {
        if self.version.stage != Stage::Vertex {
            return Err(ShaderError::InvalidVersion);
        }
        let mut declared = 0u32;
        for element in declaration.elements() {
            declared |= 1 << element.register;
        }
        for tessellation in declaration.tessellation() {
            let output = match *tessellation {
                super::Tessellation::Normal { output, .. }
                | super::Tessellation::Coordinates { output } => output,
            };
            declared |= 1 << output;
        }
        for source in self
            .instructions
            .iter()
            .flat_map(|instruction| instruction.sources.iter().flatten())
        {
            if source.register.file == super::RegisterFile::Input
                && declared & (1 << source.register.index) == 0
            {
                return Err(ShaderError::InvalidDeclaration);
            }
        }
        Ok(())
    }
}
