use super::{Instruction, Operation, Register, RegisterFile, ShaderError, Source, Stage, Version};

pub(super) fn validate(version: Version, instructions: &[Instruction]) -> Result<(), ShaderError> {
    use Operation::*;
    let mut initialized = [0u8; 12];
    let mut previous_phase_initialized = [0u8; 12];
    let mut texture_initialized = [0u8; 6];
    let mut previous_texture_initialized = texture_initialized;
    let mut bump_consumed = 0u8;
    let mut coordinate_order = [None; 6];
    let mut divides = 0;
    let mut bem_used = false;
    let mut previous_initialized = initialized;
    let mut address_initialized = false;
    let mut position = 0u8;
    let mut arithmetic_slots = 0u16;
    let mut texture_slots = 0u16;
    let has_phase = instructions
        .iter()
        .any(|instruction| instruction.operation == Phase);
    let mut phase = if has_phase { 1 } else { 2 };
    let mut arithmetic_started = false;
    let mut body_started = false;
    let mut depth_written = false;
    let mut matrix_pads = 0;
    let mut matrix_source = None;
    let mut matrix_last_destination = 0;
    for (index, instruction) in instructions.iter().enumerate() {
        let operation = instruction.operation;
        if operation == Phase {
            if version.stage != Stage::Pixel
                || version.minor != 4
                || phase != 1
                || index == 0
                || index + 1 == instructions.len()
                || instruction.coissue
                || matrix_pads != 0
            {
                return Err(ShaderError::InvalidInstruction);
            }
            previous_phase_initialized = initialized;
            for mask in &mut initialized {
                *mask &= 7;
            }
            phase = 2;
            arithmetic_slots = 0;
            texture_slots = 0;
            arithmetic_started = false;
            continue;
        }
        if operation == Define {
            if body_started || instruction.coissue {
                return Err(ShaderError::InvalidInstruction);
            }
            continue;
        }
        body_started = true;
        let (mut arithmetic, texture) = operation.slots(version);
        if instruction.coissue {
            if version.stage != Stage::Pixel
                || version.minor == 4
                || index == 0
                || operation.is_texture()
            {
                return Err(ShaderError::InvalidInstruction);
            }
            let previous = &instructions[index - 1];
            let Some(before) = previous.destination else {
                return Err(ShaderError::InvalidInstruction);
            };
            let Some(current) = instruction.destination else {
                return Err(ShaderError::InvalidInstruction);
            };
            if previous.coissue
                || previous.operation.is_texture()
                || before.write_mask == 15
                || current.write_mask == 15
                || before.write_mask & current.write_mask != 0
            {
                return Err(ShaderError::InvalidInstruction);
            }
            arithmetic = 0;
        }
        if version.stage == Stage::Pixel {
            if operation.is_texture() && arithmetic_started {
                return Err(ShaderError::InvalidInstruction);
            }
            if arithmetic != 0 && !operation.is_texture() {
                arithmetic_started = true;
            }
        }
        arithmetic_slots = arithmetic_slots
            .checked_add(arithmetic)
            .ok_or(ShaderError::InstructionLimit)?;
        texture_slots = texture_slots
            .checked_add(texture)
            .ok_or(ShaderError::InstructionLimit)?;
        if (version.stage == Stage::Vertex && arithmetic_slots > 128)
            || (version.stage == Stage::Pixel
                && (arithmetic_slots > 8 || texture_slots > if version.minor == 4 { 6 } else { 4 }))
        {
            return Err(ShaderError::InstructionLimit);
        }
        let read_initialized = if instruction.coissue {
            previous_initialized
        } else {
            initialized
        };
        let read_texture_initialized = if instruction.coissue {
            previous_texture_initialized
        } else {
            texture_initialized
        };
        if operation == Bump14 {
            if phase != 1 || bem_used || instruction.destination.unwrap().write_mask != 3 {
                return Err(ShaderError::InvalidInstruction);
            }
            bem_used = true;
        }
        if operation == Conditional && version.stage == Stage::Pixel && version.minor < 4 {
            let condition = instruction.sources[0].unwrap();
            if condition.register
                != (Register {
                    file: RegisterFile::Temporary,
                    index: 0,
                })
                || condition.swizzle != [3; 4]
                || condition.modifier != super::SourceModifier::None
            {
                return Err(ShaderError::InvalidRegister);
            }
        }
        for (source_index, source) in instruction.sources.iter().flatten().enumerate() {
            if source.relative && !address_initialized {
                return Err(ShaderError::UninitializedRegister);
            }
            if version.stage == Stage::Pixel && version.minor == 4 {
                if matches!(operation, Sample | TextureCoordinate)
                    && source.register.file == RegisterFile::Texture
                {
                    let order = [source.swizzle[0], source.swizzle[1], source.swizzle[2]];
                    let previous = &mut coordinate_order[source.register.index as usize];
                    if previous.is_some_and(|previous| previous != order) {
                        return Err(ShaderError::InvalidModifier);
                    }
                    *previous = Some(order);
                }
                if source.modifier == super::SourceModifier::DivideZ {
                    divides += 1;
                    if divides > 2 {
                        return Err(ShaderError::InvalidModifier);
                    }
                }
                if phase == 1 && source.register.file == RegisterFile::Input {
                    return Err(ShaderError::InvalidRegister);
                }
                if depth_written
                    && source.register
                        == (Register {
                            file: RegisterFile::Temporary,
                            index: 5,
                        })
                {
                    return Err(ShaderError::InvalidRegister);
                }
                if operation == Sample
                    && phase == 1
                    && source.register.file == RegisterFile::Temporary
                {
                    return Err(ShaderError::InvalidRegister);
                }
                if operation == TextureCoordinate && source.register.file != RegisterFile::Texture {
                    return Err(ShaderError::InvalidRegister);
                }
                if operation == Sample
                    && source.register.file == RegisterFile::Temporary
                    && (!has_phase
                        || previous_phase_initialized[source.register.index as usize] & 7 != 7)
                {
                    return Err(ShaderError::UninitializedRegister);
                }
            }
            if version.stage == Stage::Pixel
                && version.minor < 4
                && source.register.file == RegisterFile::Texture
            {
                let needed = swizzled_mask(
                    *source,
                    read_mask(
                        operation,
                        source_index,
                        instruction
                            .destination
                            .map_or(15, |destination| destination.write_mask),
                    ),
                );
                if read_texture_initialized[source.register.index as usize] & needed != needed {
                    return Err(ShaderError::UninitializedRegister);
                }
                if bump_consumed & (1 << source.register.index) != 0
                    && !matches!(operation, Bump | BumpLuminance)
                {
                    return Err(ShaderError::InvalidRegister);
                }
                if operation.is_texture()
                    && instruction.destination.unwrap().register.index <= source.register.index
                {
                    return Err(ShaderError::InvalidRegister);
                }
                if matches!(operation, Bump | BumpLuminance) {
                    bump_consumed |= 1 << source.register.index;
                }
            }
            if source.register.file == RegisterFile::Temporary {
                let needed = if version.minor == 4 && operation == Sample {
                    7
                } else {
                    read_mask(
                        operation,
                        source_index,
                        instruction
                            .destination
                            .map_or(15, |destination| destination.write_mask),
                    )
                };
                let needed = swizzled_mask(*source, needed);
                if read_initialized[source.register.index as usize] & needed != needed {
                    return Err(ShaderError::UninitializedRegister);
                }
            }
        }
        read_ports(version, instruction)?;
        if matches!(
            operation,
            Matrix4x4 | Matrix4x3 | Matrix3x4 | Matrix3x3 | Matrix3x2
        ) {
            let source = instruction.sources[1].unwrap();
            let rows = match operation {
                Matrix4x4 | Matrix3x4 => 4,
                Matrix4x3 | Matrix3x3 => 3,
                _ => 2,
            };
            let limit = match source.register.file {
                RegisterFile::Constant => 96,
                RegisterFile::Temporary => 12,
                RegisterFile::Input => 16,
                _ => 0,
            };
            if source.register.index + rows > limit {
                return Err(ShaderError::InvalidRegister);
            }
            if source.modifier != super::SourceModifier::None || source.swizzle != [0, 1, 2, 3] {
                return Err(ShaderError::InvalidModifier);
            }
            if source.register.file == RegisterFile::Temporary {
                let required = if matches!(operation, Matrix4x4 | Matrix4x3) {
                    15
                } else {
                    7
                };
                for row in 0..rows {
                    if read_initialized[(source.register.index + row) as usize] & required
                        != required
                    {
                        return Err(ShaderError::UninitializedRegister);
                    }
                }
            }
        }
        previous_initialized = initialized;
        previous_texture_initialized = texture_initialized;
        if let Some(destination) = instruction.destination {
            if operation == Kill {
                if version.minor == 4
                    && destination.register.file == RegisterFile::Temporary
                    && read_initialized[destination.register.index as usize] & 7 != 7
                {
                    return Err(ShaderError::UninitializedRegister);
                }
            } else if operation == TextureDepth {
                if phase != 2 || !has_phase || depth_written || read_initialized[5] & 3 != 3 {
                    return Err(ShaderError::InvalidInstruction);
                }
                depth_written = true;
            } else if destination.register.file == RegisterFile::Temporary {
                if depth_written && destination.register.index == 5 {
                    return Err(ShaderError::InvalidRegister);
                }
                if operation == TextureCoordinate && version.minor == 4 {
                    initialized[destination.register.index as usize] = destination.write_mask;
                } else {
                    initialized[destination.register.index as usize] |= destination.write_mask;
                }
            } else if destination.register.file == RegisterFile::Texture {
                texture_initialized[destination.register.index as usize] |= destination.write_mask;
            } else if destination.register.file == RegisterFile::Address {
                address_initialized = true;
            } else if destination.register.file == RegisterFile::RasterOutput
                && destination.register.index == 0
            {
                position |= destination.write_mask;
            }
            if matches!(operation, Matrix3x2Pad | Matrix3x3Pad) {
                let source = instruction.sources[0].unwrap().register;
                if destination.register.index <= source.index
                    || (matrix_pads != 0
                        && (matrix_source != Some(source)
                            || destination.register.index != matrix_last_destination + 1))
                {
                    return Err(ShaderError::InvalidInstruction);
                }
                matrix_pads += 1;
                matrix_source = Some(source);
                matrix_last_destination = destination.register.index;
                if matrix_pads > 2 {
                    return Err(ShaderError::InvalidInstruction);
                }
            } else if matches!(
                operation,
                Matrix3x2Sample
                    | Matrix3x2Depth
                    | Matrix3x3Sample
                    | Matrix3x3Specular
                    | Matrix3x3VSpecular
                    | TextureMatrix3x3
            ) {
                let required = if matches!(operation, Matrix3x2Sample | Matrix3x2Depth) {
                    1
                } else {
                    2
                };
                if matrix_pads != required
                    || matrix_source != Some(instruction.sources[0].unwrap().register)
                    || destination.register.index != matrix_last_destination + 1
                {
                    return Err(ShaderError::InvalidInstruction);
                }
                matrix_pads = 0;
                matrix_source = None;
            } else if matrix_pads != 0 {
                return Err(ShaderError::InvalidInstruction);
            }
        }
    }
    if matrix_pads != 0
        || (version.stage == Stage::Vertex && position != 15)
        || (version.stage == Stage::Pixel && initialized[0] != 15)
    {
        return Err(ShaderError::UninitializedRegister);
    }
    Ok(())
}

fn read_mask(operation: Operation, source: usize, destination: u8) -> u8 {
    use Operation::*;
    match operation {
        Reciprocal | ReciprocalSqrt | Exp | Log | ExpPartial | LogPartial => 1,
        Dot3 | Matrix3x4 | Matrix3x3 | Matrix3x2 => 7,
        Dot4 | Matrix4x4 | Matrix4x3 => 15,
        Lit => {
            if destination & 4 != 0 {
                11
            } else if destination & 2 != 0 {
                1
            } else {
                0
            }
        }
        Distance => {
            if source == 0 {
                (destination & 2) | (destination & 4)
            } else {
                (destination & 2) | (destination & 8)
            }
        }
        operation if operation.is_texture() => 15,
        _ => destination,
    }
}

fn swizzled_mask(source: Source, mask: u8) -> u8 {
    let mut result = 0;
    for component in 0..4 {
        if mask & (1 << component) != 0 {
            result |= 1 << source.swizzle[component];
        }
    }
    result
}

fn read_ports(version: Version, instruction: &Instruction) -> Result<(), ShaderError> {
    for file in [
        RegisterFile::Constant,
        RegisterFile::Input,
        RegisterFile::Temporary,
        RegisterFile::Texture,
    ] {
        let mut unique = [None; 3];
        let mut count = 0;
        for source in instruction
            .sources
            .iter()
            .flatten()
            .filter(|source| source.register.file == file)
        {
            if !unique.contains(&Some(source.register.index)) {
                unique[count] = Some(source.register.index);
                count += 1;
            }
        }
        let limit = if version.stage == Stage::Vertex {
            if matches!(file, RegisterFile::Constant | RegisterFile::Input) {
                1
            } else {
                3
            }
        } else {
            match file {
                RegisterFile::Constant | RegisterFile::Input => 2,
                RegisterFile::Temporary => {
                    if version.minor == 4 {
                        3
                    } else {
                        2
                    }
                }
                RegisterFile::Texture => {
                    if version.minor == 4 {
                        1
                    } else if version.minor >= 2 {
                        3
                    } else {
                        2
                    }
                }
                _ => 0,
            }
        };
        if count > limit {
            return Err(ShaderError::InvalidRegister);
        }
    }
    Ok(())
}
